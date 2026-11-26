use gpui::{prelude::*, *};

use crate::gui::drag_drop::{
    compute_split_drop_target, SplitDrag, SplitDragGhost, SplitDragState,
};
use crate::gui::grid_canvas::{GRID_PADDING_X, GRID_PADDING_Y};
use crate::gui::icons::{gui_palette, icon, tuim_logo};
use crate::gui::{EffectiveWorkspaceLayout, GtuimView};
use crate::tui::app::{FocusRegion, Mode, NvimWin, PaletteMode, TabInfo};
use crate::tui::events;
use crate::tui::layout::Rect;
use crate::tui::theme::blend_u32;
use crate::tui::widgets::menu_bar::MENUS;
use crate::tui::workspace;

pub(crate) const TAB_OVERFLOW_CONTROLS_W: f32 = 82.0;

pub(crate) fn tab_display_label(tab: &TabInfo) -> String {
    if !tab.relative_name.is_empty() {
        tab.relative_name.clone()
    } else if !tab.name.is_empty() {
        std::path::Path::new(&tab.name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&tab.name)
            .to_string()
    } else {
        "[No Name]".to_string()
    }
}

/// Conservative pixel width estimate for each tab (icon + label + dirty dot + close + padding).
pub(crate) fn estimate_tab_widths(tabs: &[TabInfo]) -> Vec<f32> {
    tabs.iter()
        .map(|tab| {
            let label = tab_display_label(tab);
            let chars = label.chars().count().min(22) as f32;
            let dirty_extra = if tab.changed { 14.0 } else { 0.0 };
            (66.0 + chars * 7.4 + dirty_extra).clamp(96.0, 196.0)
        })
        .collect()
}

/// Compute `(start_idx, end_idx_exclusive, overflowing)` so rendered tabs fit cleanly inside
/// `available_width` without mid-word clipping, and when `follow_active` is true the active tab
/// is guaranteed to be within `[start_idx..end_idx_exclusive)`.
pub(crate) fn compute_visible_tab_range(
    tab_widths: &[f32],
    active_tab: usize,
    current_start: usize,
    available_width: f32,
    controls_width: f32,
    follow_active: bool,
) -> (usize, usize, bool) {
    let n = tab_widths.len();
    if n == 0 {
        return (0, 0, false);
    }
    let avail = available_width.max(80.0);
    let total: f32 = tab_widths.iter().sum();
    if total <= avail {
        return (0, n, false);
    }

    let budget = (avail - controls_width).max(90.0);
    let active = active_tab.min(n - 1);
    let mut start = current_start.min(n - 1);

    if follow_active {
        if active < start {
            start = active;
        }
        while start < active {
            let span: f32 = tab_widths[start..=active].iter().sum();
            if span <= budget {
                break;
            }
            start += 1;
        }
    }

    let mut end = start;
    let mut used = 0.0f32;
    while end < n {
        let w = tab_widths[end];
        if end > start && used + w > budget {
            break;
        }
        used += w;
        end += 1;
    }

    // If we reached the rightmost tab and have leftover budget, pull start left so no dead space remains.
    while end == n && start > 0 {
        let prev_w = tab_widths[start - 1];
        if used + prev_w <= budget {
            start -= 1;
            used += prev_w;
        } else {
            break;
        }
    }

    (start, end, true)
}

/// Zed-style top application header bar: menus on left, centered command trigger, pane toggles on right.
pub fn render_header_bar(
    view: &GtuimView,
    layout: EffectiveWorkspaceLayout,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let show_brand_text = !layout.ultra_compact_header;
    let show_branch = layout.viewport_w >= 660.0;
    let show_full_menus = layout.viewport_w >= 430.0;
    let show_center_bar = !layout.ultra_compact_header;
    let compact_btn = layout.ultra_compact_header;
    let btn_sz = if compact_btn { 22.0 } else { 26.0 };

    let cmd_shortcut =
        workspace::format_shortcut_display(&app.settings_widget.config.keybindings.commands);
    let cmd_badge = if cmd_shortcut.is_empty() {
        "F1".to_string()
    } else {
        cmd_shortcut
    };

    let branch = if !show_branch || app.git_panel.snapshot.branch.is_empty() {
        None
    } else {
        Some(app.git_panel.snapshot.branch.clone())
    };

    // Left: workspace pill + menu items
    let mut left = div()
        .debug_selector(|| "hdr-left".into())
        .flex()
        .items_center()
        .gap(px(if compact_btn { 2.0 } else { 4.0 }))
        .pl(px(if compact_btn { 4.0 } else { 8.0 }))
        .min_w_0()
        .flex_shrink_0()
        .child(
            div()
                .id("hdr-workspace")
                .px(px(if compact_btn { 4.0 } else { 8.0 }))
                .py(px(3.0))
                .rounded(px(5.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.app.menu_bar.open_menu = None;
                    this.app.workspace.sidebar_visible = true;
                    this.app.workspace.overview = !this.app.workspace.overview;
                    this.app.workspace.focus = FocusRegion::Sidebar;
                    cx.notify();
                }))
                .child(tuim_logo(if compact_btn { 16.0 } else { 18.0 }))
                .when(show_brand_text, |d| {
                    d.child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(p.text_strong))
                            .child("Gtuim"),
                    )
                }),
        )
        .when_some(branch, |d, br| {
            d.child(
                div()
                    .id("hdr-branch")
                    .px_2()
                    .py(px(2.5))
                    .rounded(px(5.0))
                    .bg(rgb(p.bg_elevated))
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .max_w(px(140.0))
                    .overflow_hidden()
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.app.menu_bar.open_menu = None;
                        this.run_command("changes", cx);
                    }))
                    .child(icon("git-branch", 12.0, p.text_muted).flex_shrink_0())
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(11.0))
                            .text_color(rgb(p.text))
                            .child(br),
                    ),
            )
        })
        .when(show_brand_text, |d| {
            d.child(div().w(px(1.0)).h(px(14.0)).mx_1().bg(rgb(p.border)))
        });

    if show_full_menus {
        let menu_px = if layout.compact_header { 5.0 } else { 8.0 };
        for (i, menu) in MENUS.iter().enumerate() {
            let open = app.menu_bar.open_menu == Some(i);
            left = left.child(
                div()
                    .id(("top-menu", i))
                    .debug_selector(move || format!("top-menu-{i}"))
                    .px(px(menu_px))
                    .py(px(3.0))
                    .rounded(px(5.0))
                    .whitespace_nowrap()
                    .text_size(px(if layout.compact_header { 11.5 } else { 12.0 }))
                    .text_color(rgb(if open { p.text_strong } else { p.text_muted }))
                    .when(open, |d| d.bg(rgb(p.bg_selected)))
                    .hover(|s| s.bg(rgb(p.bg_hover)).text_color(rgb(p.text_strong)))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.app.menu_bar.open_menu == Some(i) {
                            this.app.menu_bar.open_menu = None;
                        } else {
                            this.app.menu_bar.open_menu = Some(i);
                            this.app.menu_bar.selected_item = 0;
                        }
                        cx.notify();
                    }))
                    .child(menu.title),
            );
        }
    } else {
        let open = app.menu_bar.open_menu.is_some();
        left = left.child(
            div()
                .id("top-menu-compact")
                .debug_selector(|| "top-menu-0".into())
                .px(px(6.0))
                .py(px(2.5))
                .rounded(px(5.0))
                .whitespace_nowrap()
                .text_size(px(11.0))
                .text_color(rgb(if open { p.text_strong } else { p.text_muted }))
                .when(open, |d| d.bg(rgb(p.bg_selected)))
                .hover(|s| s.bg(rgb(p.bg_hover)).text_color(rgb(p.text_strong)))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.app.menu_bar.open_menu.is_some() {
                        this.app.menu_bar.open_menu = None;
                    } else {
                        this.app.menu_bar.open_menu = Some(0);
                        this.app.menu_bar.selected_item = 0;
                    }
                    cx.notify();
                }))
                .child("Menu ▾"),
        );
    }

    // Center: Zed-style command palette search bar (collapses to icon button on tiny windows)
    let center = if show_center_bar {
        let bar_w = if layout.compact_header { 200.0 } else { 360.0 };
        let placeholder = if layout.compact_header {
            "Search..."
        } else {
            "Search commands, files, buffers..."
        };
        div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .px(px(if layout.compact_header { 6.0 } else { 16.0 }))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("hdr-command-bar")
                    .debug_selector(|| "hdr-command-bar".into())
                    .w(px(bar_w))
                    .max_w(relative(1.0))
                    .min_w_0()
                    .h(px(24.0))
                    .px_2()
                    .rounded(px(6.0))
                    .bg(rgb(p.bg_input))
                    .border_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .overflow_hidden()
                    .hover(|s| s.border_color(rgb(p.accent)).bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.app.menu_bar.open_menu = None;
                        events::open_palette(&mut this.app, PaletteMode::Commands);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(icon("search", 12.5, p.text_muted).flex_shrink_0())
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_size(px(11.5))
                                    .text_color(rgb(p.text_muted))
                                    .text_ellipsis()
                                    .child(placeholder),
                            ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .px(px(5.0))
                            .py(px(1.0))
                            .rounded(px(3.0))
                            .bg(rgb(p.bg_elevated))
                            .text_size(px(10.0))
                            .text_color(rgb(p.text_muted))
                            .child(cmd_badge),
                    ),
            )
    } else {
        div().flex_1().min_w_0()
    };

    // Right: clean pane & layout toggle icon buttons
    let mut right = div()
        .debug_selector(|| "hdr-right".into())
        .flex()
        .items_center()
        .gap(px(if compact_btn { 2.0 } else { 4.0 }))
        .pr(px(if compact_btn { 4.0 } else { 8.0 }))
        .flex_shrink_0();

    if !show_center_bar {
        right = right.child(header_icon_btn_sized(
            "btn-header-palette",
            "search",
            app.workspace.palette_open,
            btn_sz,
            p,
            cx.listener(|this, _, _, cx| {
                this.app.menu_bar.open_menu = None;
                events::open_palette(&mut this.app, PaletteMode::Commands);
                cx.notify();
            }),
        ));
    }

    right = right
        .child(
            div().id("btn-profile").debug_selector(|| "btn-profile".into())
                .w(px(btn_sz)).h(px(btn_sz)).flex().items_center().justify_center()
                .rounded(px(5.0)).cursor_pointer().text_color(rgb(crate::tui::theme::readable_u32(p.success, p.bg_surface, 3.0)))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .when_some(view.profiles.avatar_path(), |d, path| d.child(img(path).w(px(btn_sz - 2.0)).h(px(btn_sz - 2.0)).object_fit(ObjectFit::Contain)))
                .when(view.profiles.avatar_path().is_none(), |d| d.child(super::profiles::AVATARS[view.profiles.active().avatar % super::profiles::AVATARS.len()]))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_profile_panel_open(!this.profiles.open, window, cx);
                })),
        )
        .child(header_icon_btn_sized(
            "btn-toggle-sidebar",
            "panel-left",
            app.workspace.sidebar_visible,
            btn_sz,
            p,
            cx.listener(|this, _, _, cx| {
                this.app.menu_bar.open_menu = None;
                this.run_command("toggle_explorer", cx);
            }),
        ))
        .when(layout.viewport_w >= 380.0, |d| {
            d.child(header_icon_btn_sized(
                "btn-split-right",
                "split-vertical",
                app.nvim_wins.len() > 1,
                btn_sz,
                p,
                cx.listener(|this, _, _, cx| {
                    this.app.menu_bar.open_menu = None;
                    this.run_command("split_right", cx);
                }),
            ))
        })
        .child(header_icon_btn_sized(
            "btn-toggle-bterm",
            "panel-bottom",
            app.workspace.drawer_visible,
            btn_sz,
            p,
            cx.listener(|this, _, _, cx| {
                this.app.menu_bar.open_menu = None;
                this.run_command("toggle_terminal", cx);
            }),
        ))
        .when(layout.viewport_w >= 340.0, |d| {
            d.child(header_icon_btn_sized(
                "btn-toggle-vterm",
                "panel-right",
                app.workspace.vertical_terminal_visible,
                btn_sz,
                p,
                cx.listener(|this, _, _, cx| {
                    this.app.menu_bar.open_menu = None;
                    this.run_command("toggle_vertical_terminal", cx);
                }),
            ))
        })
        .child(header_icon_btn_sized(
            "btn-toggle-zen",
            "maximize",
            app.mode == Mode::Zen,
            btn_sz,
            p,
            cx.listener(|this, _, _, cx| {
                this.app.menu_bar.open_menu = None;
                this.run_command("toggle_zen", cx);
            }),
        ));

    div()
        .debug_selector(|| "hdr-bar".into())
        .h(px(layout.header_h))
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_between()
        .bg(rgb(p.bg_surface))
        .border_b_1()
        .border_color(rgb(p.border))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.app.menu_bar.open_menu.is_some() {
                    this.app.menu_bar.open_menu = None;
                    cx.notify();
                }
            }),
        )
        .child(left)
        .child(center)
        .child(right)
}

fn header_icon_btn_sized(
    id: &'static str,
    icon_name: &'static str,
    active: bool,
    size_px: f32,
    p: crate::gui::icons::GuiPalette,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .debug_selector(move || id.to_string())
        .size(px(size_px))
        .rounded(px(5.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .when(active, |d| d.bg(rgb(p.bg_selected)))
        .hover(|s| s.bg(rgb(p.bg_hover)))
        .cursor_pointer()
        .on_click(handler)
        .child(icon(
            icon_name,
            if size_px < 24.0 { 13.0 } else { 14.5 },
            if active { p.accent } else { p.text_muted },
        ))
}

/// Dedicated editor tab strip right above the Neovim viewport.
pub fn render_tab_bar(
    view: &GtuimView,
    layout: EffectiveWorkspaceLayout,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let tab_widths = estimate_tab_widths(&app.tabs);
    let (vis_start, vis_end, overflowing) = compute_visible_tab_range(
        &tab_widths,
        app.active_tab,
        view.tab_scroll_start,
        layout.main_col_w,
        TAB_OVERFLOW_CONTROLS_W,
        false,
    );
    let can_scroll_left = vis_start > 0;
    let can_scroll_right = vis_end < app.tabs.len();
    let hidden_count = app.tabs.len().saturating_sub(vis_end.saturating_sub(vis_start));

    let mut tabs_row = div()
        .flex()
        .items_center()
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_hidden();

    if app.tabs.is_empty() {
        let name = app.active_tab_display_name();
        tabs_row = tabs_row.child(
            div()
                .h_full()
                .px_3()
                .flex()
                .items_center()
                .gap(px(6.0))
                .bg(rgb(p.bg_editor))
                .border_r_1()
                .border_color(rgb(p.border_subtle))
                .child(icon("file-code", 13.0, p.accent).flex_shrink_0())
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(p.text_strong))
                        .child(name),
                ),
        );
    } else {
        let single_visible = vis_end.saturating_sub(vis_start) <= 1;
        for idx in vis_start..vis_end {
            let tab = &app.tabs[idx];
            let base = tab_display_label(tab);
            let is_active = idx == app.active_tab;
            let bufnr = tab.bufnr;
            let changed = tab.changed;
            let is_preview = tab.is_preview;

            let bg = if is_active { p.bg_editor } else { p.bg_panel };
            let fg = if is_active { p.text_strong } else { p.text_muted };

            let mut tab_el = div()
                .id(("tab", idx))
                .debug_selector(move || format!("tab-{idx}"))
                .h_full()
                .pl(px(10.0))
                .pr(px(6.0))
                .min_w_0()
                .overflow_hidden()
                .flex()
                .items_center()
                .gap(px(6.0))
                .bg(rgb(bg))
                .border_r_1()
                .border_color(rgb(p.border_subtle))
                .when(is_active, |d| d.border_t_2().border_color(rgb(p.accent)))
                .when(!is_active, |d| {
                    d.border_b_1()
                        .border_color(rgb(p.border))
                        .hover(|s| s.bg(rgb(p.bg_hover)))
                })
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.active_tab = idx;
                    this.select_buffer(bufnr, cx);
                }))
                .child(
                    icon(
                        "file-code",
                        13.0,
                        if is_active { p.accent } else { p.text_muted },
                    )
                    .flex_shrink_0(),
                )
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(px(12.0))
                        .text_color(rgb(fg))
                        .when(is_active, |d| d.font_weight(FontWeight::MEDIUM))
                        .when(is_preview, |d| d.italic())
                        .child(base),
                )
                .when(changed, |d| {
                    d.child(
                        div()
                            .size(px(6.5))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(rgb(p.warning)),
                    )
                })
                .child(
                    div()
                        .id(("tab-close", idx))
                        .size(px(18.0))
                        .flex_shrink_0()
                        .rounded(px(4.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(rgb(p.bg_elevated)))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.close_buffer(bufnr, cx);
                        }))
                        .child(icon("x", 11.0, p.text_muted)),
                );

            tab_el = if single_visible {
                tab_el.max_w(relative(1.0))
            } else {
                tab_el.flex_shrink_0().max_w(px(190.0))
            };

            tabs_row = tabs_row.child(tab_el);
        }
    }

    // Empty filler to the right of tabs with bottom hairline
    tabs_row = tabs_row.child(
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .border_b_1()
            .border_color(rgb(p.border)),
    );

    let overflow_controls = overflowing.then(|| {
        div()
            .debug_selector(|| "tab-overflow-controls".into())
            .h_full()
            .px(px(4.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(2.0))
            .bg(rgb(p.bg_panel))
            .border_l_1()
            .border_b_1()
            .border_color(rgb(p.border))
            .child(
                div()
                    .id("tab-scroll-left")
                    .debug_selector(|| "tab-scroll-left".into())
                    .size(px(22.0))
                    .rounded(px(4.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(can_scroll_left, |d| {
                        d.cursor_pointer().hover(|s| s.bg(rgb(p.bg_hover)))
                    })
                    .when(!can_scroll_left, |d| d.opacity(0.35))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.tab_scroll_start > 0 {
                            this.tab_scroll_start -= 1;
                            cx.notify();
                        }
                    }))
                    .child(icon("chevron-left", 13.0, p.text_muted)),
            )
            .child(
                div()
                    .id("tab-scroll-right")
                    .debug_selector(|| "tab-scroll-right".into())
                    .size(px(22.0))
                    .rounded(px(4.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(can_scroll_right, |d| {
                        d.cursor_pointer().hover(|s| s.bg(rgb(p.bg_hover)))
                    })
                    .when(!can_scroll_right, |d| d.opacity(0.35))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.tab_scroll_start + 1 < this.app.tabs.len() {
                            this.tab_scroll_start += 1;
                            cx.notify();
                        }
                    }))
                    .child(icon("chevron-right", 13.0, p.text_muted)),
            )
            .child(
                div()
                    .id("tab-overflow-menu")
                    .debug_selector(|| "tab-overflow-menu".into())
                    .px(px(5.0))
                    .h(px(22.0))
                    .rounded(px(4.0))
                    .bg(rgb(p.bg_elevated))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(10.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(p.text_muted))
                    .hover(|s| s.bg(rgb(p.bg_hover)).text_color(rgb(p.text_strong)))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        events::open_palette(&mut this.app, PaletteMode::Buffers);
                        cx.notify();
                    }))
                    .child(format!("+{}", hidden_count.max(1))),
            )
    });

    div()
        .id("editor-tab-bar")
        .debug_selector(|| "editor-tab-bar".into())
        .h(px(layout.tab_bar_h))
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .flex_shrink_0()
        .flex()
        .items_center()
        .bg(rgb(p.bg_panel))
        .on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, _, cx| {
            if !overflowing || this.app.tabs.len() <= 1 {
                return;
            }
            cx.stop_propagation();
            let delta = match ev.delta {
                ScrollDelta::Lines(d) => {
                    if d.x.abs() > d.y.abs() {
                        d.x
                    } else {
                        d.y
                    }
                }
                ScrollDelta::Pixels(d) => {
                    let dx = f32::from(d.x);
                    let dy = f32::from(d.y);
                    if dx.abs() > dy.abs() {
                        dx
                    } else {
                        dy
                    }
                }
            };
            if delta < 0.0 && this.tab_scroll_start + 1 < this.app.tabs.len() {
                this.tab_scroll_start += 1;
                cx.notify();
            } else if delta > 0.0 && this.tab_scroll_start > 0 {
                this.tab_scroll_start -= 1;
                cx.notify();
            }
        }))
        .child(tabs_row)
        .children(overflow_controls)
}

/// Returns true if a floating Neovim window or native picker covers this split's header row.
pub fn split_header_occluded(win: &NvimWin, floats: &[Rect], telescope: Option<Rect>) -> bool {
    let overlaps = |r: Rect| {
        r.w > 0
            && r.h > 0
            && win.row >= r.y
            && win.row < r.y.saturating_add(r.h)
            && win.col < r.x.saturating_add(r.w)
            && win.col.saturating_add(win.width) > r.x
    };
    floats.iter().copied().any(overlaps) || telescope.is_some_and(overlaps)
}

fn split_parent_dir(win: &NvimWin) -> Option<String> {
    if win.relative_name.is_empty() || win.is_ai || win.buftype == "terminal" {
        return None;
    }
    let parent = std::path::Path::new(&win.relative_name)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or("");
    if parent.is_empty() || parent == "." {
        None
    } else {
        Some(parent.to_string())
    }
}

/// Native GPUI split/pane headers overlaid cleanly on Neovim's reserved 1-row winbar slot.
/// Shown when multiple non-floating splits are open, or for a single pane when the top tab bar is hidden.
pub fn render_split_headers(view: &GtuimView, cx: &mut Context<GtuimView>) -> Vec<AnyElement> {
    let app = &view.app;
    let is_multi_split = app.nvim_wins.len() > 1;
    let show_single_header = !app.workspace.tab_bar_visible && app.mode != Mode::Zen;
    if app.nvim_wins.is_empty() || (!is_multi_split && !show_single_header) {
        return Vec::new();
    }

    let p = gui_palette(&app.theme);
    let metrics = *view.editor_metrics.borrow();
    let cell_w = f32::from(metrics.cell_w).max(6.0);
    let cell_h = f32::from(metrics.cell_h).max(16.0);

    let btn_size = (cell_h - 4.0).clamp(14.0, 20.0);
    let icon_sz = (cell_h * 0.56).clamp(10.0, 12.5);
    let font_px = (cell_h * 0.54).clamp(10.0, 12.0);
    let active_bg = blend_u32(p.bg_panel, p.accent, 8);
    let close_hover_bg = blend_u32(p.bg_panel, p.error, 22);
    let dragged_win_id = view.split_drag.as_ref().map(|s| s.source.win_id);
    let view_entity = cx.entity();

    let mut headers = Vec::with_capacity(app.nvim_wins.len());
    for (idx, win) in app.nvim_wins.iter().enumerate() {
        if win.width == 0 || split_header_occluded(win, &app.nvim_floats, app.telescope_main_rect) {
            continue;
        }

        let left = GRID_PADDING_X + (win.col as f32) * cell_w;
        let top = GRID_PADDING_Y + (win.row as f32) * cell_h;
        let width = (win.width as f32) * cell_w;
        let height = cell_h;

        let win_id = win.id;
        let bufnr = win.bufnr;
        let is_active = win.active;
        let is_drag_source = dragged_win_id == Some(win_id);
        let title = if win.name.is_empty() {
            "[No Name]".to_string()
        } else {
            win.name.clone()
        };
        let parent_dir = if width >= 220.0 {
            split_parent_dir(win)
        } else {
            None
        };
        let show_grip = width >= 96.0;
        let show_kind_icon = width >= 76.0;
        let show_only_btn = width >= 140.0;
        let show_readonly = win.readonly && width >= 170.0;

        let kind_icon = if win.is_ai {
            "sparkles"
        } else if win.buftype == "terminal" {
            "terminal"
        } else {
            "file-code"
        };

        let drag_payload = SplitDrag {
            win_id,
            bufnr,
            title: title.clone(),
            is_ai: win.is_ai,
            is_terminal: win.buftype == "terminal",
        };

        let header_bg = if is_active { active_bg } else { p.bg_panel };
        let title_fg = if is_active { p.text_strong } else { p.text_muted };
        let icon_fg = if is_active { p.accent } else { p.text_muted };

        let mut left_group = div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(5.0));

        if show_grip {
            left_group = left_group.child(icon("grip-vertical", (icon_sz - 1.0).max(9.5), p.text_muted));
        }

        if show_kind_icon {
            left_group = left_group.child(icon(kind_icon, icon_sz, icon_fg));
        }

        left_group = left_group
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(font_px))
                    .text_color(rgb(title_fg))
                    .when(is_active, |d| d.font_weight(FontWeight::MEDIUM))
                    .child(title),
            )
            .when_some(parent_dir, |d, dir| {
                d.child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_size(px((font_px - 1.0).max(9.5)))
                        .text_color(rgb(p.text_muted))
                        .child(dir),
                )
            })
            .when(win.modified, |d| {
                d.child(
                    div()
                        .size(px(6.0))
                        .rounded_full()
                        .flex_shrink_0()
                        .bg(rgb(p.warning)),
                )
            })
            .when(show_readonly, |d| {
                d.child(
                    div()
                        .px(px(4.0))
                        .rounded(px(3.0))
                        .flex_shrink_0()
                        .bg(rgb(p.bg_elevated))
                        .text_size(px(9.5))
                        .text_color(rgb(p.text_muted))
                        .child("RO"),
                )
            });

        let mut actions = div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(2.0));

        if show_only_btn {
            let only_group = SharedString::from(format!("split-only-grp-{idx}"));
            actions = actions.child(
                div()
                    .id(("split-only", idx))
                    .group(only_group.clone())
                    .debug_selector(move || format!("split-only-{idx}"))
                    .size(px(btn_size))
                    .rounded(px(4.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        window.focus(&this.root_focus);
                        this.keep_only_split(win_id, cx);
                    }))
                    .child(
                        icon("maximize", (icon_sz - 1.0).max(10.0), p.text_muted)
                            .group_hover(only_group, |s| s.text_color(rgb(p.text_strong))),
                    ),
            );
        }

        let close_group = SharedString::from(format!("split-close-grp-{idx}"));
        let close_fg = if is_active { p.text_strong } else { p.text };
        actions = actions.child(
            div()
                .id(("split-close", idx))
                .group(close_group.clone())
                .debug_selector(move || format!("split-close-{idx}"))
                .size(px(btn_size))
                .rounded(px(4.0))
                .flex()
                .items_center()
                .justify_center()
                .hover(move |s| s.bg(rgb(close_hover_bg)))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.root_focus);
                    if this.app.nvim_wins.len() > 1 {
                        this.close_split(win_id, bufnr, cx);
                    } else {
                        this.close_buffer(bufnr, cx);
                    }
                }))
                .child(
                    icon("x", icon_sz.max(11.0), close_fg)
                        .group_hover(close_group, move |s| s.text_color(rgb(p.error))),
                ),
        );

        let drag_view = view_entity.clone();
        let bar = div()
            .id(("split-header", idx))
            .debug_selector(move || format!("split-header-{idx}"))
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .px(px(6.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(4.0))
            .bg(rgb(header_bg))
            .border_b_1()
            .border_color(rgb(if is_active { p.border } else { p.border_subtle }))
            .when(!is_active && win.row > 0, |d| {
                d.border_t_1().border_color(rgb(p.border_subtle))
            })
            .when(is_drag_source, |d| d.opacity(0.55))
            .when(is_multi_split, |d| {
                d.cursor_grab().on_drag(
                    drag_payload,
                    move |drag: &SplitDrag, _, window, cx| {
                        let palette = drag_view.update(cx, |this, cx| {
                            this.app.workspace.focus = FocusRegion::Editor;
                            this.app.editor_context_menu.is_open = false;
                            let metrics = *this.editor_metrics.borrow();
                            let pane_bounds = this.last_editor_pane_bounds.unwrap_or_else(|| {
                                Bounds::new(
                                    point(
                                        metrics.bounds.origin.x - px(GRID_PADDING_X),
                                        metrics.bounds.origin.y - px(GRID_PADDING_Y),
                                    ),
                                    size(
                                        metrics.bounds.size.width + px(GRID_PADDING_X * 2.0),
                                        metrics.bounds.size.height + px(GRID_PADDING_Y * 2.0),
                                    ),
                                )
                            });
                            let target = compute_split_drop_target(
                                drag.win_id,
                                &this.app.nvim_wins,
                                pane_bounds,
                                metrics,
                                window.mouse_position(),
                            );
                            this.split_drag = Some(SplitDragState {
                                source: drag.clone(),
                                target,
                            });
                            cx.notify();
                            gui_palette(&this.app.theme)
                        });
                        cx.set_active_drag_cursor_style(CursorStyle::ClosedHand, window);
                        cx.new(|_| SplitDragGhost {
                            title: drag.title.clone(),
                            is_ai: drag.is_ai,
                            is_terminal: drag.is_terminal,
                            palette,
                        })
                    },
                )
            })
            .when(!is_multi_split, |d| d.cursor_pointer())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.root_focus);
                    this.focus_split(win_id, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.root_focus);
                    if this.app.nvim_wins.len() > 1 {
                        this.close_split(win_id, bufnr, cx);
                    } else {
                        this.close_buffer(bufnr, cx);
                    }
                }),
            )
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .when(is_active, |d| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(2.0))
                        .bg(rgb(p.accent)),
                )
            })
            .child(left_group)
            .child(actions);

        headers.push(bar.into_any_element());
    }

    headers
}

pub fn render_drawer_header(
    view: &GtuimView,
    layout: EffectiveWorkspaceLayout,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let active_tab = app.workspace.drawer_tab;
    let scrollback = app.shell_terminal.screen().scrollback();
    let narrow = layout.main_col_w < 480.0;

    let tabs = [
        (0usize, if narrow { "Term" } else { "Terminal" }, Some("terminal")),
        (1usize, if narrow { "Debug" } else { "Debug Console" }, None),
        (2usize, "Output", None),
        (3usize, if narrow { "Build" } else { "Compilation" }, None),
    ];

    let mut row = div()
        .flex()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .items_center()
        .gap_1()
        .px(px(if narrow { 4.0 } else { 8.0 }));
    // Keep the active compilation tab reachable even when the drawer is very narrow.
    for (idx, label, ic) in tabs.into_iter().cycle().skip(if narrow && active_tab == 3 { 3 } else { 0 }).take(tabs.len()) {
        let active = active_tab == idx;
        row = row.child(
            div()
                .id(("drawer-tab", idx))
                .debug_selector(move || format!("drawer-tab-{idx}"))
                .flex_shrink_0()
                .px(px(if narrow { 5.0 } else { 8.0 }))
                .py(px(3.0))
                .rounded(px(5.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .whitespace_nowrap()
                .text_size(px(if narrow { 10.5 } else { 11.5 }))
                .font_weight(if active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(rgb(if active { p.text_strong } else { p.text_muted }))
                .when(active, |d| d.bg(rgb(p.bg_elevated)))
                .hover(|s| s.bg(rgb(p.bg_hover)).text_color(rgb(p.text_strong)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.workspace.drawer_tab = idx;
                    this.app.workspace.focus = FocusRegion::Drawer;
                    if idx == 0 {
                        events::ensure_shell_terminal(&mut this.app);
                    }
                    cx.notify();
                }))
                .when_some(ic.filter(|_| !narrow), |d, name| {
                    d.child(icon(name, 12.5, if active { p.accent } else { p.text_muted }))
                })
                .child(label),
        );
    }

    div()
        .h(px(if narrow { 26.0 } else { 30.0 }))
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_between()
        .bg(rgb(p.bg_panel))
        .border_b_1()
        .border_color(rgb(p.border_subtle))
        .child(row)
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap_2()
                .pr_2()
                .when(active_tab == 0 && scrollback > 0 && !narrow, |d| {
                    d.child(
                        div()
                            .px_2()
                            .py(px(1.5))
                            .rounded(px(4.0))
                            .bg(rgb(p.bg_selected))
                            .text_size(px(10.5))
                            .text_color(rgb(p.text_strong))
                            .child(format!("Scrollback -{}", scrollback)),
                    )
                })
                .child(
                    div()
                        .id("btn-close-drawer")
                        .debug_selector(|| "drawer-close".into())
                        .group("btn-close-drawer-grp")
                        .size(px(22.0))
                        .rounded(px(4.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(rgb(blend_u32(p.bg_panel, p.error, 22))))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.app.workspace.drawer_visible = false;
                            if this.app.workspace.focus == FocusRegion::Drawer {
                                this.app.workspace.focus = FocusRegion::Editor;
                            }
                            cx.notify();
                        }))
                        .child(
                            icon("x", 13.0, p.text_muted)
                                .group_hover("btn-close-drawer-grp", move |s| {
                                    s.text_color(rgb(p.error))
                                }),
                        ),
                ),
        )
}

pub fn render_console_panel(view: &GtuimView, is_debug: bool) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let panel = if is_debug {
        &app.debug_console
    } else {
        &app.output_panel
    };

    let mut list = div()
        .flex_1()
        .w_full()
        .min_h_0()
        .bg(rgb(app.theme.bg_terminal))
        .p_3()
        .flex()
        .flex_col()
        .gap_1()
        .overflow_hidden();

    if panel.lines.is_empty() {
        list = list.child(
            div()
                .text_size(px(12.0))
                .text_color(rgb(p.text_muted))
                .child(format!("{} is empty.", panel.title)),
        );
    } else {
        let start = panel.lines.len().saturating_sub(40);
        for (i, line) in panel.lines[start..].iter().enumerate() {
            let idx = start + i;
            let sel = idx == panel.list_state.selected;
            list = list.child(
                div()
                    .px_2()
                    .py(px(1.5))
                    .rounded(px(4.0))
                    .when(sel, |d| d.bg(rgb(p.bg_hover)))
                    .text_size(px(12.0))
                    .text_color(rgb(p.text))
                    .child(line.clone()),
            );
        }
    }

    list
}

pub fn render_status_bar(
    view: &GtuimView,
    layout: EffectiveWorkspaceLayout,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);

    if app.native_picker_chrome && app.telescope_main_rect.is_some() {
        let hints = [
            ("Enter Open", Some("open")),
            ("↑/↓ Move", None),
            ("Tab Mark", Some("mark")),
            ("Alt+P Preview", Some("preview")),
            ("Esc Close", Some("close")),
        ];
        let mut row = div()
            .h(px(layout.status_h))
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .flex_shrink_0()
            .px_3()
            .flex()
            .items_center()
            .gap_4()
            .bg(rgb(p.accent))
            .text_size(px(11.5))
            .text_color(rgb(p.accent_fg));
        for (i, (label, act)) in hints.into_iter().enumerate() {
            let mut item = div()
                .id(("picker-hint", i))
                .flex_shrink_0()
                .whitespace_nowrap()
                .px_1()
                .rounded(px(3.0))
                .child(label);
            if let Some(action) = act {
                item = item
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(0xffffff22)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let lua = format!(
                            "if _G.tuim_picker_action then _G.tuim_picker_action('{}') end",
                            action
                        );
                        let _ = this.app.editor_rpc.notify(
                            "nvim_exec_lua",
                            vec![rmpv::Value::from(lua), rmpv::Value::Array(vec![])],
                        );
                        cx.notify();
                    }));
            }
            row = row.child(item);
        }
        return row;
    }

    let (mode_label, mode_bg) = match app.mode {
        Mode::Zen => ("ZEN", p.accent),
        Mode::Ide => ("IDE", p.accent),
        Mode::Normal => match app.ui_state.current_mode.as_str() {
            "insert" => ("INSERT", p.success),
            "visual" => ("VISUAL", p.warning),
            _ => ("NORMAL", p.accent),
        },
    };

    let region_label = match app.workspace.focus {
        FocusRegion::Editor => "Editor",
        FocusRegion::Sidebar => "Sidebar",
        FocusRegion::Drawer => "Terminal",
        FocusRegion::VerticalTerminal => "Split Terminal",
    };

    let file_name = app.active_tab_display_name();
    let (cur_ln, cur_col) = view.effective_doc_cursor();
    let show_secondary_right = !layout.compact_status;
    let show_zen_btn = layout.viewport_w >= 420.0;

    let zen_key =
        workspace::format_shortcut_display(&app.settings_widget.config.keybindings.toggle_zen);
    let zen_str = if app.mode == Mode::Zen {
        format!("{} Exit Zen", if zen_key.is_empty() { "F11" } else { &zen_key })
    } else {
        format!("{} Zen", if zen_key.is_empty() { "F11" } else { &zen_key })
    };

    div()
        .debug_selector(|| "status-bar".into())
        .h(px(layout.status_h))
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .flex_shrink_0()
        .px_2()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .bg(rgb(p.bg_surface))
        .border_t_1()
        .border_color(rgb(p.border))
        .text_size(px(if layout.compact_status { 10.5 } else { 11.0 }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .flex()
                .items_center()
                .gap(px(if layout.compact_status { 6.0 } else { 12.0 }))
                .child(
                    div()
                        .id("status-mode-badge")
                        .flex_shrink_0()
                        .px_2()
                        .py(px(1.5))
                        .rounded(px(4.0))
                        .bg(rgb(mode_bg))
                        .text_color(rgb(p.accent_fg))
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(10.0))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| this.cycle_mode(cx)))
                        .child(mode_label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(rgb(p.text))
                        .child(if layout.viewport_w < 380.0 {
                            file_name
                        } else {
                            format!("{}  ·  {}", file_name, region_label)
                        }),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .debug_selector(|| "status-cursor-pos".into())
                        .whitespace_nowrap()
                        .px(px(if layout.compact_status { 4.0 } else { 8.0 }))
                        .text_color(rgb(p.text_muted))
                        .child(format!("Ln {}, Col {}", cur_ln, cur_col)),
                )
                .when(show_secondary_right, |d| {
                    d.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_px()
                            .child(
                                div()
                                    .id("status-zoom-out")
                                    .px(px(5.0))
                                    .py(px(1.0))
                                    .rounded(px(3.0))
                                    .text_color(rgb(p.text_muted))
                                    .hover(|s| s.bg(rgb(p.bg_hover)).text_color(rgb(p.text_strong)))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.adjust_font_size(this.font_size - 1.0, cx);
                                    }))
                                    .child("−"),
                            )
                            .child(
                                div()
                                    .px_1()
                                    .text_color(rgb(p.text_muted))
                                    .child(format!("{}px", self_font_px(view.font_size))),
                            )
                            .child(
                                div()
                                    .id("status-zoom-in")
                                    .px(px(5.0))
                                    .py(px(1.0))
                                    .rounded(px(3.0))
                                    .text_color(rgb(p.text_muted))
                                    .hover(|s| s.bg(rgb(p.bg_hover)).text_color(rgb(p.text_strong)))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.adjust_font_size(this.font_size + 1.0, cx);
                                    }))
                                    .child("+"),
                            ),
                    )
                    .child(
                        div()
                            .id("status-bug")
                            .px_2()
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .text_color(rgb(p.text_muted))
                            .hover(|s| s.bg(rgb(p.bg_hover)).text_color(rgb(p.text_strong)))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.run_command("report_bug", cx)))
                            .child(icon("bug", 12.0, p.text_muted))
                            .child("Bug"),
                    )
                })
                .when(show_zen_btn, |d| {
                    d.child(
                        div()
                            .id("status-zen")
                            .whitespace_nowrap()
                            .px_2()
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .text_color(rgb(if app.mode == Mode::Zen {
                                p.accent
                            } else {
                                p.text_muted
                            }))
                            .hover(|s| s.bg(rgb(p.bg_hover)).text_color(rgb(p.text_strong)))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| this.run_command("toggle_zen", cx)))
                            .child(zen_str),
                    )
                }),
        )
}

fn self_font_px(sz: f32) -> i32 {
    sz.round() as i32
}
