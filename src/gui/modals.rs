use gpui::{prelude::*, *};

use crate::gui::icons::{gui_palette, icon, GuiPalette, ICON_LSP, ICON_SETTINGS};
use crate::gui::{EffectiveWorkspaceLayout, GtuimView};
use crate::tui::app::PaletteMode;
use crate::tui::input::InputEvent;
use crate::tui::theme::blend_u32;
use crate::tui::widgets::bug_report::{BugStage, BUG_CATEGORIES, MAX_DRAFT_URL_BYTES};
use crate::tui::widgets::compile::{AnsiColor, CompileLine, CompileStage};
use crate::tui::widgets::editor_context_menu::MENU_ITEMS as EDITOR_CTX_ITEMS;
use crate::tui::widgets::extension_shop::{ExtensionShop, ShopConfirm, ShopSection, CATEGORIES};
use crate::tui::widgets::lazy::LAZY_TABS;
use crate::tui::widgets::mason::MASON_TABS;
use crate::tui::widgets::menu_bar::MENUS;
use crate::tui::widgets::settings::{Keybindings, SETTINGS_TABS};
use crate::tui::workspace;

const SAVE_FILE_AS_CMD: workspace::CommandDef = workspace::CommandDef {
    id: "save_file_as",
    label: "Save file as...",
    key_field: "save_file_as",
};

const PALETTE_ROW_H: f32 = 32.0;
const PALETTE_ROW_GAP: f32 = 2.0;
const PALETTE_LIST_PAD_Y: f32 = 6.0;

pub(crate) fn compute_palette_window(
    total: usize,
    selected: usize,
    current_scroll: usize,
    max_visible_rows: usize,
) -> (usize, usize, usize) {
    let cap = max_visible_rows.max(1);
    if total == 0 {
        return (0, 0, cap);
    }
    let visible = total.min(cap);
    let max_start = total.saturating_sub(visible);
    let mut start = current_scroll.min(max_start);
    let sel = selected.min(total.saturating_sub(1));
    if sel < start {
        start = sel;
    } else if sel >= start + visible {
        start = sel + 1 - visible;
    }
    (start, (start + visible).min(total), visible)
}

pub(crate) fn palette_filtered_commands(app: &crate::tui::app::App) -> Vec<&'static workspace::CommandDef> {
    let mut cmds = workspace::filtered_commands(app);
    let has_save_as = cmds.iter().any(|c| c.id == "save_file_as");
    if !has_save_as {
        let raw = app.workspace.palette_query.trim_start_matches('>').trim();
        let q = raw.to_lowercase();
        let matches_save_as = q.is_empty()
            || SAVE_FILE_AS_CMD.label.to_lowercase().contains(&q)
            || SAVE_FILE_AS_CMD.id.to_lowercase().contains(&q);
        if matches_save_as {
            if let Some(pos) = cmds.iter().position(|c| c.id == "save_file") {
                cmds.insert(pos + 1, &SAVE_FILE_AS_CMD);
            } else {
                cmds.push(&SAVE_FILE_AS_CMD);
            }
        }
    }
    cmds
}

pub(crate) fn file_menu_items_with_save_as(
    menu_idx: usize,
    items: &'static [(&'static str, &'static str)],
) -> Vec<(&'static str, &'static str)> {
    let mut out: Vec<(&'static str, &'static str)> = items.to_vec();
    if menu_idx == 0 && !out.iter().any(|&(_, act)| act == "save_file_as") {
        if let Some(pos) = out.iter().position(|&(_, act)| act == "save_file") {
            out.insert(pos + 1, ("Save As...", "save_file_as"));
        } else {
            out.push(("Save As...", "save_file_as"));
        }
    }
    out
}

pub fn render_overlays(
    view: &GtuimView,
    layout: EffectiveWorkspaceLayout,
    cx: &mut Context<GtuimView>,
) -> Vec<AnyElement> {
    let mut out = Vec::new();
    let app = &view.app;

    if app.extension_shop.is_open {
        out.push(render_extension_shop(view, cx).into_any_element());
    }
    if app.workspace.palette_open {
        out.push(render_palette(view, layout, cx).into_any_element());
    }
    if app.settings_widget.is_open {
        out.push(render_settings(view, layout, cx).into_any_element());
    }
    if app.mason_widget.is_open {
        out.push(render_mason(view, cx).into_any_element());
    }
    if app.lazy_widget.is_open {
        out.push(render_lazy(view, cx).into_any_element());
    }
    if app.git_detailed_widget.is_open {
        out.push(render_git_detailed(view, cx).into_any_element());
    }
    if app.orchestrator.is_open {
        out.push(render_orchestrator(view, cx).into_any_element());
    }
    if app.bug_report.is_open {
        out.push(render_bug_report(view, cx).into_any_element());
    }
    if app.compile_widget.is_open {
        out.push(render_compile(view, cx).into_any_element());
    }
    if app.onboarding.is_open {
        if app.onboarding.help_only {
            out.push(super::help::render_help(view, cx).into_any_element());
        } else {
            out.push(super::onboarding::render_onboarding(view, cx).into_any_element());
        }
    }
    if app.menu_bar.open_menu.is_some() {
        out.push(render_menu_dropdown(view, layout, cx).into_any_element());
    }
    if app.explorer.context_menu_open {
        out.push(render_explorer_context_menu(view, cx).into_any_element());
    }
    if app.editor_context_menu.is_open {
        out.push(render_editor_context_menu(view, cx).into_any_element());
    }
    if app.notice.is_some() {
        out.push(render_notice(view).into_any_element());
    }

    out
}

pub(super) fn backdrop(
    id: &'static str,
    on_dismiss: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .inset_0()
        .occlude()
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .bg(rgba(0x00000078))
        .flex()
        .items_center()
        .justify_center()
        .on_mouse_down(MouseButton::Left, on_dismiss)
}

pub(super) fn modal_card(p: GuiPalette, width: f32, height: f32) -> Div {
    div()
        .debug_selector(|| "modal-card".into())
        .w(px(width))
        .h(px(height))
        .max_w(relative(0.92))
        .max_h(relative(0.88))
        .min_w_0()
        .min_h_0()
        .flex_shrink_0()
        .rounded(px(10.0))
        .bg(rgb(p.bg_panel))
        .border_1()
        .border_color(rgb(p.border))
        .shadow_lg()
        .flex()
        .flex_col()
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

pub(super) fn modal_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    primary: bool,
    danger: bool,
    p: GuiPalette,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let bg = if danger {
        p.error
    } else if primary {
        p.accent
    } else {
        p.bg_elevated
    };
    let fg = if primary || danger {
        p.accent_fg
    } else {
        p.text_strong
    };
    div()
        .id(id)
        .flex_shrink_0()
        .whitespace_nowrap()
        .px_3()
        .py(px(5.0))
        .rounded(px(6.0))
        .bg(rgb(bg))
        .text_size(px(12.0))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(fg))
        .hover(|s| s.opacity(0.88))
        .cursor_pointer()
        .on_click(handler)
        .child(label.into())
}

fn render_palette(
    view: &GtuimView,
    layout: EffectiveWorkspaceLayout,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let placeholder = match app.workspace.palette_mode {
        PaletteMode::Commands => "Type a command (F2 rebind · # buffers · : line)...",
        PaletteMode::Buffers => "Filter open buffers...",
        PaletteMode::LineJump => "Enter line number...",
    };

    let compact_vp = layout.viewport_h < 380.0;
    let top_pad = if compact_vp { 14.0 } else { 58.0 };
    let header_h = 44.0f32;
    let status_h = if app.workspace.palette_status.is_empty() {
        0.0
    } else {
        32.0
    };
    let max_card_h = ((layout.viewport_h - top_pad - 16.0).min(layout.viewport_h * 0.88))
        .min(460.0)
        .max(140.0);

    // Reserve footer space if list overflows
    let avail_for_rows = (max_card_h - header_h - status_h - 24.0 - PALETTE_LIST_PAD_Y * 2.0 - 2.0)
        .max(PALETTE_ROW_H);
    let max_visible_rows = ((avail_for_rows + PALETTE_ROW_GAP) / (PALETTE_ROW_H + PALETTE_ROW_GAP))
        .floor()
        .clamp(1.0, 10.0) as usize;

    let mut total_items: usize = 0;
    let mut visible_range: (usize, usize) = (0, 0);
    let mut fixed_list_height: Option<f32> = None;

    let mut list = super::modal_layout::scroll_column("palette-list")
        .px_2()
        .py(px(PALETTE_LIST_PAD_Y))
        .flex()
        .flex_col()
        .gap(px(PALETTE_ROW_GAP));

    match app.workspace.palette_mode {
        PaletteMode::Commands => {
            let cmds = palette_filtered_commands(app);
            total_items = cmds.len();
            if cmds.is_empty() {
                list = list.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .p_3()
                        .text_size(px(12.0))
                        .text_color(rgb(p.text_muted))
                        .child("No matching commands"),
                );
            } else {
                let (start, end, vis_count) = compute_palette_window(
                    cmds.len(),
                    app.workspace.palette_index,
                    app.workspace.palette_scroll,
                    max_visible_rows,
                );
                visible_range = (start, end);
                let exact_h = (vis_count as f32) * PALETTE_ROW_H
                    + (vis_count.saturating_sub(1) as f32) * PALETTE_ROW_GAP
                    + PALETTE_LIST_PAD_Y * 2.0;
                fixed_list_height = Some(exact_h);

                for idx in start..end {
                    let cmd = cmds[idx];
                    let sel = idx == app.workspace.palette_index;
                    let cmd_id = cmd.id;
                    let key_field = cmd.key_field;
                    let raw_key = if key_field.is_empty() {
                        ""
                    } else {
                        app.settings_widget.config.keybindings.get_by_field(key_field)
                    };
                    let shortcut_disp = if sel && app.workspace.palette_recording_key {
                        "Press key...".to_string()
                    } else if raw_key.is_empty() && cmd_id == "save_file_as" {
                        "Ctrl+Shift+S".to_string()
                    } else {
                        workspace::format_shortcut_display(raw_key)
                    };

                    list = list.child(
                        super::modal_layout::list_card()
                            .id(("pal-cmd", idx))
                            .debug_selector(move || format!("palette-cmd-{}", idx))
                            .h(px(PALETTE_ROW_H))
                            .px_3()
                            .rounded(px(6.0))
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .when(sel, |d| d.bg(rgb(p.bg_selected)))
                            .hover(|s| s.bg(rgb(p.bg_hover)))
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.app.workspace.palette_open = false;
                                this.run_command(cmd_id, cx);
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(px(12.5))
                                    .text_color(rgb(if sel { p.text_strong } else { p.text }))
                                    .child(cmd.label),
                            )
                            .when(!shortcut_disp.is_empty() || !key_field.is_empty(), |d| {
                                d.child(
                                    div()
                                        .id(("pal-rebind", idx))
                                        .debug_selector(move || format!("palette-shortcut-{}", idx))
                                        .flex_shrink_0()
                                        .whitespace_nowrap()
                                        .px_2()
                                        .py(px(2.0))
                                        .rounded(px(4.0))
                                        .bg(rgb(p.bg_surface))
                                        .text_size(px(10.5))
                                        .text_color(rgb(if sel && app.workspace.palette_recording_key {
                                            p.accent
                                        } else {
                                            p.text_muted
                                        }))
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation();
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.app.workspace.palette_index = idx;
                                            if !key_field.is_empty() {
                                                this.app.workspace.palette_recording_key = true;
                                                this.app.workspace.palette_status.clear();
                                                cx.notify();
                                            }
                                        }))
                                        .child(if shortcut_disp.is_empty() {
                                            "bind".to_string()
                                        } else {
                                            shortcut_disp
                                        }),
                                )
                            }),
                    );
                }
            }
        }
        PaletteMode::Buffers => {
            let bufs = workspace::filtered_buffers(app);
            total_items = bufs.len();
            let single_wrap = bufs.len() <= 1;
            let (start, end, vis_count) = if single_wrap {
                (0, bufs.len(), bufs.len())
            } else {
                compute_palette_window(
                    bufs.len(),
                    app.workspace.palette_index,
                    app.workspace.palette_scroll,
                    max_visible_rows,
                )
            };
            visible_range = (start, end);
            if !single_wrap && vis_count > 0 {
                fixed_list_height = Some(
                    (vis_count as f32) * PALETTE_ROW_H
                        + (vis_count.saturating_sub(1) as f32) * PALETTE_ROW_GAP
                        + PALETTE_LIST_PAD_Y * 2.0,
                );
            }
            for idx in start..end {
                let (_, ref name, bufnr) = bufs[idx];
                let sel = idx == app.workspace.palette_index;
                let mut row = super::modal_layout::list_card()
                    .id(("pal-buf", idx))
                    .debug_selector(move || format!("palette-buf-{}", idx))
                    .px_3()
                    .rounded(px(6.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(sel, |d| d.bg(rgb(p.bg_selected)))
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .text_size(px(12.5))
                    .text_color(rgb(if sel { p.text_strong } else { p.text }))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.app.workspace.palette_open = false;
                        this.select_buffer(bufnr, cx);
                    }))
                    .child(icon("file-code", 13.0, p.accent).flex_shrink_0());

                row = if single_wrap {
                    row.py(px(6.0)).child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_normal()
                            .child(name.clone()),
                    )
                } else {
                    row.h(px(PALETTE_ROW_H)).overflow_hidden().child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(name.clone()),
                    )
                };
                list = list.child(row);
            }
        }
        PaletteMode::LineJump => {
            let line_str = app.workspace.palette_query.trim_start_matches(':').trim();
            list = list.child(
                div()
                    .w_full()
                    .min_w_0()
                    .whitespace_normal()
                    .p_3()
                    .text_size(px(12.5))
                    .text_color(rgb(p.text))
                    .child(if line_str.is_empty() {
                        "Enter a line number to jump to...".to_string()
                    } else {
                        format!("Jump to line {}", line_str)
                    }),
            );
        }
    }

    let is_windowed = total_items > (visible_range.1.saturating_sub(visible_range.0));
    let footer_h = if is_windowed { 24.0 } else { 0.0 };
    if total_items > 1 {
        let cap_rows = max_visible_rows;
        list = list.on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, _, cx| {
            cx.stop_propagation();
            let delta = match ev.delta {
                ScrollDelta::Lines(d) => d.y,
                ScrollDelta::Pixels(d) => f32::from(d.y) / PALETTE_ROW_H,
            };
            let max_start = total_items.saturating_sub(cap_rows);
            if delta < 0.0 {
                let next = (this.app.workspace.palette_scroll + 1).min(max_start);
                this.app.workspace.palette_scroll = next;
                if this.app.workspace.palette_index < next {
                    this.app.workspace.palette_index = next;
                }
                cx.notify();
            } else if delta > 0.0 && this.app.workspace.palette_scroll > 0 {
                let next = this.app.workspace.palette_scroll - 1;
                this.app.workspace.palette_scroll = next;
                if this.app.workspace.palette_index >= next + cap_rows {
                    this.app.workspace.palette_index = (next + cap_rows).saturating_sub(1);
                }
                cx.notify();
            }
        }));
    }

    let card_h = if let Some(lh) = fixed_list_height {
        (header_h + lh + status_h + footer_h + 2.0).min(max_card_h)
    } else {
        420.0f32.min(max_card_h)
    };

    backdrop(
        "palette-backdrop",
        cx.listener(|this, _, _, cx| {
            this.app.workspace.palette_open = false;
            cx.notify();
        }),
    )
    .items_start()
    .pt(px(top_pad))
    .child(
        modal_card(p, 600.0, card_h)
            .child(
                div()
                    .debug_selector(|| "palette-header".into())
                    .h(px(header_h))
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_3()
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon("search", 15.0, p.accent).flex_shrink_0())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(13.5))
                            .text_color(rgb(if app.workspace.palette_query.is_empty() {
                                p.text_muted
                            } else {
                                p.text_strong
                            }))
                            .child(if app.workspace.palette_query.is_empty() {
                                placeholder.to_string()
                            } else {
                                format!("{}▏", app.workspace.palette_query)
                            }),
                    ),
            )
            .child(list)
            .when(is_windowed, |d| {
                d.child(
                    div()
                        .debug_selector(|| "palette-footer".into())
                        .h(px(footer_h))
                        .w_full()
                        .min_w_0()
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
                        .child(format!(
                            "Showing {}–{} of {}",
                            visible_range.0 + 1,
                            visible_range.1,
                            total_items
                        ))
                        .child("↑/↓ or scroll"),
                )
            })
            .when(!app.workspace.palette_status.is_empty(), |d| {
                d.child(
                    div()
                        .id("palette-status")
                        .debug_selector(|| "palette-status".into())
                        .w_full()
                        .min_w_0()
                        .max_h(px(54.0))
                        .overflow_y_scroll()
                        .flex_shrink_0()
                        .whitespace_normal()
                        .px_3()
                        .py(px(6.0))
                        .bg(rgb(p.bg_surface))
                        .border_t_1()
                        .border_color(rgb(p.border))
                        .text_size(px(11.0))
                        .text_color(rgb(p.accent))
                        .child(app.workspace.palette_status.clone()),
                )
            }),
    )
}

fn render_settings(
    view: &GtuimView,
    layout: EffectiveWorkspaceLayout,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let app = &view.app;
    let sw = &app.settings_widget;
    let p = gui_palette(&app.theme);
    let compact = layout.viewport_w < 560.0 || layout.viewport_h < 380.0;

    let mut nav = if compact {
        div()
            .id("settings-nav")
            .debug_selector(|| "settings-nav".into())
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .overflow_x_scroll()
            .bg(rgb(p.bg_surface))
            .border_b_1()
            .border_color(rgb(p.border))
            .px_2()
            .py(px(4.0))
            .flex()
            .items_center()
            .gap_1()
    } else {
        div()
            .id("settings-nav")
            .debug_selector(|| "settings-nav".into())
            .w(px(180.0))
            .max_w(relative(0.30))
            .min_w_0()
            .min_h_0()
            .h_full()
            .overflow_y_scroll()
            .flex_shrink_0()
            .bg(rgb(p.bg_surface))
            .border_r_1()
            .border_color(rgb(p.border))
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
    };

    for (i, label) in SETTINGS_TABS.iter().enumerate() {
        let active = i == sw.active_tab;
        let tab_item = if compact {
            div()
                .id(("set-tab", i))
                .debug_selector(move || format!("settings-tab-{}", i))
                .flex_shrink_0()
                .whitespace_nowrap()
                .px_2()
                .py(px(3.0))
                .rounded(px(5.0))
                .text_size(px(11.0))
                .when(active, |d| {
                    d.bg(rgb(p.bg_selected))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(p.text_strong))
                })
                .when(!active, |d| d.text_color(rgb(p.text)))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.settings_widget.active_tab = i;
                    this.app.settings_widget.selected_row = 0;
                    this.app.settings_widget.focus_sidebar = false;
                    this.app.settings_widget.dropdown_open = false;
                    cx.notify();
                }))
                .child(*label)
        } else {
            super::modal_layout::list_card()
                .id(("set-tab", i))
                .debug_selector(move || format!("settings-tab-{}", i))
                .px_3()
                .py(px(6.0))
                .rounded(px(6.0))
                .whitespace_normal()
                .text_size(px(12.5))
                .when(active, |d| {
                    d.bg(rgb(p.bg_selected))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(p.text_strong))
                })
                .when(!active, |d| d.text_color(rgb(p.text)))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.settings_widget.active_tab = i;
                    this.app.settings_widget.selected_row = 0;
                    this.app.settings_widget.focus_sidebar = false;
                    this.app.settings_widget.dropdown_open = false;
                    cx.notify();
                }))
                .child(*label)
        };
        nav = nav.child(tab_item);
    }

    let rows = sw.rows_for_tab(sw.active_tab);
    let visible = sw.visible_rows();

    let mut content = super::modal_layout::scroll_column("settings-rows")
        .p(px(if compact { 6.0 } else { 12.0 }))
        .flex()
        .flex_col()
        .gap(px(if compact { 4.0 } else { 6.0 }));

    if sw.active_tab == 4 {
        content = content.child(
            div()
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .pb_2()
                .flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .child(modal_button(
                    "kb-vim-safe",
                    "Vim-safe Preset (v)",
                    false,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| {
                        this.app.settings_widget.config.keybindings = Keybindings::vim_safe_preset();
                        this.app.settings_widget.status_msg = "Applied Vim-safe preset".into();
                        cx.notify();
                    }),
                ))
                .child(modal_button(
                    "kb-familiar",
                    "Familiar Preset (p)",
                    false,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| {
                        this.app.settings_widget.config.keybindings = Keybindings::familiar_preset();
                        this.app.settings_widget.status_msg = "Applied Familiar preset".into();
                        cx.notify();
                    }),
                ))
                .child(modal_button(
                    "kb-reset",
                    "Reset Defaults (r)",
                    false,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| {
                        this.app.settings_widget.config.keybindings = Keybindings::default();
                        this.app.settings_widget.status_msg =
                            "Reset keybindings to defaults".into();
                        cx.notify();
                    }),
                )),
        );
    }

    for &row_idx in &visible {
        let row = &rows[row_idx];
        let sel = row_idx == sw.selected_row;
        let value_text = if sw.active_tab == 4 && sel && sw.recording_key {
            "Press new key...".to_string()
        } else {
            row.value.clone()
        };

        content = content.child(
            super::modal_layout::list_card()
                .id(("set-row", row_idx))
                .debug_selector(move || format!("settings-row-{}", row_idx))
                .px(px(if compact { 6.0 } else { 12.0 }))
                .py(px(if compact { 5.0 } else { 8.0 }))
                .rounded(px(7.0))
                .bg(rgb(if sel { p.bg_selected } else { p.bg_surface }))
                .border_1()
                .border_color(rgb(if sel { p.accent } else { p.border_subtle }))
                .flex()
                .items_center()
                .justify_between()
                .when(!compact, |d| d.flex_wrap())
                .gap(px(if compact { 6.0 } else { 12.0 }))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.settings_widget.focus_sidebar = false;
                    this.app.settings_widget.selected_row = row_idx;
                    this.app.settings_widget.activate_selected_row();
                    this.after_widget_change(cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .debug_selector(move || format!("settings-copy-{}", row_idx))
                        .flex()
                        .flex_col()
                        .gap(px(if compact { 1.0 } else { 4.0 }))
                        // A real basis lets the value move to the next line on
                        // narrow dialogs; flex_1's zero basis would squeeze prose.
                        .flex_basis(px(if compact { 100.0 } else { 240.0 }))
                        .flex_grow()
                        .min_w_0()
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(if compact { 11.5 } else { 12.5 }))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_strong))
                                .child(row.label),
                        )
                        .when(!compact || layout.viewport_h >= 280.0, |d| {
                            d.child(
                                div()
                                    .w_full()
                                    .min_w_0()
                                    .whitespace_normal()
                                    .text_size(px(if compact { 10.0 } else { 11.0 }))
                                    .text_color(rgb(p.text_muted))
                                    .child(row.description),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap_1()
                        .when(sw.active_tab == 1 && row_idx == 5, |d| {
                            d.child(
                                div()
                                    .id("settings-font-dec")
                                    .debug_selector(|| "settings-font-dec".into())
                                    .size(px(24.0))
                                    .rounded(px(5.0))
                                    .bg(rgb(p.bg_elevated))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(12.0))
                                    .text_color(rgb(p.text_strong))
                                    .hover(|s| s.bg(rgb(p.bg_hover)))
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        cx.stop_propagation();
                                        let cur = this.app.settings_widget.config.font_size.clamp(9, 26);
                                        this.app.settings_widget.config.font_size = cur.saturating_sub(1).max(9);
                                        this.sync_font_from_settings();
                                        cx.notify();
                                    }))
                                    .child("−"),
                            )
                        })
                        .child(
                            div()
                                .debug_selector(move || format!("settings-value-{}", row_idx))
                                .flex_shrink_0()
                                // Percentage maxima on intrinsic-width text can resolve
                                // to zero during measurement. Bound only exceptional values.
                                .max_w(px(if compact { 120.0 } else { 260.0 }))
                                .whitespace_normal()
                                .px_2()
                                .py(px(if compact { 2.0 } else { 4.0 }))
                                .rounded(px(5.0))
                                .bg(rgb(p.bg_elevated))
                                .text_size(px(if compact { 11.0 } else { 12.0 }))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(p.text_strong))
                                .child(value_text),
                        )
                        .when(sw.active_tab == 1 && row_idx == 5, |d| {
                            d.child(
                                div()
                                    .id("settings-font-inc")
                                    .debug_selector(|| "settings-font-inc".into())
                                    .size(px(24.0))
                                    .rounded(px(5.0))
                                    .bg(rgb(p.bg_elevated))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(12.0))
                                    .text_color(rgb(p.text_strong))
                                    .hover(|s| s.bg(rgb(p.bg_hover)))
                                    .cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        cx.stop_propagation();
                                        let cur = this.app.settings_widget.config.font_size.clamp(9, 26);
                                        this.app.settings_widget.config.font_size = (cur + 1).min(26);
                                        this.sync_font_from_settings();
                                        cx.notify();
                                    }))
                                    .child("+"),
                            )
                        }),
                ),
        );
    }

    if sw.active_tab == 1 && !compact {
        content = content.child(
            div()
                .debug_selector(|| "settings-font-preview".into())
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .p_3()
                .rounded(px(7.0))
                .bg(rgb(p.bg_editor))
                .border_1()
                .border_color(rgb(p.border_subtle))
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(rgb(p.text_muted))
                        .child(format!(
                            "Live Font Preview  ·  {}  ·  {}  ·  {} px",
                            view.mono_font,
                            crate::tui::widgets::settings::SettingsWidget::font_weight_label(
                                sw.config.font_weight
                            ),
                            view.font_size.round() as u8
                        )),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .font_family(view.mono_font.clone())
                        .font_weight(view.font_weight)
                        .text_size(px(view.font_size))
                        .text_color(rgb(p.text_strong))
                        .child("fn main() { println!(\"Hello, Gtuim!\"); // 0O 1lI != => }"),
                ),
        );
    }

    let dropdown_overlay = sw.dropdown_open.then(|| {
        let opts: Vec<String> = sw
            .dropdown_options()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let mut dd = div()
            .id("settings-dropdown")
            .debug_selector(|| "settings-dropdown".into())
            .occlude()
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .absolute()
            .right(px(28.0))
            .top(px(96.0))
            .w(px(260.0))
            .max_w(relative(0.85))
            .min_w_0()
            .max_h(relative(0.60))
            .overflow_y_scroll()
            .rounded(px(8.0))
            .bg(rgb(p.bg_elevated))
            .border_1()
            .border_color(rgb(p.accent))
            .shadow_lg()
            .p_1()
            .flex()
            .flex_col()
            .gap(px(2.0));

        for (idx, opt) in opts.into_iter().enumerate() {
            let sel = idx == sw.dropdown_idx;
            dd = dd.child(
                super::modal_layout::list_card()
                    .id(("set-dd-opt", idx))
                    .debug_selector(move || format!("settings-dd-opt-{}", idx))
                    .px_3()
                    .py(px(6.0))
                    .rounded(px(5.0))
                    .whitespace_normal()
                    .when(sel, |d| d.bg(rgb(p.bg_selected)))
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .text_size(px(12.0))
                    .text_color(rgb(p.text_strong))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.app.settings_widget.select_dropdown_option(idx);
                        this.sync_font_from_settings();
                        cx.notify();
                    }))
                    .child(opt),
            );
        }
        dd
    });

    let discard_overlay = sw.confirm_discard.then(|| {
        div()
            .id("settings-discard-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .bg(rgba(0x00000099))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(380.0))
                    .max_w(relative(0.9))
                    .min_w_0()
                    .p_4()
                    .rounded(px(8.0))
                    .bg(rgb(p.bg_elevated))
                    .border_1()
                    .border_color(rgb(p.warning))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .whitespace_normal()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(p.text_strong))
                            .child("Discard unsaved settings?"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .justify_end()
                            .gap_2()
                            .child(modal_button(
                                "set-disc-no",
                                "Keep Editing",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.settings_widget.confirm_discard = false;
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "set-disc-save",
                                "Save",
                                true,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.settings_widget.confirm_discard = false;
                                    this.app
                                        .settings_widget
                                        .save_and_sync(&mut this.app.editor_rpc);
                                    this.after_widget_change(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "set-disc-yes",
                                "Discard",
                                false,
                                true,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.settings_widget.config =
                                        this.app.settings_widget.saved_config.clone();
                                    this.app.settings_widget.confirm_discard = false;
                                    this.app.settings_widget.is_open = false;
                                    this.sync_font_from_settings();
                                    cx.notify();
                                }),
                            )),
                    ),
            )
    });

    let footer = div()
        .debug_selector(|| "settings-footer".into())
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .px(px(if compact { 8.0 } else { 16.0 }))
        .py(px(if compact { 4.0 } else { 8.0 }))
        .bg(rgb(p.bg_surface))
        .border_t_1()
        .border_color(rgb(p.border))
        .flex()
        .items_center()
        .justify_between()
        .flex_wrap()
        .gap(px(if compact { 4.0 } else { 8.0 }))
        .when(!compact || sw.is_dirty() || !sw.status_msg.is_empty(), |d| {
            d.child(
                div()
                    .id("settings-status")
                    .flex_1()
                    .min_w(px(if compact { 80.0 } else { 140.0 }))
                    .max_h(px(if compact { 28.0 } else { 54.0 }))
                    .overflow_y_scroll()
                    .whitespace_normal()
                    .text_size(px(if compact { 10.5 } else { 11.5 }))
                    .text_color(rgb(p.text_muted))
                    .child(if !sw.status_msg.is_empty() {
                        sw.status_msg.clone()
                    } else if sw.is_dirty() {
                        "Unsaved changes  ·  Ctrl+S to save".to_string()
                    } else {
                        "Press / to search settings".to_string()
                    }),
            )
        })
        .child(
            div()
                .flex()
                .items_center()
                .flex_wrap()
                .justify_end()
                .gap(px(if compact { 4.0 } else { 8.0 }))
                .max_w_full()
                .min_w_0()
                .when(!compact, |d| {
                    d.child(modal_button(
                        "set-revert",
                        "Revert",
                        false,
                        false,
                        p,
                        cx.listener(|this, _, _, cx| {
                            this.app.settings_widget.config =
                                this.app.settings_widget.saved_config.clone();
                            this.app.settings_widget.status_msg = "Reverted changes".into();
                            this.sync_font_from_settings();
                            cx.notify();
                        }),
                    ))
                })
                .child(modal_button(
                    "set-cancel",
                    "Close",
                    false,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| {
                        if this.app.settings_widget.is_dirty() {
                            this.app.settings_widget.confirm_discard = true;
                        } else {
                            this.app.settings_widget.is_open = false;
                        }
                        cx.notify();
                    }),
                ))
                .child(modal_button(
                    "set-save",
                    if compact { "Save" } else { "Save (Ctrl+S)" },
                    true,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| {
                        this.app
                            .settings_widget
                            .save_and_sync(&mut this.app.editor_rpc);
                        this.after_widget_change(cx);
                        cx.notify();
                    }),
                )),
        );

    let mut card = modal_card(p, 820.0, 620.0).relative();
    if compact {
        card = card.max_w(relative(0.96)).max_h(relative(0.94));
    }

    backdrop(
        "settings-backdrop",
        cx.listener(|this, _, _, cx| {
            if this.app.settings_widget.is_dirty() {
                this.app.settings_widget.confirm_discard = true;
            } else {
                this.app.settings_widget.is_open = false;
            }
            cx.notify();
        }),
    )
    .child(
        card.child(
            div()
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .px(px(if compact { 8.0 } else { 16.0 }))
                .py(px(if compact { 5.0 } else { 12.0 }))
                .bg(rgb(p.bg_surface))
                .border_b_1()
                .border_color(rgb(p.border))
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap(px(if compact { 4.0 } else { 8.0 }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .flex_shrink_0()
                        .child(icon(ICON_SETTINGS, if compact { 13.0 } else { 16.0 }, p.accent))
                        .child(
                            div()
                                .text_size(px(if compact { 12.0 } else { 14.0 }))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(p.text_strong))
                                .child("Settings"),
                        ),
                )
                .when(!compact || sw.is_searching || !sw.search_query.is_empty(), |d| {
                    d.child(
                        div()
                            .id("set-search-box")
                            .w(px(if compact { 140.0 } else { 260.0 }))
                            .max_w(relative(0.55))
                            .min_w_0()
                            .whitespace_normal()
                            .px_2()
                            .py(px(if compact { 2.0 } else { 4.0 }))
                            .rounded(px(6.0))
                            .bg(rgb(p.bg_input))
                            .border_1()
                            .border_color(rgb(if sw.is_searching {
                                p.accent
                            } else {
                                p.border
                            }))
                            .text_size(px(if compact { 10.5 } else { 11.5 }))
                            .text_color(rgb(if sw.search_query.is_empty() && !sw.is_searching {
                                p.text_muted
                            } else {
                                p.text_strong
                            }))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.app.settings_widget.is_searching = true;
                                cx.notify();
                            }))
                            .child(if sw.is_searching {
                                format!("{}▏", sw.search_query)
                            } else if sw.search_query.is_empty() {
                                "/ Search settings...".to_string()
                            } else {
                                sw.search_query.clone()
                            }),
                    )
                }),
        )
        .child(if compact {
            div()
                .flex_1()
                .w_full()
                .min_w_0()
                .min_h_0()
                .flex()
                .flex_col()
                .child(nav)
                .child(content)
        } else {
            div()
                .flex_1()
                .w_full()
                .min_w_0()
                .min_h_0()
                .flex()
                .child(nav)
                .child(content)
        })
        .child(footer)
        .children(dropdown_overlay)
        .children(discard_overlay),
    )
}

fn render_extension_shop(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let app = &view.app;
    let shop = &app.extension_shop;
    let p = gui_palette(&app.theme);

    let mut nav = div()
        .id("plugin-center-nav")
        .w(px(200.0))
        .max_w(relative(0.30))
        .min_w_0()
        .min_h_0()
        .h_full()
        .overflow_y_scroll()
        .flex_shrink_0()
        .bg(rgb(p.bg_surface))
        .border_r_1()
        .border_color(rgb(p.border))
        .p_2()
        .flex()
        .flex_col()
        .gap(px(2.0));

    for idx in ExtensionShop::nav_order() {
        let label = match idx {
            0 => "1  Installed",
            1 => "2  Community",
            n if n == CATEGORIES.len() + 2 => "3  Supported recipes",
            _ => CATEGORIES[idx - 2].0,
        };
        let active = idx == shop.nav_index;
        nav = nav.child(
            super::modal_layout::list_card()
                .id(("shop-nav", idx))
                .whitespace_normal()
                .px_3()
                .py(px(5.0))
                .rounded(px(5.0))
                .text_size(px(12.0))
                .when(active, |d| {
                    d.bg(rgb(p.bg_selected))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(p.text_strong))
                })
                .when(!active, |d| d.text_color(rgb(p.text)))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.extension_shop.select_nav(idx);
                    cx.notify();
                }))
                .child(label),
        );
    }

    let main_area: AnyElement = if shop.recipe_form.is_some() {
        super::plugin_center::render_form(view, cx)
    } else if shop.detail_open {
        if let Some(plugin) = shop.selected_plugin() {
            let status = super::plugin_center::lifecycle_label(plugin);
            let pending_removal = ExtensionShop::is_pending_removal(plugin);
            let can_install = !plugin.installed || pending_removal;
            let can_toggle = plugin.installed && !plugin.protected && !pending_removal;
            let enabled = plugin.enabled;
            let mut meta_badges: Vec<String> = Vec::new();
            if !plugin.auto_init.is_empty() && plugin.auto_init != "pending" {
                meta_badges.push(format!("Init: {}", plugin.auto_init));
            }
            if !plugin.colorschemes.is_empty() {
                meta_badges.push(format!("Themes: {}", plugin.colorschemes.join(", ")));
            }
            if !plugin.config_path.is_empty() {
                meta_badges.push(format!("Config: {}", plugin.config_path));
            }

            super::modal_layout::scroll_column("plugin-details")
                .p_5()
                .flex()
                .flex_col()
                .gap_3()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .w_full()
                        .min_w_0()
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(18.0))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(p.text_strong))
                                .child(plugin.name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(rgb(p.text_muted))
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .child(format!("{}  ·  ★ {}", plugin.full_name, plugin.stars)),
                        )
                        .child(
                            div()
                                .px_2()
                                .py(px(2.0))
                                .rounded(px(4.0))
                                .bg(rgb(p.bg_elevated))
                                .text_size(px(11.0))
                                .text_color(rgb(p.accent))
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .child(status),
                        )
                        .when(!meta_badges.is_empty(), |d| {
                            d.child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(rgb(p.text_muted))
                                    .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .child(meta_badges.join("  ·  ")),
                            )
                        })
                        .child(
                            div()
                                .pt_3()
                                .text_size(px(13.0))
                                .text_color(rgb(p.text))
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .child(plugin.description.clone()),
                        )
                        .child(super::plugin_center::render_health(view, cx)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .flex_wrap()
                        .when(can_install, |d| {
                            d.child(modal_button(
                                "shop-install",
                                "Install Plugin",
                                true,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app
                                        .extension_shop
                                        .handle_key(&InputEvent::Key("<CR>".into()));
                                    this.app.extension_shop.can_confirm_modal = true;
                                    this.after_widget_change(cx);
                                    cx.notify();
                                }),
                            ))
                        })
                        .when(!can_install || plugin.supported, |d| {
                            d.child(modal_button(
                                "shop-configure",
                                "Configure (E)",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app
                                        .extension_shop
                                        .handle_key(&InputEvent::Key("e".into()));
                                    this.after_widget_change(cx);
                                    cx.notify();
                                }),
                            ))
                        })
                        .when(can_toggle, |d| {
                            d.child(modal_button(
                                "shop-toggle",
                                if enabled { "Disable (D)" } else { "Enable (D)" },
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app
                                        .extension_shop
                                        .handle_key(&InputEvent::Key("d".into()));
                                    this.app.extension_shop.can_confirm_modal = true;
                                    this.after_widget_change(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "shop-uninstall",
                                "Uninstall (U)",
                                false,
                                true,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app
                                        .extension_shop
                                        .handle_key(&InputEvent::Key("u".into()));
                                    this.app.extension_shop.can_confirm_modal = true;
                                    this.after_widget_change(cx);
                                    cx.notify();
                                }),
                            ))
                        }),
                )
                .into_any_element()
        } else {
            div().flex_1().into_any_element()
        }
    } else {
        let mut list = super::modal_layout::scroll_column("shop-plugins")
            .p_3()
            .flex()
            .flex_col()
            .gap_2();

        if shop.plugins.is_empty() {
            list = list.child(
                div()
                    .p_4()
                    .text_size(px(12.5))
                    .text_color(rgb(p.text_muted))
                    .child("No matching plugins"),
            );
        }

        for (idx, plugin) in shop.plugins.iter().enumerate() {
            let sel = idx == shop.list_state.selected;
            let status = super::plugin_center::lifecycle_label(plugin);
            list = list.child(
                super::modal_layout::list_card()
                    .id(("shop-item", idx))
                    .debug_selector(move || format!("shop-item-{}", idx))
                    .p_3()
                    .rounded(px(7.0))
                    .bg(rgb(if sel { p.bg_selected } else { p.bg_surface }))
                    .border_1()
                    .border_color(rgb(if sel { p.accent } else { p.border_subtle }))
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.app.extension_shop.list_state.selected = idx;
                        this.app.extension_shop.detail_open = true;
                        cx.notify();
                    }))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .debug_selector(move || format!("shop-heading-{}", idx))
                            .w_full()
                            .min_w_0()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .debug_selector(move || format!("shop-name-{}", idx))
                                    // Preserve a reading width before the badge
                                    // moves intact onto a second header line.
                                    .flex_basis(px(160.0))
                                    .flex_grow()
                                    .min_w_0()
                                    .whitespace_normal()
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(p.text_strong))
                                    .child(plugin.name.clone()),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .debug_selector(move || format!("shop-badge-{}", idx))
                                    .whitespace_nowrap()
                                    .px_2()
                                    .py(px(1.5))
                                    .rounded(px(4.0))
                                    .bg(rgb(p.bg_elevated))
                                    .text_size(px(10.5))
                                    .text_color(rgb(if plugin.installed {
                                        p.success
                                    } else {
                                        p.text_muted
                                    }))
                                    .child(status),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .whitespace_normal()
                            .text_size(px(11.5))
                            .text_color(rgb(p.text_muted))
                            .debug_selector(move || format!("shop-description-{}", idx))
                            .child(format!("{}  ·  {}", plugin.full_name, plugin.description)),
                    ),
            );
        }
        list.into_any_element()
    };

    let confirm_overlay = (shop.confirm_modal != ShopConfirm::None).then(|| {
        let (title, body_lines, yes_label, danger) = match shop.confirm_modal {
            ShopConfirm::Trust => (
                "Trust Third-Party Plugin Repository?",
                shop.trust_lines(64),
                "Trust & Proceed (y)",
                false,
            ),
            ShopConfirm::Uninstall => (
                "Uninstall Plugin?",
                vec!["Remove this plugin from your Tuim configuration?".to_string()],
                "Uninstall (y)",
                true,
            ),
            ShopConfirm::Restart => (
                "Restart Required",
                vec!["Plugin changes require restarting the embedded Neovim session.".to_string()],
                "Restart Now (y)",
                false,
            ),
            ShopConfirm::None => ("", vec![], "", false),
        };

        let mut lines_col = super::modal_layout::scroll_column("shop-confirm-body")
            .whitespace_normal().flex().flex_col().gap_1();
        for l in body_lines {
            lines_col = lines_col.child(
                super::modal_layout::list_card()
                    .whitespace_normal()
                    .text_size(px(12.0))
                    .text_color(rgb(p.text))
                    .child(if l.is_empty() { " ".to_string() } else { l }),
            );
        }

        div()
            .id("shop-confirm-layer")
            .absolute()
            .inset_0()
            .occlude()
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .bg(rgba(0x00000099))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .debug_selector(|| "shop-confirm-card".into())
                    .w(px(520.0))
                    .max_w(relative(0.92))
                    .max_h(relative(0.92))
                    .min_w_0()
                    .min_h_0()
                    .flex_shrink_0()
                    .p_5()
                    .rounded(px(8.0))
                    .bg(rgb(p.bg_elevated))
                    .border_1()
                    .border_color(rgb(if danger { p.error } else { p.warning }))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(p.text_strong))
                            .w_full()
                            .min_w_0()
                            .flex_shrink_0()
                            .whitespace_normal()
                            .child(title),
                    )
                    .child(lines_col)
                    .child(
                        div()
                            .debug_selector(|| "shop-confirm-actions".into())
                            .w_full()
                            .min_w_0()
                            .flex_shrink_0()
                            .pt_2()
                            .flex()
                            .flex_wrap()
                            .justify_end()
                            .gap_2()
                            .child(modal_button(
                                "shop-conf-no",
                                "Cancel (n)",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app
                                        .extension_shop
                                        .handle_key(&InputEvent::Key("n".into()));
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "shop-conf-yes",
                                yes_label,
                                !danger,
                                danger,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.extension_shop.can_confirm_modal = true;
                                    this.app
                                        .extension_shop
                                        .handle_key(&InputEvent::Key("y".into()));
                                    this.after_widget_change(cx);
                                    cx.notify();
                                }),
                            )),
                    ),
            )
    });

    backdrop(
        "shop-backdrop",
        cx.listener(|this, _, _, cx| {
            if this.app.extension_shop.recipe_form.is_some() {
                this.app.extension_shop.close_recipe_form();
            } else {
                this.app.extension_shop.is_open = false;
            }
            cx.notify();
        }),
    )
    .child(
        modal_card(p, 1020.0, 700.0)
            .relative()
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .flex_wrap()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .bg(rgb(p.bg_surface))
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon("blocks", 16.0, p.accent))
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(p.text_strong))
                                    .min_w_0()
                                    .whitespace_normal()
                                    .child(match shop.section {
                                        ShopSection::Installed => "Plugin Center — Installed",
                                        ShopSection::Supported => "Plugin Center — Supported",
                                        ShopSection::Discover => "Plugin Center — Community",
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id("shop-search")
                            .w(px(280.0))
                            .max_w(relative(0.48))
                            .min_w_0()
                            .px_2()
                            .py(px(4.0))
                            .rounded(px(6.0))
                            .bg(rgb(p.bg_input))
                            .border_1()
                            .border_color(rgb(if shop.is_searching {
                                p.accent
                            } else {
                                p.border
                            }))
                            .text_size(px(11.5))
                            .text_color(rgb(p.text_strong))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.app.extension_shop.recipe_form.is_some() { return; }
                                this.app.extension_shop.is_searching = true;
                                this.app.extension_shop.detail_open = false;
                                cx.notify();
                            }))
                            .child(if shop.is_searching {
                                format!("{}▏", shop.search_query)
                            } else if shop.search_query.is_empty() {
                                "/ Search plugins...".to_string()
                            } else {
                                shop.search_query.clone()
                            }),
                    ),
            )
            .child(div().w_full().min_w_0().flex_1().min_h_0().flex().child(nav).child(main_area))
            .child(
                div()
                    .debug_selector(|| "shop-footer".into())
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .flex_wrap()
                    .gap_2()
                    .px_4()
                    .py_2()
                    .bg(rgb(p.bg_surface))
                    .border_t_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(p.text_muted))
                            .id("shop-status")
                            .debug_selector(|| "shop-status".into())
                            .flex_1()
                            .min_w(px(140.0))
                            .max_h(px(54.0))
                            .overflow_y_scroll()
                            .whitespace_normal()
                            .child(if !shop.status_msg.is_empty() {
                                shop.status_msg.clone()
                            } else {
                                format!("{} plugins", shop.plugins.len())
                            }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_wrap()
                            .gap_2()
                            .child(modal_button(
                                "shop-plugins-lua",
                                "User plugins.lua",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.extension_shop.open_user_plugins_file();
                                    this.after_widget_change(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "shop-refresh",
                                "Refresh",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.extension_shop.refresh();
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "shop-close",
                                if shop.recipe_form.is_some() { "Cancel form" } else if shop.detail_open { "Back" } else { "Close" },
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    if this.app.extension_shop.recipe_form.is_some() {
                                        this.app.extension_shop.close_recipe_form();
                                    } else if this.app.extension_shop.detail_open {
                                        this.app.extension_shop.detail_open = false;
                                    } else {
                                        this.app.extension_shop.is_open = false;
                                    }
                                    cx.notify();
                                }),
                            )),
                    ),
            )
            .children(confirm_overlay),
    )
}

fn render_mason(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let mw = &view.app.mason_widget;
    let p = gui_palette(&view.app.theme);

    let mut tabs = div()
        .id("mason-tabs")
        .debug_selector(|| "mason-tabs".into())
        .w(px(150.0))
        .max_w(relative(0.30))
        .min_w_0()
        .min_h_0()
        .h_full()
        .overflow_y_scroll()
        .flex_shrink_0()
        .bg(rgb(p.bg_surface))
        .border_r_1()
        .border_color(rgb(p.border))
        .p_2()
        .flex()
        .flex_col()
        .gap_1();

    for (i, (label, _)) in MASON_TABS.iter().enumerate() {
        let active = i == mw.active_tab;
        tabs = tabs.child(
            super::modal_layout::list_card()
                .id(("mason-tab", i))
                .px_3()
                .py(px(5.0))
                .rounded(px(5.0))
                .whitespace_normal()
                .when(active, |d| {
                    d.bg(rgb(p.bg_selected))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(p.text_strong))
                })
                .when(!active, |d| d.text_color(rgb(p.text)))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .text_size(px(12.0))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.mason_widget.active_tab = i;
                    this.app.mason_widget.apply_filter();
                    cx.notify();
                }))
                .child(label.trim()),
        );
    }

    let mut list = super::modal_layout::scroll_column("mason-list")
        .p_3()
        .flex()
        .flex_col()
        .gap_1();

    if mw.filtered.is_empty() {
        list = list.child(
            div()
                .w_full()
                .min_w_0()
                .whitespace_normal()
                .p_3()
                .text_size(px(12.0))
                .text_color(rgb(p.text_muted))
                .child("No language tools found"),
        );
    }

    for (vis_idx, &pkg_idx) in mw.filtered.iter().enumerate() {
        let pkg = &mw.packages[pkg_idx];
        let sel = vis_idx == mw.list_state.selected;
        let state_badge = if mw.pending_install.contains(&pkg.name) {
            ("+ install", p.accent)
        } else if mw.pending_uninstall.contains(&pkg.name) {
            ("- remove", p.error)
        } else if pkg.installed {
            ("installed", p.success)
        } else {
            ("available", p.text_muted)
        };

        list = list.child(
            super::modal_layout::list_card()
                .id(("mason-pkg", vis_idx))
                .debug_selector(move || format!("mason-item-{}", vis_idx))
                .px_3()
                .py(px(6.0))
                .rounded(px(5.0))
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .when(sel, |d| d.bg(rgb(p.bg_selected)))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.mason_widget.list_state.selected = vis_idx;
                    this.app.mason_widget.toggle_selected();
                    cx.notify();
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(p.text_strong))
                                .child(pkg.name.clone()),
                        )
                        .child(
                            div()
                                .debug_selector(move || format!("mason-desc-{}", vis_idx))
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(11.0))
                                .text_color(rgb(p.text_muted))
                                .child(pkg.desc.clone()),
                        ),
                )
                .child(
                    div()
                        .debug_selector(move || format!("mason-badge-{}", vis_idx))
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(state_badge.1))
                        .child(state_badge.0),
                ),
        );
    }

    backdrop(
        "mason-backdrop",
        cx.listener(|this, _, _, cx| {
            this.app.mason_widget.is_open = false;
            cx.notify();
        }),
    )
    .child(
        modal_card(p, 800.0, 540.0)
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_4()
                    .py_3()
                    .bg(rgb(p.bg_surface))
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon(ICON_LSP, 15.0, p.accent))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .whitespace_normal()
                                    .text_size(px(14.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(p.text_strong))
                                    .child("Language Tools (Mason)"),
                            ),
                    )
                    .child(
                        div()
                            .id("mason-health")
                            .w_full()
                            .min_w_0()
                            .max_h(px(54.0))
                            .overflow_y_scroll()
                            .whitespace_normal()
                            .text_size(px(11.0))
                            .text_color(rgb(p.text_muted))
                            .child(mw.health_summary.clone()),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .child(tabs)
                    .child(list),
            )
            .child(
                div()
                    .debug_selector(|| "mason-footer".into())
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .bg(rgb(p.bg_surface))
                    .border_t_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(160.0))
                            .whitespace_normal()
                            .text_size(px(11.0))
                            .text_color(rgb(p.text_muted))
                            .child("Click packages to queue install/uninstall · / to search"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .justify_end()
                            .gap_2()
                            .max_w_full()
                            .min_w_0()
                            .child(modal_button(
                                "mason-close",
                                "Close",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.mason_widget.is_open = false;
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "mason-apply",
                                "Apply Changes",
                                true,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app
                                        .mason_widget
                                        .apply_and_close(&mut this.app.editor_rpc);
                                    cx.notify();
                                }),
                            )),
                    ),
            ),
    )
}

fn render_lazy(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let lw = &view.app.lazy_widget;
    let p = gui_palette(&view.app.theme);

    let mut tabs = div()
        .id("lazy-tabs")
        .debug_selector(|| "lazy-tabs".into())
        .w(px(140.0))
        .max_w(relative(0.30))
        .min_w_0()
        .min_h_0()
        .h_full()
        .overflow_y_scroll()
        .flex_shrink_0()
        .bg(rgb(p.bg_surface))
        .border_r_1()
        .border_color(rgb(p.border))
        .p_2()
        .flex()
        .flex_col()
        .gap_1();

    for (i, label) in LAZY_TABS.iter().enumerate() {
        let active = i == lw.active_tab;
        tabs = tabs.child(
            super::modal_layout::list_card()
                .id(("lazy-tab", i))
                .px_3()
                .py(px(5.0))
                .rounded(px(5.0))
                .whitespace_normal()
                .when(active, |d| {
                    d.bg(rgb(p.bg_selected))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(p.text_strong))
                })
                .when(!active, |d| d.text_color(rgb(p.text)))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .text_size(px(12.0))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.lazy_widget.active_tab = i;
                    this.app.lazy_widget.apply_filter();
                    cx.notify();
                }))
                .child(label.trim()),
        );
    }

    let mut list = super::modal_layout::scroll_column("lazy-list")
        .p_3()
        .flex()
        .flex_col()
        .gap_1();

    for (vis_idx, &idx) in lw.filtered.iter().enumerate() {
        let plugin = &lw.plugins[idx];
        let sel = vis_idx == lw.list_state.selected;
        list = list.child(
            super::modal_layout::list_card()
                .id(("lazy-item", vis_idx))
                .debug_selector(move || format!("lazy-item-{}", vis_idx))
                .px_3()
                .py(px(6.0))
                .rounded(px(5.0))
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .when(sel, |d| d.bg(rgb(p.bg_selected)))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.lazy_widget.list_state.selected = vis_idx;
                    cx.notify();
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(12.5))
                        .text_color(rgb(p.text_strong))
                        .child(plugin.name.clone()),
                )
                .child(
                    div()
                        .debug_selector(move || format!("lazy-badge-{}", vis_idx))
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .text_size(px(11.0))
                        .text_color(rgb(if plugin.loaded {
                            p.success
                        } else {
                            p.text_muted
                        }))
                        .child(if plugin.loaded { "loaded" } else { "lazy" }),
                ),
        );
    }

    backdrop(
        "lazy-backdrop",
        cx.listener(|this, _, _, cx| {
            this.app.lazy_widget.is_open = false;
            cx.notify();
        }),
    )
    .child(
        modal_card(p, 840.0, 520.0)
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .whitespace_normal()
                    .px_4()
                    .py_3()
                    .bg(rgb(p.bg_surface))
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .text_size(px(14.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(p.text_strong))
                    .child("Plugin Manager (Lazy.nvim)"),
            )
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .child(tabs)
                    .child(list),
            )
            .child(
                div()
                    .debug_selector(|| "lazy-footer".into())
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .bg(rgb(p.bg_surface))
                    .border_t_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        div()
                            .id("lazy-status")
                            .flex_1()
                            .min_w(px(160.0))
                            .max_h(px(54.0))
                            .overflow_y_scroll()
                            .whitespace_normal()
                            .text_size(px(11.0))
                            .text_color(rgb(p.text_muted))
                            .child(if lw.status_msg.is_empty() {
                                format!("{} plugins", lw.filtered.len())
                            } else {
                                lw.status_msg.clone()
                            }),
                    )
                    .child(
                        div()
                            .debug_selector(|| "lazy-footer-actions".into())
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .justify_end()
                            .gap_2()
                            .max_w_full()
                            .min_w_0()
                            .child(modal_button(
                                "lazy-configure",
                                "Configure (E)",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.lazy_widget.handle_key(
                                        &InputEvent::Key("e".into()),
                                        &mut this.app.editor_rpc,
                                    );
                                    this.after_widget_change(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "lazy-plugins-lua",
                                "plugins.lua (P)",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.lazy_widget.handle_key(
                                        &InputEvent::Key("p".into()),
                                        &mut this.app.editor_rpc,
                                    );
                                    this.after_widget_change(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "lazy-sync",
                                "Sync (S)",
                                true,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.lazy_widget.handle_key(
                                        &InputEvent::Key("s".into()),
                                        &mut this.app.editor_rpc,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "lazy-update",
                                "Update (U)",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.lazy_widget.handle_key(
                                        &InputEvent::Key("u".into()),
                                        &mut this.app.editor_rpc,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "lazy-clean",
                                "Clean (X)",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.lazy_widget.handle_key(
                                        &InputEvent::Key("x".into()),
                                        &mut this.app.editor_rpc,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "lazy-close",
                                "Close",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.lazy_widget.is_open = false;
                                    cx.notify();
                                }),
                            )),
                    ),
            ),
    )
}

fn render_git_detailed(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let gd = &view.app.git_detailed_widget;
    let p = gui_palette(&view.app.theme);

    let mut list = super::modal_layout::scroll_column("git-det-list")
        .p_3()
        .flex()
        .flex_col()
        .gap_1();

    if gd.active_tab == 2 {
        if let Some(diff) = &gd.diff_preview {
            list = list.child(div().text_size(px(12.0)).text_color(rgb(p.text_muted))
                .child(diff.title.clone()));
            for (idx, line) in diff.text.lines().enumerate() {
                let color = if line.starts_with('+') { p.success }
                    else if line.starts_with('-') { p.error }
                    else { p.text_strong };
                list = list.child(div().id(("git-diff-line", idx))
                    .debug_selector(move || format!("git-diff-line-{idx}"))
                    .font_family("monospace").text_size(px(12.0))
                    .whitespace_normal().text_color(rgb(color)).child(line.to_string()));
            }
        }
    } else if gd.active_tab == 0 {
        for (idx, cm) in gd.snapshot.commits.iter().enumerate() {
            let sel = idx == gd.list_state.selected;
            list = list.child(
                super::modal_layout::list_card()
                    .id(("gd-cm", idx))
                    .debug_selector(move || format!("git-det-commit-{}", idx))
                    .px_3()
                    .py(px(6.0))
                    .rounded(px(5.0))
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap_3()
                    .when(sel, |d| d.bg(rgb(p.bg_selected)))
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.app.git_detailed_widget.list_state.selected = idx;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(p.accent))
                                    .child(cm.hash.clone()),
                            )
                            .child(
                                div()
                                    .debug_selector(move || format!("git-det-msg-{}", idx))
                                    .flex_1()
                                    .min_w_0()
                                    .whitespace_normal()
                                    .text_size(px(12.0))
                                    .text_color(rgb(p.text_strong))
                                    .child(cm.message.clone()),
                            ),
                    )
                    .child(
                        div()
                            .debug_selector(move || format!("git-det-meta-{}", idx))
                            .flex_shrink_0()
                            .w(px(180.0))
                            .max_w_full()
                            .whitespace_normal()
                            .text_size(px(11.0))
                            .text_color(rgb(p.text_muted))
                            .child(format!("{} · {}", cm.author, cm.date)),
                    ),
            );
        }
    } else {
        for (idx, br) in gd.snapshot.branches.iter().enumerate() {
            let sel = idx == gd.list_state.selected;
            let current = *br == gd.snapshot.branch;
            list = list.child(
                super::modal_layout::list_card()
                    .id(("gd-br", idx))
                    .debug_selector(move || format!("git-det-branch-{}", idx))
                    .px_3()
                    .py(px(6.0))
                    .rounded(px(5.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(sel, |d| d.bg(rgb(p.bg_selected)))
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.app.git_detailed_widget.list_state.selected = idx;
                        cx.notify();
                    }))
                    .child(icon(
                        "git-branch",
                        13.0,
                        if current { p.success } else { p.text_muted },
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_normal()
                            .text_size(px(12.5))
                            .font_weight(if current {
                                FontWeight::BOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(rgb(if current { p.success } else { p.text_strong }))
                            .child(br.clone()),
                    ),
            );
        }
    }

    backdrop(
        "git-det-backdrop",
        cx.listener(|this, _, _, cx| {
            this.app.git_detailed_widget.is_open = false;
            cx.notify();
        }),
    )
    .child(
        modal_card(p, 760.0, 500.0)
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_4()
                    .py_3()
                    .bg(rgb(p.bg_surface))
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(180.0))
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon("history", 15.0, p.accent))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .whitespace_normal()
                                    .text_size(px(14.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(p.text_strong))
                                    .child(if gd.active_tab == 2 { "Git Diff Preview" } else { "Git History & Branches" }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap_1()
                            .max_w_full()
                            .min_w_0()
                            .child(modal_button(
                                "gd-tab-0",
                                "1 History",
                                gd.active_tab == 0,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.git_detailed_widget.active_tab = 0;
                                    this.app.git_detailed_widget.list_state.move_home();
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "gd-tab-1",
                                "2 Branches",
                                gd.active_tab == 1,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.git_detailed_widget.active_tab = 1;
                                    this.app.git_detailed_widget.list_state.move_home();
                                    cx.notify();
                                }),
                            )),
                    ),
            )
            .child(list)
            .when(!gd.status_msg.is_empty(), |d| d.child(div().px_4().py_2()
                .text_size(px(12.0)).text_color(rgb(p.text_muted)).child(gd.status_msg.clone())))
            .child(
                div()
                    .debug_selector(|| "git-det-footer".into())
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .bg(rgb(p.bg_surface))
                    .border_t_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap_2()
                    .child(modal_button(
                        "gd-checkout",
                        if gd.active_tab == 2 { "Diff (read-only)" } else if gd.active_tab == 1 { "Switch Branch (Enter)" } else { "Detach at Commit (Enter)" },
                        true,
                        false,
                        p,
                        cx.listener(|this, _, _, cx| {
                            this.app
                                .git_detailed_widget
                                .handle_key(&InputEvent::Key("<CR>".into()));
                            this.after_widget_change(cx);
                            cx.notify();
                        }),
                    ))
                    .child(modal_button(
                        "gd-close",
                        "Close",
                        false,
                        false,
                        p,
                        cx.listener(|this, _, _, cx| {
                            this.app.git_detailed_widget.is_open = false;
                            cx.notify();
                        }),
                    )),
            ),
    )
}

fn render_orchestrator(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let orch = &view.app.orchestrator;
    let p = gui_palette(&view.app.theme);

    let mut list = super::modal_layout::scroll_column("orch-list")
        .p_3()
        .flex()
        .flex_col()
        .gap_1();

    for (idx, wt) in orch.worktrees.iter().enumerate() {
        let sel = idx == orch.list_state.selected;
        list = list.child(
            super::modal_layout::list_card()
                .id(("orch-wt", idx))
                .debug_selector(move || format!("orch-item-{}", idx))
                .px_3()
                .py(px(7.0))
                .rounded(px(5.0))
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .when(sel, |d| d.bg(rgb(p.bg_selected)))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.orchestrator.list_state.selected = idx;
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_strong))
                                .child(wt.branch.clone()),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(11.0))
                                .text_color(rgb(p.text_muted))
                                .child(wt.path.clone()),
                        ),
                )
                .child(
                    div()
                        .debug_selector(move || format!("orch-head-{}", idx))
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .text_size(px(11.0))
                        .text_color(rgb(p.accent))
                        .child(wt.head.clone()),
                ),
        );
    }

    backdrop(
        "orch-backdrop",
        cx.listener(|this, _, _, cx| {
            this.app.orchestrator.is_open = false;
            cx.notify();
        }),
    )
    .child(
        modal_card(p, 700.0, 460.0)
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_4()
                    .py_3()
                    .bg(rgb(p.bg_surface))
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon("git-fork", 15.0, p.accent))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_normal()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(p.text_strong))
                            .child("Worktree & Agent Orchestrator"),
                    ),
            )
            .when(orch.is_creating, |d| {
                d.child(
                    div()
                        .id("orch-branch-input")
                        .w_full()
                        .min_w_0()
                        .max_h(px(54.0))
                        .overflow_y_scroll()
                        .flex_shrink_0()
                        .whitespace_normal()
                        .px_4()
                        .py_2()
                        .bg(rgb(p.bg_input))
                        .border_b_1()
                        .border_color(rgb(p.accent))
                        .text_size(px(12.5))
                        .text_color(rgb(p.text_strong))
                        .child(format!("New branch name: {}▏", orch.branch_input)),
                )
            })
            .child(list)
            .child(
                div()
                    .debug_selector(|| "orch-footer".into())
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .bg(rgb(p.bg_surface))
                    .border_t_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        div()
                            .id("orch-status")
                            .flex_1()
                            .min_w(px(160.0))
                            .max_h(px(54.0))
                            .overflow_y_scroll()
                            .whitespace_normal()
                            .text_size(px(11.0))
                            .text_color(rgb(p.text_muted))
                            .child(if orch.status_msg.is_empty() {
                                "n: New Worktree · Enter: Launch Codex in selected worktree"
                                    .to_string()
                            } else {
                                orch.status_msg.clone()
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .justify_end()
                            .gap_2()
                            .max_w_full()
                            .min_w_0()
                            .child(modal_button(
                                "orch-new",
                                "New Worktree (n)",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.orchestrator.is_creating = true;
                                    this.app.orchestrator.branch_input.clear();
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "orch-launch",
                                "Launch Agent",
                                true,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app
                                        .orchestrator
                                        .handle_key(&InputEvent::Key("<CR>".into()));
                                    this.after_widget_change(cx);
                                    cx.notify();
                                }),
                            ))
                            .child(modal_button(
                                "orch-close",
                                "Close",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.orchestrator.is_open = false;
                                    cx.notify();
                                }),
                            )),
                    ),
            ),
    )
}

fn format_compile_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 10.0 {
        format!("{:.1}s", secs)
    } else if secs < 60.0 {
        format!("{:.0}s", secs)
    } else {
        let total = d.as_secs();
        format!("{}m {}s", total / 60, total % 60)
    }
}

fn resolve_ansi_color(color: AnsiColor, p: GuiPalette) -> u32 {
    match color {
        AnsiColor::Default => p.text,
        AnsiColor::Muted => p.text_muted,
        AnsiColor::Error => p.error,
        AnsiColor::Success => p.success,
        AnsiColor::Warning => p.warning,
        AnsiColor::Accent => p.accent,
        AnsiColor::Cyan => blend_u32(p.accent, p.success, 48),
        AnsiColor::Magenta => blend_u32(p.accent, p.error, 45),
        AnsiColor::Rgb(c) => c,
    }
}

pub(super) fn render_compile_line(line: &CompileLine, p: GuiPalette, base_bg: u32) -> AnyElement {
    if line.plain.is_empty() {
        return super::modal_layout::list_card()
            .h(px(16.0))
            .into_any_element();
    }

    let mut text = String::with_capacity(line.plain.len());
    let mut highlights: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    for span in &line.spans {
        if span.text.is_empty() {
            continue;
        }
        let start = text.len();
        text.push_str(&span.text);
        let end = text.len();
        let mut fg = resolve_ansi_color(span.color, p);
        if span.dim {
            fg = blend_u32(base_bg, fg, 62);
        }
        let mut style = HighlightStyle::default();
        if span.color != AnsiColor::Default || span.dim {
            style.color = Some(rgb(fg).into());
        }
        if span.bold {
            style.font_weight = Some(FontWeight::BOLD);
        }
        if style.color.is_some() || style.font_weight.is_some() {
            highlights.push((start..end, style));
        }
    }

    let content = if highlights.is_empty() {
        StyledText::new(text)
    } else {
        StyledText::new(text).with_highlights(highlights)
    };

    super::modal_layout::list_card()
        .whitespace_normal()
        .text_size(px(12.0))
        .line_height(px(18.0))
        .text_color(rgb(p.text))
        .child(content)
        .into_any_element()
}

fn render_compile(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let cw = &view.app.compile_widget;
    let p = gui_palette(&view.app.theme);
    let term_bg = view.app.theme.bg_terminal;
    let is_running = cw.is_running();
    let elapsed_str = format_compile_duration(cw.elapsed());
    let cwd_display = cw.display_cwd();

    let (badge_label, badge_fg, badge_bg, badge_icon): (String, u32, u32, Option<&'static str>) =
        match cw.stage {
            CompileStage::Ready => ("Ready".into(), p.text_muted, p.bg_elevated, None),
            CompileStage::Running => (
                format!("Running · {}", elapsed_str),
                p.accent,
                blend_u32(p.bg_surface, p.accent, 20),
                None,
            ),
            CompileStage::Succeeded => (
                format!("Passed · {}", elapsed_str),
                p.success,
                blend_u32(p.bg_surface, p.success, 20),
                Some("check"),
            ),
            CompileStage::Failed(code) => (
                format!("Failed (exit {}) · {}", code, elapsed_str),
                p.error,
                blend_u32(p.bg_surface, p.error, 20),
                Some("x"),
            ),
            CompileStage::Stopped => (
                format!("Stopped · {}", elapsed_str),
                p.warning,
                blend_u32(p.bg_surface, p.warning, 20),
                None,
            ),
            CompileStage::StartError => (
                "Start Error".into(),
                p.error,
                blend_u32(p.bg_surface, p.error, 20),
                Some("x"),
            ),
        };

    // Split command text at cursor for accurate caret rendering.
    let byte_pos = cw
        .command
        .char_indices()
        .nth(cw.cursor)
        .map(|(idx, _)| idx)
        .unwrap_or(cw.command.len());
    let (cmd_before, cmd_after) = cw.command.split_at(byte_pos);
    let cmd_is_empty = cw.command.is_empty();
    let has_custom_override =
        !cw.default_command.is_empty() && cw.command.trim() != cw.default_command.trim();

    let partial = cw.active_partial_line();
    let total_lines = cw.lines.len() + usize::from(partial.is_some());
    let has_output = total_lines > 0;

    let mut output_col = super::modal_layout::scroll_column("compile-output")
        .track_scroll(&view.compile_scroll)
        .bg(rgb(term_bg))
        .font_family(view.mono_font.clone())
        .px_4()
        .py_3()
        .flex()
        .flex_col()
        .gap(px(1.0));

    if !has_output {
        let (empty_title, empty_hint) = match cw.stage {
            CompileStage::Running => (
                "Compiling…",
                "Waiting for compiler output…",
            ),
            CompileStage::Succeeded => (
                "Build succeeded with no output.",
                "Press Enter or click Re-run to build again.",
            ),
            CompileStage::StartError => (
                "Could not start build command.",
                if cw.error_message.is_empty() {
                    "Check the command above and try again."
                } else {
                    cw.error_message.as_str()
                },
            ),
            _ => (
                "Ready to compile",
                "Edit the build command above, then press Enter or click Run.",
            ),
        };
        output_col = output_col.child(
            div()
                .w_full()
                .flex_1()
                .py_8()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .font_family(view.ui_font.clone())
                .child(icon(
                    "terminal",
                    24.0,
                    if is_running { p.accent } else { p.text_muted },
                ))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(p.text_strong))
                        .child(empty_title),
                )
                .child(
                    div()
                        .max_w(px(460.0))
                        .whitespace_normal()
                        .text_size(px(11.5))
                        .text_color(rgb(p.text_muted))
                        .child(empty_hint.to_string()),
                ),
        );
    } else {
        for line in &cw.lines {
            output_col = output_col.child(render_compile_line(line, p, term_bg));
        }
        if let Some(ref tail) = partial {
            output_col = output_col.child(render_compile_line(tail, p, term_bg));
        }
    }

    let close_hover_bg = blend_u32(p.bg_surface, p.error, 22);
    let run_label = if cw.stage == CompileStage::Ready {
        "Run"
    } else {
        "Re-run"
    };

    backdrop(
        "compile-backdrop",
        cx.listener(|this, _, _, cx| {
            this.app.compile_widget.close();
            cx.notify();
        }),
    )
    .child(
        modal_card(p, 840.0, 580.0)
            // Top header bar
            .child(
                div()
                    .debug_selector(|| "compile-header".into())
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_4()
                    .py_3()
                    .bg(rgb(p.bg_surface))
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon("terminal", 15.0, p.accent))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(14.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(p.text_strong))
                                    .child("Compile"),
                            )
                            .when(!cwd_display.is_empty(), |d| {
                                d.child(
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .px_2()
                                        .py(px(2.0))
                                        .rounded(px(4.0))
                                        .bg(rgb(p.bg_elevated))
                                        .text_size(px(11.0))
                                        .text_color(rgb(p.text_muted))
                                        .child(cwd_display),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .debug_selector(|| "compile-status-badge".into())
                                    .flex_shrink_0()
                                    .whitespace_nowrap()
                                    .px_2()
                                    .py(px(3.0))
                                    .rounded(px(5.0))
                                    .bg(rgb(badge_bg))
                                    .flex()
                                    .items_center()
                                    .gap(px(5.0))
                                    .when(is_running, |d| {
                                        d.child(
                                            div()
                                                .size(px(6.5))
                                                .rounded_full()
                                                .bg(rgb(p.accent)),
                                        )
                                    })
                                    .when_some(badge_icon, |d, ic| {
                                        d.child(icon(ic, 11.5, badge_fg))
                                    })
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(rgb(badge_fg))
                                            .child(badge_label),
                                    ),
                            )
                            .child(
                                div()
                                    .id("compile-header-close")
                                    .group("compile-header-close-grp")
                                    .size(px(24.0))
                                    .rounded(px(5.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .hover(move |s| s.bg(rgb(close_hover_bg)))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.app.compile_widget.close();
                                        cx.notify();
                                    }))
                                    .child(
                                        icon("x", 13.5, p.text_muted).group_hover(
                                            "compile-header-close-grp",
                                            move |s| s.text_color(rgb(p.error)),
                                        ),
                                    ),
                            ),
                    ),
            )
            // Editable command input bar
            .child(
                div()
                    .debug_selector(|| "compile-command-bar".into())
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_4()
                    .py(px(10.0))
                    .bg(rgb(p.bg_panel))
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id("compile-command-input")
                            .flex_1()
                            .min_w_0()
                            .px_3()
                            .py(px(6.0))
                            .rounded(px(6.0))
                            .bg(rgb(p.bg_input))
                            .border_1()
                            .border_color(rgb(if is_running { p.border } else { p.accent }))
                            .flex()
                            .items_center()
                            .gap_2()
                            .font_family(view.mono_font.clone())
                            .text_size(px(12.5))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(if is_running {
                                        p.text_muted
                                    } else {
                                        p.accent
                                    }))
                                    .child("$"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_color(rgb(if cmd_is_empty {
                                        p.text_muted
                                    } else if is_running {
                                        p.text
                                    } else {
                                        p.text_strong
                                    }))
                                    .child(if cmd_is_empty && is_running {
                                        String::new()
                                    } else if cmd_is_empty {
                                        "Enter build command (e.g. cargo build)…".to_string()
                                    } else if is_running {
                                        cw.command.clone()
                                    } else {
                                        format!("{}▏{}", cmd_before, cmd_after)
                                    }),
                            ),
                    )
                    .when(has_custom_override && !is_running, |d| {
                        d.child(modal_button(
                            "compile-reset",
                            "Reset",
                            false,
                            false,
                            p,
                            cx.listener(|this, _, _, cx| {
                                this.app.compile_widget.reset_to_default();
                                cx.notify();
                            }),
                        ))
                    })
                    .child(if is_running {
                        modal_button(
                            "compile-stop",
                            "Stop",
                            false,
                            true,
                            p,
                            cx.listener(|this, _, _, cx| {
                                this.app.compile_widget.stop();
                                cx.notify();
                            }),
                        )
                        .into_any_element()
                    } else {
                        modal_button(
                            "compile-run",
                            run_label,
                            true,
                            false,
                            p,
                            cx.listener(|this, _, _, cx| {
                                this.app.compile_widget.start();
                                cx.notify();
                            }),
                        )
                        .into_any_element()
                    }),
            )
            // Live scrollable monospace output
            .child(output_col)
            // Footer bar
            .child(
                div()
                    .debug_selector(|| "compile-footer".into())
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .bg(rgb(p.bg_surface))
                    .border_t_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(160.0))
                            .text_size(px(11.0))
                            .text_color(rgb(p.text_muted))
                            .child(if has_output {
                                format!(
                                    "{} {} · Enter {} · Ctrl+C Stop · Esc Close",
                                    total_lines,
                                    if total_lines == 1 { "line" } else { "lines" },
                                    run_label
                                )
                            } else {
                                "Enter Run · Ctrl+R Re-run · Ctrl+C Stop · Esc Close".to_string()
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .justify_end()
                            .gap_2()
                            .max_w_full()
                            .min_w_0()
                            .when(has_output, |d| {
                                d.child(modal_button(
                                    "compile-copy",
                                    "Copy Output",
                                    false,
                                    false,
                                    p,
                                    cx.listener(|this, _, _, cx| {
                                        let text = this.app.compile_widget.plain_output();
                                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                                        this.app
                                            .set_notice("info", "Compile output copied to clipboard");
                                        cx.notify();
                                    }),
                                ))
                            })
                            .when(has_output && !is_running, |d| {
                                d.child(modal_button(
                                    "compile-clear",
                                    "Clear",
                                    false,
                                    false,
                                    p,
                                    cx.listener(|this, _, _, cx| {
                                        this.app.compile_widget.clear_output();
                                        cx.notify();
                                    }),
                                ))
                            })
                            .child(modal_button(
                                "compile-close",
                                "Close",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| {
                                    this.app.compile_widget.close();
                                    cx.notify();
                                }),
                            )),
                    ),
            ),
    )
}

pub(super) fn advance_bug_report_to_consent(view: &mut GtuimView, cx: &mut Context<GtuimView>) {
    if view.app.bug_report.summary.trim().is_empty() {
        view.app.bug_report.focus = 1;
        view.app
            .set_notice("warning", "Please enter a summary before reviewing the draft.");
        cx.notify();
        return;
    }
    view.app.bug_report.error_message.clear();
    view.app.bug_report.stage = BugStage::Consent;
    cx.notify();
}

pub(super) fn copy_bug_report_markdown(view: &mut GtuimView, cx: &mut Context<GtuimView>) {
    let diag = view.diagnostics_json();
    let markdown = view.app.bug_report.report_markdown(&diag);
    cx.write_to_clipboard(ClipboardItem::new_string(markdown));
    view.app.set_notice(
        "info",
        "Full redacted bug report copied to clipboard (review before posting publicly).",
    );
    cx.notify();
}

pub(super) fn open_bug_report_draft(view: &mut GtuimView, cx: &mut Context<GtuimView>) {
    let diag = view.diagnostics_json();
    match view.app.bug_report.prepare_draft(&diag) {
        Ok(draft) => {
            if draft.needs_clipboard {
                cx.write_to_clipboard(ClipboardItem::new_string(draft.report_markdown.clone()));
            }
            cx.open_url(&draft.url);
            view.app.bug_report.issue_url = draft.url;
            view.app.bug_report.needs_clipboard = draft.needs_clipboard;
            view.app.bug_report.error_message.clear();
            view.app.bug_report.stage = BugStage::Success;
            if draft.needs_clipboard {
                view.app.set_notice(
                    "info",
                    "Full redacted report copied; asked your browser to open the GitHub issue draft.",
                );
            } else {
                view.app.set_notice(
                    "info",
                    "Asked your browser to open the GitHub issue draft (not yet submitted).",
                );
            }
        }
        Err(error) => {
            view.app.bug_report.error_message = error;
            view.app.bug_report.needs_clipboard = false;
            view.app.bug_report.stage = BugStage::Failure;
        }
    }
    cx.notify();
}

fn copy_bug_report_url(view: &mut GtuimView, cx: &mut Context<GtuimView>) {
    let url = view.app.bug_report.issue_url.clone();
    if !url.is_empty() {
        cx.write_to_clipboard(ClipboardItem::new_string(url));
        view.app.set_notice("info", "GitHub draft URL copied to clipboard.");
        cx.notify();
    }
}

fn reopen_bug_report_draft(view: &mut GtuimView, cx: &mut Context<GtuimView>) {
    let url = view.app.bug_report.issue_url.clone();
    if !url.is_empty() {
        cx.open_url(&url);
        view.app.set_notice("info", "Asked your browser to open the GitHub issue draft again.");
        cx.notify();
    }
}

/// Keep GUI keyboard actions on the same native clipboard/browser path as clicks.
pub(super) fn handle_bug_report_tuim_key(
    view: &mut GtuimView,
    key: &str,
    cx: &mut Context<GtuimView>,
) -> bool {
    if !view.app.bug_report.is_open {
        return false;
    }
    let stage = view.app.bug_report.stage;
    if !matches!(stage, BugStage::Consent | BugStage::Success | BugStage::Failure) {
        return false;
    }
    match (stage, key.to_ascii_lowercase().as_str()) {
        (BugStage::Consent, "y" | "<cr>") => open_bug_report_draft(view, cx),
        (BugStage::Consent, "n" | "<esc>")
        | (BugStage::Success | BugStage::Failure, "r") => {
            view.app.bug_report.stage = BugStage::Form;
            cx.notify();
        }
        (_, "c") => copy_bug_report_markdown(view, cx),
        (BugStage::Success, "o") => reopen_bug_report_draft(view, cx),
        (BugStage::Success, "u") => copy_bug_report_url(view, cx),
        (BugStage::Success | BugStage::Failure, "<esc>" | "<cr>" | "q") => {
            view.app.bug_report.is_open = false;
            cx.notify();
        }
        _ => {}
    }
    // Swallow unhandled keys so they cannot leak to the editor or TUI launcher.
    true
}

pub(super) fn handle_bug_report_key(
    view: &mut GtuimView,
    ev: &KeyDownEvent,
    cx: &mut Context<GtuimView>,
) -> bool {
    let ks = &ev.keystroke;
    let key = if ks.modifiers.control || ks.modifiers.alt || ks.modifiers.platform {
        "" // Modified shortcuts must not accidentally trigger an unmodified action.
    } else {
        match ks.key.as_str() {
            "enter" => "<CR>",
            "escape" => "<Esc>",
            key => key,
        }
    };
    handle_bug_report_tuim_key(view, key, cx)
}

fn bug_button(
    id: &'static str,
    label: &'static str,
    primary: bool,
    danger: bool,
    p: GuiPalette,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .debug_selector(move || id.to_string())
        .flex_shrink_0()
        .child(modal_button(id, label, primary, danger, p, handler))
}

fn render_bug_report(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let br = &view.app.bug_report;
    let p = gui_palette(&view.app.theme);
    let summary_ready = !br.summary.trim().is_empty();
    let repo_label = br.report_repo.trim().to_string();

    let body: AnyElement = match br.stage {
        BugStage::Form => {
            let mut cats = div().w_full().min_w_0().flex().flex_wrap().gap_1();
            for (idx, cat) in BUG_CATEGORIES.iter().enumerate() {
                let active = idx == br.category_idx;
                cats = cats.child(
                    div()
                        .id(("bug-cat", idx))
                        .px_2()
                        .py(px(4.0))
                        .rounded(px(5.0))
                        .bg(rgb(if active { p.accent } else { p.bg_surface }))
                        .text_size(px(11.5))
                        .text_color(rgb(if active { p.accent_fg } else { p.text }))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.app.bug_report.category_idx = idx;
                            cx.notify();
                        }))
                        .child(*cat),
                );
            }

            super::modal_layout::scroll_column("bug-form-body")
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex_shrink_0()
                        .whitespace_normal()
                        .text_size(px(11.5))
                        .text_color(rgb(p.text_muted))
                        .child(format!(
                            "Opens a PUBLIC GitHub issue draft on {} (override with GTUIM_BUG_REPORT_REPO). Nothing is submitted automatically.",
                            if repo_label.is_empty() { "<unconfigured>" } else { repo_label.as_str() }
                        )),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(10.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_muted))
                                .child("CATEGORY"),
                        )
                        .child(cats),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(10.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_muted))
                                .child("SUMMARY (REQUIRED)"),
                        )
                        .child(
                            div()
                                .id("bug-summary")
                                .debug_selector(|| "bug-summary".into())
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .px_3()
                                .py_2()
                                .rounded(px(6.0))
                                .bg(rgb(p.bg_input))
                                .border_1()
                                .border_color(rgb(if br.focus == 1 {
                                    p.accent
                                } else {
                                    p.border
                                }))
                                .text_size(px(12.5))
                                .text_color(rgb(p.text_strong))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.app.bug_report.focus = 1;
                                    cx.notify();
                                }))
                                .child(if br.focus == 1 {
                                    format!("{}▏", br.summary)
                                } else if br.summary.is_empty() {
                                    "Short summary of the bug...".to_string()
                                } else {
                                    br.summary.clone()
                                }),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex_1()
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(10.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_muted))
                                .child("DESCRIPTION"),
                        )
                        .child(
                            div()
                                .id("bug-desc")
                                .debug_selector(|| "bug-desc".into())
                                .flex_1()
                                .min_h(px(100.0))
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .px_3()
                                .py_2()
                                .rounded(px(6.0))
                                .bg(rgb(p.bg_input))
                                .border_1()
                                .border_color(rgb(if br.focus == 2 {
                                    p.accent
                                } else {
                                    p.border
                                }))
                                .text_size(px(12.5))
                                .text_color(rgb(p.text_strong))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.app.bug_report.focus = 2;
                                    cx.notify();
                                }))
                                .child(if br.focus == 2 {
                                    format!("{}▏", br.description)
                                } else if br.description.is_empty() {
                                    "Steps to reproduce...".to_string()
                                } else {
                                    br.description.clone()
                                }),
                        ),
                )
                .into_any_element()
        }
        BugStage::Consent => {
            let preview = br.prepare_draft(&view.diagnostics_json());
            let mut col = super::modal_layout::scroll_column("bug-consent-body")
                .p_5()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(p.text_strong))
                        .child("Open a PUBLIC GitHub Issue Draft?"),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(12.0))
                        .text_color(rgb(p.text))
                        .child(format!(
                            "This opens a PUBLIC GitHub issue draft in your browser for {}. Nothing is submitted as an issue automatically (GitHub sign-in is required to post), but opening the draft sends URL-embedded report content to GitHub and may record it in browser history. Long reports over {MAX_DRAFT_URL_BYTES} bytes use the local clipboard instead of embedding the report in the URL.",
                            if repo_label.is_empty() { "<unconfigured>" } else { repo_label.as_str() }
                        )),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(12.0))
                        .text_color(rgb(p.text))
                        .child("Best-effort redaction strips your home path and common secret patterns from your summary, description, and runtime diagnostics, but cannot guarantee every private detail is removed. Review your text before opening the draft and before posting publicly on GitHub."),
                );
            match preview {
                Ok(draft) if draft.needs_clipboard => {
                    col = col.child(
                        div()
                            .debug_selector(|| "bug-consent-clipboard-notice".into())
                            .w_full()
                            .min_w_0()
                            .whitespace_normal()
                            .p_3()
                            .rounded(px(6.0))
                            .bg(rgb(p.bg_surface))
                            .border_1()
                            .border_color(rgb(p.accent))
                            .text_size(px(11.5))
                            .text_color(rgb(p.text_strong))
                            .child(format!(
                                "Long report: exceeds the {MAX_DRAFT_URL_BYTES}-byte browser URL limit. Gtuim will copy the full redacted Markdown report to your clipboard first and open a short GitHub draft so you can paste the full report and set the title without losing any text."
                            )),
                    );
                }
                Err(error) => {
                    col = col.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .whitespace_normal()
                            .p_3()
                            .rounded(px(6.0))
                            .bg(rgb(p.bg_surface))
                            .border_1()
                            .border_color(rgb(p.error))
                            .text_size(px(11.5))
                            .text_color(rgb(p.error))
                            .child(error),
                    );
                }
                Ok(_) => {}
            }
            col.child(
                div()
                    .w_full()
                    .min_w_0()
                    .whitespace_normal()
                    .text_size(px(11.0))
                    .text_color(rgb(p.text_muted))
                    .child("Keys: Enter / Y Open Draft · C Copy Full Report · N / Esc Back"),
            )
            .into_any_element()
        }
        BugStage::Submitting => super::modal_layout::scroll_column("bug-submitting-body")
            .p_5()
            .flex()
            .items_center()
            .justify_center()
            .whitespace_normal()
            .text_size(px(13.0))
            .text_color(rgb(p.text_strong))
            .child("Opening GitHub issue draft in your browser (not submitting)...")
            .into_any_element(),
        BugStage::Success => {
            let mut col = super::modal_layout::scroll_column("bug-success-body")
                .p_5()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(14.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(p.success))
                        .child("GitHub Draft Requested — Not Yet Submitted"),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(12.0))
                        .text_color(rgb(p.text))
                        .child("Gtuim asked your browser to open a PUBLIC GitHub issue draft. Nothing has been submitted yet — review the redacted content for private information and submit it on GitHub."),
                );
            if br.needs_clipboard {
                col = col.child(
                    div()
                        .debug_selector(|| "bug-clipboard-callout".into())
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .p_3()
                        .rounded(px(6.0))
                        .bg(rgb(p.bg_surface))
                        .border_1()
                        .border_color(rgb(p.accent))
                        .text_size(px(12.0))
                        .text_color(rgb(p.text_strong))
                        .child("Full redacted report copied to your clipboard (the draft was too long for a browser URL, so nothing was truncated). Paste it into the GitHub issue body and set the title from its Summary section."),
                );
            }
            if !br.issue_url.is_empty() {
                col = col.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(10.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_muted))
                                .child("DRAFT URL (IF BROWSER DID NOT OPEN, COPY REPORT OR RETRY)"),
                        )
                        .child(
                            div()
                                .debug_selector(|| "bug-draft-url".into())
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .p_2()
                                .rounded(px(6.0))
                                .bg(rgb(p.bg_input))
                                .border_1()
                                .border_color(rgb(p.border))
                                .text_size(px(11.0))
                                .text_color(rgb(p.text))
                                .child(br.issue_url.clone()),
                        ),
                );
            }
            col.child(
                div()
                    .w_full()
                    .min_w_0()
                    .whitespace_normal()
                    .text_size(px(11.0))
                    .text_color(rgb(p.text_muted))
                    .child(if br.issue_url.is_empty() {
                        "Keys: C Copy Report · R Edit Form · Enter / Esc Close"
                    } else {
                        "Keys: C Copy Report · R Edit Form · O Open Draft · U Copy URL · Enter / Esc Close"
                    }),
            )
            .into_any_element()
        }
        BugStage::Failure => super::modal_layout::scroll_column("bug-failure-body")
            .p_5()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .whitespace_normal()
                    .text_size(px(14.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(p.error))
                    .child("Could Not Open GitHub Draft"),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .whitespace_normal()
                    .text_size(px(12.0))
                    .text_color(rgb(p.text))
                    .child(br.error_message.clone()),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .whitespace_normal()
                    .text_size(px(11.5))
                    .text_color(rgb(p.text_muted))
                    .child("Your summary and description are preserved. You can edit and retry, or copy the full redacted Markdown report to paste into GitHub manually."),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .whitespace_normal()
                    .text_size(px(11.0))
                    .text_color(rgb(p.text_muted))
                    .child("Keys: C Copy Report · R Edit & Retry · Enter / Esc Close"),
            )
            .into_any_element(),
    };

    let has_url = !br.issue_url.is_empty();

    backdrop(
        "bug-backdrop",
        cx.listener(|this, _, _, cx| {
            this.app.bug_report.is_open = false;
            cx.notify();
        }),
    )
    .child(
        modal_card(p, 640.0, 440.0)
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_4()
                    .py_3()
                    .bg(rgb(p.bg_surface))
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon("bug", 15.0, p.accent))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_normal()
                            .text_size(px(14.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(p.text_strong))
                            .child("Report a Bug — GitHub Draft (F12)"),
                    ),
            )
            .child(body)
            .child(
                div()
                    .debug_selector(|| "bug-footer".into())
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .bg(rgb(p.bg_surface))
                    .border_t_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap_2()
                    .when(br.stage == BugStage::Form, |d| {
                        d.child(bug_button(
                            "bug-copy",
                            "Copy Report",
                            false,
                            false,
                            p,
                            cx.listener(|this, _, _, cx| copy_bug_report_markdown(this, cx)),
                        ))
                        .child(bug_button(
                            "bug-send",
                            "Review Draft",
                            summary_ready,
                            false,
                            p,
                            cx.listener(|this, _, _, cx| advance_bug_report_to_consent(this, cx)),
                        ))
                    })
                    .when(br.stage == BugStage::Consent, |d| {
                        d.child(bug_button(
                            "bug-back",
                            "Back",
                            false,
                            false,
                            p,
                            cx.listener(|this, _, _, cx| {
                                this.app.bug_report.stage = BugStage::Form;
                                cx.notify();
                            }),
                        ))
                        .child(bug_button(
                            "bug-copy",
                            "Copy Report",
                            false,
                            false,
                            p,
                            cx.listener(|this, _, _, cx| copy_bug_report_markdown(this, cx)),
                        ))
                        .child(bug_button(
                            "bug-confirm",
                            "Open GitHub Draft",
                            true,
                            false,
                            p,
                            cx.listener(|this, _, _, cx| open_bug_report_draft(this, cx)),
                        ))
                    })
                    .when(br.stage == BugStage::Submitting, |d| {
                        d.child(bug_button(
                            "bug-copy",
                            "Copy Report",
                            false,
                            false,
                            p,
                            cx.listener(|this, _, _, cx| copy_bug_report_markdown(this, cx)),
                        ))
                    })
                    .when(br.stage == BugStage::Success, |d| {
                        d.child(bug_button(
                            "bug-back",
                            "Edit Form",
                            false,
                            false,
                            p,
                            cx.listener(|this, _, _, cx| {
                                this.app.bug_report.stage = BugStage::Form;
                                cx.notify();
                            }),
                        ))
                        .child(bug_button(
                            "bug-copy",
                            "Copy Report",
                            br.needs_clipboard,
                            false,
                            p,
                            cx.listener(|this, _, _, cx| copy_bug_report_markdown(this, cx)),
                        ))
                        .when(has_url, |d| {
                            d.child(bug_button(
                                "bug-copy-url",
                                "Copy URL",
                                false,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| copy_bug_report_url(this, cx)),
                            ))
                            .child(bug_button(
                                "bug-reopen",
                                "Open Draft",
                                !br.needs_clipboard,
                                false,
                                p,
                                cx.listener(|this, _, _, cx| reopen_bug_report_draft(this, cx)),
                            ))
                        })
                    })
                    .when(br.stage == BugStage::Failure, |d| {
                        d.child(bug_button(
                            "bug-retry",
                            "Edit & Retry",
                            true,
                            false,
                            p,
                            cx.listener(|this, _, _, cx| {
                                this.app.bug_report.stage = BugStage::Form;
                                cx.notify();
                            }),
                        ))
                        .child(bug_button(
                            "bug-copy",
                            "Copy Report",
                            false,
                            false,
                            p,
                            cx.listener(|this, _, _, cx| copy_bug_report_markdown(this, cx)),
                        ))
                    })
                    .child(bug_button(
                        "bug-close",
                        "Close",
                        false,
                        false,
                        p,
                        cx.listener(|this, _, _, cx| {
                            this.app.bug_report.is_open = false;
                            cx.notify();
                        }),
                    )),
            ),
    )
}

fn render_menu_dropdown(
    view: &GtuimView,
    layout: EffectiveWorkspaceLayout,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let idx = app.menu_bar.open_menu.unwrap_or(0).min(MENUS.len().saturating_sub(1));
    let menu = &MENUS[idx];
    let menu_items = file_menu_items_with_save_as(idx, menu.items);
    let compact_switcher = layout.viewport_w < 430.0;

    let mut items = div()
        .id("menu-dd-list")
        .debug_selector(|| "menu-dd-list".into())
        .min_w(px(if compact_switcher { 180.0 } else { 220.0 }))
        .max_w(px(320.0))
        .max_h(px((layout.viewport_h - layout.header_h - 16.0).clamp(120.0, 360.0)))
        .overflow_y_scroll()
        .rounded(px(8.0))
        .bg(rgb(p.bg_elevated))
        .border_1()
        .border_color(rgb(p.border))
        .shadow_lg()
        .p_1()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

    if compact_switcher {
        let mut switcher = div()
            .w_full()
            .flex_shrink_0()
            .pb_1()
            .mb_1()
            .border_b_1()
            .border_color(rgb(p.border_subtle))
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(2.0));
        for (m_idx, m) in MENUS.iter().enumerate() {
            let active = m_idx == idx;
            switcher = switcher.child(
                div()
                    .id(("compact-menu-tab", m_idx))
                    .px(px(6.0))
                    .py(px(2.0))
                    .rounded(px(4.0))
                    .text_size(px(10.5))
                    .when(active, |d| {
                        d.bg(rgb(p.bg_selected))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(p.text_strong))
                    })
                    .when(!active, |d| d.text_color(rgb(p.text_muted)))
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.app.menu_bar.open_menu = Some(m_idx);
                        this.app.menu_bar.selected_item = 0;
                        cx.notify();
                    }))
                    .child(m.title),
            );
        }
        items = items.child(switcher);
    }

    for (i, &(label, action)) in menu_items.iter().enumerate() {
        let sel = i == app.menu_bar.selected_item;
        let display_label = if action == "toggle_tab_bar" {
            if app.workspace.tab_bar_visible {
                "Hide Tab Bar"
            } else {
                "Show Tab Bar"
            }
        } else {
            label
        };
        let shortcut_hint = match action {
            "save_file" => workspace::format_shortcut_display(&app.settings_widget.config.keybindings.save_file),
            "save_file_as" => {
                let bound = app.settings_widget.config.keybindings.get_by_field("save_file_as");
                if bound.is_empty() {
                    "Ctrl+Shift+S".to_string()
                } else {
                    workspace::format_shortcut_display(bound)
                }
            }
            "new_file" => workspace::format_shortcut_display(&app.settings_widget.config.keybindings.new_file),
            "find_file" => workspace::format_shortcut_display(&app.settings_widget.config.keybindings.find_file),
            "find" if app.mode == crate::tui::app::Mode::Ide => "Ctrl+F".to_string(),
            "replace" if app.mode == crate::tui::app::Mode::Ide => "Ctrl+H".to_string(),
            "find_replace_gui" => "Ctrl+Shift+H".to_string(),
            "quit" => workspace::format_shortcut_display(&app.settings_widget.config.keybindings.quit),
            _ => String::new(),
        };
        items = items.child(
            super::modal_layout::list_card()
                .id(("menu-dd-item", i))
                .debug_selector(move || format!("menu-dd-item-{}", i))
                .px_3()
                .py(px(5.0))
                .rounded(px(5.0))
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .when(sel, |d| d.bg(rgb(p.bg_selected)))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .text_size(px(12.0))
                .text_color(rgb(p.text_strong))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.menu_bar.open_menu = None;
                    this.open_menu_action(action, cx);
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .child(display_label),
                )
                .when(!shortcut_hint.is_empty(), |d| {
                    d.child(
                        div()
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .text_size(px(10.5))
                            .text_color(rgb(p.text_muted))
                            .child(shortcut_hint),
                    )
                }),
        );
    }

    let left_offset = if compact_switcher {
        8.0
    } else {
        let base = if layout.ultra_compact_header {
            34.0
        } else if layout.viewport_w >= 660.0 && !app.git_panel.snapshot.branch.is_empty() {
            165.0
        } else {
            92.0
        };
        let step = if layout.compact_header { 38.0 } else { 46.0 };
        base + (idx as f32) * step
    };

    // Start backdrop below the header bar so clicking another top menu title
    // switches dropdowns in a single click instead of dismissing first.
    div()
        .id("menu-dd-backdrop")
        .absolute()
        .top(px(layout.header_h))
        .left_0()
        .right_0()
        .bottom_0()
        .occlude()
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.app.menu_bar.open_menu = None;
                cx.notify();
            }),
        )
        .child(
            anchored()
                .position(point(px(left_offset), px(layout.header_h + 2.0)))
                .anchor(Corner::TopLeft)
                .snap_to_window_with_margin(px(8.0))
                .child(items),
        )
}

fn render_editor_context_menu(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let pos = view.editor_ctx_pos;

    let mut menu = div()
        .id("ed-ctx-list")
        .debug_selector(|| "ed-ctx-list".into())
        .min_w(px(240.0))
        .max_w(px(340.0))
        .max_h(px(360.0))
        .overflow_y_scroll()
        .rounded(px(8.0))
        .bg(rgb(p.bg_elevated))
        .border_1()
        .border_color(rgb(p.border))
        .shadow_lg()
        .p_1()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

    for (i, &(label, shortcut, action)) in EDITOR_CTX_ITEMS.iter().enumerate() {
        let sel = i == app.editor_context_menu.selected;
        menu = menu.child(
            super::modal_layout::list_card()
                .id(("ed-ctx", i))
                .debug_selector(move || format!("ed-ctx-item-{}", i))
                .px_3()
                .py(px(5.0))
                .rounded(px(5.0))
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .when(sel, |d| d.bg(rgb(p.bg_selected)))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app
                        .editor_context_menu
                        .execute_action(action, &mut this.app.editor_rpc);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(12.0))
                        .text_color(rgb(p.text_strong))
                        .child(label),
                )
                .when(!shortcut.is_empty(), |d| {
                    d.child(
                        div()
                            .debug_selector(move || format!("ed-ctx-shortcut-{}", i))
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .text_size(px(10.5))
                            .text_color(rgb(p.text_muted))
                            .child(shortcut),
                    )
                }),
        );
    }

    div()
        .id("ed-ctx-backdrop")
        .absolute()
        .inset_0()
        .occlude()
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.app.editor_context_menu.is_open = false;
                cx.notify();
            }),
        )
        .child(
            anchored()
                .position(point(pos.x.max(px(8.0)), pos.y.max(px(38.0))))
                .anchor(Corner::TopLeft)
                .snap_to_window_with_margin(px(8.0))
                .child(menu),
        )
}

fn render_explorer_context_menu(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let pos = view.explorer_ctx_pos;
    let items: &[&str] = if app.explorer.context_target.is_some() {
        &["New File", "New Folder", "Rename", "Delete"]
    } else {
        &["New File", "New Folder"]
    };

    let mut menu = div()
        .id("exp-ctx-list")
        .debug_selector(|| "exp-ctx-list".into())
        .min_w(px(170.0))
        .max_w(px(260.0))
        .max_h(px(360.0))
        .overflow_y_scroll()
        .rounded(px(8.0))
        .bg(rgb(p.bg_elevated))
        .border_1()
        .border_color(rgb(p.border))
        .shadow_lg()
        .p_1()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

    for (i, label) in items.iter().copied().enumerate() {
        let sel = i == app.explorer.context_menu_idx;
        menu = menu.child(
            super::modal_layout::list_card()
                .id(("exp-ctx", i))
                .debug_selector(move || format!("exp-ctx-item-{}", i))
                .px_3()
                .py(px(5.0))
                .rounded(px(5.0))
                .whitespace_normal()
                .when(sel, |d| d.bg(rgb(p.bg_selected)))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .text_size(px(12.0))
                .text_color(rgb(if i == 3 { p.error } else { p.text_strong }))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.explorer.context_menu_open = false;
                    this.app.explorer.trigger_context_action(i);
                    cx.notify();
                }))
                .child(label),
        );
    }

    div()
        .id("exp-ctx-backdrop")
        .absolute()
        .inset_0()
        .occlude()
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.app.explorer.context_menu_open = false;
                cx.notify();
            }),
        )
        .child(
            anchored()
                .position(point(pos.x.max(px(8.0)), pos.y.max(px(38.0))))
                .anchor(Corner::TopLeft)
                .snap_to_window_with_margin(px(8.0))
                .child(menu),
        )
}

fn render_notice(view: &GtuimView) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let Some(notice) = &app.notice else {
        return div();
    };
    let accent = match notice.level.as_str() {
        "error" => p.error,
        "warning" => p.warning,
        _ => p.accent,
    };

    div()
        .absolute()
        .right(px(16.0))
        .bottom(px(38.0))
        .max_w(relative(0.88))
        .max_h(relative(0.35))
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .child(
            div()
                .id("notice-toast")
                .debug_selector(|| "notice-toast".into())
                .occlude()
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .max_w(px(520.0))
                .min_w_0()
                .min_h_0()
                .flex_1()
                .overflow_y_scroll()
                .whitespace_normal()
                .px_3()
                .py_2()
                .rounded(px(8.0))
                .bg(rgb(p.bg_elevated))
                .border_1()
                .border_color(rgb(accent))
                .shadow_lg()
                .text_size(px(12.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(p.text_strong))
                .child(notice.message.clone()),
        )
}
