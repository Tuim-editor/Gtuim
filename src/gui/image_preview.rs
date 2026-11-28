//! Native GPUI PNG/JPEG image preview.
//!
//! Decodes and validates PNG and JPEG files asynchronously on GPUI's background
//! executor with bounded allocation/dimension limits, EXIF orientation support,
//! Fit/1:1 sizing, zoom/pan controls, canvas background switching for transparent
//! PNGs, Full/Split modes, and automatic reload on file modification.

use std::fmt;
use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use gpui::{
    div, point, prelude::*, px, rgb, AnyElement, App, ClickEvent, Context, FontWeight, IntoElement,
    Keystroke, MouseButton, ObjectFit, ScrollHandle, Window,
};
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat as DecodedFormat, ImageReader, Limits};

use super::icons::{gui_palette, icon};
use super::GtuimView;
use crate::tui::app::{App as TuimApp, Mode};
use crate::tui::theme::blend_u32;

const MAX_IMAGE_FILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 16_384;
const MAX_DECODE_ALLOC_BYTES: u64 = 256 * 1024 * 1024;
const RELOAD_CHECK_INTERVAL: Duration = Duration::from_millis(500);
const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 8.0;
const ZOOM_STEP: f32 = 0.25;
const CANVAS_PAD_PX: f32 = 24.0;
const KEYBOARD_PAN_STEP_PX: f32 = 96.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ImageViewMode {
    /// Replaces the editor viewport when viewing a `.png`/`.jpg`/`.jpeg` buffer.
    #[default]
    Full,
    /// Side-by-side preview pane to the right of the editor.
    Split,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ImageFitMode {
    /// Scales down to fit within the viewport while keeping smaller images at 100%.
    #[default]
    Fit,
    /// Renders at native pixel dimensions (`1:1` at 100% zoom).
    ActualSize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ImageCanvasBg {
    #[default]
    Theme,
    Light,
    Dark,
}

impl ImageCanvasBg {
    pub fn next(self) -> Self {
        match self {
            Self::Theme => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::Theme,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Theme => "Bg: Theme",
            Self::Light => "Bg: Light",
            Self::Dark => "Bg: Dark",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
}

impl ImageKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageMetadata {
    pub width_px: u32,
    pub height_px: u32,
    pub kind: ImageKind,
    pub file_bytes: u64,
}

#[derive(Clone)]
pub struct LoadedImage {
    pub image: Arc<gpui::Image>,
    pub metadata: ImageMetadata,
}

#[derive(Debug)]
pub struct DecodedImagePayload {
    pub bytes: Vec<u8>,
    pub gpui_format: gpui::ImageFormat,
    pub metadata: ImageMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageError {
    NotFound(String),
    NotAFile(String),
    TooLarge { size_bytes: u64, limit_bytes: u64 },
    UnsupportedFormat(String),
    DecodeFailed(String),
    IoError(String),
}

impl fmt::Display for ImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(path) => write!(f, "Image file not found: {path}"),
            Self::NotAFile(path) => write!(f, "Path is not a regular image file: {path}"),
            Self::TooLarge {
                size_bytes,
                limit_bytes,
            } => write!(
                f,
                "Image file ({}) exceeds the {} preview safety limit.",
                format_byte_size(*size_bytes),
                format_byte_size(*limit_bytes)
            ),
            Self::UnsupportedFormat(detail) => {
                write!(f, "Only PNG and JPEG images are supported ({detail}).")
            }
            Self::DecodeFailed(msg) => write!(f, "Could not decode image: {msg}"),
            Self::IoError(msg) => write!(f, "Could not read image file: {msg}"),
        }
    }
}

pub struct ImagePreview {
    pub open: bool,
    pub mode: ImageViewMode,
    pub pinned: bool,
    pub path: Option<String>,
    pub dismissed_for_path: Option<String>,
    pub loaded: Option<LoadedImage>,
    pub zoom: f32,
    pub fit_mode: ImageFitMode,
    pub canvas_bg: ImageCanvasBg,
    pub loading: bool,
    pub error: Option<String>,
    pub file_stamp: Option<FileStamp>,
    last_reload_check: Option<Instant>,
    generation: u64,
    active_load: bool,
    pending_load: Option<(PathBuf, u64)>,
    pub scroll: ScrollHandle,
}

impl Default for ImagePreview {
    fn default() -> Self {
        Self {
            open: false,
            mode: ImageViewMode::Full,
            pinned: false,
            path: None,
            dismissed_for_path: None,
            loaded: None,
            zoom: 1.0,
            fit_mode: ImageFitMode::Fit,
            canvas_bg: ImageCanvasBg::Theme,
            loading: false,
            error: None,
            file_stamp: None,
            last_reload_check: None,
            generation: 0,
            active_load: false,
            pending_load: None,
            scroll: ScrollHandle::new(),
        }
    }
}

pub fn is_image_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ext.eq_ignore_ascii_case("png")
                || ext.eq_ignore_ascii_case("jpg")
                || ext.eq_ignore_ascii_case("jpeg")
        })
}

pub fn active_tab_path(app: &TuimApp) -> Option<String> {
    let tab = app.tabs.get(app.active_tab)?;
    let raw = if !tab.name.is_empty() {
        tab.name.as_str()
    } else if !tab.relative_name.is_empty() {
        tab.relative_name.as_str()
    } else {
        return None;
    };
    Some(raw.to_string())
}

pub fn active_image_path(app: &TuimApp) -> Option<String> {
    let path = active_tab_path(app)?;
    if is_image_path(&path) {
        Some(path)
    } else {
        None
    }
}

pub fn resolve_path(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().map(|cwd| cwd.join(&p)).unwrap_or(p)
    }
}

pub fn read_file_stamp(path: &Path) -> Option<FileStamp> {
    let meta = fs::metadata(path).ok()?;
    Some(FileStamp {
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

pub fn format_byte_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = 1024.0 * 1024.0;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if (bytes as f64) < MIB {
        format!("{:.1} KiB", bytes as f64 / KIB)
    } else {
        format!("{:.2} MiB", bytes as f64 / MIB)
    }
}

pub fn compute_display_size(
    width_px: u32,
    height_px: u32,
    pane_width_px: f32,
    viewport_height_px: f32,
    zoom: f32,
    fit_mode: ImageFitMode,
) -> (f32, f32) {
    let img_w = width_px.max(1) as f32;
    let img_h = height_px.max(1) as f32;
    let clamped_zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);

    let base_scale = match fit_mode {
        ImageFitMode::Fit => {
            let avail_w = (pane_width_px - CANVAS_PAD_PX * 2.0).max(1.0);
            let avail_h = (viewport_height_px - CANVAS_PAD_PX * 2.0).max(1.0);
            (avail_w / img_w).min(avail_h / img_h).min(1.0)
        }
        ImageFitMode::ActualSize => 1.0,
    };

    let scale = (base_scale * clamped_zoom).max(0.01);
    ((img_w * scale).max(1.0), (img_h * scale).max(1.0))
}

/// Reads at most `limit_bytes` from `reader`, capping allocation to `limit_bytes + 1`
/// even if the underlying file grows concurrently.
pub fn read_bounded_bytes<R: Read>(reader: R, limit_bytes: u64) -> Result<Vec<u8>, ImageError> {
    let mut buf = Vec::new();
    reader
        .take(limit_bytes.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|err| ImageError::IoError(err.to_string()))?;
    if buf.len() as u64 > limit_bytes {
        return Err(ImageError::TooLarge {
            size_bytes: buf.len() as u64,
            limit_bytes,
        });
    }
    Ok(buf)
}

/// Reads and validates a PNG or JPEG image from disk, enforcing bounded file reads
/// (`File::open` + `Read::take`), decoder allocation/dimension limits, and EXIF
/// orientation normalization.
pub fn load_image_from_disk(path: &Path) -> Result<DecodedImagePayload, ImageError> {
    let file = File::open(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            ImageError::NotFound(path.display().to_string())
        } else {
            ImageError::IoError(err.to_string())
        }
    })?;
    let meta = file
        .metadata()
        .map_err(|err| ImageError::IoError(err.to_string()))?;
    if !meta.is_file() {
        return Err(ImageError::NotAFile(path.display().to_string()));
    }
    let file_bytes = meta.len();
    if file_bytes > MAX_IMAGE_FILE_BYTES {
        return Err(ImageError::TooLarge {
            size_bytes: file_bytes,
            limit_bytes: MAX_IMAGE_FILE_BYTES,
        });
    }

    let raw_bytes = read_bounded_bytes(file, MAX_IMAGE_FILE_BYTES)?;
    decode_and_validate_image_bytes(raw_bytes)
}

pub fn decode_and_validate_image_bytes(
    raw_bytes: Vec<u8>,
) -> Result<DecodedImagePayload, ImageError> {
    let file_bytes = raw_bytes.len() as u64;
    if file_bytes == 0 {
        return Err(ImageError::DecodeFailed("image data is empty".into()));
    }
    if file_bytes > MAX_IMAGE_FILE_BYTES {
        return Err(ImageError::TooLarge {
            size_bytes: file_bytes,
            limit_bytes: MAX_IMAGE_FILE_BYTES,
        });
    }

    let mut reader = ImageReader::new(Cursor::new(raw_bytes.as_slice()))
        .with_guessed_format()
        .map_err(|err| ImageError::DecodeFailed(err.to_string()))?;

    let (kind, gpui_format) = match reader.format() {
        Some(DecodedFormat::Png) => (ImageKind::Png, gpui::ImageFormat::Png),
        Some(DecodedFormat::Jpeg) => (ImageKind::Jpeg, gpui::ImageFormat::Jpeg),
        Some(other) => {
            return Err(ImageError::UnsupportedFormat(format!(
                "detected {other:?}"
            )))
        }
        None => {
            return Err(ImageError::UnsupportedFormat(
                "unrecognized image header".into(),
            ))
        }
    };

    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC_BYTES);
    reader.limits(limits);

    let mut decoder = reader
        .into_decoder()
        .map_err(|err| ImageError::DecodeFailed(err.to_string()))?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut decoded = DynamicImage::from_decoder(decoder)
        .map_err(|err| ImageError::DecodeFailed(err.to_string()))?;

    if orientation != Orientation::NoTransforms {
        decoded.apply_orientation(orientation);
        let (width_px, height_px) = (decoded.width(), decoded.height());
        if width_px == 0 || height_px == 0 {
            return Err(ImageError::DecodeFailed(
                "image has zero dimensions".into(),
            ));
        }
        let mut normalized_png = Vec::new();
        decoded
            .write_to(&mut Cursor::new(&mut normalized_png), DecodedFormat::Png)
            .map_err(|err| ImageError::DecodeFailed(err.to_string()))?;
        return Ok(DecodedImagePayload {
            bytes: normalized_png,
            gpui_format: gpui::ImageFormat::Png,
            metadata: ImageMetadata {
                width_px,
                height_px,
                kind,
                file_bytes,
            },
        });
    }

    let (width_px, height_px) = (decoded.width(), decoded.height());
    if width_px == 0 || height_px == 0 {
        return Err(ImageError::DecodeFailed(
            "image has zero dimensions".into(),
        ));
    }

    Ok(DecodedImagePayload {
        bytes: raw_bytes,
        gpui_format,
        metadata: ImageMetadata {
            width_px,
            height_px,
            kind,
            file_bytes,
        },
    })
}

impl ImagePreview {
    pub fn is_full_mode(&self) -> bool {
        self.open && self.mode == ImageViewMode::Full
    }

    pub fn is_split_mode(&self) -> bool {
        self.open && self.mode == ImageViewMode::Split
    }

    pub fn metadata(&self) -> Option<&ImageMetadata> {
        self.loaded.as_ref().map(|loaded| &loaded.metadata)
    }

    pub fn reset_for_new_path(&mut self, path: String, stamp: Option<FileStamp>) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.path = Some(path);
        self.loaded = None;
        self.loading = true;
        self.error = None;
        self.zoom = 1.0;
        self.fit_mode = ImageFitMode::Fit;
        self.file_stamp = stamp;
        self.last_reload_check = Some(Instant::now());
        self.scroll.set_offset(point(px(0.0), px(0.0)));
        self.generation
    }

    pub fn prepare_reload(&mut self, stamp: Option<FileStamp>) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.loading = true;
        self.error = None;
        self.file_stamp = stamp;
        self.last_reload_check = Some(Instant::now());
        self.generation
    }

    pub fn apply_load_result(
        &mut self,
        generation: u64,
        result: Result<DecodedImagePayload, ImageError>,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        self.loading = false;
        match result {
            Ok(payload) => {
                self.loaded = Some(LoadedImage {
                    image: Arc::new(gpui::Image::from_bytes(payload.gpui_format, payload.bytes)),
                    metadata: payload.metadata,
                });
                self.error = None;
            }
            Err(err) => {
                self.loaded = None;
                self.error = Some(err.to_string());
            }
        }
        true
    }
}

impl GtuimView {
    /// Synchronizes image preview state with the active Neovim buffer and checks
    /// for on-disk file modifications while an image preview is open.
    pub fn sync_image_preview(&mut self, cx: &mut Context<Self>) {
        if let Some(image_path) = active_image_path(&self.app) {
            let dismissed = self.image_preview.dismissed_for_path.as_deref()
                == Some(image_path.as_str());
            let pinned_full_pdf = self.pdf_preview.is_full_mode() && self.pdf_preview.pinned;
            if !dismissed
                && !pinned_full_pdf
                && !(self.image_preview.open && self.image_preview.pinned)
            {
                let path_changed = self.image_preview.path.as_deref() != Some(image_path.as_str());
                if !self.image_preview.open || path_changed {
                    self.open_image_path(image_path, ImageViewMode::Full, false, cx);
                    return;
                }
            }
        } else {
            if self.image_preview.dismissed_for_path.is_some() {
                self.image_preview.dismissed_for_path = None;
            }
            if self.image_preview.open
                && !self.image_preview.pinned
                && self.image_preview.mode == ImageViewMode::Full
            {
                self.image_preview.open = false;
                cx.notify();
            }
        }

        // Throttled auto-reload check while an image preview is open.
        // Compares Option<FileStamp> directly so deletion transitions to NotFound once
        // and subsequent recreation triggers a clean reload.
        if self.image_preview.open && !self.image_preview.loading {
            let should_check = self
                .image_preview
                .last_reload_check
                .is_none_or(|t| t.elapsed() >= RELOAD_CHECK_INTERVAL);
            if should_check {
                self.image_preview.last_reload_check = Some(Instant::now());
                if let Some(path) = self.image_preview.path.clone() {
                    let resolved = resolve_path(&path);
                    let stamp = read_file_stamp(&resolved);
                    if stamp != self.image_preview.file_stamp {
                        self.reload_image_preview(cx);
                    }
                }
            }
        }
    }

    pub fn open_image_path(
        &mut self,
        path: String,
        mode: ImageViewMode,
        pinned: bool,
        cx: &mut Context<Self>,
    ) {
        let resolved = resolve_path(&path);
        let stamp = read_file_stamp(&resolved);
        self.image_preview.open = true;
        self.image_preview.mode = mode;
        self.image_preview.pinned = pinned;
        self.image_preview.dismissed_for_path = None;
        let generation = self.image_preview.reset_for_new_path(path, stamp);
        cx.notify();

        self.request_image_load(resolved, generation, cx);
    }

    pub fn reload_image_preview(&mut self, cx: &mut Context<Self>) {
        if self.image_preview.loading {
            return;
        }
        let Some(path) = self.image_preview.path.clone() else {
            return;
        };
        let resolved = resolve_path(&path);
        let stamp = read_file_stamp(&resolved);
        let generation = self.image_preview.prepare_reload(stamp);
        cx.notify();

        self.request_image_load(resolved, generation, cx);
    }

    pub fn toggle_image_preview(&mut self, cx: &mut Context<Self>) {
        if self.image_preview.open {
            if let Some(active_img) = active_image_path(&self.app) {
                self.image_preview.dismissed_for_path = Some(active_img);
            }
            self.image_preview.open = false;
            self.image_preview.pinned = false;
            cx.notify();
            return;
        }

        if let Some(active_img) = active_image_path(&self.app) {
            self.open_image_path(active_img, ImageViewMode::Full, false, cx);
            return;
        }

        if let Some(last_img) = self.image_preview.path.clone() {
            self.open_image_path(last_img, ImageViewMode::Split, true, cx);
            return;
        }

        self.image_preview.open = true;
        self.image_preview.mode = ImageViewMode::Split;
        self.image_preview.pinned = true;
        self.image_preview.loading = false;
        self.image_preview.error = None;
        cx.notify();
    }

    pub fn toggle_image_view_mode(&mut self, cx: &mut Context<Self>) {
        if !self.image_preview.open {
            return;
        }
        match self.image_preview.mode {
            ImageViewMode::Full => {
                self.image_preview.mode = ImageViewMode::Split;
                self.image_preview.pinned = true;
            }
            ImageViewMode::Split => {
                self.image_preview.mode = ImageViewMode::Full;
                if active_image_path(&self.app).as_deref() == self.image_preview.path.as_deref() {
                    self.image_preview.pinned = false;
                }
            }
        }
        cx.notify();
    }

    pub fn image_adjust_zoom(&mut self, delta: f32, cx: &mut Context<Self>) {
        let next = (self.image_preview.zoom + delta).clamp(MIN_ZOOM, MAX_ZOOM);
        let rounded = (next * 100.0).round() / 100.0;
        if (rounded - self.image_preview.zoom).abs() < 0.001 {
            return;
        }
        self.image_preview.zoom = rounded;
        cx.notify();
    }

    pub fn image_reset_zoom(&mut self, cx: &mut Context<Self>) {
        self.image_preview.zoom = 1.0;
        self.image_preview.fit_mode = ImageFitMode::Fit;
        self.image_preview.scroll.set_offset(point(px(0.0), px(0.0)));
        cx.notify();
    }

    pub fn image_set_actual_size(&mut self, cx: &mut Context<Self>) {
        self.image_preview.zoom = 1.0;
        self.image_preview.fit_mode = ImageFitMode::ActualSize;
        self.image_preview.scroll.set_offset(point(px(0.0), px(0.0)));
        cx.notify();
    }

    pub fn image_toggle_fit_mode(&mut self, cx: &mut Context<Self>) {
        self.image_preview.fit_mode = match self.image_preview.fit_mode {
            ImageFitMode::Fit => ImageFitMode::ActualSize,
            ImageFitMode::ActualSize => ImageFitMode::Fit,
        };
        self.image_preview.scroll.set_offset(point(px(0.0), px(0.0)));
        cx.notify();
    }

    pub fn image_cycle_background(&mut self, cx: &mut Context<Self>) {
        self.image_preview.canvas_bg = self.image_preview.canvas_bg.next();
        cx.notify();
    }

    pub fn image_scroll_by(&mut self, delta_x: f32, delta_y: f32, cx: &mut Context<Self>) {
        let cur = self.image_preview.scroll.offset();
        let max_off = self.image_preview.scroll.max_offset();
        let cur_x = (-f32::from(cur.x)).max(0.0);
        let cur_y = (-f32::from(cur.y)).max(0.0);
        let max_x = f32::from(max_off.width).max(0.0);
        let max_y = f32::from(max_off.height).max(0.0);
        let next_x = (cur_x + delta_x).clamp(0.0, max_x);
        let next_y = (cur_y + delta_y).clamp(0.0, max_y);
        self.image_preview
            .scroll
            .set_offset(point(px(-next_x), px(-next_y)));
        cx.notify();
    }

    /// Handles zoom, fit, pan, background, and reload keys when image preview is active in Full mode.
    pub fn handle_image_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        if !self.image_preview.is_full_mode() {
            return false;
        }
        if ks.modifiers.alt || ks.modifiers.platform {
            return false;
        }

        if ks.modifiers.control {
            return match ks.key.as_str() {
                "=" | "+" => {
                    self.image_adjust_zoom(ZOOM_STEP, cx);
                    true
                }
                "-" => {
                    self.image_adjust_zoom(-ZOOM_STEP, cx);
                    true
                }
                "0" => {
                    self.image_reset_zoom(cx);
                    true
                }
                _ => false,
            };
        }

        match ks.key.as_str() {
            "+" | "=" => {
                self.image_adjust_zoom(ZOOM_STEP, cx);
                true
            }
            "-" => {
                self.image_adjust_zoom(-ZOOM_STEP, cx);
                true
            }
            "0" => {
                self.image_reset_zoom(cx);
                true
            }
            "1" => {
                self.image_set_actual_size(cx);
                true
            }
            "f" => {
                self.image_toggle_fit_mode(cx);
                true
            }
            "b" => {
                self.image_cycle_background(cx);
                true
            }
            "r" => {
                self.reload_image_preview(cx);
                true
            }
            "down" | "j" => {
                self.image_scroll_by(0.0, KEYBOARD_PAN_STEP_PX, cx);
                true
            }
            "up" | "k" => {
                self.image_scroll_by(0.0, -KEYBOARD_PAN_STEP_PX, cx);
                true
            }
            "right" | "l" => {
                self.image_scroll_by(KEYBOARD_PAN_STEP_PX, 0.0, cx);
                true
            }
            "left" | "h" => {
                self.image_scroll_by(-KEYBOARD_PAN_STEP_PX, 0.0, cx);
                true
            }
            // Preserve Escape and function keys for global/UI actions, but swallow
            // plain editing/navigation keys so they cannot mutate the hidden binary buffer.
            "escape" => false,
            key if key.starts_with('f') && key[1..].parse::<u8>().is_ok() => false,
            "backspace" | "delete" | "enter" | "space" | "home" | "end" | "pageup"
            | "pagedown" => true,
            _ => ks.key_char.is_some() || ks.key.chars().count() == 1,
        }
    }

    /// Ensures at most one image decode job runs at a time; newer path requests coalesce
    /// into a single pending slot instead of spawning unbounded parallel 256 MiB decodes.
    fn request_image_load(&mut self, path: PathBuf, generation: u64, cx: &mut Context<Self>) {
        if self.image_preview.active_load {
            self.image_preview.pending_load = Some((path, generation));
            return;
        }
        self.image_preview.active_load = true;
        self.spawn_image_load(path, generation, cx);
    }

    fn spawn_image_load(&self, path: PathBuf, generation: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |view, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { load_image_from_disk(&path) })
                .await;

            let _ = view.update(cx, |this, cx| {
                this.image_preview.active_load = false;
                let applied = this.image_preview.apply_load_result(generation, result);
                if let Some((next_path, next_gen)) = this.image_preview.pending_load.take() {
                    if next_gen == this.image_preview.generation {
                        this.request_image_load(next_path, next_gen, cx);
                    }
                }
                if applied {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

/// Compact top banner shown when viewing a raw image buffer so the user can reopen Image Preview.
pub fn render_toggle(view: &GtuimView, cx: &mut Context<GtuimView>) -> AnyElement {
    let p = gui_palette(&view.app.theme);
    div()
        .id("image-preview-toggle")
        .debug_selector(|| "image-preview-toggle".into())
        .w_full()
        .h(px(28.0))
        .px_3()
        .bg(rgb(p.bg_panel))
        .border_b_1()
        .border_color(rgb(p.border_subtle))
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(icon("image", 13.0, p.accent))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(rgb(p.text_muted))
                        .child("Viewing raw image buffer"),
                ),
        )
        .child(
            div()
                .id("image-preview-reopen-btn")
                .px_2()
                .py(px(2.0))
                .rounded(px(4.0))
                .bg(rgb(p.bg_selected))
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(p.accent))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_image_preview(cx)))
                .child("Open Image Preview · Ctrl+Shift+I"),
        )
        .into_any_element()
}

fn toolbar_icon_btn(
    id: &'static str,
    selector: &'static str,
    icon_name: &'static str,
    enabled: bool,
    danger: bool,
    p: super::icons::GuiPalette,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let fg = if !enabled {
        blend_u32(p.bg_panel, p.text_muted, 45)
    } else {
        p.text
    };
    let hover_bg = if danger {
        blend_u32(p.bg_panel, p.error, 22)
    } else {
        p.bg_hover
    };
    div()
        .id(id)
        .debug_selector(move || selector.into())
        .size(px(24.0))
        .rounded(px(4.0))
        .flex()
        .items_center()
        .justify_center()
        .when(enabled, |d| {
            d.cursor_pointer()
                .hover(move |s| s.bg(rgb(hover_bg)))
                .on_click(handler)
        })
        .child(icon(icon_name, 13.0, fg))
        .into_any_element()
}

fn image_pane_share(
    center_width: f32,
    image_split: bool,
    pdf_split: bool,
    markdown: bool,
) -> f32 {
    let panes = 1 + usize::from(image_split) + usize::from(pdf_split) + usize::from(markdown);
    (center_width / panes as f32).max(1.0)
}

fn measure_image_viewport(view: &GtuimView, window: &Window) -> (f32, f32) {
    let viewport = window.viewport_size();
    let workspace = &view.app.workspace;
    let layout = super::compute_effective_workspace_layout(
        f32::from(viewport.width),
        f32::from(viewport.height),
        view.app.mode == Mode::Zen,
        workspace.sidebar_visible,
        workspace.drawer_visible,
        workspace.vertical_terminal_visible,
        workspace.tab_bar_visible,
        0,
        view.sidebar_px,
        view.drawer_px,
        view.vterm_px,
        workspace.focus,
    );
    let pane_w = image_pane_share(
        layout.editor_avail_w.max(1.0),
        view.image_preview.is_split_mode(),
        view.pdf_preview.is_split_mode(),
        view.markdown_preview.open,
    );
    let toolbar_and_footer_h = 34.0 + 22.0;
    let avail_h = (layout.editor_avail_h - toolbar_and_footer_h).max(120.0);
    let scroll_bounds = view.image_preview.scroll.bounds();
    let measured_h = f32::from(scroll_bounds.size.height);
    let viewport_h = if measured_h >= 80.0 && measured_h <= f32::from(viewport.height) {
        measured_h
    } else {
        avail_h
    };
    (pane_w, viewport_h)
}

pub fn render(view: &GtuimView, window: &Window, cx: &mut Context<GtuimView>) -> AnyElement {
    let p = gui_palette(&view.app.theme);
    let state = &view.image_preview;
    let is_split = state.is_split_mode();
    let has_path = state.path.is_some();
    let (pane_w, viewport_h) = measure_image_viewport(view, window);

    let can_zoom_out = state.zoom > MIN_ZOOM + 0.01;
    let can_zoom_in = state.zoom < MAX_ZOOM - 0.01;

    let filename = state
        .path
        .as_deref()
        .and_then(|s| Path::new(s).file_name().and_then(|n| n.to_str()))
        .unwrap_or("Image Preview")
        .to_string();

    let zoom_label = format!("{}%", (state.zoom * 100.0).round() as u32);
    let fit_label = match state.fit_mode {
        ImageFitMode::Fit => "Fit",
        ImageFitMode::ActualSize => "1:1",
    };

    let compact = pane_w < 460.0;
    let show_meta = pane_w >= 580.0;
    let show_filename = pane_w >= 360.0;
    let show_bg_toggle = pane_w >= 520.0;

    let canvas_bg_hex = match state.canvas_bg {
        ImageCanvasBg::Theme => p.bg_rail,
        ImageCanvasBg::Light => 0xf3f4f6,
        ImageCanvasBg::Dark => 0x111318,
    };

    let toolbar = div()
        .id("image-preview-toolbar")
        .debug_selector(|| "image-preview-toolbar".into())
        .h(px(34.0))
        .w_full()
        .flex_shrink_0()
        .overflow_hidden()
        .px(px(if compact { 6.0 } else { 10.0 }))
        .bg(rgb(p.bg_panel))
        .border_b_1()
        .border_color(rgb(p.border))
        .flex()
        .items_center()
        .justify_between()
        .gap(px(if compact { 4.0 } else { 8.0 }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(5.0))
                .min_w_0()
                .when(show_filename, |d| d.flex_1())
                .overflow_hidden()
                .child(icon("image", 13.5, p.accent))
                .when(show_filename, |d| {
                    d.child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(p.text_strong))
                            .child(filename),
                    )
                })
                .when_some(show_meta.then(|| state.metadata()).flatten(), |d, meta| {
                    d.child(
                        div()
                            .px(px(5.0))
                            .py(px(1.0))
                            .rounded(px(3.0))
                            .flex_shrink_0()
                            .bg(rgb(p.bg_elevated))
                            .text_size(px(10.0))
                            .text_color(rgb(p.accent))
                            .child(meta.kind.label()),
                    )
                    .child(
                        div()
                            .px(px(5.0))
                            .py(px(1.0))
                            .rounded(px(3.0))
                            .flex_shrink_0()
                            .bg(rgb(p.bg_elevated))
                            .text_size(px(10.0))
                            .text_color(rgb(p.text_muted))
                            .child(format!(
                                "{}×{} · {}",
                                meta.width_px,
                                meta.height_px,
                                format_byte_size(meta.file_bytes)
                            )),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(if compact { 2.0 } else { 4.0 }))
                .flex_shrink_0()
                .child(toolbar_icon_btn(
                    "image-zoom-out",
                    "image-zoom-out",
                    "minus",
                    can_zoom_out,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| this.image_adjust_zoom(-ZOOM_STEP, cx)),
                ))
                .child(
                    div()
                        .id("image-zoom-reset")
                        .debug_selector(|| "image-zoom-reset".into())
                        .px(px(if compact { 4.0 } else { 8.0 }))
                        .py(px(2.0))
                        .rounded(px(4.0))
                        .bg(rgb(p.bg_input))
                        .border_1()
                        .border_color(rgb(p.border_subtle))
                        .text_size(px(11.0))
                        .text_color(rgb(p.text))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(p.bg_hover)))
                        .on_click(cx.listener(|this, _, _, cx| this.image_reset_zoom(cx)))
                        .child(zoom_label),
                )
                .child(toolbar_icon_btn(
                    "image-zoom-in",
                    "image-zoom-in",
                    "plus",
                    can_zoom_in,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| this.image_adjust_zoom(ZOOM_STEP, cx)),
                ))
                .when(!compact, |d| {
                    d.child(
                        div()
                            .id("image-fit-toggle")
                            .debug_selector(|| "image-fit-toggle".into())
                            .px_2()
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(rgb(if state.fit_mode == ImageFitMode::Fit {
                                p.bg_selected
                            } else {
                                p.bg_elevated
                            }))
                            .text_size(px(11.0))
                            .text_color(rgb(if state.fit_mode == ImageFitMode::Fit {
                                p.accent
                            } else {
                                p.text
                            }))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(p.bg_hover)))
                            .on_click(cx.listener(|this, _, _, cx| this.image_toggle_fit_mode(cx)))
                            .child(fit_label),
                    )
                })
                .when(show_bg_toggle, |d| {
                    d.child(
                        div()
                            .id("image-bg-toggle")
                            .debug_selector(|| "image-bg-toggle".into())
                            .px_2()
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(rgb(p.bg_elevated))
                            .text_size(px(11.0))
                            .text_color(rgb(p.text_muted))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(p.bg_hover)))
                            .on_click(cx.listener(|this, _, _, cx| this.image_cycle_background(cx)))
                            .child(state.canvas_bg.label()),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(if compact { 1.0 } else { 4.0 }))
                .flex_shrink_0()
                .child(toolbar_icon_btn(
                    "image-reload",
                    "image-reload",
                    "refresh",
                    has_path,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| this.reload_image_preview(cx)),
                ))
                .child(toolbar_icon_btn(
                    "image-mode-toggle",
                    "image-mode-toggle",
                    if is_split {
                        "maximize"
                    } else {
                        "split-vertical"
                    },
                    true,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| this.toggle_image_view_mode(cx)),
                ))
                .child(toolbar_icon_btn(
                    "image-close",
                    "image-close",
                    "x",
                    true,
                    true,
                    p,
                    cx.listener(|this, _, _, cx| this.toggle_image_preview(cx)),
                )),
        );

    let body: AnyElement = if let Some(err) = state.error.as_ref() {
        div()
            .id("image-preview-error")
            .debug_selector(|| "image-preview-error".into())
            .flex_1()
            .w_full()
            .p_6()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                div()
                    .max_w(px(480.0))
                    .p_4()
                    .rounded(px(8.0))
                    .bg(rgb(p.bg_panel))
                    .border_1()
                    .border_color(rgb(p.error))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(p.error))
                            .child("Unable to preview image"),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(rgb(p.text))
                            .whitespace_normal()
                            .child(err.clone()),
                    )
                    .child(
                        div()
                            .pt_1()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id("image-error-retry")
                                    .px_3()
                                    .py_1()
                                    .rounded(px(5.0))
                                    .bg(rgb(p.accent))
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(p.accent_fg))
                                    .cursor_pointer()
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.reload_image_preview(cx)),
                                    )
                                    .child("Reload"),
                            )
                            .child(
                                div()
                                    .id("image-error-close")
                                    .px_3()
                                    .py_1()
                                    .rounded(px(5.0))
                                    .bg(rgb(p.bg_elevated))
                                    .text_size(px(11.5))
                                    .text_color(rgb(p.text))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(p.bg_hover)))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.toggle_image_preview(cx)),
                                    )
                                    .child("Close Preview"),
                            ),
                    ),
            )
            .into_any_element()
    } else if state.path.is_none() {
        div()
            .id("image-preview-empty")
            .debug_selector(|| "image-preview-empty".into())
            .flex_1()
            .w_full()
            .p_6()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .child(icon("image", 28.0, p.text_muted))
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(p.text_strong))
                    .child("No image open"),
            )
            .child(
                div()
                    .max_w(px(360.0))
                    .text_size(px(12.0))
                    .text_color(rgb(p.text_muted))
                    .whitespace_normal()
                    .child("Open any .png, .jpg, or .jpeg file from the Explorer or editor to preview it here."),
            )
            .into_any_element()
    } else if let Some(loaded) = state.loaded.as_ref() {
        let (display_w, display_h) = compute_display_size(
            loaded.metadata.width_px,
            loaded.metadata.height_px,
            pane_w,
            viewport_h,
            state.zoom,
            state.fit_mode,
        );
        let stage_w = (display_w + CANVAS_PAD_PX * 2.0).max(pane_w);
        let stage_h = (display_h + CANVAS_PAD_PX * 2.0).max(viewport_h);

        let image_card = div()
            .id("image-preview-surface")
            .debug_selector(|| "image-preview-surface".into())
            .w(px(display_w))
            .h(px(display_h))
            .flex_shrink_0()
            .rounded(px(4.0))
            .border_1()
            .border_color(rgb(p.border))
            .overflow_hidden()
            .child(
                gpui::img(loaded.image.clone())
                    .w(px(display_w))
                    .h(px(display_h))
                    .object_fit(ObjectFit::Contain),
            );

        div()
            .id("image-preview-scroll")
            .debug_selector(|| "image-preview-scroll".into())
            .flex_1()
            .min_h_0()
            .min_w_0()
            .w_full()
            .overflow_scroll()
            .track_scroll(&state.scroll)
            .bg(rgb(canvas_bg_hex))
            .child(
                div()
                    .w(px(stage_w))
                    .h(px(stage_h))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(image_card),
            )
            .into_any_element()
    } else {
        div()
            .id("image-preview-loading")
            .debug_selector(|| "image-preview-loading".into())
            .flex_1()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.5))
            .text_color(rgb(p.text_muted))
            .child("Loading image...")
            .into_any_element()
    };

    let footer = div()
        .h(px(22.0))
        .w_full()
        .flex_shrink_0()
        .px_3()
        .bg(rgb(p.bg_surface))
        .border_t_1()
        .border_color(rgb(p.border_subtle))
        .flex()
        .items_center()
        .justify_between()
        .text_size(px(10.5))
        .text_color(rgb(p.text_muted))
        .child(if state.is_full_mode() {
            "+/-: Zoom · 0: Reset · 1: 1:1 · f: Fit · b: Bg · r: Reload · Ctrl+Shift+I: Raw"
        } else {
            "Split Image Preview · Auto-reloads on change · Ctrl+Shift+I: Close"
        })
        .when(state.loading && state.loaded.is_some(), |d| {
            d.child(div().text_color(rgb(p.accent)).child("Reloading..."))
        });

    let mut container = div()
        .id("image-preview-pane")
        .debug_selector(|| "image-preview-pane".into())
        .h_full()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(rgb(p.bg_rail))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                window.focus(&this.root_focus);
                this.app.workspace.focus = crate::tui::app::FocusRegion::Editor;
                cx.notify();
            }),
        );

    if is_split {
        container = container
            .flex_1()
            .overflow_hidden()
            .border_l_1()
            .border_color(rgb(p.border));
    } else {
        container = container.flex_1().overflow_hidden();
    }

    container
        .child(toolbar)
        .child(body)
        .child(footer)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{size, TestAppContext};
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    struct TempFileGuard(PathBuf);
    impl Drop for TempFileGuard {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn encode_test_png(width: u32, height: u32) -> Vec<u8> {
        let img = RgbaImage::from_pixel(width, height, Rgba([40, 140, 220, 200]));
        let mut buf = Vec::new();
        DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut buf), DecodedFormat::Png)
            .expect("encode test png");
        buf
    }

    fn encode_test_jpeg(width: u32, height: u32) -> Vec<u8> {
        let img = RgbImage::from_pixel(width, height, Rgb([210, 90, 60]));
        let mut buf = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut Cursor::new(&mut buf), DecodedFormat::Jpeg)
            .expect("encode test jpeg");
        buf
    }

    #[::core::prelude::v1::test]
    fn recognizes_png_and_jpeg_extensions_case_insensitively() {
        assert!(is_image_path("photo.png"));
        assert!(is_image_path("/tmp/dir/PHOTO.PNG"));
        assert!(is_image_path("shot.jpg"));
        assert!(is_image_path("shot.JPEG"));
        assert!(is_image_path("assets/banner.Jpg"));
        assert!(!is_image_path("doc.pdf"));
        assert!(!is_image_path("notes.md"));
        assert!(!is_image_path("src/main.rs"));
        assert!(!is_image_path(""));
    }

    #[::core::prelude::v1::test]
    fn decodes_valid_png_and_jpeg_and_rejects_corrupt_or_unsupported_files() {
        let png = decode_and_validate_image_bytes(encode_test_png(320, 180)).expect("valid png");
        assert_eq!(png.metadata.kind, ImageKind::Png);
        assert_eq!((png.metadata.width_px, png.metadata.height_px), (320, 180));
        assert_eq!(png.gpui_format, gpui::ImageFormat::Png);

        let jpg = decode_and_validate_image_bytes(encode_test_jpeg(400, 250)).expect("valid jpeg");
        assert_eq!(jpg.metadata.kind, ImageKind::Jpeg);
        assert_eq!((jpg.metadata.width_px, jpg.metadata.height_px), (400, 250));
        assert_eq!(jpg.gpui_format, gpui::ImageFormat::Jpeg);

        assert!(matches!(
            decode_and_validate_image_bytes(Vec::new()),
            Err(ImageError::DecodeFailed(_))
        ));
        assert!(matches!(
            decode_and_validate_image_bytes(b"not an image at all".to_vec()),
            Err(ImageError::UnsupportedFormat(_))
        ));
        // Valid PNG signature followed by truncated/corrupt body must fail decoding.
        let mut corrupt_png = b"\x89PNG\r\n\x1a\n".to_vec();
        corrupt_png.extend_from_slice(&[0, 1, 2, 3, 4, 5]);
        assert!(matches!(
            decode_and_validate_image_bytes(corrupt_png),
            Err(ImageError::DecodeFailed(_))
        ));

        let missing = load_image_from_disk(Path::new("/tmp/gtuim-nonexistent-image-404.png"));
        assert!(matches!(missing, Err(ImageError::NotFound(_))));
    }

    #[::core::prelude::v1::test]
    fn bounded_read_caps_allocation_at_limit_plus_one() {
        // Exact limit succeeds.
        let exact = read_bounded_bytes(Cursor::new(vec![7u8; 16]), 16).expect("at limit");
        assert_eq!(exact.len(), 16);

        // Oversized stream stops after reading limit + 1 bytes and returns TooLarge.
        let mut cursor = Cursor::new(vec![9u8; 1024]);
        let err = read_bounded_bytes(&mut cursor, 16).unwrap_err();
        assert_eq!(
            err,
            ImageError::TooLarge {
                size_bytes: 17,
                limit_bytes: 16,
            }
        );
        assert_eq!(
            cursor.position(),
            17,
            "reader must stop at limit + 1 bytes without consuming the rest"
        );
    }

    #[::core::prelude::v1::test]
    fn display_sizing_fits_large_images_without_upscaling_small_icons() {
        // Large 2000x1000 photo in 648x448 pane (avail 600x400) -> scales to 600x300.
        let (w, h) = compute_display_size(2000, 1000, 648.0, 448.0, 1.0, ImageFitMode::Fit);
        assert!((w - 600.0).abs() < 0.01);
        assert!((h - 300.0).abs() < 0.01);

        // Tall 1000x2000 photo in 648x448 pane (avail 600x400) -> constrained by height -> 200x400.
        let (tw, th) = compute_display_size(1000, 2000, 648.0, 448.0, 1.0, ImageFitMode::Fit);
        assert!((tw - 200.0).abs() < 0.01);
        assert!((th - 400.0).abs() < 0.01);

        // Small 64x48 icon stays at native 64x48 in Fit mode at 100% zoom.
        let (sw, sh) = compute_display_size(64, 48, 648.0, 448.0, 1.0, ImageFitMode::Fit);
        assert!((sw - 64.0).abs() < 0.01);
        assert!((sh - 48.0).abs() < 0.01);

        // ActualSize ignores viewport constraints and applies zoom directly.
        let (aw, ah) = compute_display_size(800, 600, 300.0, 200.0, 1.5, ImageFitMode::ActualSize);
        assert!((aw - 1200.0).abs() < 0.01);
        assert!((ah - 900.0).abs() < 0.01);
    }

    #[::core::prelude::v1::test]
    fn stale_async_load_result_is_ignored() {
        let mut state = ImagePreview::default();
        let gen1 = state.reset_for_new_path("/tmp/first.png".into(), None);
        let gen2 = state.reset_for_new_path("/tmp/second.jpg".into(), None);
        let png = decode_and_validate_image_bytes(encode_test_png(32, 32)).unwrap();
        let jpg = decode_and_validate_image_bytes(encode_test_jpeg(64, 48)).unwrap();

        assert!(!state.apply_load_result(gen1, Ok(png)));
        assert!(state.loaded.is_none());
        assert!(state.apply_load_result(gen2, Ok(jpg)));
        assert_eq!(
            state.metadata().map(|m| (m.width_px, m.height_px, m.kind)),
            Some((64, 48, ImageKind::Jpeg))
        );
    }

    #[gpui::test]
    fn png_and_jpeg_tabs_auto_open_full_preview_and_support_zoom_fit_split_and_raw_toggle(
        cx: &mut TestAppContext,
    ) {
        let dir = std::env::temp_dir();
        let pid = std::process::id();
        let png_path = dir.join(format!("gtuim-img-test-{pid}.png"));
        let jpg_path = dir.join(format!("gtuim-img-test-{pid}.jpeg"));
        fs::write(&png_path, encode_test_png(480, 320)).unwrap();
        fs::write(&jpg_path, encode_test_jpeg(640, 400)).unwrap();
        let _g1 = TempFileGuard(png_path.clone());
        let _g2 = TempFileGuard(jpg_path.clone());

        let (view, window) = cx.add_window_view(|_, cx| {
            let mut view = super::super::modal_tests::fixture(cx);
            view.app.extension_shop.is_open = false;
            view.app.tabs[0].name = png_path.to_string_lossy().to_string();
            view.sync_image_preview(cx);
            view
        });
        window.simulate_resize(size(px(1100.0), px(720.0)));
        window.run_until_parked();

        view.read_with(window, |this, _| {
            assert!(this.image_preview.is_full_mode());
            assert!(this.image_preview.error.is_none(), "err: {:?}", this.image_preview.error);
            let meta = this.image_preview.metadata().expect("loaded png metadata");
            assert_eq!((meta.width_px, meta.height_px, meta.kind), (480, 320, ImageKind::Png));
        });
        assert!(window.debug_bounds("image-preview-pane").is_some());
        assert!(window.debug_bounds("image-preview-surface").is_some());
        assert!(window.debug_bounds("nvim-editor-pane").is_none());

        // Test keyboard zoom, 1:1, background cycle, and reset in Full mode.
        view.update(window, |this, cx| {
            let plus = Keystroke::parse("+").unwrap();
            assert!(this.handle_image_key(&plus, cx));
            assert!((this.image_preview.zoom - 1.25).abs() < 0.001);

            let one = Keystroke::parse("1").unwrap();
            assert!(this.handle_image_key(&one, cx));
            assert_eq!(this.image_preview.fit_mode, ImageFitMode::ActualSize);
            assert!((this.image_preview.zoom - 1.0).abs() < 0.001);

            let bg = Keystroke::parse("b").unwrap();
            assert!(this.handle_image_key(&bg, cx));
            assert_eq!(this.image_preview.canvas_bg, ImageCanvasBg::Light);

            let reset = Keystroke::parse("0").unwrap();
            assert!(this.handle_image_key(&reset, cx));
            assert_eq!(this.image_preview.fit_mode, ImageFitMode::Fit);
            assert!((this.image_preview.zoom - 1.0).abs() < 0.001);
        });
        window.run_until_parked();

        // Switch to Split mode and confirm editor + image preview coexist side-by-side.
        view.update(window, |this, cx| this.toggle_image_view_mode(cx));
        window.run_until_parked();
        let editor = window
            .debug_bounds("nvim-editor-pane")
            .expect("editor visible in split mode");
        let preview = window
            .debug_bounds("image-preview-pane")
            .expect("image preview visible in split mode");
        assert!(editor.right() <= preview.left() + px(1.0));

        // Return to Full mode and switch active tab to JPEG file.
        view.update(window, |this, cx| {
            this.toggle_image_view_mode(cx);
            this.app.tabs[0].name = jpg_path.to_string_lossy().to_string();
            this.sync_image_preview(cx);
        });
        window.run_until_parked();
        view.read_with(window, |this, _| {
            assert!(this.image_preview.is_full_mode());
            let meta = this.image_preview.metadata().expect("loaded jpeg metadata");
            assert_eq!((meta.width_px, meta.height_px, meta.kind), (640, 400, ImageKind::Jpeg));
        });

        // Toggle off -> raw buffer with reopen banner; switching to non-image tab closes preview.
        view.update(window, |this, cx| this.toggle_image_preview(cx));
        window.run_until_parked();
        view.read_with(window, |this, _| {
            assert!(!this.image_preview.open);
            assert_eq!(
                this.image_preview.dismissed_for_path.as_deref(),
                jpg_path.to_str()
            );
        });
        let toggle_bar = window
            .debug_bounds("image-preview-toggle")
            .expect("raw image reopen banner appears after closing preview");
        assert!(toggle_bar.size.height > px(20.0));

        // Reopen preview and verify auto-reload on disk modification.
        view.update(window, |this, cx| {
            this.toggle_image_preview(cx);
        });
        window.run_until_parked();
        view.read_with(window, |this, _| assert!(this.image_preview.is_full_mode()));

        fs::write(&jpg_path, encode_test_jpeg(800, 500)).unwrap();
        view.update(window, |this, cx| {
            this.image_preview.last_reload_check =
                Some(Instant::now() - RELOAD_CHECK_INTERVAL - Duration::from_millis(50));
            this.sync_image_preview(cx);
        });
        window.run_until_parked();
        view.read_with(window, |this, _| {
            let meta = this.image_preview.metadata().expect("reloaded jpeg metadata");
            assert_eq!((meta.width_px, meta.height_px, meta.kind), (800, 500, ImageKind::Jpeg));
        });

        // Switching to a non-image tab auto-closes unpinned Full preview.
        view.update(window, |this, cx| {
            this.app.tabs[0].name = "src/main.rs".into();
            this.sync_image_preview(cx);
        });
        window.run_until_parked();
        view.read_with(window, |this, _| assert!(!this.image_preview.open));
    }

    #[gpui::test]
    fn corrupt_image_renders_error_card_without_panicking(cx: &mut TestAppContext) {
        let path = std::env::temp_dir().join(format!("gtuim-corrupt-{}.png", std::process::id()));
        fs::write(&path, b"this is definitely not a png").unwrap();
        let _guard = TempFileGuard(path.clone());

        let (view, window) = cx.add_window_view(|_, cx| {
            let mut view = super::super::modal_tests::fixture(cx);
            view.app.extension_shop.is_open = false;
            view.app.tabs[0].name = path.to_string_lossy().to_string();
            view.sync_image_preview(cx);
            view
        });
        window.simulate_resize(size(px(1000.0), px(680.0)));
        window.run_until_parked();

        view.read_with(window, |this, _| {
            assert!(this.image_preview.open);
            assert!(this.image_preview.loaded.is_none());
            assert!(this.image_preview.error.is_some());
        });
        assert!(window.debug_bounds("image-preview-error").is_some());
    }

    #[gpui::test]
    fn image_preview_command_is_in_palette_and_routes_to_gui(cx: &mut TestAppContext) {
        assert!(crate::tui::workspace::COMMANDS
            .iter()
            .any(|command| command.id == "toggle_image_preview"));
        let (_, window) = cx.add_window_view(|_, cx| {
            let mut view = super::super::modal_tests::fixture(cx);
            crate::tui::events::execute_command(&mut view.app, "toggle_image_preview");
            assert!(view.app.toggle_image_preview_requested);
            view
        });
        window.run_until_parked();
    }

    #[gpui::test]
    fn full_image_over_pinned_full_pdf_blocks_hidden_pdf_keys(cx: &mut TestAppContext) {
        use super::super::pdf_preview::{PdfViewMode, RenderedPng};

        let png_bytes = encode_test_png(200, 120);
        let png_path =
            std::env::temp_dir().join(format!("gtuim-img-over-pdf-{}.png", std::process::id()));
        fs::write(&png_path, &png_bytes).unwrap();
        let _guard = TempFileGuard(png_path.clone());

        let (view, window) = cx.add_window_view(|_, cx| {
            let mut view = super::super::modal_tests::fixture(cx);
            view.app.extension_shop.is_open = false;
            view.app.workspace.focus = crate::tui::app::FocusRegion::Editor;

            // Seed a pinned Full-mode multi-page PDF at page 2.
            view.pdf_preview.seed_test_page(
                "/tmp/pinned.pdf",
                4,
                RenderedPng {
                    bytes: png_bytes.clone(),
                    width_px: 200,
                    height_px: 120,
                },
            );
            view.pdf_preview.mode = PdfViewMode::Full;
            view.pdf_preview.pinned = true;
            view.pdf_preview.current_page = 2;

            // Open Full-mode image preview over the pinned Full PDF.
            view.open_image_path(
                png_path.to_string_lossy().to_string(),
                ImageViewMode::Full,
                false,
                cx,
            );
            view
        });
        window.simulate_resize(size(px(1000.0), px(700.0)));
        window.run_until_parked();

        // Dispatch PDF-only navigation keys through the root key handler while Full image is on top.
        view.update_in(window, |this, window, cx| {
            assert!(this.image_preview.is_full_mode());
            assert!(this.pdf_preview.is_full_mode());
            assert_eq!(this.pdf_preview.current_page, 2);
            for key in ["n", "space", "home", "end", "pagedown", "pageup"] {
                let ev = gpui::KeyDownEvent {
                    keystroke: Keystroke::parse(key).unwrap(),
                    is_held: false,
                };
                this.handle_key_down(&ev, window, cx);
                assert_eq!(
                    this.pdf_preview.current_page, 2,
                    "key `{key}` must not mutate hidden Full PDF underneath Full image preview"
                );
            }

            // Image preview keys still work normally.
            let zoom_ev = gpui::KeyDownEvent {
                keystroke: Keystroke::parse("+").unwrap(),
                is_held: false,
            };
            this.handle_key_down(&zoom_ev, window, cx);
            assert!((this.image_preview.zoom - 1.25).abs() < 0.001);

            // Plain editing keys are swallowed in Full image mode while Ctrl shortcuts pass through.
            assert!(this.handle_image_key(&Keystroke::parse("x").unwrap(), cx));
            assert!(this.handle_image_key(&Keystroke::parse("backspace").unwrap(), cx));
            assert!(!this.handle_image_key(&Keystroke::parse("ctrl-p").unwrap(), cx));
            assert!(!this.handle_image_key(&Keystroke::parse("escape").unwrap(), cx));
        });
        window.run_until_parked();
    }

    #[gpui::test]
    fn reload_coalesces_while_loading_and_sync_handles_deletion_and_recreation(
        cx: &mut TestAppContext,
    ) {
        let path =
            std::env::temp_dir().join(format!("gtuim-img-del-recreate-{}.png", std::process::id()));
        fs::write(&path, encode_test_png(120, 80)).unwrap();
        let _guard = TempFileGuard(path.clone());

        let (view, window) = cx.add_window_view(|_, cx| {
            let mut view = super::super::modal_tests::fixture(cx);
            view.app.extension_shop.is_open = false;
            view.app.tabs[0].name = path.to_string_lossy().to_string();
            view.sync_image_preview(cx);

            // While initial load is in flight, repeated reloads must be ignored (no parallel decode jobs).
            let gen_while_loading = view.image_preview.generation;
            assert!(view.image_preview.loading && view.image_preview.active_load);
            view.reload_image_preview(cx);
            assert_eq!(view.image_preview.generation, gen_while_loading);
            assert!(view.image_preview.pending_load.is_none());
            view
        });
        window.run_until_parked();

        view.read_with(window, |this, _| {
            assert!(!this.image_preview.loading && !this.image_preview.active_load);
            assert_eq!(
                this.image_preview.metadata().map(|m| (m.width_px, m.height_px)),
                Some((120, 80))
            );
        });

        // Deleting the file transitions to NotFound once without repeated reloads.
        fs::remove_file(&path).unwrap();
        view.update(window, |this, cx| {
            this.image_preview.last_reload_check =
                Some(Instant::now() - RELOAD_CHECK_INTERVAL - Duration::from_millis(50));
            this.sync_image_preview(cx);
        });
        window.run_until_parked();

        let gen_after_delete = view.read_with(window, |this, _| {
            assert!(this.image_preview.loaded.is_none());
            assert!(this.image_preview.file_stamp.is_none());
            assert!(this
                .image_preview
                .error
                .as_deref()
                .is_some_and(|e| e.contains("not found")));
            this.image_preview.generation
        });

        // Subsequent sync while still deleted must NOT trigger another reload.
        view.update(window, |this, cx| {
            this.image_preview.last_reload_check =
                Some(Instant::now() - RELOAD_CHECK_INTERVAL - Duration::from_millis(50));
            this.sync_image_preview(cx);
        });
        window.run_until_parked();
        view.read_with(window, |this, _| {
            assert_eq!(this.image_preview.generation, gen_after_delete);
        });

        // Recreating the file reloads cleanly.
        fs::write(&path, encode_test_png(240, 160)).unwrap();
        view.update(window, |this, cx| {
            this.image_preview.last_reload_check =
                Some(Instant::now() - RELOAD_CHECK_INTERVAL - Duration::from_millis(50));
            this.sync_image_preview(cx);
        });
        window.run_until_parked();
        view.read_with(window, |this, _| {
            assert!(this.image_preview.error.is_none());
            assert_eq!(
                this.image_preview.metadata().map(|m| (m.width_px, m.height_px)),
                Some((240, 160))
            );
        });
    }
}
