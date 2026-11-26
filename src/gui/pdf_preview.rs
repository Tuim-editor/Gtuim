//! Native GPUI PDF preview powered by Poppler (`pdfinfo` + `pdftoppm`).
//!
//! Renders PDF pages lazily and asynchronously on GPUI's background executor
//! into in-memory PNG `gpui::Image` buffers inside a continuously scrollable,
//! virtualized vertical page stack with bounded LRU caching, DPI-tiered zoom,
//! live viewport page tracking, Full/Split modes, and automatic reload on file
//! modification.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use gpui::{
    div, point, prelude::*, px, rgb, AnyElement, App, ClickEvent, Context, FontWeight, IntoElement,
    Keystroke, MouseButton, ObjectFit, ScrollHandle, Window,
};

use super::icons::{gui_palette, icon};
use super::GtuimView;
use crate::tui::app::{App as TuimApp, Mode};
use crate::tui::theme::blend_u32;

const MAX_CACHED_PAGES: usize = 16;
const MAX_PNG_BYTES: usize = 32 * 1024 * 1024;
const RELOAD_CHECK_INTERVAL: Duration = Duration::from_millis(500);
const MIN_ZOOM: f32 = 0.5;
const MAX_ZOOM: f32 = 3.0;
const ZOOM_STEP: f32 = 0.25;
const CANVAS_PAD_PX: f32 = 16.0;
const PAGE_GAP_PX: f32 = 16.0;
const PREFETCH_PAGES: usize = 1;
const MAX_VISIBLE_WINDOW_PAGES: usize = 6;
const MAX_CONCURRENT_RENDERS: usize = 4;
const KEYBOARD_SCROLL_STEP_PX: f32 = 96.0;
const DEFAULT_PAGE_PTS: (f32, f32) = (612.0, 792.0);
const DEFAULT_PANE_WIDTH_PX: f32 = 640.0;
const DEFAULT_VIEWPORT_HEIGHT_PX: f32 = 600.0;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PdfViewMode {
    /// Replaces the editor viewport when viewing a `.pdf` buffer.
    #[default]
    Full,
    /// Side-by-side preview pane to the right of the editor (e.g. alongside `.tex`/`.typ`).
    Split,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PdfFitMode {
    #[default]
    FitWidth,
    ActualSize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PdfMetadata {
    pub page_count: usize,
    pub title: Option<String>,
    pub page_size_pts: Option<(f32, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Clone)]
pub struct RenderedPage {
    pub image: Arc<gpui::Image>,
    pub width_px: u32,
    pub height_px: u32,
    pub dpi: u32,
    last_used: u64,
}

#[derive(Debug)]
pub struct RenderedPng {
    pub bytes: Vec<u8>,
    pub width_px: u32,
    pub height_px: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PdfError {
    MissingTool(String),
    NotFound(String),
    RenderFailed(String),
    InvalidPng(String),
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PdfError::MissingTool(tool) => write!(
                f,
                "PDF preview requires Poppler (`{tool}` was not found in PATH). Install `poppler` and click Reload."
            ),
            PdfError::NotFound(path) => write!(f, "PDF file not found: {path}"),
            PdfError::RenderFailed(msg) => write!(f, "Could not render PDF: {msg}"),
            PdfError::InvalidPng(msg) => write!(f, "Invalid rendered page image: {msg}"),
        }
    }
}

/// Pure, O(1)/O(log N) vertical layout model for continuously stacked PDF pages.
#[derive(Clone, Debug)]
pub struct PageLayout {
    pub page_count: usize,
    pub default_size: (f32, f32),
    overrides: Vec<(usize, (f32, f32))>,
}

impl PageLayout {
    pub fn build(
        page_count: usize,
        default_pts: (f32, f32),
        pts_overrides: &HashMap<usize, (f32, f32)>,
        pane_width_px: f32,
        zoom: f32,
        fit_mode: PdfFitMode,
    ) -> Self {
        let default_size = compute_display_size_from_pts(
            default_pts.0,
            default_pts.1,
            pane_width_px,
            zoom,
            fit_mode,
        );
        let mut overrides: Vec<(usize, (f32, f32))> = pts_overrides
            .iter()
            .filter(|(&page, _)| page >= 1 && page <= page_count)
            .map(|(&page, &(w_pt, h_pt))| {
                (
                    page,
                    compute_display_size_from_pts(w_pt, h_pt, pane_width_px, zoom, fit_mode),
                )
            })
            .collect();
        overrides.sort_by_key(|(page, _)| *page);
        Self {
            page_count,
            default_size,
            overrides,
        }
    }

    pub fn page_size(&self, page: usize) -> (f32, f32) {
        if !self.overrides.is_empty() {
            if let Ok(idx) = self.overrides.binary_search_by_key(&page, |(p, _)| *p) {
                return self.overrides[idx].1;
            }
        }
        self.default_size
    }

    pub fn page_top(&self, page: usize) -> f32 {
        if self.page_count == 0 {
            return CANVAS_PAD_PX;
        }
        let clamped = page.clamp(1, self.page_count);
        let stride = self.default_size.1 + PAGE_GAP_PX;
        let mut top = CANVAS_PAD_PX + ((clamped - 1) as f32) * stride;
        for &(ov_page, (_, ov_h)) in &self.overrides {
            if ov_page < clamped {
                top += ov_h - self.default_size.1;
            } else {
                break;
            }
        }
        top
    }

    pub fn page_bottom(&self, page: usize) -> f32 {
        self.page_top(page) + self.page_size(page).1
    }

    pub fn total_height(&self) -> f32 {
        if self.page_count == 0 {
            return 0.0;
        }
        self.page_bottom(self.page_count) + CANVAS_PAD_PX
    }

    /// Returns the 1-indexed inclusive range of pages intersecting `[scroll_y, scroll_y + viewport_h]`.
    pub fn visible_range(&self, scroll_y: f32, viewport_h: f32) -> (usize, usize) {
        if self.page_count == 0 {
            return (1, 0);
        }
        let view_top = scroll_y.max(0.0);
        let view_bottom = view_top + viewport_h.max(1.0);

        // First page with page_bottom(p) > view_top
        let mut lo = 1usize;
        let mut hi = self.page_count;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.page_bottom(mid) <= view_top {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let vis_start = lo;

        // Last page with page_top(p) < view_bottom
        let mut lo2 = vis_start;
        let mut hi2 = self.page_count;
        while lo2 < hi2 {
            let mid = lo2 + (hi2 - lo2).div_ceil(2);
            if self.page_top(mid) >= view_bottom {
                hi2 = mid - 1;
            } else {
                lo2 = mid;
            }
        }
        let vis_end = lo2.max(vis_start);
        (vis_start, vis_end)
    }

    /// Returns the page that best represents the current viewport (largest visible overlap,
    /// honoring exact top/bottom endpoints).
    pub fn active_page_for_viewport(&self, scroll_y: f32, viewport_h: f32) -> usize {
        if self.page_count <= 1 {
            return self.page_count.max(1);
        }
        let view_top = scroll_y.max(0.0);
        let view_h = viewport_h.max(1.0);
        let view_bottom = view_top + view_h;
        let max_scroll = (self.total_height() - view_h).max(0.0);

        if view_top <= 1.0 {
            return 1;
        }
        if max_scroll > 1.0 && view_top >= max_scroll - 2.0 {
            return self.page_count;
        }

        let (vis_start, vis_end) = self.visible_range(view_top, view_h);
        let focal_y = view_top + view_h * 0.35;
        let mut best_page = vis_start;
        let mut best_overlap = -1.0f32;
        let mut best_dist = f32::INFINITY;

        for page in vis_start..=vis_end {
            let top = self.page_top(page);
            let bottom = top + self.page_size(page).1;
            let overlap = (bottom.min(view_bottom) - top.max(view_top)).max(0.0);
            let center_dist = ((top + bottom) * 0.5 - focal_y).abs();
            if overlap > best_overlap + 0.5
                || ((overlap - best_overlap).abs() <= 0.5 && center_dist < best_dist)
            {
                best_overlap = overlap;
                best_dist = center_dist;
                best_page = page;
            }
        }
        best_page
    }

    /// Returns the bounded 1-indexed inclusive page window to materialize & lazily rasterize.
    pub fn render_window(&self, scroll_y: f32, viewport_h: f32) -> (usize, usize) {
        if self.page_count == 0 {
            return (1, 0);
        }
        let (vis_start, vis_end) = self.visible_range(scroll_y, viewport_h);
        let mut w_start = vis_start.saturating_sub(PREFETCH_PAGES).max(1);
        let mut w_end = (vis_end + PREFETCH_PAGES).min(self.page_count);
        if w_end - w_start + 1 > MAX_VISIBLE_WINDOW_PAGES {
            let active = self.active_page_for_viewport(scroll_y, viewport_h);
            let half = MAX_VISIBLE_WINDOW_PAGES / 2;
            w_start = active.saturating_sub(half).max(1);
            w_end = (w_start + MAX_VISIBLE_WINDOW_PAGES - 1).min(self.page_count);
            w_start = w_end.saturating_sub(MAX_VISIBLE_WINDOW_PAGES - 1).max(1);
        }
        (w_start, w_end)
    }

    /// Exact `(top_spacer, bottom_spacer)` heights so total scroll height stays invariant
    /// regardless of which pages are materialized in `[window_start, window_end]`.
    pub fn spacers(&self, window_start: usize, window_end: usize) -> (f32, f32) {
        if self.page_count == 0 || window_start > window_end {
            return (0.0, 0.0);
        }
        let top_spacer = self.page_top(window_start);
        let bottom_spacer = (self.total_height() - self.page_bottom(window_end)).max(0.0);
        (top_spacer, bottom_spacer)
    }
}

pub struct PdfPreview {
    pub open: bool,
    pub mode: PdfViewMode,
    pub pinned: bool,
    pub path: Option<String>,
    pub dismissed_for_path: Option<String>,
    pub metadata: Option<PdfMetadata>,
    pub current_page: usize,
    pub zoom: f32,
    pub fit_mode: PdfFitMode,
    pub loading: bool,
    pub error: Option<String>,
    pub file_stamp: Option<FileStamp>,
    last_reload_check: Option<Instant>,
    generation: u64,
    touch_seq: u64,
    pages: HashMap<(usize, u32), RenderedPage>,
    inflight: HashSet<(usize, u32)>,
    active_tasks: usize,
    pending_probe: Option<(PathBuf, u64, usize, u32)>,
    page_errors: HashMap<usize, String>,
    page_pts_overrides: HashMap<usize, (f32, f32)>,
    fallback_pages: HashMap<usize, RenderedPage>,
    visible_window: (usize, usize),
    last_pane_width: f32,
    last_viewport_height: f32,
    last_scroll_y: f32,
    pending_scroll_page: Option<usize>,
    sync_scheduled: Cell<bool>,
    pub scroll: ScrollHandle,
}

impl Default for PdfPreview {
    fn default() -> Self {
        Self {
            open: false,
            mode: PdfViewMode::Full,
            pinned: false,
            path: None,
            dismissed_for_path: None,
            metadata: None,
            current_page: 1,
            zoom: 1.0,
            fit_mode: PdfFitMode::FitWidth,
            loading: false,
            error: None,
            file_stamp: None,
            last_reload_check: None,
            generation: 0,
            touch_seq: 0,
            pages: HashMap::new(),
            inflight: HashSet::new(),
            active_tasks: 0,
            pending_probe: None,
            page_errors: HashMap::new(),
            page_pts_overrides: HashMap::new(),
            fallback_pages: HashMap::new(),
            visible_window: (1, 1),
            last_pane_width: DEFAULT_PANE_WIDTH_PX,
            last_viewport_height: DEFAULT_VIEWPORT_HEIGHT_PX,
            last_scroll_y: 0.0,
            pending_scroll_page: None,
            sync_scheduled: Cell::new(false),
            scroll: ScrollHandle::new(),
        }
    }
}

pub fn is_pdf_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
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

pub fn active_pdf_path(app: &TuimApp) -> Option<String> {
    let path = active_tab_path(app)?;
    if is_pdf_path(&path) {
        Some(path)
    } else {
        None
    }
}

/// If the active buffer is a source file (e.g. `.tex`, `.typ`, `.md`), check if a sibling `.pdf` exists.
pub fn companion_pdf_path(path: &str) -> Option<String> {
    if path.is_empty() || is_pdf_path(path) {
        return None;
    }
    let resolved = resolve_path(path);
    let candidate = resolved.with_extension("pdf");
    if candidate.is_file() {
        candidate.to_str().map(|s| s.to_string())
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

/// Maps a zoom factor to a discrete rasterization DPI bucket so slight zoom adjustments
/// hit the cache while higher zoom levels rasterize crisply.
pub fn dpi_for_zoom(zoom: f32) -> u32 {
    if zoom <= 0.85 {
        96
    } else if zoom <= 1.35 {
        144
    } else if zoom <= 2.05 {
        216
    } else {
        288
    }
}

pub fn parse_png_dimensions(bytes: &[u8]) -> Result<(u32, u32), PdfError> {
    if bytes.len() < 24 || &bytes[0..8] != PNG_SIGNATURE {
        return Err(PdfError::InvalidPng("missing PNG header".into()));
    }
    if &bytes[12..16] != b"IHDR" {
        return Err(PdfError::InvalidPng("missing PNG IHDR chunk".into()));
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    if width == 0 || height == 0 {
        return Err(PdfError::InvalidPng("PNG has zero dimensions".into()));
    }
    Ok((width, height))
}

pub fn parse_pdfinfo_output(stdout: &str) -> Result<PdfMetadata, PdfError> {
    let mut page_count: Option<usize> = None;
    let mut title: Option<String> = None;
    let mut page_size_pts: Option<(f32, f32)> = None;
    let mut page_rot: i32 = 0;

    for line in stdout.lines() {
        let Some((key, raw_val)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let val = raw_val.trim();
        match key {
            "Pages" => {
                if let Ok(n) = val.parse::<usize>() {
                    page_count = Some(n);
                }
            }
            "Title" if !val.is_empty() => {
                title = Some(val.to_string());
            }
            "Page rot" => {
                if let Ok(rot) = val.parse::<i32>() {
                    page_rot = rot;
                }
            }
            "Page size" => {
                let tokens: Vec<&str> = val.split_whitespace().collect();
                if tokens.len() >= 3 && tokens[1] == "x" {
                    if let (Ok(w), Ok(h)) = (tokens[0].parse::<f32>(), tokens[2].parse::<f32>()) {
                        if w > 0.0 && h > 0.0 {
                            page_size_pts = Some((w, h));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if let Some((w, h)) = page_size_pts {
        if page_rot.rem_euclid(180) == 90 {
            page_size_pts = Some((h, w));
        }
    }

    let count = page_count.ok_or_else(|| {
        PdfError::RenderFailed("pdfinfo did not report a valid page count".into())
    })?;
    if count == 0 {
        return Err(PdfError::RenderFailed("PDF contains 0 pages".into()));
    }

    Ok(PdfMetadata {
        page_count: count,
        title,
        page_size_pts,
    })
}

fn summarize_stderr(stderr: &[u8], fallback: &str) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|s| s.chars().take(180).collect())
        .unwrap_or_else(|| fallback.to_string())
}

pub fn probe_pdf_info(path: &Path) -> Result<PdfMetadata, PdfError> {
    if !path.exists() {
        return Err(PdfError::NotFound(path.display().to_string()));
    }
    let output = Command::new("pdfinfo")
        .arg("--")
        .arg(path)
        .output()
        .map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                PdfError::MissingTool("pdfinfo".into())
            } else {
                PdfError::RenderFailed(format!("failed to invoke pdfinfo: {err}"))
            }
        })?;
    if !output.status.success() {
        return Err(PdfError::RenderFailed(summarize_stderr(
            &output.stderr,
            "pdfinfo exited with an error",
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_pdfinfo_output(&stdout)
}

pub fn render_pdf_page_png(path: &Path, page: usize, dpi: u32) -> Result<RenderedPng, PdfError> {
    if !path.exists() {
        return Err(PdfError::NotFound(path.display().to_string()));
    }
    let clamped_dpi = dpi.clamp(72, 288);
    let page_num = page.max(1).to_string();
    let dpi_str = clamped_dpi.to_string();

    let output = Command::new("pdftoppm")
        .args([
            "-png",
            "-r",
            &dpi_str,
            "-f",
            &page_num,
            "-l",
            &page_num,
            "-singlefile",
            "--",
        ])
        .arg(path)
        .output()
        .map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                PdfError::MissingTool("pdftoppm".into())
            } else {
                PdfError::RenderFailed(format!("failed to invoke pdftoppm: {err}"))
            }
        })?;

    if !output.status.success() {
        return Err(PdfError::RenderFailed(summarize_stderr(
            &output.stderr,
            "pdftoppm exited with an error",
        )));
    }
    if output.stdout.len() > MAX_PNG_BYTES {
        return Err(PdfError::InvalidPng(format!(
            "rendered page exceeded {} MiB safety limit",
            MAX_PNG_BYTES / (1024 * 1024)
        )));
    }
    let (width_px, height_px) = parse_png_dimensions(&output.stdout)?;
    Ok(RenderedPng {
        bytes: output.stdout,
        width_px,
        height_px,
    })
}

pub fn compute_display_size_from_pts(
    w_pt: f32,
    h_pt: f32,
    pane_width_px: f32,
    zoom: f32,
    fit_mode: PdfFitMode,
) -> (f32, f32) {
    let w = w_pt.max(1.0);
    let h = h_pt.max(1.0);
    let aspect = (h / w).clamp(0.1, 10.0);
    let clamped_zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);

    let target_w = match fit_mode {
        PdfFitMode::FitWidth => {
            let base = (pane_width_px - 48.0).clamp(1.0, 1400.0);
            base * clamped_zoom
        }
        PdfFitMode::ActualSize => {
            let css_w = w * (96.0 / 72.0);
            css_w * clamped_zoom
        }
    };

    let target_w = target_w.max(1.0);
    let target_h = (target_w * aspect).max(1.0);
    (target_w, target_h)
}

pub fn compute_display_size(
    width_px: u32,
    height_px: u32,
    dpi: u32,
    pane_width_px: f32,
    zoom: f32,
    fit_mode: PdfFitMode,
) -> (f32, f32) {
    let w = width_px.max(1) as f32;
    let h = height_px.max(1) as f32;
    let scale = 72.0 / dpi.max(72) as f32;
    compute_display_size_from_pts(w * scale, h * scale, pane_width_px, zoom, fit_mode)
}

impl PdfPreview {
    pub fn is_full_mode(&self) -> bool {
        self.open && self.mode == PdfViewMode::Full
    }

    pub fn is_split_mode(&self) -> bool {
        self.open && self.mode == PdfViewMode::Split
    }

    pub fn page_count(&self) -> usize {
        self.metadata.as_ref().map(|m| m.page_count).unwrap_or(0)
    }

    pub fn target_dpi(&self) -> u32 {
        dpi_for_zoom(self.zoom)
    }

    pub fn cached_page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn inflight_count(&self) -> usize {
        self.inflight.len()
    }

    pub fn active_task_count(&self) -> usize {
        self.active_tasks
    }

    fn try_acquire_task_permit(&mut self) -> bool {
        if self.active_tasks < MAX_CONCURRENT_RENDERS {
            self.active_tasks += 1;
            true
        } else {
            false
        }
    }

    fn release_task_permit(&mut self) {
        self.active_tasks = self.active_tasks.saturating_sub(1);
    }

    pub fn layout_for(&self, pane_width_px: f32) -> PageLayout {
        let default_pts = self
            .metadata
            .as_ref()
            .and_then(|m| m.page_size_pts)
            .unwrap_or(DEFAULT_PAGE_PTS);
        PageLayout::build(
            self.page_count(),
            default_pts,
            &self.page_pts_overrides,
            pane_width_px,
            self.zoom,
            self.fit_mode,
        )
    }

    pub fn current_layout(&self) -> PageLayout {
        self.layout_for(self.last_pane_width)
    }

    pub fn rendered_page(&self, page: usize) -> Option<&RenderedPage> {
        let dpi = self.target_dpi();
        self.pages
            .get(&(page, dpi))
            .or_else(|| {
                self.pages
                    .iter()
                    .find(|((p, _), _)| *p == page)
                    .map(|(_, entry)| entry)
            })
            .or_else(|| self.fallback_pages.get(&page))
    }

    pub fn current_rendered_page(&self) -> Option<&RenderedPage> {
        self.rendered_page(self.current_page)
    }

    pub fn reset_for_new_path(&mut self, path: String, stamp: Option<FileStamp>) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.path = Some(path);
        self.metadata = None;
        self.current_page = 1;
        self.loading = true;
        self.error = None;
        self.file_stamp = stamp;
        self.last_reload_check = Some(Instant::now());
        self.pages.clear();
        self.inflight.clear();
        self.pending_probe = None;
        self.page_errors.clear();
        self.page_pts_overrides.clear();
        self.fallback_pages.clear();
        self.visible_window = (1, 1);
        self.last_scroll_y = 0.0;
        self.pending_scroll_page = None;
        self.scroll.set_offset(point(px(0.0), px(0.0)));
        self.generation
    }

    pub fn prepare_reload(&mut self, stamp: Option<FileStamp>) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        let (ws, we) = self.visible_window;
        self.fallback_pages.clear();
        for p in ws..=we {
            if let Some(rendered) = self.rendered_page(p).cloned() {
                self.fallback_pages.insert(p, rendered);
            }
        }
        if let Some(cur) = self.current_rendered_page().cloned() {
            self.fallback_pages.entry(self.current_page).or_insert(cur);
        }
        self.pages.clear();
        self.inflight.clear();
        self.pending_probe = None;
        self.page_errors.clear();
        self.page_pts_overrides.clear();
        self.loading = true;
        self.error = None;
        self.file_stamp = stamp;
        self.last_reload_check = Some(Instant::now());
        self.generation
    }

    pub fn insert_page(
        &mut self,
        generation: u64,
        page: usize,
        dpi: u32,
        png: RenderedPng,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        self.inflight.remove(&(page, dpi));
        self.page_errors.remove(&page);
        self.fallback_pages.remove(&page);

        // Anchor viewport scroll position if a page above the active page changes geometry.
        let anchor_page = self.current_page.clamp(1, self.page_count().max(1));
        let old_anchor_top = if page < anchor_page {
            Some(self.layout_for(self.last_pane_width).page_top(anchor_page))
        } else {
            None
        };
        let size_changed = self.record_page_size_override(page, png.width_px, png.height_px, dpi);
        if size_changed {
            if let Some(old_top) = old_anchor_top {
                let new_top = self.layout_for(self.last_pane_width).page_top(anchor_page);
                let delta_y = new_top - old_top;
                if delta_y.abs() > 0.5 {
                    let cur_offset = self.scroll.offset();
                    let old_scroll_y = (-f32::from(cur_offset.y)).max(0.0);
                    let new_scroll_y = (old_scroll_y + delta_y).max(0.0);
                    self.scroll
                        .set_offset(point(cur_offset.x, px(-new_scroll_y)));
                    self.last_scroll_y = new_scroll_y;
                }
            }
        }

        self.touch_seq = self.touch_seq.wrapping_add(1);
        let rendered = RenderedPage {
            image: Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png.bytes)),
            width_px: png.width_px,
            height_px: png.height_px,
            dpi,
            last_used: self.touch_seq,
        };
        if page == self.current_page {
            self.loading = false;
            self.error = None;
        }
        self.pages.insert((page, dpi), rendered);
        self.evict_if_needed();
        true
    }

    fn record_page_size_override(
        &mut self,
        page: usize,
        width_px: u32,
        height_px: u32,
        dpi: u32,
    ) -> bool {
        if width_px == 0 || height_px == 0 || dpi == 0 {
            return false;
        }
        let scale = 72.0 / dpi as f32;
        let w_pt = width_px as f32 * scale;
        let h_pt = height_px as f32 * scale;
        let default_pts = self
            .metadata
            .as_ref()
            .and_then(|m| m.page_size_pts)
            .unwrap_or(DEFAULT_PAGE_PTS);
        // Record override whenever physical width OR height differs (handles both mixed
        // aspect ratios AND same-aspect mixed paper sizes like A5 inside an A4 document).
        if (w_pt - default_pts.0).abs() > 1.0 || (h_pt - default_pts.1).abs() > 1.0 {
            let changed = self
                .page_pts_overrides
                .get(&page)
                .is_none_or(|&(pw, ph)| (pw - w_pt).abs() > 1.0 || (ph - h_pt).abs() > 1.0);
            if changed {
                self.page_pts_overrides.insert(page, (w_pt, h_pt));
            }
            changed
        } else {
            self.page_pts_overrides.remove(&page).is_some()
        }
    }

    pub fn apply_metadata(
        &mut self,
        generation: u64,
        result: Result<PdfMetadata, PdfError>,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        match result {
            Ok(meta) => {
                self.current_page = self.current_page.clamp(1, meta.page_count);
                self.metadata = Some(meta);
                self.error = None;
                true
            }
            Err(err) => {
                self.loading = false;
                self.error = Some(err.to_string());
                true
            }
        }
    }

    pub fn apply_probe_and_page_result(
        &mut self,
        generation: u64,
        meta: PdfMetadata,
        rendered_page: usize,
        dpi: u32,
        png: RenderedPng,
    ) -> bool {
        if !self.apply_metadata(generation, Ok(meta)) {
            return false;
        }
        // Cache the rendered page without overwriting `current_page`, which may
        // have advanced while reload was in flight (apply_metadata already clamped it).
        self.insert_page(generation, rendered_page, dpi, png);
        self.sync_current_page_flags();
        true
    }

    /// Derives `loading` and clears stale `error` when the active page is already cached.
    pub fn sync_current_page_flags(&mut self) {
        let key = (self.current_page, self.target_dpi());
        let is_cached = self.pages.contains_key(&key);
        self.loading = !is_cached;
        if is_cached {
            self.error = None;
            self.page_errors.remove(&self.current_page);
            self.fallback_pages.remove(&self.current_page);
        } else if let Some(msg) = self.page_errors.get(&self.current_page) {
            self.loading = false;
            self.error = Some(msg.clone());
        } else {
            self.error = None;
        }
    }

    pub fn apply_render_error(
        &mut self,
        generation: u64,
        page: usize,
        dpi: u32,
        err: PdfError,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        self.inflight.remove(&(page, dpi));
        let msg = err.to_string();
        self.page_errors.insert(page, msg.clone());
        if page == self.current_page {
            self.loading = false;
            self.error = Some(msg);
            true
        } else {
            // Still notify so an inline per-page error card in the visible window updates.
            page >= self.visible_window.0 && page <= self.visible_window.1
        }
    }

    fn evict_if_needed(&mut self) {
        while self.pages.len() > MAX_CACHED_PAGES {
            let current = self.current_page;
            let (ws, we) = self.visible_window;
            // First evict pages outside the visible window; only fall back to non-current pages if needed.
            let victim = self
                .pages
                .iter()
                .filter(|((p, _), _)| *p != current && (*p < ws || *p > we))
                .min_by_key(|(_, entry)| entry.last_used)
                .or_else(|| {
                    self.pages
                        .iter()
                        .filter(|((p, _), _)| *p != current)
                        .min_by_key(|(_, entry)| entry.last_used)
                })
                .map(|(key, _)| *key);
            if let Some(key) = victim {
                self.pages.remove(&key);
            } else {
                break;
            }
        }
    }

    fn touch_visible_window(&mut self) {
        let dpi = self.target_dpi();
        let (ws, we) = self.visible_window;
        for page in ws..=we {
            if let Some(entry) = self.pages.get_mut(&(page, dpi)) {
                self.touch_seq = self.touch_seq.wrapping_add(1);
                entry.last_used = self.touch_seq;
            }
        }
    }

    #[cfg(test)]
    pub fn seed_test_page(&mut self, path: &str, page_count: usize, png: RenderedPng) {
        self.open = true;
        self.path = Some(path.to_string());
        self.metadata = Some(PdfMetadata {
            page_count,
            title: Some("Sample PDF".into()),
            page_size_pts: Some(DEFAULT_PAGE_PTS),
        });
        self.current_page = 1;
        self.visible_window = (1, page_count.min(2));
        self.loading = false;
        self.error = None;
        let gen = self.generation;
        let dpi = self.target_dpi();
        self.insert_page(gen, 1, dpi, png);
    }
}

impl GtuimView {
    /// Synchronizes PDF preview state with the currently active Neovim buffer and
    /// checks for on-disk modifications when a PDF is open.
    pub fn sync_pdf_preview(&mut self, cx: &mut Context<Self>) {
        if let Some(pdf_path) = active_pdf_path(&self.app) {
            let dismissed =
                self.pdf_preview.dismissed_for_path.as_deref() == Some(pdf_path.as_str());
            if !dismissed && !(self.pdf_preview.open && self.pdf_preview.pinned) {
                let path_changed = self.pdf_preview.path.as_deref() != Some(pdf_path.as_str());
                if !self.pdf_preview.open || path_changed {
                    self.open_pdf_path(pdf_path, PdfViewMode::Full, false, cx);
                    return;
                }
            }
        } else {
            if self.pdf_preview.dismissed_for_path.is_some() {
                self.pdf_preview.dismissed_for_path = None;
            }
            if self.pdf_preview.open
                && !self.pdf_preview.pinned
                && self.pdf_preview.mode == PdfViewMode::Full
            {
                self.pdf_preview.open = false;
                cx.notify();
            }
        }

        // Keep viewport page & lazy window synced even during background polls.
        if self.pdf_preview.open && self.pdf_preview.page_count() > 0 {
            let scroll_y = (-f32::from(self.pdf_preview.scroll.offset().y)).max(0.0);
            if (scroll_y - self.pdf_preview.last_scroll_y).abs() > 0.5 {
                self.sync_pdf_after_scroll(
                    self.pdf_preview.last_pane_width,
                    self.pdf_preview.last_viewport_height,
                    cx,
                );
            }
        }

        // Throttled auto-reload check while a PDF preview is visible.
        if self.pdf_preview.open && !self.pdf_preview.loading {
            let should_check = self
                .pdf_preview
                .last_reload_check
                .is_none_or(|t| t.elapsed() >= RELOAD_CHECK_INTERVAL);
            if should_check {
                self.pdf_preview.last_reload_check = Some(Instant::now());
                if let Some(path) = self.pdf_preview.path.clone() {
                    let resolved = resolve_path(&path);
                    let stamp = read_file_stamp(&resolved);
                    if stamp.is_some() && stamp != self.pdf_preview.file_stamp {
                        self.reload_pdf_preview(cx);
                    }
                }
            }
        }
    }

    pub fn open_pdf_path(
        &mut self,
        path: String,
        mode: PdfViewMode,
        pinned: bool,
        cx: &mut Context<Self>,
    ) {
        let resolved = resolve_path(&path);
        let stamp = read_file_stamp(&resolved);
        self.pdf_preview.open = true;
        self.pdf_preview.mode = mode;
        self.pdf_preview.pinned = pinned;
        self.pdf_preview.dismissed_for_path = None;
        let generation = self.pdf_preview.reset_for_new_path(path, stamp);
        let dpi = self.pdf_preview.target_dpi();
        self.pdf_preview.inflight.insert((1, dpi));
        cx.notify();

        self.request_pdf_probe_and_render(resolved, generation, 1, dpi, cx);
    }

    pub fn reload_pdf_preview(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.pdf_preview.path.clone() else {
            return;
        };
        let resolved = resolve_path(&path);
        let stamp = read_file_stamp(&resolved);
        let generation = self.pdf_preview.prepare_reload(stamp);
        let page = self.pdf_preview.current_page.max(1);
        let dpi = self.pdf_preview.target_dpi();
        self.pdf_preview.inflight.insert((page, dpi));
        cx.notify();

        self.request_pdf_probe_and_render(resolved, generation, page, dpi, cx);
    }

    pub fn toggle_pdf_preview(&mut self, cx: &mut Context<Self>) {
        if self.pdf_preview.open {
            if let Some(active_pdf) = active_pdf_path(&self.app) {
                self.pdf_preview.dismissed_for_path = Some(active_pdf);
            }
            self.pdf_preview.open = false;
            self.pdf_preview.pinned = false;
            cx.notify();
            return;
        }

        if let Some(active_pdf) = active_pdf_path(&self.app) {
            self.open_pdf_path(active_pdf, PdfViewMode::Full, false, cx);
            return;
        }

        if let Some(active_path) = active_tab_path(&self.app) {
            if let Some(sibling_pdf) = companion_pdf_path(&active_path) {
                self.open_pdf_path(sibling_pdf, PdfViewMode::Split, true, cx);
                return;
            }
        }

        if let Some(last_pdf) = self.pdf_preview.path.clone() {
            self.open_pdf_path(last_pdf, PdfViewMode::Split, true, cx);
            return;
        }

        self.pdf_preview.open = true;
        self.pdf_preview.mode = PdfViewMode::Split;
        self.pdf_preview.pinned = true;
        self.pdf_preview.loading = false;
        self.pdf_preview.error = None;
        cx.notify();
    }

    pub fn toggle_pdf_view_mode(&mut self, cx: &mut Context<Self>) {
        if !self.pdf_preview.open {
            return;
        }
        match self.pdf_preview.mode {
            PdfViewMode::Full => {
                self.pdf_preview.mode = PdfViewMode::Split;
                self.pdf_preview.pinned = true;
            }
            PdfViewMode::Split => {
                self.pdf_preview.mode = PdfViewMode::Full;
                if active_pdf_path(&self.app).as_deref() == self.pdf_preview.path.as_deref() {
                    self.pdf_preview.pinned = false;
                }
            }
        }
        cx.notify();
    }

    /// Scrolls the continuous page stack to `page` and triggers lazy rendering around it.
    pub fn pdf_go_to_page(&mut self, page: usize, cx: &mut Context<Self>) {
        let count = self.pdf_preview.page_count();
        if count == 0 {
            return;
        }
        let target = page.clamp(1, count);
        let layout = self.pdf_preview.current_layout();
        let target_y = (layout.page_top(target) - CANVAS_PAD_PX).max(0.0);
        let cur_x = self.pdf_preview.scroll.offset().x;

        self.pdf_preview.current_page = target;
        self.pdf_preview.page_errors.remove(&target);
        self.pdf_preview.error = None;
        self.pdf_preview
            .scroll
            .set_offset(point(cur_x, px(-target_y)));
        self.pdf_preview.last_scroll_y = target_y;
        self.pdf_preview.pending_scroll_page = Some(target);
        self.pdf_preview.visible_window =
            layout.render_window(target_y, self.pdf_preview.last_viewport_height);
        self.pdf_preview.touch_visible_window();
        self.ensure_visible_pdf_pages_rendered(cx);
        cx.notify();
    }

    pub fn pdf_step_page(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.pdf_preview.page_count();
        if count == 0 {
            return;
        }
        let next =
            (self.pdf_preview.current_page as isize + delta).clamp(1, count as isize) as usize;
        self.pdf_go_to_page(next, cx);
    }

    /// Scrolls smoothly by `delta_y` pixels (positive scrolls downward across pages).
    pub fn pdf_scroll_by(&mut self, delta_y: f32, cx: &mut Context<Self>) {
        let count = self.pdf_preview.page_count();
        if count == 0 {
            return;
        }
        let layout = self.pdf_preview.current_layout();
        let view_h = self.pdf_preview.last_viewport_height;
        let max_scroll = (layout.total_height() - view_h).max(0.0);
        let cur_offset = self.pdf_preview.scroll.offset();
        let cur_y = (-f32::from(cur_offset.y)).max(0.0);
        let next_y = (cur_y + delta_y).clamp(0.0, max_scroll);

        self.pdf_preview.pending_scroll_page = None;
        self.pdf_preview
            .scroll
            .set_offset(point(cur_offset.x, px(-next_y)));
        self.pdf_preview.last_scroll_y = next_y;
        self.pdf_preview.current_page = layout.active_page_for_viewport(next_y, view_h);
        self.pdf_preview.visible_window = layout.render_window(next_y, view_h);
        self.pdf_preview.touch_visible_window();
        self.ensure_visible_pdf_pages_rendered(cx);
        cx.notify();
    }

    fn preserve_scroll_ratio_across_layout_change(
        &mut self,
        change: impl FnOnce(&mut PdfPreview),
        cx: &mut Context<Self>,
    ) {
        let old_layout = self.pdf_preview.current_layout();
        let old_total = old_layout.total_height().max(1.0);
        let old_scroll_y = (-f32::from(self.pdf_preview.scroll.offset().y)).max(0.0);
        let ratio = (old_scroll_y / old_total).clamp(0.0, 1.0);

        change(&mut self.pdf_preview);
        self.pdf_preview.error = None;
        self.pdf_preview.page_errors.clear();

        let new_layout = self.pdf_preview.current_layout();
        let view_h = self.pdf_preview.last_viewport_height;
        let max_scroll = (new_layout.total_height() - view_h).max(0.0);
        let new_y = (ratio * new_layout.total_height()).clamp(0.0, max_scroll);
        self.pdf_preview
            .scroll
            .set_offset(point(px(0.0), px(-new_y)));
        self.pdf_preview.last_scroll_y = new_y;
        self.pdf_preview.pending_scroll_page = None;
        if new_layout.page_count > 0 {
            self.pdf_preview.visible_window = new_layout.render_window(new_y, view_h);
        }
        self.ensure_visible_pdf_pages_rendered(cx);
        cx.notify();
    }

    pub fn pdf_adjust_zoom(&mut self, delta: f32, cx: &mut Context<Self>) {
        let next = (self.pdf_preview.zoom + delta).clamp(MIN_ZOOM, MAX_ZOOM);
        let rounded = (next * 100.0).round() / 100.0;
        if (rounded - self.pdf_preview.zoom).abs() < 0.001 {
            return;
        }
        self.preserve_scroll_ratio_across_layout_change(|pdf| pdf.zoom = rounded, cx);
    }

    pub fn pdf_reset_zoom(&mut self, cx: &mut Context<Self>) {
        self.preserve_scroll_ratio_across_layout_change(
            |pdf| {
                pdf.zoom = 1.0;
                pdf.fit_mode = PdfFitMode::FitWidth;
            },
            cx,
        );
    }

    pub fn pdf_toggle_fit_mode(&mut self, cx: &mut Context<Self>) {
        self.preserve_scroll_ratio_across_layout_change(
            |pdf| {
                pdf.fit_mode = match pdf.fit_mode {
                    PdfFitMode::FitWidth => PdfFitMode::ActualSize,
                    PdfFitMode::ActualSize => PdfFitMode::FitWidth,
                };
            },
            cx,
        );
    }

    /// Handles PDF navigation/zoom/scroll keys when PDF preview is active in Full mode.
    /// Returns `true` if the keystroke was consumed.
    pub fn handle_pdf_key(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        if !self.pdf_preview.is_full_mode() {
            return false;
        }
        if ks.modifiers.alt || ks.modifiers.platform {
            return false;
        }

        if ks.modifiers.control {
            return match ks.key.as_str() {
                "=" | "+" => {
                    self.pdf_adjust_zoom(ZOOM_STEP, cx);
                    true
                }
                "-" => {
                    self.pdf_adjust_zoom(-ZOOM_STEP, cx);
                    true
                }
                "0" => {
                    self.pdf_reset_zoom(cx);
                    true
                }
                _ => false,
            };
        }

        match ks.key.as_str() {
            "down" | "j" => {
                self.pdf_scroll_by(KEYBOARD_SCROLL_STEP_PX, cx);
                true
            }
            "up" | "k" => {
                self.pdf_scroll_by(-KEYBOARD_SCROLL_STEP_PX, cx);
                true
            }
            "right" | "pagedown" | "]" | "n" | "space" => {
                self.pdf_step_page(1, cx);
                true
            }
            "left" | "pageup" | "[" | "p" => {
                self.pdf_step_page(-1, cx);
                true
            }
            "home" => {
                self.pdf_go_to_page(1, cx);
                true
            }
            "end" => {
                let last = self.pdf_preview.page_count().max(1);
                self.pdf_go_to_page(last, cx);
                true
            }
            "g" => {
                if ks.modifiers.shift {
                    let last = self.pdf_preview.page_count().max(1);
                    self.pdf_go_to_page(last, cx);
                } else {
                    self.pdf_go_to_page(1, cx);
                }
                true
            }
            "+" | "=" => {
                self.pdf_adjust_zoom(ZOOM_STEP, cx);
                true
            }
            "-" => {
                self.pdf_adjust_zoom(-ZOOM_STEP, cx);
                true
            }
            "0" => {
                self.pdf_reset_zoom(cx);
                true
            }
            "f" => {
                self.pdf_toggle_fit_mode(cx);
                true
            }
            "r" => {
                self.reload_pdf_preview(cx);
                true
            }
            _ => false,
        }
    }

    fn sync_pdf_viewport_scroll(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.pdf_preview.pending_scroll_page = None;
        let (pane_w, viewport_h) = measure_pdf_viewport(self, window);
        self.sync_pdf_after_scroll(pane_w, viewport_h, cx);
    }

    fn sync_pdf_after_scroll(&mut self, pane_w: f32, viewport_h: f32, cx: &mut Context<Self>) {
        self.pdf_preview.sync_scheduled.set(false);
        if !self.pdf_preview.open || self.pdf_preview.page_count() == 0 {
            return;
        }
        let next_pane_w = pane_w.max(200.0);
        let next_view_h = viewport_h.max(120.0);
        let width_changed = (next_pane_w - self.pdf_preview.last_pane_width).abs() > 2.0;

        if width_changed && self.pdf_preview.fit_mode == PdfFitMode::FitWidth {
            let old_layout = self
                .pdf_preview
                .layout_for(self.pdf_preview.last_pane_width);
            let old_total = old_layout.total_height().max(1.0);
            let old_scroll_y = (-f32::from(self.pdf_preview.scroll.offset().y)).max(0.0);
            let ratio = (old_scroll_y / old_total).clamp(0.0, 1.0);
            self.pdf_preview.last_pane_width = next_pane_w;
            let new_layout = self.pdf_preview.layout_for(next_pane_w);
            let max_scroll = (new_layout.total_height() - next_view_h).max(0.0);
            let new_y = (ratio * new_layout.total_height()).clamp(0.0, max_scroll);
            let cur_x = self.pdf_preview.scroll.offset().x;
            self.pdf_preview.scroll.set_offset(point(cur_x, px(-new_y)));
            self.pdf_preview.last_scroll_y = new_y;
        } else {
            self.pdf_preview.last_pane_width = next_pane_w;
        }
        self.pdf_preview.last_viewport_height = next_view_h;

        let layout = self
            .pdf_preview
            .layout_for(self.pdf_preview.last_pane_width);
        let scroll_y = (-f32::from(self.pdf_preview.scroll.offset().y)).max(0.0);
        let mut changed = width_changed;

        if self.pdf_preview.pending_scroll_page.take().is_some() {
            // Settle after a programmatic `pdf_go_to_page` jump (including GPUI clamping)
            // without snapping `current_page` away from the explicitly requested target.
            self.pdf_preview.last_scroll_y = scroll_y;
        } else if (scroll_y - self.pdf_preview.last_scroll_y).abs() > 0.5 {
            self.pdf_preview.last_scroll_y = scroll_y;
            let active =
                layout.active_page_for_viewport(scroll_y, self.pdf_preview.last_viewport_height);
            if active != self.pdf_preview.current_page {
                self.pdf_preview.current_page = active;
                changed = true;
            }
        }

        let new_window = layout.render_window(scroll_y, self.pdf_preview.last_viewport_height);
        if new_window != self.pdf_preview.visible_window {
            self.pdf_preview.visible_window = new_window;
            changed = true;
        }

        self.pdf_preview.touch_visible_window();
        self.ensure_visible_pdf_pages_rendered(cx);
        if changed {
            cx.notify();
        }
    }

    fn ensure_visible_pdf_pages_rendered(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.pdf_preview.path.clone() else {
            return;
        };
        let count = self.pdf_preview.page_count();
        if count == 0 {
            return;
        }
        let current = self.pdf_preview.current_page.clamp(1, count);
        let dpi = self.pdf_preview.target_dpi();
        let generation = self.pdf_preview.generation;
        let resolved = resolve_path(&path);

        self.pdf_preview.sync_current_page_flags();

        let (ws, we) = self.pdf_preview.visible_window;
        let win_start = ws.clamp(1, count);
        let win_end = we.clamp(win_start, count);

        // Prioritize current page first, then closest visible/prefetch pages, capped by MAX_CONCURRENT_RENDERS.
        let mut candidates: Vec<usize> = (win_start..=win_end).collect();
        if !candidates.contains(&current) {
            candidates.push(current);
        }
        candidates.sort_by_key(|&p| p.abs_diff(current));

        for page in candidates {
            if self.pdf_preview.pages.contains_key(&(page, dpi))
                || self.pdf_preview.inflight.contains(&(page, dpi))
                || self.pdf_preview.page_errors.contains_key(&page)
            {
                continue;
            }
            if !self.pdf_preview.try_acquire_task_permit() {
                break;
            }
            self.pdf_preview.inflight.insert((page, dpi));
            self.spawn_pdf_page_render(resolved.clone(), generation, page, dpi, cx);
        }
    }

    fn drain_pdf_render_queue(&mut self, cx: &mut Context<Self>) {
        if let Some((path, gen, page, dpi)) = self.pdf_preview.pending_probe.take() {
            if gen == self.pdf_preview.generation {
                if self.pdf_preview.try_acquire_task_permit() {
                    self.spawn_pdf_probe_and_render(path, gen, page, dpi, cx);
                } else {
                    self.pdf_preview.pending_probe = Some((path, gen, page, dpi));
                    return;
                }
            }
        }
        self.ensure_visible_pdf_pages_rendered(cx);
    }

    fn request_pdf_probe_and_render(
        &mut self,
        path: PathBuf,
        generation: u64,
        requested_page: usize,
        dpi: u32,
        cx: &mut Context<Self>,
    ) {
        if self.pdf_preview.try_acquire_task_permit() {
            self.spawn_pdf_probe_and_render(path, generation, requested_page, dpi, cx);
        } else {
            self.pdf_preview.pending_probe = Some((path, generation, requested_page, dpi));
        }
    }

    fn retry_pdf_page(&mut self, page: usize, cx: &mut Context<Self>) {
        self.pdf_preview.page_errors.remove(&page);
        if page == self.pdf_preview.current_page {
            self.pdf_preview.error = None;
        }
        self.ensure_visible_pdf_pages_rendered(cx);
        cx.notify();
    }

    fn spawn_pdf_probe_and_render(
        &self,
        path: PathBuf,
        generation: u64,
        requested_page: usize,
        dpi: u32,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |view, cx| {
            let path_for_task = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let meta = probe_pdf_info(&path_for_task)?;
                    let rendered_page = requested_page.clamp(1, meta.page_count);
                    let png = render_pdf_page_png(&path_for_task, rendered_page, dpi)?;
                    Ok::<_, PdfError>((meta, rendered_page, png))
                })
                .await;

            let _ = view.update(cx, |this, cx| {
                this.pdf_preview.release_task_permit();
                let mut should_notify = false;
                match result {
                    Ok((meta, rendered_page, png)) => {
                        if this.pdf_preview.apply_probe_and_page_result(
                            generation,
                            meta,
                            rendered_page,
                            dpi,
                            png,
                        ) {
                            let layout = this.pdf_preview.current_layout();
                            let scroll_y =
                                (-f32::from(this.pdf_preview.scroll.offset().y)).max(0.0);
                            this.pdf_preview.visible_window = layout
                                .render_window(scroll_y, this.pdf_preview.last_viewport_height);
                            should_notify = true;
                        }
                    }
                    Err(err) => {
                        if this.pdf_preview.apply_metadata(generation, Err(err)) {
                            should_notify = true;
                        }
                    }
                }
                // Always refill current generation's queue even if this completion was stale.
                this.drain_pdf_render_queue(cx);
                if should_notify {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn spawn_pdf_page_render(
        &self,
        path: PathBuf,
        generation: u64,
        page: usize,
        dpi: u32,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |view, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { render_pdf_page_png(&path, page, dpi) })
                .await;

            let _ = view.update(cx, |this, cx| {
                this.pdf_preview.release_task_permit();
                let should_notify = match result {
                    Ok(png) => this.pdf_preview.insert_page(generation, page, dpi, png),
                    Err(err) => this
                        .pdf_preview
                        .apply_render_error(generation, page, dpi, err),
                };
                // Always refill current generation's queue even if this completion was stale.
                this.drain_pdf_render_queue(cx);
                if should_notify {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

/// Compact top banner shown when viewing a `.pdf` buffer in Raw mode so the user can re-enable PDF Preview.
pub fn render_toggle(view: &GtuimView, cx: &mut Context<GtuimView>) -> AnyElement {
    let p = gui_palette(&view.app.theme);
    div()
        .id("pdf-preview-toggle")
        .debug_selector(|| "pdf-preview-toggle".into())
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
                .child(icon("book-open", 13.0, p.accent))
                .child(
                    div()
                        .text_size(px(11.5))
                        .text_color(rgb(p.text_muted))
                        .child("Viewing raw PDF buffer"),
                ),
        )
        .child(
            div()
                .id("pdf-preview-reopen-btn")
                .px_2()
                .py(px(2.0))
                .rounded(px(4.0))
                .bg(rgb(p.bg_selected))
                .text_size(px(11.5))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(p.accent))
                .cursor_pointer()
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_pdf_preview(cx)))
                .child("Open PDF Preview · Ctrl+Shift+D"),
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

fn estimate_pdf_pane_width(view: &GtuimView, window: &Window) -> f32 {
    let viewport = window.viewport_size();
    let workspace = &view.app.workspace;
    // Use the same responsive layout as the owning GUI, including panes that
    // have been temporarily collapsed. Measuring last frame's scroll bounds
    // instead can lock FitWidth to the old width after a Split/Full toggle.
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
    let center_w = layout.editor_avail_w.max(1.0);
    pdf_pane_share(
        center_w,
        view.pdf_preview.is_split_mode(),
        view.markdown_preview.open,
    )
}

fn pdf_pane_share(center_width: f32, split: bool, markdown: bool) -> f32 {
    let panes = 1 + usize::from(split) + usize::from(markdown);
    (center_width / panes as f32).max(1.0)
}

fn measure_pdf_viewport(view: &GtuimView, window: &Window) -> (f32, f32) {
    let pane_w = estimate_pdf_pane_width(view, window);
    let vp_h = f32::from(window.viewport_size().height).max(240.0);
    let scroll_bounds = view.pdf_preview.scroll.bounds();
    let measured_h = f32::from(scroll_bounds.size.height);
    let viewport_h = if measured_h >= 80.0 && measured_h <= vp_h {
        measured_h
    } else {
        (vp_h - 140.0).max(160.0)
    };
    (pane_w, viewport_h)
}

pub fn render(view: &GtuimView, window: &Window, cx: &mut Context<GtuimView>) -> AnyElement {
    let p = gui_palette(&view.app.theme);
    let state = &view.pdf_preview;
    let is_split = state.is_split_mode();
    let page_count = state.page_count();
    let has_path = state.path.is_some();

    let (pane_w, viewport_h) = measure_pdf_viewport(view, window);
    let layout = state.layout_for(pane_w);
    let scroll_y = (-f32::from(state.scroll.offset().y)).max(0.0);

    let (window_start, window_end) = if page_count > 0 {
        layout.render_window(scroll_y, viewport_h)
    } else {
        (1, 0)
    };

    // Derive live displayed page directly from viewport scroll when the user scrolled.
    let display_page = if page_count == 0 {
        0
    } else if let Some(pending) = state.pending_scroll_page {
        pending.clamp(1, page_count)
    } else if (scroll_y - state.last_scroll_y).abs() > 0.5 {
        layout.active_page_for_viewport(scroll_y, viewport_h)
    } else {
        state.current_page.clamp(1, page_count)
    };

    // Schedule a single post-frame sync if scroll position, viewport size, or uncached visible pages changed.
    if page_count > 0 && has_path && !state.sync_scheduled.get() {
        let dpi = state.target_dpi();
        let needs_page_render = state.active_tasks < MAX_CONCURRENT_RENDERS
            && (window_start..=window_end).any(|page| {
                !state.pages.contains_key(&(page, dpi))
                    && !state.inflight.contains(&(page, dpi))
                    && !state.page_errors.contains_key(&page)
            });
        let viewport_changed = (pane_w - state.last_pane_width).abs() > 2.0
            || (viewport_h - state.last_viewport_height).abs() > 2.0
            || (scroll_y - state.last_scroll_y).abs() > 0.5
            || display_page != state.current_page
            || (window_start, window_end) != state.visible_window
            || state.pending_scroll_page.is_some();
        if needs_page_render || viewport_changed {
            state.sync_scheduled.set(true);
            cx.spawn(async move |view, cx| {
                let _ = view.update(cx, |this, cx| {
                    this.sync_pdf_after_scroll(pane_w, viewport_h, cx);
                });
            })
            .detach();
        }
    }

    let can_prev = page_count > 1 && display_page > 1;
    let can_next = page_count > 1 && display_page < page_count;
    let can_zoom_out = state.zoom > MIN_ZOOM + 0.01;
    let can_zoom_in = state.zoom < MAX_ZOOM - 0.01;

    let filename = state
        .path
        .as_deref()
        .and_then(|s| Path::new(s).file_name().and_then(|n| n.to_str()))
        .unwrap_or("PDF Preview")
        .to_string();

    let page_label = if page_count > 0 {
        format!("{} / {}", display_page, page_count)
    } else if state.loading {
        "Loading...".to_string()
    } else {
        "—".to_string()
    };

    let zoom_label = format!("{}%", (state.zoom * 100.0).round() as u32);
    let fit_label = match state.fit_mode {
        PdfFitMode::FitWidth => "Fit",
        PdfFitMode::ActualSize => "1:1",
    };

    let compact = pane_w < 460.0;
    let show_page_size = pane_w >= 560.0;
    let show_filename = pane_w >= 390.0;

    let toolbar = div()
        .id("pdf-preview-toolbar")
        .debug_selector(|| "pdf-preview-toolbar".into())
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
                .child(icon("book-open", 13.5, p.accent))
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
                .when_some(
                    show_page_size
                        .then(|| state.metadata.as_ref().and_then(|m| m.page_size_pts))
                        .flatten(),
                    |d, (w_pt, h_pt)| {
                        d.child(
                            div()
                                .px(px(5.0))
                                .py(px(1.0))
                                .rounded(px(3.0))
                                .flex_shrink_0()
                                .bg(rgb(p.bg_elevated))
                                .text_size(px(10.0))
                                .text_color(rgb(p.text_muted))
                                .child(format!(
                                    "{}×{}pt",
                                    w_pt.round() as i32,
                                    h_pt.round() as i32
                                )),
                        )
                    },
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(if compact { 2.0 } else { 4.0 }))
                .flex_shrink_0()
                .child(toolbar_icon_btn(
                    "pdf-prev-page",
                    "pdf-prev-page",
                    "chevron-left",
                    can_prev,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| this.pdf_step_page(-1, cx)),
                ))
                .child(
                    div()
                        .id("pdf-page-indicator")
                        .debug_selector(|| "pdf-page-indicator".into())
                        .px(px(if compact { 5.0 } else { 8.0 }))
                        .py(px(2.0))
                        .rounded(px(4.0))
                        .bg(rgb(p.bg_input))
                        .border_1()
                        .border_color(rgb(p.border_subtle))
                        .text_size(px(11.0))
                        .text_color(rgb(p.text_strong))
                        .child(page_label),
                )
                .child(toolbar_icon_btn(
                    "pdf-next-page",
                    "pdf-next-page",
                    "chevron-right",
                    can_next,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| this.pdf_step_page(1, cx)),
                ))
                .when(!compact, |d| {
                    d.child(div().w(px(1.0)).h(px(14.0)).mx_1().bg(rgb(p.border)))
                })
                .child(toolbar_icon_btn(
                    "pdf-zoom-out",
                    "pdf-zoom-out",
                    "minus",
                    can_zoom_out,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| this.pdf_adjust_zoom(-ZOOM_STEP, cx)),
                ))
                .child(
                    div()
                        .id("pdf-zoom-reset")
                        .debug_selector(|| "pdf-zoom-reset".into())
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
                        .on_click(cx.listener(|this, _, _, cx| this.pdf_reset_zoom(cx)))
                        .child(zoom_label),
                )
                .child(toolbar_icon_btn(
                    "pdf-zoom-in",
                    "pdf-zoom-in",
                    "plus",
                    can_zoom_in,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| this.pdf_adjust_zoom(ZOOM_STEP, cx)),
                ))
                .when(!compact, |d| {
                    d.child(
                        div()
                            .id("pdf-fit-toggle")
                            .debug_selector(|| "pdf-fit-toggle".into())
                            .px_2()
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(rgb(if state.fit_mode == PdfFitMode::FitWidth {
                                p.bg_selected
                            } else {
                                p.bg_elevated
                            }))
                            .text_size(px(11.0))
                            .text_color(rgb(if state.fit_mode == PdfFitMode::FitWidth {
                                p.accent
                            } else {
                                p.text
                            }))
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(p.bg_hover)))
                            .on_click(cx.listener(|this, _, _, cx| this.pdf_toggle_fit_mode(cx)))
                            .child(fit_label),
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
                    "pdf-reload",
                    "pdf-reload",
                    "refresh",
                    has_path,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| this.reload_pdf_preview(cx)),
                ))
                .child(toolbar_icon_btn(
                    "pdf-mode-toggle",
                    "pdf-mode-toggle",
                    if is_split {
                        "maximize"
                    } else {
                        "split-vertical"
                    },
                    true,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| this.toggle_pdf_view_mode(cx)),
                ))
                .child(toolbar_icon_btn(
                    "pdf-close",
                    "pdf-close",
                    "x",
                    true,
                    true,
                    p,
                    cx.listener(|this, _, _, cx| this.toggle_pdf_preview(cx)),
                )),
        );

    let body: AnyElement = if let Some(err) = state.error.as_ref().filter(|_| page_count == 0) {
        div()
            .id("pdf-preview-error")
            .debug_selector(|| "pdf-preview-error".into())
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
                            .child("Unable to preview PDF"),
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
                                    .id("pdf-error-retry")
                                    .px_3()
                                    .py_1()
                                    .rounded(px(5.0))
                                    .bg(rgb(p.accent))
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(p.accent_fg))
                                    .cursor_pointer()
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.reload_pdf_preview(cx)),
                                    )
                                    .child("Reload"),
                            )
                            .child(
                                div()
                                    .id("pdf-error-close")
                                    .px_3()
                                    .py_1()
                                    .rounded(px(5.0))
                                    .bg(rgb(p.bg_elevated))
                                    .text_size(px(11.5))
                                    .text_color(rgb(p.text))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgb(p.bg_hover)))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.toggle_pdf_preview(cx)),
                                    )
                                    .child("Close Preview"),
                            ),
                    ),
            )
            .into_any_element()
    } else if state.path.is_none() {
        div()
            .id("pdf-preview-empty")
            .debug_selector(|| "pdf-preview-empty".into())
            .flex_1()
            .w_full()
            .p_6()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .child(icon("book-open", 28.0, p.text_muted))
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(p.text_strong))
                    .child("No PDF document open"),
            )
            .child(
                div()
                    .max_w(px(360.0))
                    .text_size(px(12.0))
                    .text_color(rgb(p.text_muted))
                    .whitespace_normal()
                    .child("Open any .pdf file from the Explorer or open a .tex/.typ file with a matching .pdf to preview it here."),
            )
            .into_any_element()
    } else if page_count > 0 {
        let (top_spacer, bottom_spacer) = layout.spacers(window_start, window_end);
        let mut stack = div()
            .w_full()
            .px(px(CANVAS_PAD_PX))
            .flex()
            .flex_col()
            .items_center();

        if top_spacer > 0.0 {
            stack = stack.child(div().w_full().h(px(top_spacer)).flex_shrink_0());
        }

        for page_num in window_start..=window_end {
            let (display_w, display_h) = layout.page_size(page_num);
            let is_active_page = page_num == display_page;
            let page_surface: AnyElement = if let Some(rendered) = state.rendered_page(page_num) {
                let page_image = rendered.image.clone();
                div()
                    .id(("pdf-page-card", page_num))
                    .debug_selector(move || {
                        if is_active_page {
                            "pdf-page-surface".into()
                        } else {
                            format!("pdf-page-card-{page_num}")
                        }
                    })
                    .w(px(display_w))
                    .h(px(display_h))
                    .flex_shrink_0()
                    .rounded(px(4.0))
                    .bg(rgb(0xffffff))
                    .border_1()
                    .border_color(rgb(p.border))
                    .overflow_hidden()
                    .child(
                        gpui::img(page_image)
                            .w(px(display_w))
                            .h(px(display_h))
                            .object_fit(ObjectFit::Contain),
                    )
                    .into_any_element()
            } else if let Some(page_err) = state.page_errors.get(&page_num) {
                div()
                    .id(("pdf-page-error", page_num))
                    .debug_selector(move || format!("pdf-page-error-{page_num}"))
                    .w(px(display_w))
                    .h(px(display_h))
                    .flex_shrink_0()
                    .rounded(px(4.0))
                    .bg(rgb(p.bg_panel))
                    .border_1()
                    .border_color(rgb(p.error))
                    .p_4()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(p.error))
                            .child(format!("Page {page_num} failed to render")),
                    )
                    .child(
                        div()
                            .max_w(px(360.0))
                            .text_size(px(11.5))
                            .text_color(rgb(p.text_muted))
                            .whitespace_normal()
                            .child(page_err.clone()),
                    )
                    .child(
                        div()
                            .id(("pdf-page-retry", page_num))
                            .px_3()
                            .py_1()
                            .rounded(px(4.0))
                            .bg(rgb(p.accent))
                            .text_size(px(11.0))
                            .text_color(rgb(p.accent_fg))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.retry_pdf_page(page_num, cx);
                            }))
                            .child("Retry Page"),
                    )
                    .into_any_element()
            } else {
                div()
                    .id(("pdf-page-placeholder", page_num))
                    .debug_selector(move || {
                        if is_active_page {
                            "pdf-page-surface".into()
                        } else {
                            format!("pdf-page-card-{page_num}")
                        }
                    })
                    .w(px(display_w))
                    .h(px(display_h))
                    .flex_shrink_0()
                    .rounded(px(4.0))
                    .bg(rgb(p.bg_panel))
                    .border_1()
                    .border_color(rgb(p.border_subtle))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(p.text_strong))
                            .child(format!("Page {page_num}")),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(p.text_muted))
                            .child("Rendering..."),
                    )
                    .into_any_element()
            };

            let mut slot = div()
                .id(("pdf-page-slot", page_num))
                .debug_selector(move || format!("pdf-page-slot-{page_num}"))
                .flex_shrink_0()
                .child(page_surface);
            if page_num < window_end {
                slot = slot.mb(px(PAGE_GAP_PX));
            }
            stack = stack.child(slot);
        }

        if bottom_spacer > 0.0 {
            stack = stack.child(div().w_full().h(px(bottom_spacer)).flex_shrink_0());
        }

        div()
            .id("pdf-preview-scroll")
            .debug_selector(|| "pdf-preview-scroll".into())
            .flex_1()
            .min_h_0()
            .min_w_0()
            .w_full()
            .overflow_scroll()
            .track_scroll(&state.scroll)
            .bg(rgb(p.bg_rail))
            .on_scroll_wheel(cx.listener(|this, _, window, cx| {
                this.sync_pdf_viewport_scroll(window, cx);
            }))
            .child(stack)
            .into_any_element()
    } else {
        div()
            .id("pdf-preview-loading")
            .debug_selector(|| "pdf-preview-loading".into())
            .flex_1()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.5))
            .text_color(rgb(p.text_muted))
            .child("Rendering PDF page...")
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
            "Scroll / j,k: Continuous · ←/→ or PgUp/PgDn: Page · +/-: Zoom · 0: Reset · f: Fit · r: Reload · Ctrl+Shift+D: Raw"
        } else {
            "Split PDF Preview · Continuous scroll · Auto-reloads on change · Ctrl+Shift+D: Close"
        })
        .when(
            (state.loading || !state.inflight.is_empty()) && page_count > 0,
            |d| d.child(div().text_color(rgb(p.accent)).child("Updating...")),
        );

    let mut container = div()
        .id("pdf-preview-pane")
        .debug_selector(|| "pdf-preview-pane".into())
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
pub(crate) mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn full_and_split_share_width_with_markdown_without_clipping_fit_pages() {
        for (split, markdown, panes) in [
            (false, false, 1.),
            (false, true, 2.),
            (true, false, 2.),
            (true, true, 3.),
        ] {
            let pane = pdf_pane_share(900., split, markdown);
            assert_eq!(pane, 900. / panes);
            let (width, _) =
                compute_display_size_from_pts(600., 800., pane, 1., PdfFitMode::FitWidth);
            assert!(width <= pane, "FitWidth must respect every visible pane");
        }
        let (width, _) = compute_display_size_from_pts(600., 800., 80., 1., PdfFitMode::FitWidth);
        assert!(
            width <= 80.,
            "narrow panes must not retain the old 200px floor"
        );
    }

    struct TempFileGuard(PathBuf);
    impl Drop for TempFileGuard {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    /// Generates a valid minimal N-page PDF in memory with accurate xref offsets.
    pub(crate) fn build_sample_multipage_pdf(pages: usize) -> Vec<u8> {
        let count = pages.max(1);
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.4\n");

        let font_obj_id = 3 + count * 2;
        let mut objects: Vec<Vec<u8>> = Vec::with_capacity(font_obj_id);

        // 1: Catalog
        objects.push(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_vec());

        // 2: Pages
        let kids: String = (0..count)
            .map(|i| format!("{} 0 R", 3 + i * 2))
            .collect::<Vec<_>>()
            .join(" ");
        objects.push(
            format!("2 0 obj\n<< /Type /Pages /Kids [{kids}] /Count {count} >>\nendobj\n")
                .into_bytes(),
        );

        // Page + Content stream pairs
        for i in 0..count {
            let page_id = 3 + i * 2;
            let stream_id = page_id + 1;
            let page_label = i + 1;
            objects.push(
                format!(
                    "{page_id} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Contents {stream_id} 0 R /Resources << /Font << /F1 {font_obj_id} 0 R >> >> >>\nendobj\n"
                )
                .into_bytes(),
            );
            let stream_body = format!("BT /F1 24 Tf 40 300 Td (Page {page_label} Content) Tj ET");
            objects.push(
                format!(
                    "{stream_id} 0 obj\n<< /Length {} >>\nstream\n{stream_body}\nendstream\nendobj\n",
                    stream_body.len()
                )
                .into_bytes(),
            );
        }

        // Font object
        objects.push(
            format!(
                "{font_obj_id} 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n"
            )
            .into_bytes(),
        );

        let mut offsets = Vec::with_capacity(objects.len());
        for obj in &objects {
            offsets.push(out.len());
            out.extend_from_slice(obj);
        }
        let xref_offset = out.len();
        let total_objs = objects.len() + 1;
        out.extend_from_slice(format!("xref\n0 {total_objs}\n0000000000 65535 f \n").as_bytes());
        for off in offsets {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {total_objs} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n"
            )
            .as_bytes(),
        );
        out
    }

    pub(crate) fn build_sample_two_page_pdf() -> Vec<u8> {
        build_sample_multipage_pdf(2)
    }

    #[test]
    fn detects_pdf_paths_case_insensitively() {
        assert!(is_pdf_path("report.pdf"));
        assert!(is_pdf_path("/tmp/PAPER.PDF"));
        assert!(is_pdf_path("a/b/c.Pdf"));
        assert!(!is_pdf_path("README.md"));
        assert!(!is_pdf_path("pdf"));
        assert!(!is_pdf_path(""));
    }

    #[test]
    fn parses_pdfinfo_including_rotation_and_title() {
        let sample = "\
Title:          Quarterly Review
Pages:          12
Page size:      612 x 792 pts (letter)
Page rot:       90
";
        let meta = parse_pdfinfo_output(sample).expect("valid pdfinfo");
        assert_eq!(meta.page_count, 12);
        assert_eq!(meta.title.as_deref(), Some("Quarterly Review"));
        assert_eq!(meta.page_size_pts, Some((792.0, 612.0)));
    }

    #[test]
    fn rejects_invalid_or_zero_page_pdfinfo() {
        assert!(parse_pdfinfo_output("Pages: 0\n").is_err());
        assert!(parse_pdfinfo_output("Title: Only title\n").is_err());
    }

    #[test]
    fn parses_png_ihdr_and_rejects_corrupt_bytes() {
        let mut png = Vec::new();
        png.extend_from_slice(PNG_SIGNATURE);
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&800u32.to_be_bytes());
        png.extend_from_slice(&1000u32.to_be_bytes());
        assert_eq!(parse_png_dimensions(&png).unwrap(), (800, 1000));
        assert!(parse_png_dimensions(b"not a png").is_err());
        assert!(parse_png_dimensions(&png[..16]).is_err());
    }

    #[test]
    fn dpi_buckets_and_display_sizing_preserve_aspect_ratio() {
        assert_eq!(dpi_for_zoom(0.5), 96);
        assert_eq!(dpi_for_zoom(1.0), 144);
        assert_eq!(dpi_for_zoom(1.75), 216);
        assert_eq!(dpi_for_zoom(2.5), 288);

        let (fw, fh) = compute_display_size(600, 800, 144, 648.0, 1.0, PdfFitMode::FitWidth);
        assert!((fw - 600.0).abs() < 0.01);
        assert!((fh - 800.0).abs() < 0.01);

        let (aw, ah) = compute_display_size(600, 800, 144, 648.0, 1.5, PdfFitMode::ActualSize);
        assert!((aw - 600.0).abs() < 0.01);
        assert!((ah - 800.0).abs() < 0.01);
    }

    #[test]
    fn cache_evicts_lru_and_ignores_stale_generation() {
        let mut state = PdfPreview::default();
        let gen = state.reset_for_new_path("/tmp/doc.pdf".into(), None);
        let dummy = || RenderedPng {
            bytes: vec![0x89, b'P', b'N', b'G'],
            width_px: 100,
            height_px: 200,
        };
        assert!(!state.insert_page(gen.wrapping_sub(1), 1, 144, dummy()));
        for p in 1..=(MAX_CACHED_PAGES + 4) {
            state.current_page = p;
            state.visible_window = (p.saturating_sub(1).max(1), p + 1);
            assert!(state.insert_page(gen, p, 144, dummy()));
        }
        assert!(state.pages.len() <= MAX_CACHED_PAGES);
        assert!(state.pages.contains_key(&(state.current_page, 144)));
    }

    #[test]
    fn poppler_renders_real_multipage_pdf_and_handles_leading_dash_filename() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("-gtuim-test-{}.pdf", std::process::id()));
        fs::write(&path, build_sample_two_page_pdf()).expect("write sample pdf");
        let _guard = TempFileGuard(path.clone());

        let meta = probe_pdf_info(&path).expect("pdfinfo succeeds on real pdf");
        assert_eq!(meta.page_count, 2);
        assert_eq!(meta.page_size_pts, Some((300.0, 400.0)));

        let p1 = render_pdf_page_png(&path, 1, 144).expect("render page 1");
        assert_eq!((p1.width_px, p1.height_px), (600, 800));
        assert!(p1.bytes.starts_with(PNG_SIGNATURE));

        let p2 = render_pdf_page_png(&path, 2, 144).expect("render page 2");
        assert_eq!((p2.width_px, p2.height_px), (600, 800));
        assert_ne!(p1.bytes, p2.bytes);

        let missing = probe_pdf_info(Path::new("/tmp/definitely-missing-gtuim-file.pdf"));
        assert!(matches!(missing, Err(PdfError::NotFound(_))));
    }

    #[test]
    fn cached_page_navigation_clears_loading_and_stale_error() {
        let mut state = PdfPreview::default();
        let gen = state.reset_for_new_path("/tmp/doc.pdf".into(), None);
        let dpi = state.target_dpi();
        let dummy = || RenderedPng {
            bytes: PNG_SIGNATURE.to_vec(),
            width_px: 600,
            height_px: 800,
        };
        assert!(state.apply_probe_and_page_result(
            gen,
            PdfMetadata {
                page_count: 3,
                title: None,
                page_size_pts: Some((300.0, 400.0)),
            },
            1,
            dpi,
            dummy(),
        ));
        assert!(!state.loading);
        assert!(state.error.is_none());

        state.current_page = 2;
        state.inflight.insert((2, dpi));
        state.sync_current_page_flags();
        assert!(state.loading);

        state.current_page = 1;
        state.sync_current_page_flags();
        assert!(!state.loading);
        assert!(state.error.is_none());

        assert!(state.insert_page(gen, 2, dpi, dummy()));
        assert!(!state.loading);
        assert!(state.pages.contains_key(&(2, dpi)));

        state.current_page = 3;
        state.sync_current_page_flags();
        assert!(state.loading);
        assert!(state.apply_render_error(
            gen,
            3,
            dpi,
            PdfError::RenderFailed("corrupt page".into())
        ));
        assert!(state.error.is_some());
        assert!(!state.loading);

        state.current_page = 1;
        state.sync_current_page_flags();
        assert!(state.error.is_none());
        assert!(!state.loading);
        assert!(state.current_rendered_page().is_some());
    }

    #[test]
    fn reload_completion_preserves_newer_page_navigation_and_clamps_on_shrink() {
        let mut state = PdfPreview::default();
        let gen1 = state.reset_for_new_path("/tmp/doc.pdf".into(), None);
        let dpi = state.target_dpi();
        let dummy = || RenderedPng {
            bytes: PNG_SIGNATURE.to_vec(),
            width_px: 600,
            height_px: 800,
        };
        assert!(state.apply_probe_and_page_result(
            gen1,
            PdfMetadata {
                page_count: 3,
                title: None,
                page_size_pts: None,
            },
            1,
            dpi,
            dummy(),
        ));

        let gen2 = state.prepare_reload(None);
        state.current_page = 2;
        assert!(state.apply_probe_and_page_result(
            gen2,
            PdfMetadata {
                page_count: 3,
                title: None,
                page_size_pts: None,
            },
            1,
            dpi,
            dummy(),
        ));
        assert_eq!(state.current_page, 2, "reload must not jump back to page 1");
        assert!(state.pages.contains_key(&(1, dpi)));
        assert!(state.loading, "page 2 is still uncached after reload");

        let gen3 = state.prepare_reload(None);
        assert!(state.apply_probe_and_page_result(
            gen3,
            PdfMetadata {
                page_count: 1,
                title: None,
                page_size_pts: None,
            },
            1,
            dpi,
            dummy(),
        ));
        assert_eq!(state.current_page, 1);
        assert!(!state.loading);
    }

    #[test]
    fn continuous_layout_virtualizes_large_pdf_and_tracks_active_page_across_boundaries() {
        let overrides = HashMap::from([(2usize, (800.0, 400.0))]);
        let layout = PageLayout::build(
            500,
            (300.0, 400.0),
            &overrides,
            648.0,
            1.0,
            PdfFitMode::FitWidth,
        );
        let viewport_h = 600.0;
        let total = layout.total_height();

        // Check spacer invariance and bounded render window across top, middle, and bottom.
        for &target_page in &[1usize, 2, 250, 500] {
            let scroll_y = (layout.page_top(target_page) - CANVAS_PAD_PX).max(0.0);
            let (ws, we) = layout.render_window(scroll_y, viewport_h);
            assert!(
                we - ws + 1 <= MAX_VISIBLE_WINDOW_PAGES,
                "window {ws}..={we} must stay bounded <= {MAX_VISIBLE_WINDOW_PAGES}"
            );
            let (top_sp, bot_sp) = layout.spacers(ws, we);
            let rendered_span = layout.page_bottom(we) - layout.page_top(ws);
            assert!(
                (top_sp + rendered_span + bot_sp - total).abs() < 0.5,
                "spacer sum must equal total height for page {target_page}"
            );
        }

        // Scroll naturally from page 1 -> page 2 -> page 3 -> bottom (page 500).
        assert_eq!(layout.active_page_for_viewport(0.0, viewport_h), 1);
        let p2_scroll = layout.page_top(2) - 40.0;
        assert_eq!(layout.active_page_for_viewport(p2_scroll, viewport_h), 2);
        let p3_scroll = layout.page_top(3) - 40.0;
        assert_eq!(layout.active_page_for_viewport(p3_scroll, viewport_h), 3);
        let bottom_scroll = (total - viewport_h).max(0.0);
        assert_eq!(
            layout.active_page_for_viewport(bottom_scroll, viewport_h),
            500
        );
    }

    #[test]
    fn cache_eviction_protects_visible_window_pages() {
        let mut state = PdfPreview::default();
        let gen = state.reset_for_new_path("/tmp/large.pdf".into(), None);
        let dpi = state.target_dpi();
        let dummy = || RenderedPng {
            bytes: PNG_SIGNATURE.to_vec(),
            width_px: 300,
            height_px: 400,
        };
        state.metadata = Some(PdfMetadata {
            page_count: 100,
            title: None,
            page_size_pts: Some((300.0, 400.0)),
        });

        // Simulate scrolling through pages 1..=30; visible window ends at 28..=31.
        for p in 1..=30 {
            state.current_page = p;
            state.visible_window = (p.saturating_sub(1).max(1), (p + 1).min(100));
            assert!(state.insert_page(gen, p, dpi, dummy()));
        }
        assert!(state.pages.len() <= MAX_CACHED_PAGES);
        assert!(state.pages.contains_key(&(29, dpi)));
        assert!(state.pages.contains_key(&(30, dpi)));
        assert!(!state.pages.contains_key(&(1, dpi)));
    }

    #[test]
    fn active_task_permits_span_generations_across_reload_and_reopen() {
        let mut state = PdfPreview::default();
        for _ in 0..MAX_CONCURRENT_RENDERS {
            assert!(state.try_acquire_task_permit());
        }
        assert!(!state.try_acquire_task_permit());

        // Switching files or reloading clears generation-local `inflight` but MUST NOT reset permits.
        let _ = state.reset_for_new_path("/tmp/other.pdf".into(), None);
        assert_eq!(state.active_task_count(), MAX_CONCURRENT_RENDERS);
        assert!(!state.try_acquire_task_permit());

        let _ = state.prepare_reload(None);
        assert_eq!(state.active_task_count(), MAX_CONCURRENT_RENDERS);
        assert!(!state.try_acquire_task_permit());

        // Stale task completion frees a permit so the new generation can run.
        state.release_task_permit();
        assert_eq!(state.active_task_count(), MAX_CONCURRENT_RENDERS - 1);
        assert!(state.try_acquire_task_permit());
    }

    #[test]
    fn mixed_paper_sizes_with_same_aspect_preserve_actual_size_dimensions() {
        let mut state = PdfPreview::default();
        let gen = state.reset_for_new_path("/tmp/mixed.pdf".into(), None);
        state.fit_mode = PdfFitMode::ActualSize;
        state.metadata = Some(PdfMetadata {
            page_count: 2,
            title: None,
            // A4: 595 x 842 pts
            page_size_pts: Some((595.0, 842.0)),
        });
        // Page 2 is A5 (420 x 595 pts): nearly identical sqrt(2) aspect ratio, smaller physical size.
        // At 144 DPI (2x points): 840 x 1190 px.
        assert!(state.insert_page(
            gen,
            2,
            144,
            RenderedPng {
                bytes: PNG_SIGNATURE.to_vec(),
                width_px: 840,
                height_px: 1190,
            },
        ));
        let layout = state.layout_for(800.0);
        let (w1, h1) = layout.page_size(1);
        let (w2, h2) = layout.page_size(2);
        assert!(
            w2 < w1 - 100.0,
            "A5 width ({w2}) must be smaller than A4 ({w1}) in ActualSize"
        );
        assert!(
            h2 < h1 - 100.0,
            "A5 height ({h2}) must be smaller than A4 ({h1}) in ActualSize"
        );
    }

    #[test]
    fn above_viewport_page_size_change_anchors_scroll_offset() {
        let mut state = PdfPreview::default();
        let gen = state.reset_for_new_path("/tmp/mixed-orient.pdf".into(), None);
        state.last_pane_width = 648.0;
        state.metadata = Some(PdfMetadata {
            page_count: 4,
            title: None,
            page_size_pts: Some((300.0, 400.0)),
        });
        // User is viewing page 3 at scroll_y = page_top(3) + 50px.
        let before = state.layout_for(state.last_pane_width);
        let old_p3_top = before.page_top(3);
        let initial_scroll_y = old_p3_top + 50.0;
        state.current_page = 3;
        state
            .scroll
            .set_offset(point(px(0.0), px(-initial_scroll_y)));
        state.last_scroll_y = initial_scroll_y;

        // Above-viewport page 2 finishes rendering as landscape (800 x 400 pts -> shorter height at FitWidth).
        assert!(state.insert_page(
            gen,
            2,
            72,
            RenderedPng {
                bytes: PNG_SIGNATURE.to_vec(),
                width_px: 800,
                height_px: 400,
            },
        ));
        let after = state.layout_for(state.last_pane_width);
        let new_p3_top = after.page_top(3);
        let new_scroll_y = -f32::from(state.scroll.offset().y);
        assert!((new_p3_top - old_p3_top).abs() > 100.0);
        // Offset relative to page 3's top must remain locked at 50px!
        assert!(
            ((new_scroll_y - new_p3_top) - 50.0).abs() < 1.0,
            "viewport jumped: old_rel=50, new_rel={}",
            new_scroll_y - new_p3_top
        );
    }
}
