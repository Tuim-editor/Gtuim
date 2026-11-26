use gpui::{prelude::*, *};

use crate::gui::grid_canvas::{GridMetrics, GRID_PADDING_X, GRID_PADDING_Y};
use crate::gui::icons::{icon, GuiPalette};
use crate::tui::app::NvimWin;

/// Drag payload for moving a file or folder inside the Explorer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplorerDrag {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
}

/// Active explorer drag state held on `GtuimView` for visual feedback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplorerDragState {
    pub source_path: String,
    pub source_name: String,
    pub is_dir: bool,
}

/// Floating pill rendered under the cursor while dragging an explorer item.
pub struct ExplorerDragGhost {
    pub name: String,
    pub is_dir: bool,
    pub palette: GuiPalette,
}

impl Render for ExplorerDragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let ic = if self.is_dir { "folder" } else { "file-code" };
        div()
            .px(px(10.0))
            .py(px(5.0))
            .rounded(px(6.0))
            .bg(rgb(p.bg_elevated))
            .border_1()
            .border_color(rgb(p.accent))
            .shadow_md()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(icon(ic, 13.0, p.accent))
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(p.text_strong))
                    .whitespace_nowrap()
                    .child(self.name.clone()),
            )
    }
}

/// Drag payload for rearranging Neovim split panes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitDrag {
    pub win_id: i64,
    pub bufnr: i64,
    pub title: String,
    pub is_ai: bool,
    pub is_terminal: bool,
}

/// Target rearrangement operation for a split drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitMoveKind {
    DockLeft,
    DockRight,
    DockTop,
    DockBottom,
    SplitLeft(i64),
    SplitRight(i64),
    SplitAbove(i64),
    SplitBelow(i64),
    Swap(i64),
}

impl SplitMoveKind {
    /// Returns `(lua_action, target_win_id)` expected by `_G.tuim_move_split`.
    pub fn to_lua_args(self) -> (&'static str, i64) {
        match self {
            SplitMoveKind::DockLeft => ("dock_left", 0),
            SplitMoveKind::DockRight => ("dock_right", 0),
            SplitMoveKind::DockTop => ("dock_top", 0),
            SplitMoveKind::DockBottom => ("dock_bottom", 0),
            SplitMoveKind::SplitLeft(id) => ("left", id),
            SplitMoveKind::SplitRight(id) => ("right", id),
            SplitMoveKind::SplitAbove(id) => ("above", id),
            SplitMoveKind::SplitBelow(id) => ("below", id),
            SplitMoveKind::Swap(id) => ("swap", id),
        }
    }
}

/// Resolved drop target and preview rectangle (relative to `nvim-editor-pane` origin).
#[derive(Clone, Debug, PartialEq)]
pub struct SplitDropTarget {
    pub kind: SplitMoveKind,
    pub preview_left: f32,
    pub preview_top: f32,
    pub preview_width: f32,
    pub preview_height: f32,
    pub label: String,
}

/// Active split drag state held on `GtuimView` for rendering the drop preview overlay.
#[derive(Clone, Debug, PartialEq)]
pub struct SplitDragState {
    pub source: SplitDrag,
    pub target: Option<SplitDropTarget>,
}

/// Floating pill rendered under the cursor while dragging a split header.
pub struct SplitDragGhost {
    pub title: String,
    pub is_ai: bool,
    pub is_terminal: bool,
    pub palette: GuiPalette,
}

impl Render for SplitDragGhost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let ic = if self.is_ai {
            "sparkles"
        } else if self.is_terminal {
            "terminal"
        } else {
            "file-code"
        };
        div()
            .px(px(10.0))
            .py(px(5.0))
            .rounded(px(6.0))
            .bg(rgb(p.bg_elevated))
            .border_1()
            .border_color(rgb(p.accent))
            .shadow_md()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(icon("grip-vertical", 12.0, p.text_muted))
            .child(icon(ic, 13.0, p.accent))
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(p.text_strong))
                    .whitespace_nowrap()
                    .child(self.title.clone()),
            )
    }
}

#[derive(Clone, Copy)]
struct WinPxRect {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

impl WinPxRect {
    fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.left
            && x <= self.left + self.width
            && y >= self.top
            && y <= self.top + self.height
    }

    fn dist_sq(&self, x: f32, y: f32) -> f32 {
        let dx = if x < self.left {
            self.left - x
        } else if x > self.left + self.width {
            x - (self.left + self.width)
        } else {
            0.0
        };
        let dy = if y < self.top {
            self.top - y
        } else if y > self.top + self.height {
            y - (self.top + self.height)
        } else {
            0.0
        };
        dx * dx + dy * dy
    }
}

fn win_px_rect(win: &NvimWin, cell_w: f32, cell_h: f32) -> WinPxRect {
    WinPxRect {
        left: GRID_PADDING_X + (win.col as f32) * cell_w,
        top: GRID_PADDING_Y + (win.row as f32) * cell_h,
        width: (win.width as f32) * cell_w,
        height: (win.height as f32) * cell_h,
    }
}

fn is_already_docked(win: &NvimWin, wins: &[NvimWin], kind: SplitMoveKind) -> bool {
    let max_right = wins
        .iter()
        .map(|w| w.col.saturating_add(w.width))
        .max()
        .unwrap_or(0);
    let max_bottom = wins
        .iter()
        .map(|w| w.row.saturating_add(w.height))
        .max()
        .unwrap_or(0);
    if max_right == 0 || max_bottom == 0 {
        return false;
    }
    match kind {
        SplitMoveKind::DockLeft => {
            win.col == 0 && win.row == 0 && win.height.saturating_add(1) >= max_bottom
        }
        SplitMoveKind::DockRight => {
            win.col > 0
                && win.col.saturating_add(win.width).saturating_add(1) >= max_right
                && win.row == 0
                && win.height.saturating_add(1) >= max_bottom
        }
        SplitMoveKind::DockTop => {
            win.row == 0 && win.col == 0 && win.width.saturating_add(1) >= max_right
        }
        SplitMoveKind::DockBottom => {
            win.row > 0
                && win.row.saturating_add(win.height).saturating_add(1) >= max_bottom
                && win.col == 0
                && win.width.saturating_add(1) >= max_right
        }
        _ => false,
    }
}

fn make_dock_target(kind: SplitMoveKind, pane_w: f32, pane_h: f32) -> SplitDropTarget {
    let pad = 4.0f32;
    let half_w = (pane_w * 0.48 - pad).max(24.0);
    let half_h = (pane_h * 0.48 - pad).max(24.0);
    match kind {
        SplitMoveKind::DockTop => SplitDropTarget {
            kind,
            preview_left: pad,
            preview_top: pad,
            preview_width: (pane_w - pad * 2.0).max(24.0),
            preview_height: half_h,
            label: "Move to Top".to_string(),
        },
        SplitMoveKind::DockBottom => SplitDropTarget {
            kind,
            preview_left: pad,
            preview_top: (pane_h - half_h - pad).max(pad),
            preview_width: (pane_w - pad * 2.0).max(24.0),
            preview_height: half_h,
            label: "Move to Bottom".to_string(),
        },
        SplitMoveKind::DockLeft => SplitDropTarget {
            kind,
            preview_left: pad,
            preview_top: pad,
            preview_width: half_w,
            preview_height: (pane_h - pad * 2.0).max(24.0),
            label: "Move to Left".to_string(),
        },
        SplitMoveKind::DockRight => SplitDropTarget {
            kind,
            preview_left: (pane_w - half_w - pad).max(pad),
            preview_top: pad,
            preview_width: half_w,
            preview_height: (pane_h - pad * 2.0).max(24.0),
            label: "Move to Right".to_string(),
        },
        _ => unreachable!(),
    }
}

fn short_win_label(win: &NvimWin) -> &str {
    if win.name.is_empty() {
        "[No Name]"
    } else {
        win.name.as_str()
    }
}

/// Computes the active split drop target for a cursor position over `nvim-editor-pane`.
pub fn compute_split_drop_target(
    source_win_id: i64,
    wins: &[NvimWin],
    pane_bounds: Bounds<Pixels>,
    metrics: GridMetrics,
    cursor: Point<Pixels>,
) -> Option<SplitDropTarget> {
    if wins.len() <= 1 || !pane_bounds.contains(&cursor) {
        return None;
    }

    let source_win = wins.iter().find(|w| w.id == source_win_id)?;
    let pane_w = f32::from(pane_bounds.size.width).max(1.0);
    let pane_h = f32::from(pane_bounds.size.height).max(1.0);
    let rel_x = f32::from(cursor.x - pane_bounds.origin.x).clamp(0.0, pane_w);
    let rel_y = f32::from(cursor.y - pane_bounds.origin.y).clamp(0.0, pane_h);

    let cell_w = f32::from(metrics.cell_w).max(6.0);
    let cell_h = f32::from(metrics.cell_h).max(16.0);

    // Find hovered split window (or closest window if cursor sits on a 1-cell border gutter).
    let mut hovered: Option<(&NvimWin, WinPxRect)> = None;
    for win in wins {
        let rect = win_px_rect(win, cell_w, cell_h);
        if rect.contains(rel_x, rel_y) {
            hovered = Some((win, rect));
            break;
        }
    }
    if hovered.is_none() {
        let mut best: Option<(&NvimWin, WinPxRect, f32)> = None;
        for win in wins {
            let rect = win_px_rect(win, cell_w, cell_h);
            let d = rect.dist_sq(rel_x, rel_y);
            if best.map_or(true, |(_, _, bd)| d < bd) {
                best = Some((win, rect, d));
            }
        }
        if let Some((win, rect, _)) = best {
            hovered = Some((win, rect));
        }
    }

    let (hovered_win, hovered_rect) = hovered?;

    // Case 1: Dragging over the source split itself — support dragging straight to an outer edge
    // (e.g. grabbing a right split and pulling it straight down to the bottom of the editor).
    if hovered_win.id == source_win_id {
        let px_n = rel_x / pane_w;
        let py_n = rel_y / pane_h;
        let on_own_header = source_win.row == 0 && rel_y <= hovered_rect.top + cell_h + 8.0;

        let mut candidates: [(SplitMoveKind, f32, bool); 4] = [
            (
                SplitMoveKind::DockBottom,
                1.0 - py_n,
                py_n >= 0.72 && !is_already_docked(source_win, wins, SplitMoveKind::DockBottom),
            ),
            (
                SplitMoveKind::DockTop,
                py_n,
                py_n <= 0.28
                    && !on_own_header
                    && !is_already_docked(source_win, wins, SplitMoveKind::DockTop),
            ),
            (
                SplitMoveKind::DockLeft,
                px_n,
                px_n <= 0.22 && !is_already_docked(source_win, wins, SplitMoveKind::DockLeft),
            ),
            (
                SplitMoveKind::DockRight,
                1.0 - px_n,
                px_n >= 0.78 && !is_already_docked(source_win, wins, SplitMoveKind::DockRight),
            ),
        ];
        candidates.sort_by(|a, b| a.1.total_cmp(&b.1));
        for (kind, _, eligible) in candidates {
            if eligible {
                return Some(make_dock_target(kind, pane_w, pane_h));
            }
        }
        return None;
    }

    // Case 2: Dragging over a different split window.
    // For 3+ splits, check a dedicated outer editor edge strip for full-width/full-height docking.
    if wins.len() >= 3 {
        let outer_x = (pane_w * 0.08).clamp(22.0, 48.0);
        let outer_y = (pane_h * 0.10).clamp(24.0, 52.0);
        let outer_candidates = [
            (SplitMoveKind::DockBottom, pane_h - rel_y, rel_y >= pane_h - outer_y),
            (SplitMoveKind::DockTop, rel_y, rel_y <= outer_y),
            (SplitMoveKind::DockLeft, rel_x, rel_x <= outer_x),
            (SplitMoveKind::DockRight, pane_w - rel_x, rel_x >= pane_w - outer_x),
        ];
        let mut best_outer: Option<(SplitMoveKind, f32)> = None;
        for (kind, dist, in_strip) in outer_candidates {
            if in_strip && !is_already_docked(source_win, wins, kind) {
                if best_outer.map_or(true, |(_, bd)| dist < bd) {
                    best_outer = Some((kind, dist));
                }
            }
        }
        if let Some((kind, _)) = best_outer {
            return Some(make_dock_target(kind, pane_w, pane_h));
        }
    }

    // Subdivide the target split into 4 directional edges (28% each) + center Swap zone.
    let tx = ((rel_x - hovered_rect.left) / hovered_rect.width.max(1.0)).clamp(0.0, 1.0);
    let ty = ((rel_y - hovered_rect.top) / hovered_rect.height.max(1.0)).clamp(0.0, 1.0);
    let d_top = ty;
    let d_bottom = 1.0 - ty;
    let d_left = tx;
    let d_right = 1.0 - tx;

    const EDGE_THRESHOLD: f32 = 0.28;
    let min_d = d_top.min(d_bottom).min(d_left).min(d_right);

    let pad = 3.0f32;
    let target_name = short_win_label(hovered_win);

    if min_d <= EDGE_THRESHOLD {
        let side = if min_d == d_bottom {
            0 // bottom
        } else if min_d == d_top {
            1 // top
        } else if min_d == d_left {
            2 // left
        } else {
            3 // right
        };

        if wins.len() == 2 {
            let dock_kind = match side {
                0 => SplitMoveKind::DockBottom,
                1 => SplitMoveKind::DockTop,
                2 => SplitMoveKind::DockLeft,
                _ => SplitMoveKind::DockRight,
            };
            if is_already_docked(source_win, wins, dock_kind) {
                return None;
            }
            return Some(make_dock_target(dock_kind, pane_w, pane_h));
        }

        let left = (hovered_rect.left + pad).clamp(0.0, pane_w);
        let top = (hovered_rect.top + pad).clamp(0.0, pane_h);
        let w = (hovered_rect.width - pad * 2.0).max(20.0);
        let h = (hovered_rect.height - pad * 2.0).max(20.0);

        let target = match side {
            0 => SplitDropTarget {
                kind: SplitMoveKind::SplitBelow(hovered_win.id),
                preview_left: left,
                preview_top: top + h * 0.5,
                preview_width: w,
                preview_height: (h * 0.5).max(16.0),
                label: format!("Split Below {}", target_name),
            },
            1 => SplitDropTarget {
                kind: SplitMoveKind::SplitAbove(hovered_win.id),
                preview_left: left,
                preview_top: top,
                preview_width: w,
                preview_height: (h * 0.5).max(16.0),
                label: format!("Split Above {}", target_name),
            },
            2 => SplitDropTarget {
                kind: SplitMoveKind::SplitLeft(hovered_win.id),
                preview_left: left,
                preview_top: top,
                preview_width: (w * 0.5).max(16.0),
                preview_height: h,
                label: format!("Split Left of {}", target_name),
            },
            _ => SplitDropTarget {
                kind: SplitMoveKind::SplitRight(hovered_win.id),
                preview_left: left + w * 0.5,
                preview_top: top,
                preview_width: (w * 0.5).max(16.0),
                preview_height: h,
                label: format!("Split Right of {}", target_name),
            },
        };
        return Some(target);
    }

    // Center zone: Swap source split and target split.
    let left = (hovered_rect.left + pad).clamp(0.0, pane_w);
    let top = (hovered_rect.top + pad).clamp(0.0, pane_h);
    let w = (hovered_rect.width - pad * 2.0).max(20.0);
    let h = (hovered_rect.height - pad * 2.0).max(20.0);
    Some(SplitDropTarget {
        kind: SplitMoveKind::Swap(hovered_win.id),
        preview_left: left,
        preview_top: top,
        preview_width: w,
        preview_height: h,
        label: format!("Swap with {}", target_name),
    })
}

/// Renders the glowing drop-zone preview overlay inside `nvim-editor-pane`.
pub fn render_split_drop_preview(target: &SplitDropTarget, p: GuiPalette) -> AnyElement {
    let fill = rgba((p.accent << 8) | 0x30);
    div()
        .id("split-drop-preview")
        .debug_selector(|| "split-drop-preview".into())
        .absolute()
        .left(px(target.preview_left))
        .top(px(target.preview_top))
        .w(px(target.preview_width))
        .h(px(target.preview_height))
        .rounded(px(6.0))
        .bg(fill)
        .border_2()
        .border_color(rgb(p.accent))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .px(px(10.0))
                .py(px(5.0))
                .rounded(px(6.0))
                .bg(rgb(p.bg_elevated))
                .border_1()
                .border_color(rgb(p.accent))
                .shadow_md()
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(p.text_strong))
                .child(target.label.clone()),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{
        compute_split_drop_target, GridMetrics, NvimWin, SplitMoveKind, GRID_PADDING_X,
        GRID_PADDING_Y,
    };
    use gpui::{point, px, size, Bounds, Pixels};

    fn test_metrics() -> GridMetrics {
        GridMetrics {
            bounds: Bounds::new(
                point(px(GRID_PADDING_X), px(GRID_PADDING_Y)),
                size(px(800.0), px(480.0)),
            ),
            cell_w: px(10.0),
            cell_h: px(20.0),
            cols: 80,
            rows: 24,
        }
    }

    fn pane_bounds() -> Bounds<Pixels> {
        Bounds::new(
            point(px(0.0), px(0.0)),
            size(px(816.0), px(492.0)),
        )
    }

    fn two_side_by_side_wins() -> Vec<NvimWin> {
        vec![
            NvimWin {
                id: 1001,
                bufnr: 1,
                row: 0,
                col: 0,
                width: 39,
                height: 24,
                active: false,
                name: "left.rs".into(),
                ..Default::default()
            },
            NvimWin {
                id: 1002,
                bufnr: 2,
                row: 0,
                col: 40,
                width: 40,
                height: 24,
                active: true,
                name: "right.rs".into(),
                ..Default::default()
            },
        ]
    }

    #[test]
    fn two_split_right_pane_dragged_to_bottom_docks_bottom() {
        let wins = two_side_by_side_wins();
        let bounds = pane_bounds();
        let metrics = test_metrics();

        // Straight down inside right pane's bottom edge.
        let straight_down = compute_split_drop_target(
            1002,
            &wins,
            bounds,
            metrics,
            point(px(620.0), px(440.0)),
        )
        .expect("should dock bottom");
        assert_eq!(straight_down.kind, SplitMoveKind::DockBottom);
        assert_eq!(straight_down.label, "Move to Bottom");
        assert!(straight_down.preview_top >= 200.0);

        // Diagonally down onto bottom of left pane.
        let diag_down = compute_split_drop_target(
            1002,
            &wins,
            bounds,
            metrics,
            point(px(200.0), px(440.0)),
        )
        .expect("should dock bottom");
        assert_eq!(diag_down.kind, SplitMoveKind::DockBottom);

        // Onto center of left pane -> Swap.
        let swap = compute_split_drop_target(
            1002,
            &wins,
            bounds,
            metrics,
            point(px(200.0), px(240.0)),
        )
        .expect("should swap");
        assert_eq!(swap.kind, SplitMoveKind::Swap(1001));

        // Onto top of left pane -> DockTop.
        let top = compute_split_drop_target(
            1002,
            &wins,
            bounds,
            metrics,
            point(px(200.0), px(40.0)),
        )
        .expect("should dock top");
        assert_eq!(top.kind, SplitMoveKind::DockTop);

        // Onto left edge of left pane -> DockLeft.
        let left = compute_split_drop_target(
            1002,
            &wins,
            bounds,
            metrics,
            point(px(40.0), px(240.0)),
        )
        .expect("should dock left");
        assert_eq!(left.kind, SplitMoveKind::DockLeft);

        // Hovering own center or own right edge produces no redundant drop target.
        assert!(compute_split_drop_target(1002, &wins, bounds, metrics, point(px(620.0), px(240.0))).is_none());
        assert!(compute_split_drop_target(1002, &wins, bounds, metrics, point(px(790.0), px(240.0))).is_none());
    }

    #[test]
    fn three_split_supports_relative_and_outer_dock_targets() {
        let wins = vec![
            NvimWin { id: 1001, bufnr: 1, row: 0, col: 0, width: 39, height: 24, name: "a.rs".into(), ..Default::default() },
            NvimWin { id: 1002, bufnr: 2, row: 0, col: 40, width: 40, height: 11, name: "b.rs".into(), ..Default::default() },
            NvimWin { id: 1003, bufnr: 3, row: 12, col: 40, width: 40, height: 12, name: "c.rs".into(), ..Default::default() },
        ];
        let bounds = pane_bounds();
        let metrics = test_metrics();

        // Dragging c (bottom-right) onto inner bottom zone of a (left, above outer strip) -> SplitBelow(1001).
        let below_a = compute_split_drop_target(
            1003,
            &wins,
            bounds,
            metrics,
            point(px(200.0), px(400.0)),
        )
        .expect("split below a");
        assert_eq!(below_a.kind, SplitMoveKind::SplitBelow(1001));

        // Dragging c to very bottom outer edge -> DockBottom.
        let dock_bottom = compute_split_drop_target(
            1003,
            &wins,
            bounds,
            metrics,
            point(px(200.0), px(475.0)),
        )
        .expect("dock bottom");
        assert_eq!(dock_bottom.kind, SplitMoveKind::DockBottom);

        // Dragging c to center of b -> Swap(1002).
        let swap_b = compute_split_drop_target(
            1003,
            &wins,
            bounds,
            metrics,
            point(px(600.0), px(120.0)),
        )
        .expect("swap with b");
        assert_eq!(swap_b.kind, SplitMoveKind::Swap(1002));
    }
}
