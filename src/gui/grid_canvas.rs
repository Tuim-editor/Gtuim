use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{prelude::*, *};

use crate::gui::icons::gui_palette;
use crate::nvim::ui_protocol::{GridData, UiState};
use crate::tui::shell_terminal::ShellTerminal;
use crate::tui::theme::Theme;

/// Collect all font family names known to GPUI's `fontdb`.
pub fn collect_system_fonts(window: &Window) -> Vec<String> {
    let mut names = window.text_system().all_font_names();
    names.sort_by_key(|a| a.to_ascii_lowercase());
    names.dedup();
    names
}

/// Resolve a user-configured monospace font family name against available system fonts.
/// Empty strings fall back to `fallback_mono`. Case-insensitive matches return the exact
/// fontdb family name so GPUI resolves it reliably.
pub fn resolve_configured_mono_font(
    configured: &str,
    fallback_mono: &SharedString,
    available: &[String],
) -> SharedString {
    let trimmed = configured.trim();
    if trimmed.is_empty() {
        return fallback_mono.clone();
    }
    if let Some(exact) = available.iter().find(|f| f.as_str() == trimmed) {
        return exact.clone().into();
    }
    if let Some(ci) = available.iter().find(|f| f.eq_ignore_ascii_case(trimmed)) {
        return ci.clone().into();
    }
    if available.is_empty() {
        return trimmed.to_string().into();
    }
    fallback_mono.clone()
}

/// Map a numeric CSS-style font weight (300..=700) to GPUI's `FontWeight`.
pub fn font_weight_from_u16(weight: u16) -> FontWeight {
    match weight {
        ..=349 => FontWeight::LIGHT,
        350..=449 => FontWeight::NORMAL,
        450..=549 => FontWeight::MEDIUM,
        550..=649 => FontWeight::SEMIBOLD,
        _ => FontWeight::BOLD,
    }
}

/// Choose a visibly heavier bold weight relative to the configured base weight.
pub fn bold_weight_for(base: FontWeight) -> FontWeight {
    if base.0 <= FontWeight::LIGHT.0 {
        FontWeight::SEMIBOLD
    } else if base.0 <= FontWeight::MEDIUM.0 {
        FontWeight::BOLD
    } else if base.0 <= FontWeight::SEMIBOLD.0 {
        FontWeight::EXTRA_BOLD
    } else {
        FontWeight::BLACK
    }
}

/// Resolve an exact monospace font family name present in GPUI's `fontdb`.
pub fn resolve_mono_font(window: &Window) -> SharedString {
    let available = window.text_system().all_font_names();

    if let Ok(out) = Command::new("fc-match")
        .args(["-f", "%{family[0]}", "monospace"])
        .output()
    {
        if out.status.success() {
            let fam = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !fam.is_empty() && available.iter().any(|f| f == &fam) {
                return fam.into();
            }
        }
    }

    const PREFERRED: &[&str] = &[
        "JetBrainsMono Nerd Font",
        "JetBrainsMono Nerd Font Mono",
        "JetBrains Mono",
        "CaskaydiaMono Nerd Font",
        "CaskaydiaMono Nerd Font Mono",
        "Cascadia Mono",
        "FiraCode Nerd Font",
        "Fira Code",
        "Iosevka Nerd Font Mono",
        "IosevkaTerm Nerd Font Mono",
        "Adwaita Mono",
        "Liberation Mono",
        "DejaVu Sans Mono",
        "Ubuntu Mono",
        "Noto Sans Mono",
    ];
    for candidate in PREFERRED {
        if available.iter().any(|f| f == candidate) {
            return (*candidate).into();
        }
    }

    if let Some(found) = available.iter().find(|f| f.contains("Mono")) {
        return found.clone().into();
    }

    "monospace".into()
}

/// Resolve a crisp UI sans-serif font family present in GPUI's `fontdb`.
pub fn resolve_ui_font(window: &Window) -> SharedString {
    let available = window.text_system().all_font_names();
    const PREFERRED: &[&str] = &[
        "Inter",
        "Adwaita Sans",
        "Cantarell",
        "IBM Plex Sans",
        "Noto Sans",
        "Liberation Sans",
        "DejaVu Sans",
    ];
    for candidate in PREFERRED {
        if available.iter().any(|f| f == candidate) {
            return (*candidate).into();
        }
    }
    if let Ok(out) = Command::new("fc-match")
        .args(["-f", "%{family[0]}", "sans-serif"])
        .output()
    {
        if out.status.success() {
            let fam = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !fam.is_empty() && available.iter().any(|f| f == &fam) {
                return fam.into();
            }
        }
    }
    ".SystemUIFont".into()
}

/// Common text stays inline. Exceptional graphemes use a frame-local side table,
/// keeping every cell 24 bytes without truncating ZWJ/combining sequences.
#[derive(Clone, Copy, Debug)]
pub struct CompactCell {
    pub bytes: [u8; 12],
    pub len: u8,
    pub flags: u8, // bold, italic, underline, continuation, side-table text
    pub fg: u32,
    pub bg: u32,
}

const FLAG_BOLD: u8 = 1 << 0;
const FLAG_ITALIC: u8 = 1 << 1;
const FLAG_UNDERLINE: u8 = 1 << 2;
const FLAG_CONT: u8 = 1 << 3;
const FLAG_OVERFLOW: u8 = 1 << 4;

impl CompactCell {
    #[inline]
    pub const fn blank(fg: u32, bg: u32) -> Self {
        let mut bytes = [0u8; 12];
        bytes[0] = b' ';
        Self {
            bytes,
            len: 1,
            flags: 0,
            fg,
            bg,
        }
    }

    #[inline]
    pub fn set_text(&mut self, s: &str, overflow: &mut Vec<Arc<str>>) {
        self.flags &= !(FLAG_CONT | FLAG_OVERFLOW);
        if s.is_empty() {
            self.len = 0;
            self.flags |= FLAG_CONT;
            return;
        }
        let clean;
        let text = if s.chars().any(char::is_control) {
            clean = s.chars().filter(|c| !c.is_control()).collect::<String>();
            if clean.is_empty() {
                " "
            } else {
                &clean
            }
        } else {
            s
        };
        if text.len() > self.bytes.len() {
            let index = overflow.len() as u64;
            overflow.push(Arc::from(text));
            self.bytes[..8].copy_from_slice(&index.to_le_bytes());
            self.len = 8;
            self.flags |= FLAG_OVERFLOW;
        } else {
            self.bytes[..text.len()].copy_from_slice(text.as_bytes());
            self.len = text.len() as u8;
        }
    }

    fn overflow_text(&self, overflow: &[Arc<str>]) -> Option<Arc<str>> {
        if self.flags & FLAG_OVERFLOW == 0 {
            return None;
        }
        let index = u64::from_le_bytes(self.bytes[..8].try_into().unwrap()) as usize;
        overflow.get(index).cloned()
    }

    #[inline]
    pub fn as_str<'a>(&'a self, overflow: &'a [Arc<str>]) -> &'a str {
        if self.flags & FLAG_OVERFLOW != 0 {
            let index = u64::from_le_bytes(self.bytes[..8].try_into().unwrap()) as usize;
            return &overflow[index];
        }
        if self.len == 0 || (self.flags & FLAG_CONT) != 0 {
            " "
        } else {
            std::str::from_utf8(&self.bytes[..self.len as usize]).expect("cell UTF-8")
        }
    }

    #[inline]
    pub fn bold(&self) -> bool {
        (self.flags & FLAG_BOLD) != 0
    }
    #[inline]
    pub fn italic(&self) -> bool {
        (self.flags & FLAG_ITALIC) != 0
    }
    #[inline]
    pub fn underline(&self) -> bool {
        (self.flags & FLAG_UNDERLINE) != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorKind {
    Block,
    Beam,
    Outline,
}

#[derive(Clone, Debug)]
pub struct CursorSnapshot {
    pub col: usize,
    pub row: usize,
    pub fg: u32,
    pub bg: u32,
    pub kind: CursorKind,
    pub glyph: String,
    pub span: usize,
}

impl CursorSnapshot {
    fn glyph_str(&self) -> &str {
        &self.glyph
    }
}

/// Content-addressed cached row (`row_hash -> (bg_runs, ShapedLine)`).
/// Survives scrolling: when lines shift vertically, 49/50 rows still hit in O(1)!
#[derive(Clone, Default)]
pub struct CachedLine {
    bg_runs: Vec<(usize, usize, u32)>,
    shaped: Option<ShapedLine>,
    positioned: Vec<(usize, ShapedLine)>,
    last_frame: u64,
}

pub struct GridBuffer {
    pub cols: usize,
    pub rows: usize,
    pub default_fg: u32,
    pub default_bg: u32,
    pub cells: Vec<CompactCell>,
    overflow: Vec<Arc<str>>,
    pub cursor: Option<CursorSnapshot>,
    pub status_footer: Option<String>,
    pub scrollback_lines: usize,
    pub generation: u64,
    // Measurement & shaping cache
    cached_font: SharedString,
    cached_font_size_bits: u32,
    cached_font_weight_bits: u32,
    cached_cell_w: Pixels,
    cached_cell_h: Pixels,
    frame_counter: u64,
    line_cache: HashMap<u64, CachedLine>,
    // Scratch buffers reused across dirty rows
    scratch_text: String,
    scratch_runs: Vec<TextRun>,
}

impl Default for GridBuffer {
    fn default() -> Self {
        Self {
            cols: 1,
            rows: 1,
            default_fg: 0xd4d4d4,
            default_bg: 0x1e1e1e,
            cells: vec![CompactCell::blank(0xd4d4d4, 0x1e1e1e)],
            overflow: Vec::new(),
            cursor: None,
            status_footer: None,
            scrollback_lines: 0,
            generation: 0,
            cached_font: SharedString::default(),
            cached_font_size_bits: 0,
            cached_font_weight_bits: 0,
            cached_cell_w: px(8.4),
            cached_cell_h: px(20.0),
            frame_counter: 0,
            line_cache: HashMap::with_capacity(256),
            scratch_text: String::with_capacity(256),
            scratch_runs: Vec::with_capacity(32),
        }
    }
}

impl GridBuffer {
    pub fn reset(&mut self, cols: usize, rows: usize, fg: u32, bg: u32) {
        self.cols = cols.max(1);
        self.rows = rows.max(1);
        self.default_fg = fg;
        self.default_bg = bg;
        let total = self.cols * self.rows;
        self.overflow.clear();
        self.cells.clear();
        self.cells.resize(total, CompactCell::blank(fg, bg));
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GridMetrics {
    pub bounds: Bounds<Pixels>,
    pub cell_w: Pixels,
    pub cell_h: Pixels,
    pub cols: u16,
    pub rows: u16,
}

impl Default for GridMetrics {
    fn default() -> Self {
        Self {
            bounds: Bounds::default(),
            cell_w: px(8.4),
            cell_h: px(20.0),
            cols: 80,
            rows: 24,
        }
    }
}

impl GridMetrics {
    pub fn pixel_to_cell(&self, pos: Point<Pixels>) -> (u16, u16) {
        let rel_x = (pos.x - self.bounds.origin.x).max(px(0.0));
        let rel_y = (pos.y - self.bounds.origin.y).max(px(0.0));
        let cw = f32::from(self.cell_w).max(1.0);
        let ch = f32::from(self.cell_h).max(1.0);
        let col = (f32::from(rel_x) / cw).floor() as u16;
        let row = (f32::from(rel_y) / ch).floor() as u16;
        (
            col.min(self.cols.saturating_sub(1)),
            row.min(self.rows.saturating_sub(1)),
        )
    }
}

#[inline]
fn resolve_nvim_hl(ui: &UiState, hl_id: u64, def_fg: u32, def_bg: u32) -> (u32, u32, u8) {
    if hl_id == 0 {
        return (def_fg, def_bg, 0);
    }
    if let Some(attr) = ui.highlights.get(&hl_id) {
        let mut fg = attr.fg.unwrap_or(def_fg);
        let mut bg = attr.bg.unwrap_or(def_bg);
        if attr.reverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        let mut flags = 0u8;
        if attr.bold {
            flags |= FLAG_BOLD;
        }
        if attr.italic {
            flags |= FLAG_ITALIC;
        }
        if attr.underline {
            flags |= FLAG_UNDERLINE;
        }
        (fg, bg, flags)
    } else {
        (def_fg, def_bg, 0)
    }
}

fn blit_nvim_grid(
    dest: &mut [CompactCell],
    overflow: &mut Vec<Arc<str>>,
    dest_cols: usize,
    dest_rows: usize,
    ui: &UiState,
    grid: &GridData,
    offset_col: usize,
    offset_row: usize,
    def_fg: u32,
    def_bg: u32,
) {
    let rows = grid.height.min(dest_rows.saturating_sub(offset_row));
    let cols = grid.width.min(dest_cols.saturating_sub(offset_col));
    let mut last_hl_id = u64::MAX;
    let mut last_resolved = (def_fg, def_bg, 0u8);

    for r in 0..rows {
        let src_row = r * grid.width;
        let dst_row = (offset_row + r) * dest_cols + offset_col;
        for c in 0..cols {
            let src = &grid.cells[src_row + c];
            if src.hl_id != last_hl_id {
                last_hl_id = src.hl_id;
                last_resolved = resolve_nvim_hl(ui, src.hl_id, def_fg, def_bg);
            }
            let dst = &mut dest[dst_row + c];
            dst.fg = last_resolved.0;
            dst.bg = last_resolved.1;
            dst.flags = last_resolved.2;
            dst.set_text(&src.text, overflow);
        }
    }
}

pub fn update_nvim_buffer(
    buf: &mut GridBuffer,
    ui: &UiState,
    theme: &Theme,
    focused: bool,
    is_ide_mode: bool,
) {
    let cols = ui.grid.width.max(1);
    let rows = ui.grid.height.max(1);
    let def_fg = if ui.default_fg != 0 {
        ui.default_fg
    } else {
        theme.fg_primary
    };
    let def_bg = if ui.default_bg != 0 {
        ui.default_bg
    } else {
        theme.bg_editor
    };

    buf.reset(cols, rows, def_fg, def_bg);
    blit_nvim_grid(
        &mut buf.cells,
        &mut buf.overflow,
        cols,
        rows,
        ui,
        &ui.grid,
        0,
        0,
        def_fg,
        def_bg,
    );

    let mut secs: Vec<(u64, &GridData)> = ui
        .secondary_grids
        .iter()
        .filter(|(_, g)| !g.hidden)
        .map(|(k, g)| (*k, g))
        .collect();
    secs.sort_by_key(|(id, g)| (g.is_float, g.zindex, *id));

    for (_, sg) in &secs {
        let ox = sg.win_col.max(0) as usize;
        let oy = sg.win_row.max(0) as usize;
        if ox < cols && oy < rows {
            blit_nvim_grid(
                &mut buf.cells,
                &mut buf.overflow,
                cols,
                rows,
                ui,
                sg,
                ox,
                oy,
                def_fg,
                def_bg,
            );
        }
    }

    let (mut cx, mut cy) = (ui.cursor_col, ui.cursor_row);
    if ui.cursor_grid != 1 {
        if let Some((_, sg)) = secs.iter().find(|(id, _)| *id == ui.cursor_grid) {
            cx = (sg.win_col.max(0) as usize).saturating_add(sg.cursor_col);
            cy = (sg.win_row.max(0) as usize).saturating_add(sg.cursor_row);
        }
    }

    buf.cursor = if cx < cols && cy < rows {
        let cell = buf.cells[cy * cols + cx];
        let is_insert = is_ide_mode
            || ui.current_mode == "insert"
            || ui.current_mode == "cmdline_normal"
            || ui.current_mode == "cmdline_insert";
        let kind = if !focused {
            CursorKind::Outline
        } else if is_insert {
            CursorKind::Beam
        } else {
            CursorKind::Block
        };
        Some(CursorSnapshot {
            col: cx,
            row: cy,
            fg: def_bg,
            bg: theme.accent,
            kind,
            glyph: cell.as_str(&buf.overflow).to_owned(),
            span: cell_span(&buf.cells[cy * cols..(cy + 1) * cols], cx),
        })
    } else {
        None
    };
    buf.status_footer = None;
    buf.scrollback_lines = 0;
}

pub fn update_shell_buffer(
    buf: &mut GridBuffer,
    term: &ShellTerminal,
    theme: &Theme,
    focused: bool,
) {
    let screen = term.screen();
    let (rows_u16, cols_u16) = screen.size();
    let cols = (cols_u16 as usize).max(1);
    let rows = (rows_u16 as usize).max(1);
    let p = gui_palette(theme);

    buf.reset(cols, rows, theme.fg_terminal, theme.bg_terminal);

    let sel_range = term
        .selection()
        .map(|(a, b)| if a <= b { (a, b) } else { (b, a) });

    for y in 0..rows {
        let row_off = y * cols;
        for x in 0..cols {
            let Some(cell) = screen.cell(y as u16, x as u16) else {
                continue;
            };
            let (mut fg, mut bg) = term.resolve_cell_colors(cell, theme);
            if let Some((start, end)) = sel_range {
                if (start..=end).contains(&(y as u16, x as u16)) {
                    bg = p.bg_selected;
                    fg = p.text_strong;
                }
            }
            let mut flags = 0u8;
            if cell.bold() {
                flags |= FLAG_BOLD;
            }
            if cell.italic() {
                flags |= FLAG_ITALIC;
            }
            if cell.underline() {
                flags |= FLAG_UNDERLINE;
            }
            let dst = &mut buf.cells[row_off + x];
            dst.fg = fg;
            dst.bg = bg;
            dst.flags = flags;
            if cell.is_wide_continuation() {
                dst.set_text("", &mut buf.overflow);
            } else if cell.has_contents() {
                dst.set_text(cell.contents(), &mut buf.overflow);
            } else {
                dst.bytes[0] = b' ';
                dst.len = 1;
            }
        }
    }

    let scrollback_lines = screen.scrollback();
    buf.cursor = if term.is_running()
        && scrollback_lines == 0
        && !screen.hide_cursor()
        && term.selection().is_none()
    {
        let (cy, cx) = screen.cursor_position();
        let (cx, cy) = (cx as usize, cy as usize);
        if cx < cols && cy < rows {
            let cell = buf.cells[cy * cols + cx];
            Some(CursorSnapshot {
                col: cx,
                row: cy,
                fg: theme.bg_terminal,
                bg: theme.fg_terminal,
                kind: if focused {
                    CursorKind::Block
                } else {
                    CursorKind::Outline
                },
                glyph: cell.as_str(&buf.overflow).to_owned(),
                span: cell_span(&buf.cells[cy * cols..(cy + 1) * cols], cx),
            })
        } else {
            None
        }
    } else {
        None
    };

    buf.status_footer = if !term.status().is_empty() {
        Some(term.status().to_string())
    } else {
        None
    };
    buf.scrollback_lines = scrollback_lines;
}

fn cell_span(row: &[CompactCell], col: usize) -> usize {
    1 + row
        .iter()
        .skip(col + 1)
        .take_while(|c| c.flags & FLAG_CONT != 0)
        .count()
}

pub const GRID_PADDING_X: f32 = 8.0;
pub const GRID_PADDING_Y: f32 = 6.0;

pub struct CellGridElement {
    buffer: Rc<RefCell<GridBuffer>>,
    metrics: Rc<RefCell<GridMetrics>>,
    font_family: SharedString,
    font_size: Pixels,
    font_weight: FontWeight,
    padding_x: Pixels,
    padding_y: Pixels,
}

impl CellGridElement {
    pub fn new(
        buffer: Rc<RefCell<GridBuffer>>,
        metrics: Rc<RefCell<GridMetrics>>,
        font_family: impl Into<SharedString>,
        font_size: Pixels,
    ) -> Self {
        Self {
            buffer,
            metrics,
            font_family: font_family.into(),
            font_size,
            font_weight: FontWeight::NORMAL,
            padding_x: px(GRID_PADDING_X),
            padding_y: px(GRID_PADDING_Y),
        }
    }

    pub fn with_font_weight(mut self, weight: FontWeight) -> Self {
        self.font_weight = weight;
        self
    }
}

pub struct GridPrepaintState {
    default_bg: u32,
    cell_h: Pixels,
    bg_quads: Vec<PaintQuad>,
    lines: Vec<(Point<Pixels>, ShapedLine)>,
    cursor_quad: Option<PaintQuad>,
    cursor_glyph: Option<(Point<Pixels>, ShapedLine)>,
}

impl IntoElement for CellGridElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

fn make_mono_font(family: &SharedString, base_weight: FontWeight, bold: bool, italic: bool) -> Font {
    Font {
        family: family.clone(),
        features: FontFeatures::disable_ligatures(),
        fallbacks: None,
        weight: if bold {
            bold_weight_for(base_weight)
        } else {
            base_weight
        },
        style: if italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
    }
}

#[inline]
fn hash_row(
    cells: &[CompactCell],
    overflow: &[Arc<str>],
    default_bg: u32,
    font_size_bits: u32,
    font_weight_bits: u32,
    font_name: &str,
) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    default_bg.hash(&mut h);
    font_size_bits.hash(&mut h);
    font_weight_bits.hash(&mut h);
    font_name.hash(&mut h);
    cells.len().hash(&mut h);
    for c in cells {
        c.fg.hash(&mut h);
        c.bg.hash(&mut h);
        c.flags.hash(&mut h);
        c.len.hash(&mut h);
        c.as_str(overflow).hash(&mut h);
    }
    h.finish()
}

impl Element for CellGridElement {
    type RequestLayoutState = ();
    type PrepaintState = GridPrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) -> GridPrepaintState {
        let mut buf = self.buffer.borrow_mut();
        let font_size_bits = f32::from(self.font_size).to_bits();
        let font_weight_bits = self.font_weight.0.to_bits();
        let base_font = make_mono_font(&self.font_family, self.font_weight, false, false);

        if buf.cached_font != self.font_family
            || buf.cached_font_size_bits != font_size_bits
            || buf.cached_font_weight_bits != font_weight_bits
        {
            let probe_text = "0000000000";
            let probe_run = TextRun {
                len: probe_text.len(),
                font: base_font.clone(),
                color: rgb(buf.default_fg).into(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let probe = window.text_system().shape_line(
                probe_text.into(),
                self.font_size,
                &[probe_run],
                None,
            );
            buf.cached_cell_w = px((f32::from(probe.width) / 10.0).max(6.0));
            buf.cached_cell_h = px((f32::from(self.font_size) * 1.48).ceil()).max(px(16.0));
            buf.cached_font = self.font_family.clone();
            buf.cached_font_size_bits = font_size_bits;
            buf.cached_font_weight_bits = font_weight_bits;
            buf.line_cache.clear();
        }

        buf.frame_counter = buf.frame_counter.wrapping_add(1);
        let current_frame = buf.frame_counter;
        if buf.line_cache.len() > 512 {
            // Non-ASCII rows carry positioned grapheme shapes. Bound the cache
            // by count as well as age, even during rapid continuous scrolling.
            let mut ages: Vec<_> = buf
                .line_cache
                .iter()
                .map(|(key, row)| (*key, row.last_frame))
                .collect();
            ages.sort_unstable_by_key(|(_, frame)| *frame);
            for (key, _) in ages.into_iter().take(buf.line_cache.len() - 512) {
                buf.line_cache.remove(&key);
            }
        }

        let cell_w = buf.cached_cell_w;
        let cell_h = buf.cached_cell_h;
        let inner_w = (bounds.size.width - self.padding_x * 2.0).max(cell_w);
        let inner_h = (bounds.size.height - self.padding_y * 2.0).max(cell_h);
        let avail_cols = (f32::from(inner_w) / f32::from(cell_w)).floor().max(1.0) as u16;
        let avail_rows = (f32::from(inner_h) / f32::from(cell_h)).floor().max(1.0) as u16;

        let content_origin = point(
            bounds.origin.x + self.padding_x,
            bounds.origin.y + self.padding_y,
        );

        {
            let mut m = self.metrics.borrow_mut();
            m.bounds = Bounds::new(content_origin, size(inner_w, inner_h));
            m.cell_w = cell_w;
            m.cell_h = cell_h;
            m.cols = avail_cols;
            m.rows = avail_rows;
        }

        let draw_rows = buf.rows.min(avail_rows as usize);
        let draw_cols = buf.cols.min(avail_cols as usize);
        let default_bg = buf.default_bg;

        let mut bg_quads = Vec::with_capacity(draw_rows * 2);
        let mut lines = Vec::with_capacity(draw_rows);

        // Pre-build the 4 monospace font variants once per frame (instead of per cell!).
        let font_variants = [
            base_font.clone(),
            make_mono_font(&self.font_family, self.font_weight, true, false),
            make_mono_font(&self.font_family, self.font_weight, false, true),
            make_mono_font(&self.font_family, self.font_weight, true, true),
        ];

        for r in 0..draw_rows {
            let row_y = content_origin.y + cell_h * (r as f32);
            let row_start = r * buf.cols;
            let row_end = row_start + draw_cols;

            let row_hash = hash_row(
                &buf.cells[row_start..row_end],
                &buf.overflow,
                default_bg,
                font_size_bits,
                font_weight_bits,
                self.font_family.as_ref(),
            );

            if !buf.line_cache.contains_key(&row_hash) {
                let mut bg_runs = Vec::new();
                let mut c = 0usize;
                while c < draw_cols {
                    let bg_color = buf.cells[row_start + c].bg;
                    if bg_color != default_bg {
                        let start_c = c;
                        c += 1;
                        while c < draw_cols && buf.cells[row_start + c].bg == bg_color {
                            c += 1;
                        }
                        bg_runs.push((start_c, c - start_c, bg_color));
                    } else {
                        c += 1;
                    }
                }

                buf.scratch_text.clear();
                buf.scratch_runs.clear();
                let mut last_style: Option<(u32, u8)> = None;
                let mut has_non_ascii = false;
                let mut has_visible = false;

                for col in 0..draw_cols {
                    let cell = buf.cells[row_start + col];
                    let long_text = cell.overflow_text(&buf.overflow);
                    let s = long_text.as_deref().unwrap_or_else(|| cell.as_str(&[]));
                    if s != " " {
                        has_visible = true;
                        if !s.is_ascii() {
                            has_non_ascii = true;
                        }
                    }
                    let byte_len = s.len();
                    buf.scratch_text.push_str(s);

                    let style_bits = cell.flags & (FLAG_BOLD | FLAG_ITALIC | FLAG_UNDERLINE);
                    let style_key = (cell.fg, style_bits);
                    if last_style == Some(style_key) {
                        if let Some(last) = buf.scratch_runs.last_mut() {
                            last.len += byte_len;
                        }
                    } else {
                        let color: Hsla = rgb(cell.fg).into();
                        let variant_idx =
                            (usize::from(cell.bold())) | (usize::from(cell.italic()) << 1);
                        let underline = if cell.underline() {
                            Some(UnderlineStyle {
                                color: Some(color),
                                thickness: px(1.0),
                                wavy: false,
                            })
                        } else {
                            None
                        };
                        buf.scratch_runs.push(TextRun {
                            len: byte_len,
                            font: font_variants[variant_idx].clone(),
                            color,
                            background_color: None,
                            underline,
                            strikethrough: None,
                        });
                        last_style = Some(style_key);
                    }
                }

                // Neovim has already chosen visual terminal cells (including Arabic
                // presentation forms). Never let paragraph BiDi or font fallback
                // advances move glyphs away from those cursor/hit-test coordinates.
                let mut positioned = Vec::new();
                if has_non_ascii {
                    for col in 0..draw_cols {
                        let cell = buf.cells[row_start + col];
                        if cell.flags & FLAG_CONT != 0 {
                            continue;
                        }
                        let text = cell.as_str(&buf.overflow);
                        if text == " " {
                            continue;
                        }
                        let color: Hsla = rgb(cell.fg).into();
                        let variant = usize::from(cell.bold()) | (usize::from(cell.italic()) << 1);
                        let run = TextRun {
                            len: text.len(),
                            font: font_variants[variant].clone(),
                            color,
                            background_color: None,
                            strikethrough: None,
                            underline: cell.underline().then_some(UnderlineStyle {
                                color: Some(color),
                                thickness: px(1.),
                                wavy: false,
                            }),
                        };
                        positioned.push((
                            col,
                            window.text_system().shape_line(
                                text.to_owned().into(),
                                self.font_size,
                                &[run],
                                None,
                            ),
                        ));
                    }
                }
                let shaped = if has_visible && !has_non_ascii {
                    Some(window.text_system().shape_line(
                        SharedString::from(buf.scratch_text.clone()),
                        self.font_size,
                        &buf.scratch_runs,
                        None,
                    ))
                } else {
                    None
                };

                buf.line_cache.insert(
                    row_hash,
                    CachedLine {
                        bg_runs,
                        shaped,
                        positioned,
                        last_frame: current_frame,
                    },
                );
            }

            if let Some(entry) = buf.line_cache.get_mut(&row_hash) {
                entry.last_frame = current_frame;
                for &(start_c, span, bg_color) in &entry.bg_runs {
                    let x = content_origin.x + cell_w * (start_c as f32);
                    let w = cell_w * (span as f32);
                    bg_quads.push(fill(
                        Bounds::new(point(x, row_y), size(w, cell_h)),
                        rgb(bg_color),
                    ));
                }
                for (col, shaped) in &entry.positioned {
                    lines.push((
                        point(content_origin.x + cell_w * (*col as f32), row_y),
                        shaped.clone(),
                    ));
                }
                if let Some(shaped) = &entry.shaped {
                    lines.push((point(content_origin.x, row_y), shaped.clone()));
                }
            }
        }

        // Cursor
        let mut cursor_quad = None;
        let mut cursor_glyph = None;
        if let Some(cur) = &buf.cursor {
            if cur.col < draw_cols && cur.row < draw_rows {
                let cx = content_origin.x + cell_w * (cur.col as f32);
                let cy = content_origin.y + cell_h * (cur.row as f32);
                match cur.kind {
                    CursorKind::Block => {
                        cursor_quad = Some(fill(
                            Bounds::new(point(cx, cy), size(cell_w * cur.span as f32, cell_h)),
                            rgb(cur.bg),
                        ));
                        let glyph_str = cur.glyph_str();
                        let run = TextRun {
                            len: glyph_str.len(),
                            font: base_font,
                            color: rgb(cur.fg).into(),
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let shaped = window.text_system().shape_line(
                            glyph_str.to_string().into(),
                            self.font_size,
                            &[run],
                            None,
                        );
                        cursor_glyph = Some((point(cx, cy), shaped));
                    }
                    CursorKind::Beam => {
                        cursor_quad = Some(fill(
                            Bounds::new(point(cx, cy), size(px(2.0), cell_h)),
                            rgb(cur.bg),
                        ));
                    }
                    CursorKind::Outline => {
                        cursor_quad = Some(PaintQuad {
                            bounds: Bounds::new(
                                point(cx, cy),
                                size(cell_w * cur.span as f32, cell_h),
                            ),
                            corner_radii: Corners::all(px(1.5)),
                            background: Hsla::transparent_black().into(),
                            border_widths: Edges::all(px(1.0)),
                            border_color: rgb(cur.bg).into(),
                            border_style: BorderStyle::Solid,
                        });
                    }
                }
            }
        }

        GridPrepaintState {
            default_bg,
            cell_h,
            bg_quads,
            lines,
            cursor_quad,
            cursor_glyph,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        state: &mut GridPrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.paint_quad(fill(bounds, rgb(state.default_bg)));
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for quad in state.bg_quads.drain(..) {
                window.paint_quad(quad);
            }
            for (origin, line) in &state.lines {
                let _ = line.paint(*origin, state.cell_h, window, cx);
            }
            if let Some(quad) = state.cursor_quad.take() {
                window.paint_quad(quad);
            }
            if let Some((origin, line)) = &state.cursor_glyph {
                let _ = line.paint(*origin, state.cell_h, window, cx);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn exceptional_graphemes_are_lossless_without_bloating_cells() {
        assert_eq!(std::mem::size_of::<CompactCell>(), 24);
        let mut overflow = Vec::new();
        let mut cell = CompactCell::blank(1, 2);
        for text in [
            "A",
            "漢",
            "👨‍👩‍👧‍👦",
            "👩🏽‍🚀",
            "a\u{301}\u{302}\u{303}\u{304}\u{305}\u{306}\u{307}",
        ] {
            cell.set_text(text, &mut overflow);
            assert_eq!(cell.as_str(&overflow), text);
        }
        cell.set_text("", &mut overflow);
        assert_ne!(cell.flags & FLAG_CONT, 0);
        cell.set_text("X", &mut overflow);
        assert_eq!(cell.flags & (FLAG_CONT | FLAG_OVERFLOW), 0);
    }

    #[::core::prelude::v1::test]
    fn overflow_hash_uses_text_not_transient_side_table_index() {
        let mut a = CompactCell::blank(1, 2);
        let mut b = a;
        let mut first = Vec::new();
        let mut second = vec![Arc::from("unused")];
        a.set_text("👨‍👩‍👧‍👦", &mut first);
        b.set_text("👨‍👩‍👧‍👦", &mut second);
        assert_eq!(
            hash_row(&[a], &first, 0, 0, 400.0f32.to_bits(), "mono"),
            hash_row(&[b], &second, 0, 0, 400.0f32.to_bits(), "mono")
        );
        b.set_text("👩🏽‍🚀", &mut second);
        assert_ne!(
            hash_row(&[a], &first, 0, 0, 400.0f32.to_bits(), "mono"),
            hash_row(&[b], &second, 0, 0, 400.0f32.to_bits(), "mono")
        );
    }

    #[::core::prelude::v1::test]
    fn font_configuration_resolves_family_and_weight_and_invalidates_row_hash() {
        let fallback = SharedString::from("JetBrains Mono");
        let available = vec![
            "DejaVu Sans Mono".to_string(),
            "Fira Code".to_string(),
            "JetBrains Mono".to_string(),
        ];
        assert_eq!(
            resolve_configured_mono_font("", &fallback, &available),
            fallback
        );
        assert_eq!(
            resolve_configured_mono_font("fira code", &fallback, &available),
            SharedString::from("Fira Code")
        );
        assert_eq!(
            resolve_configured_mono_font("Missing Font", &fallback, &available),
            fallback
        );
        assert_eq!(font_weight_from_u16(300), FontWeight::LIGHT);
        assert_eq!(font_weight_from_u16(400), FontWeight::NORMAL);
        assert_eq!(font_weight_from_u16(500), FontWeight::MEDIUM);
        assert_eq!(font_weight_from_u16(600), FontWeight::SEMIBOLD);
        assert_eq!(font_weight_from_u16(700), FontWeight::BOLD);
        assert!(bold_weight_for(FontWeight::MEDIUM).0 > FontWeight::MEDIUM.0);
        assert!(bold_weight_for(FontWeight::BOLD).0 > FontWeight::BOLD.0);

        let cell = CompactCell::blank(1, 2);
        assert_ne!(
            hash_row(&[cell], &[], 0, 14.0f32.to_bits(), FontWeight::NORMAL.0.to_bits(), "Fira Code"),
            hash_row(&[cell], &[], 0, 14.0f32.to_bits(), FontWeight::BOLD.0.to_bits(), "Fira Code")
        );
    }

    #[::core::prelude::v1::test]
    fn continuation_has_no_extra_glyph_and_cursor_spans_wide_cell() {
        let mut row = [CompactCell::blank(1, 2); 4];
        let mut overflow = Vec::new();
        row[0].set_text("漢", &mut overflow);
        row[1].set_text("", &mut overflow);
        row[2].set_text("א", &mut overflow);
        assert_eq!(cell_span(&row, 0), 2);
        assert_eq!(cell_span(&row, 2), 1);
        let mut buf = GridBuffer::default();
        buf.overflow.push(Arc::from("long grapheme"));
        buf.reset(80, 24, 1, 2);
        assert!(
            buf.overflow.is_empty(),
            "frame reset must not leak old graphemes"
        );
    }
}
