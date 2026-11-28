use std::path::Path;

#[cfg(test)]
#[path = "explorer_tests.rs"]
mod explorer_tests;

use gpui::{prelude::*, *};

use crate::gui::drag_drop::{ExplorerDrag, ExplorerDragGhost, ExplorerDragState};
use crate::gui::icons::{gui_palette, icon, GuiPalette, ICON_LSP, ICON_SETTINGS};
use crate::gui::{EffectiveWorkspaceLayout, GtuimView};
use crate::tui::app::FocusRegion;
use crate::tui::input::InputEvent;
use crate::tui::theme::blend_u32;
use crate::tui::widgets::ai_panel::{
    AiSection, AI_ACTION_ITEMS, AI_CONTEXT_ITEMS, AI_SESSION_ACTIONS,
};
use crate::tui::widgets::explorer::{FileExplorer, InputMode};
use crate::tui::workspace::{self, SidebarAction, SidebarRow};

pub fn render_activity_rail(
    view: &GtuimView,
    layout: EffectiveWorkspaceLayout,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let git_count = app.git_panel.staged.len() + app.git_panel.unstaged.len();
    let compact = layout.compact_rail;
    let btn_w = if layout.rail_w < 40.0 { 26.0 } else if compact { 30.0 } else { 36.0 };
    let btn_h = if compact { 26.0 } else { 36.0 };
    let icon_sz = if compact { 14.5 } else { 17.0 };

    let top_items: &[(&str, &'static str, Option<usize>, &str)] = &[
        ("rail-explorer", "files", Some(0), "explorer"),
        ("rail-git", "git-branch", Some(1), "changes"),
        ("rail-search", "search", Some(4), "search_sidebar"),
        ("rail-ai", "sparkles", Some(2), "ai_assistants"),
    ];

    let mut top = div()
        .id("activity-rail-top")
        .flex_1()
        .min_h_0()
        .w_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(if compact { 2.0 } else { 4.0 }))
        .pt(px(if compact { 4.0 } else { 8.0 }));
    for &(id, ic, tab_opt, cmd) in top_items {
        let active = app.workspace.sidebar_visible
            && !app.workspace.overview
            && tab_opt == Some(app.activity_bar.active_tab);
        let badge = if tab_opt == Some(1) && git_count > 0 {
            Some(git_count)
        } else {
            None
        };

        top = top.child(
            div()
                .id(id)
                .debug_selector(move || id.to_string())
                .w(px(btn_w))
                .h(px(btn_h))
                .flex_shrink_0()
                .rounded(px(7.0))
                .flex()
                .items_center()
                .justify_center()
                .relative()
                .when(active, |d| d.bg(rgb(p.bg_selected)))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    if cmd == "search_sidebar" {
                        if this.app.workspace.sidebar_visible
                            && !this.app.workspace.overview
                            && this.app.activity_bar.active_tab == 4
                        {
                            this.app.workspace.sidebar_visible = false;
                            this.app.workspace.focus = FocusRegion::Editor;
                        } else {
                            this.app.extension_shop.is_open = false;
                            this.app.workspace.sidebar_visible = true;
                            this.app.workspace.overview = false;
                            this.app.activity_bar.active_tab = 4;
                            this.app.search_panel.focus_field = 1;
                            this.app.workspace.focus = FocusRegion::Sidebar;
                        }
                        cx.notify();
                    } else if this.app.workspace.sidebar_visible
                        && !this.app.workspace.overview
                        && tab_opt == Some(this.app.activity_bar.active_tab)
                    {
                        this.app.workspace.sidebar_visible = false;
                        this.app.workspace.focus = FocusRegion::Editor;
                        cx.notify();
                    } else {
                        this.run_command(cmd, cx);
                    }
                }))
                .when(active, |d| {
                    d.child(
                        div()
                            .absolute()
                            .left(px(-4.0))
                            .top(px(6.0))
                            .bottom(px(6.0))
                            .w(px(2.5))
                            .rounded_full()
                            .bg(rgb(p.accent)),
                    )
                })
                .child(icon(ic, icon_sz, if active { p.accent } else { p.text_muted }))
                .when_some(badge, |d, count| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(1.0))
                            .right(px(1.0))
                            .px(px(3.5))
                            .rounded_full()
                            .bg(rgb(p.accent))
                            .text_size(px(8.5))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(p.accent_fg))
                            .child(format!("{}", count.min(99))),
                    )
                }),
        );
    }

    let secondary_bottom_items: &[(&str, &'static str, &str)] = &[
        ("rail-ext", "blocks", "extensions"),
        ("rail-mason", ICON_LSP, "language_tools"),
        ("rail-wt", "git-fork", "orchestrator"),
    ];

    // On tiny vertical windows, move secondary tool icons into the scrollable top stack
    // so Settings stays pinned and reachable at the bottom of the rail without clipping.
    let mut bottom = div()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(if compact { 2.0 } else { 4.0 }))
        .pb(px(if compact { 4.0 } else { 8.0 }));

    let bot_h = if compact { 26.0 } else { 34.0 };
    let bot_icon_sz = if compact { 14.0 } else { 16.5 };

    if compact {
        for &(id, ic, cmd) in secondary_bottom_items {
            top = top.child(
                div()
                    .id(id)
                    .debug_selector(move || id.to_string())
                    .w(px(btn_w))
                    .h(px(bot_h))
                    .flex_shrink_0()
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.run_command(cmd, cx);
                    }))
                    .child(icon(ic, bot_icon_sz, p.text_muted)),
            );
        }
    } else {
        for &(id, ic, cmd) in secondary_bottom_items {
            bottom = bottom.child(
                div()
                    .id(id)
                    .debug_selector(move || id.to_string())
                    .w(px(btn_w))
                    .h(px(bot_h))
                    .flex_shrink_0()
                    .rounded(px(7.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.run_command(cmd, cx);
                    }))
                    .child(icon(ic, bot_icon_sz, p.text_muted)),
            );
        }
    }

    // Settings gear is always pinned at the bottom of the activity rail.
    bottom = bottom.child(
        div()
            .id("rail-settings")
            .debug_selector(|| "rail-settings".into())
            .w(px(btn_w))
            .h(px(bot_h))
            .flex_shrink_0()
            .rounded(px(7.0))
            .flex()
            .items_center()
            .justify_center()
            .hover(|s| s.bg(rgb(p.bg_hover)))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.run_command("settings", cx);
            }))
            .child(icon(ICON_SETTINGS, bot_icon_sz, p.text_muted)),
    );

    div()
        .debug_selector(|| "activity-rail".into())
        .w(px(layout.rail_w))
        .h_full()
        .min_h_0()
        .overflow_hidden()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .justify_between()
        .items_center()
        .bg(rgb(p.bg_rail))
        .border_r_1()
        .border_color(rgb(p.border))
        .child(top)
        .child(bottom)
}

pub fn render_sidebar(
    view: &GtuimView,
    layout: EffectiveWorkspaceLayout,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);

    let body: AnyElement = if app.workspace.overview {
        render_workspace_overview(view, cx).into_any_element()
    } else {
        match app.activity_bar.active_tab {
            0 => render_explorer(view, cx).into_any_element(),
            1 => render_git_panel(view, cx).into_any_element(),
            2 => render_ai_panel(view, cx).into_any_element(),
            4 => render_search_panel(view, cx).into_any_element(),
            _ => render_explorer(view, cx).into_any_element(),
        }
    };

    div()
        .debug_selector(|| "sidebar-pane".into())
        .w(px(layout.sidebar_w))
        .h_full()
        .min_w_0()
        .overflow_hidden()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .bg(rgb(p.bg_panel))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.app.workspace.focus = FocusRegion::Sidebar;
                cx.notify();
            }),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .min_w_0()
                .flex()
                .flex_col()
                .child(body),
        )
}

fn sidebar_header(
    title: &'static str,
    p: GuiPalette,
    actions: Div,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    div()
        .h(px(32.0))
        .w_full()
        .px_3()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_between()
        .border_b_1()
        .border_color(rgb(p.border_subtle))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .id("sb-back-overview")
                        .size(px(20.0))
                        .rounded(px(4.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(|s| s.bg(rgb(p.bg_hover)))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.app.workspace.overview = !this.app.workspace.overview;
                            this.app.workspace.focus = FocusRegion::Sidebar;
                            cx.notify();
                        }))
                        .child(icon("chevron-left", 13.0, p.text_muted)),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(p.text_muted))
                        .child(title),
                ),
        )
        .child(actions)
}

fn sb_icon_btn(
    id: &'static str,
    ic: &'static str,
    p: GuiPalette,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .debug_selector(move || id.to_string())
        .size(px(22.0))
        .rounded(px(4.0))
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.bg(rgb(p.bg_hover)))
        .cursor_pointer()
        .on_click(handler)
        .child(icon(ic, 13.5, p.text_muted))
}

fn render_workspace_overview(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let focused = app.workspace.focus == FocusRegion::Sidebar;
    let (rows, _) = workspace::build_sidebar_rows(app);

    let mut list = div()
        .id("overview-list")
        .flex_1()
        .w_full()
        .min_w_0()
        .min_h_0()
        .overflow_y_scroll()
        .p_2()
        .flex()
        .flex_col()
        .gap(px(2.0));

    for (r_idx, row) in rows.into_iter().enumerate() {
        match row {
            SidebarRow::Spacer => {
                list = list.child(div().h(px(8.0)));
            }
            SidebarRow::Header(hdr) => {
                list = list.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_size(px(10.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(p.text_muted))
                        .child(hdr),
                );
            }
            SidebarRow::Item {
                item_idx,
                label,
                action,
            } => {
                let is_sel = item_idx == app.workspace.sidebar_index;
                list = list.child(
                    div()
                        .id(("ov-item", r_idx))
                        .px_2()
                        .py(px(5.0))
                        .rounded(px(5.0))
                        .text_size(px(12.0))
                        .text_color(rgb(if is_sel && focused {
                            p.text_strong
                        } else {
                            p.text
                        }))
                        .when(is_sel && focused, |d| {
                            d.bg(rgb(p.bg_selected)).font_weight(FontWeight::MEDIUM)
                        })
                        .hover(|s| s.bg(rgb(p.bg_hover)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.app.workspace.sidebar_index = item_idx;
                            match action {
                                SidebarAction::Buffer(bufnr) => {
                                    this.select_buffer(bufnr, cx);
                                }
                                SidebarAction::Tool(tool_id) => {
                                    this.run_command(tool_id, cx);
                                }
                            }
                        }))
                        .child(label),
                );
            }
        }
    }

    div()
        .flex_1()
        .w_full()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .child(sidebar_header("WORKSPACE", p, div(), cx))
        .child(list)
}

fn render_explorer(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let focused = app.workspace.focus == FocusRegion::Sidebar;

    let actions = div()
        .flex()
        .items_center()
        .gap(px(2.0))
        .child(sb_icon_btn(
            "exp-new-file",
            "file-plus",
            p,
            cx.listener(|this, _, _, cx| {
                this.app.workspace.focus = FocusRegion::Sidebar;
                this.app.explorer.begin_creation(InputMode::NewFile, None);
                cx.notify();
            }),
        ))
        .child(sb_icon_btn(
            "exp-new-dir",
            "folder-plus",
            p,
            cx.listener(|this, _, _, cx| {
                this.app.workspace.focus = FocusRegion::Sidebar;
                this.app.explorer.begin_creation(InputMode::NewFolder, None);
                cx.notify();
            }),
        ))
        .child(sb_icon_btn(
            "exp-refresh",
            "refresh",
            p,
            cx.listener(|this, _, _, cx| {
                this.app.explorer.refresh();
                this.app.request_git_refresh();
                cx.notify();
            }),
        ));

    let prompt_bar = (app.explorer.input_mode != InputMode::None).then(|| {
        let label = match app.explorer.input_mode {
            InputMode::NewFile => format!("New file: {}▏", app.explorer.input_buffer),
            InputMode::NewFolder => format!("New folder: {}▏", app.explorer.input_buffer),
            InputMode::Rename => format!("Rename: {}▏", app.explorer.input_buffer),
            InputMode::ConfirmDelete => {
                let target = app
                    .explorer
                    .action_target.as_ref()
                    .map(|e| e.name.as_str())
                    .unwrap_or("item");
                format!("Permanently delete '{}'? [y/N]", target)
            }
            InputMode::None => String::new(),
        };
        div()
            .w_full()
            .px_3()
            .py(px(6.0))
            .bg(rgb(p.bg_input))
            .border_b_1()
            .border_color(rgb(p.accent))
            .text_size(px(11.5))
            .text_color(rgb(if app.explorer.input_mode == InputMode::ConfirmDelete {
                p.warning
            } else {
                p.text_strong
            }))
            .child(label)
            .when_some(app.explorer.input_parent.clone().filter(|_| matches!(app.explorer.input_mode, InputMode::NewFile | InputMode::NewFolder)), |d, parent| {
                d.child(div().text_color(rgb(p.text_muted)).child(format!("In: {parent}")))
            })
            .when_some(app.explorer.input_error.clone(), |d, error| {
                d.child(div().text_color(rgb(p.error)).child(error))
            })
            .when(app.explorer.input_mode == InputMode::ConfirmDelete, |d| {
                d.child(div().flex().gap_2()
                    .child(div().id("exp-delete-cancel").debug_selector(|| "exp-delete-cancel".into()).px_2().cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.app.explorer.handle_key(&InputEvent::Key("<Esc>".into()));
                            cx.notify();
                        })).child("Cancel"))
                    .child(div().id("exp-delete-confirm").debug_selector(|| "exp-delete-confirm".into()).px_2().text_color(rgb(p.error)).cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.app.explorer.confirm_delete();
                            this.after_widget_change(cx);
                        })).child("Delete permanently")))
            })
    });

    let root_dir = app.explorer.root_dir.clone();
    let root_name = Path::new(&root_dir)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("root")
        .to_string();
    let active_drag = view.explorer_drag.as_ref();
    let show_root_drop_bar = active_drag
        .is_some_and(|d| FileExplorer::can_move_path_to_dir(&d.source_path, &root_dir));
    let drop_highlight_bg = blend_u32(p.bg_panel, p.accent, 26);

    let tree_drop_root = root_dir.clone();
    let tree_drag_over_root = root_dir.clone();
    let mut list = div()
        .id("explorer-tree")
        .debug_selector(|| "explorer-tree".into())
        .flex_1()
        .w_full()
        .min_w_0()
        .min_h_0()
        .overflow_y_scroll()
        .p(px(6.0))
        .flex()
        .flex_col()
        .gap(px(1.0))
        .border_1()
        .border_color(rgba(0x00000000))
        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
            this.app.explorer.selection_explicit = false;
            cx.notify();
        }))
        .on_mouse_down(MouseButton::Right, cx.listener(|this, ev: &MouseDownEvent, _, cx| {
            this.app.explorer.selection_explicit = false;
            this.app.workspace.focus = FocusRegion::Sidebar;
            this.app.explorer.context_target = None;
            this.app.explorer.context_menu_open = true;
            this.app.explorer.context_menu_idx = 0;
            this.explorer_ctx_pos = ev.position;
            cx.stop_propagation();
            cx.notify();
        }))
        .drag_over::<ExplorerDrag>(move |style, drag, _, _| {
            if FileExplorer::can_move_path_to_dir(&drag.path, &tree_drag_over_root) {
                style.border_color(rgb(blend_u32(p.border, p.accent, 65)))
            } else {
                style
            }
        })
        .on_drop::<ExplorerDrag>(cx.listener(move |this, drag: &ExplorerDrag, _, cx| {
            this.drop_explorer_item(&drag.path, &tree_drop_root, cx);
        }));

    let view_entity = cx.entity();
    for (idx, entry) in app.explorer.entries.iter().enumerate() {
        let selected = app.explorer.selection_explicit && idx == app.explorer.list_state.selected;
        let is_dir = entry.is_dir;
        let expanded = entry.expanded;
        let path = entry.path.clone();
        let entry_name = entry.name.clone();
        let indent = (entry.depth as f32) * 14.0 + 6.0;
        let is_drag_source = active_drag.is_some_and(|d| d.source_path == path);

        let row_target_dir = if is_dir {
            path.clone()
        } else {
            Path::new(&path)
                .parent()
                .and_then(|parent| parent.to_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| root_dir.clone())
        };
        let drag_over_target_dir = row_target_dir.clone();
        let drop_target_dir = row_target_dir;
        let drag_payload = ExplorerDrag {
            path: path.clone(),
            name: entry_name.clone(),
            is_dir,
        };

        let rel_path = path
            .strip_prefix(&app.explorer.root_dir)
            .map(|s| s.trim_start_matches('/'))
            .unwrap_or(&entry.name);
        let badge = app.explorer.git_badges.get(rel_path).copied();

        let row_fg = if let Some(b) = badge {
            match b {
                'M' => p.warning,
                'A' | '?' => p.success,
                'D' => p.error,
                _ => p.text,
            }
        } else if is_dir {
            p.text_strong
        } else {
            p.text
        };

        let item_icon = if is_dir {
            if expanded {
                "folder-open"
            } else {
                "folder"
            }
        } else {
            "file-code"
        };

        let drag_view = view_entity.clone();
        list = list.child(
            div()
                .id(("exp-row", idx))
                .debug_selector(move || format!("exp-row-{idx}"))
                .w_full()
                .pl(px(indent))
                .pr_2()
                .py(px(3.0))
                .rounded(px(5.0))
                .border_1()
                .border_color(rgba(0x00000000))
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .when(selected, |d| {
                    d.bg(rgb(if focused { p.bg_selected } else { p.bg_hover }))
                })
                .when(is_drag_source, |d| d.opacity(0.45))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, cx.listener(move |this, _, _, cx| {
                    this.app.explorer.list_state.selected = idx;
                    this.app.explorer.selection_explicit = true;
                    cx.stop_propagation();
                    cx.notify();
                }))
                .on_drag(
                    drag_payload,
                    move |drag: &ExplorerDrag, _, window, cx| {
                        let palette = drag_view.update(cx, |this, cx| {
                            this.app.workspace.focus = FocusRegion::Sidebar;
                            this.app.explorer.context_menu_open = false;
                            this.explorer_drag = Some(ExplorerDragState {
                                source_path: drag.path.clone(),
                                source_name: drag.name.clone(),
                                is_dir: drag.is_dir,
                            });
                            cx.notify();
                            gui_palette(&this.app.theme)
                        });
                        cx.set_active_drag_cursor_style(CursorStyle::ClosedHand, window);
                        cx.new(|_| ExplorerDragGhost {
                            name: drag.name.clone(),
                            is_dir: drag.is_dir,
                            palette,
                        })
                    },
                )
                .drag_over::<ExplorerDrag>(move |style, drag, _, _| {
                    if FileExplorer::can_move_path_to_dir(&drag.path, &drag_over_target_dir) {
                        style
                            .bg(rgb(drop_highlight_bg))
                            .border_color(rgb(p.accent))
                    } else {
                        style
                    }
                })
                .on_drop::<ExplorerDrag>(cx.listener(move |this, drag: &ExplorerDrag, _, cx| {
                    this.drop_explorer_item(&drag.path, &drop_target_dir, cx);
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.workspace.focus = FocusRegion::Sidebar;
                    this.app.explorer.list_state.selected = idx;
                    this.app.explorer.selection_explicit = true;
                    if is_dir {
                        if this.app.explorer.expanded.contains(&path) {
                            this.app.explorer.expanded.remove(&path);
                        } else {
                            this.app.explorer.expanded.insert(path.clone());
                        }
                        this.app.explorer.refresh();
                    } else {
                        this.app.explorer.open_file_requested = Some(path.clone());
                        this.after_widget_change(cx);
                    }
                    cx.notify();
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        this.app.workspace.focus = FocusRegion::Sidebar;
                        this.app.explorer.list_state.selected = idx;
                        this.app.explorer.selection_explicit = true;
                        this.app.explorer.context_target = this.app.explorer.entries.get(idx).map(|e| e.path.clone());
                        this.app.explorer.context_menu_open = true;
                        this.app.explorer.context_menu_idx = 0;
                        this.explorer_ctx_pos = ev.position;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.0))
                        .min_w_0()
                        .flex_1()
                        .overflow_hidden()
                        .when(is_dir, |d| {
                            d.child(
                                icon(
                                    if expanded {
                                        "chevron-down"
                                    } else {
                                        "chevron-right"
                                    },
                                    11.0,
                                    p.text_muted,
                                )
                                .flex_shrink_0(),
                            )
                        })
                        .when(!is_dir, |d| d.child(div().w(px(11.0)).flex_shrink_0()))
                        .child(
                            icon(
                                item_icon,
                                13.0,
                                if is_dir { p.accent } else { p.text_muted },
                            )
                            .flex_shrink_0(),
                        )
                        .child(
                            div()
                                .debug_selector(move || format!("exp-name-{idx}"))
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_size(px(12.0))
                                .text_color(rgb(row_fg))
                                .when(is_dir, |d| d.font_weight(FontWeight::MEDIUM))
                                .text_ellipsis()
                                .child(entry.name.clone()),
                        ),
                )
                .when_some(badge, |d, b| {
                    d.child(
                        div()
                            .debug_selector(move || format!("exp-badge-{idx}"))
                            .flex_shrink_0()
                            .w(px(14.0))
                            .text_right()
                            .text_size(px(10.5))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(row_fg))
                            .child(b.to_string()),
                    )
                }),
        );
    }

    let hdr_drop_root = root_dir.clone();
    let hdr_drag_over_root = root_dir;
    let header_wrap: AnyElement = if show_root_drop_bar {
        div()
            .id("exp-drop-root")
            .debug_selector(|| "exp-drop-root".into())
            .h(px(32.0))
            .w_full()
            .px_3()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(6.0))
            .bg(rgb(blend_u32(p.bg_panel, p.accent, 14)))
            .border_b_1()
            .border_color(rgb(p.accent))
            .drag_over::<ExplorerDrag>(move |style, drag, _, _| {
                if FileExplorer::can_move_path_to_dir(&drag.path, &hdr_drag_over_root) {
                    style.bg(rgb(drop_highlight_bg))
                } else {
                    style
                }
            })
            .on_drop::<ExplorerDrag>(cx.listener(move |this, drag: &ExplorerDrag, _, cx| {
                this.drop_explorer_item(&drag.path, &hdr_drop_root, cx);
            }))
            .child(icon("folder-open", 13.0, p.accent))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(p.text_strong))
                    .child(format!("Drop here → {}/", root_name)),
            )
            .into_any_element()
    } else {
        div()
            .id("exp-header-drop")
            .w_full()
            .flex_shrink_0()
            .drag_over::<ExplorerDrag>(move |style, drag, _, _| {
                if FileExplorer::can_move_path_to_dir(&drag.path, &hdr_drag_over_root) {
                    style.bg(rgb(drop_highlight_bg))
                } else {
                    style
                }
            })
            .on_drop::<ExplorerDrag>(cx.listener(move |this, drag: &ExplorerDrag, _, cx| {
                this.drop_explorer_item(&drag.path, &hdr_drop_root, cx);
            }))
            .child(sidebar_header("EXPLORER", p, actions, cx))
            .into_any_element()
    };

    div()
        .flex_1()
        .w_full()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .child(header_wrap)
        .children(prompt_bar)
        .child(list)
}

fn render_git_panel(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let focused = app.workspace.focus == FocusRegion::Sidebar;
    let gp = &app.git_panel;

    if !gp.snapshot.is_repository {
        return div().flex().flex_col().gap_3().p_3()
            .child(div().text_color(rgb(p.text_muted)).child("This project is not a Git repository."))
            .child(div().id("git-init").debug_selector(|| "git-init".into()).px_3().py_2().rounded(px(6.0))
                .bg(rgb(p.accent)).text_color(rgb(p.accent_fg)).cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.app.git_panel.initialize_repository();
                    this.after_widget_change(cx);
                    cx.notify();
                })).child("Initialize Repository"))
            .child(gp.status_feedback.clone()).into_any_element();
    }

    let actions = div()
        .flex()
        .items_center()
        .gap(px(2.0))
        .child(sb_icon_btn(
            "git-btn-stage-all",
            "plus",
            p,
            cx.listener(|this, _, _, cx| {
                this.app.git_panel.stage_all();
                this.after_widget_change(cx);
                cx.notify();
            }),
        ))
        .child(sb_icon_btn(
            "git-btn-unstage-all",
            "minus",
            p,
            cx.listener(|this, _, _, cx| {
                this.app.git_panel.unstage_all();
                this.after_widget_change(cx);
                cx.notify();
            }),
        ))
        .child(sb_icon_btn(
            "git-btn-pull",
            "download",
            p,
            cx.listener(|this, _, _, cx| {
                this.app.git_panel.handle_key(&InputEvent::Key("p".into()));
                this.after_widget_change(cx);
                cx.notify();
            }),
        ))
        .child(sb_icon_btn(
            "git-btn-push",
            "upload",
            p,
            cx.listener(|this, _, _, cx| {
                this.app.git_panel.handle_key(&InputEvent::Key("P".into()));
                this.after_widget_change(cx);
                cx.notify();
            }),
        ))
        .child(sb_icon_btn(
            "git-more",
            "history",
            p,
            cx.listener(|this, _, _, cx| {
                this.app.git_panel.open_detailed_requested = true;
                this.after_widget_change(cx);
                cx.notify();
            }),
        ));

    let commit_box = div()
        .p_3()
        .flex()
        .flex_col()
        .gap_2()
        .border_b_1()
        .border_color(rgb(p.border_subtle))
        .child(
            div()
                .id("git-commit-input")
                .w_full()
                .px(px(10.0))
                .py(px(7.0))
                .rounded(px(6.0))
                .bg(rgb(p.bg_input))
                .border_1()
                .border_color(rgb(if gp.is_typing_commit {
                    p.accent
                } else {
                    p.border
                }))
                .text_size(px(12.0))
                .text_color(rgb(if gp.commit_msg.is_empty() && !gp.is_typing_commit {
                    p.text_muted
                } else {
                    p.text_strong
                }))
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.app.workspace.focus = FocusRegion::Sidebar;
                    this.app.git_panel.is_typing_commit = true;
                    cx.notify();
                }))
                .child(if gp.is_typing_commit {
                    format!("{}▏", gp.commit_msg)
                } else if gp.commit_msg.is_empty() {
                    "Commit message (press c)".to_string()
                } else {
                    gp.commit_msg.clone()
                }),
        )
        .child(
            div()
                .id("git-btn-commit")
                .w_full()
                .py(px(6.0))
                .rounded(px(6.0))
                .bg(rgb(p.accent))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(p.accent_fg))
                .hover(|s| s.opacity(0.9))
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.app.git_panel.commit();
                    this.after_widget_change(cx);
                    cx.notify();
                }))
                .child(icon("check", 13.0, p.accent_fg))
                .child("Commit"),
        )
        .when(!gp.status_feedback.is_empty(), |d| {
            d.child(
                div()
                    .text_size(px(11.0))
                    .text_color(rgb(p.text_muted))
                    .child(gp.status_feedback.clone()),
            )
        });

    let mut body = div()
        .id("git-changes-list")
        .flex_1()
        .w_full()
        .min_w_0()
        .min_h_0()
        .overflow_y_scroll()
        .p_2()
        .flex()
        .flex_col()
        .gap_1();

    if !gp.staged.is_empty() {
        body = body.child(
            div()
                .px_1()
                .py(px(2.0))
                .text_size(px(10.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(p.text_muted))
                .child(format!("STAGED CHANGES ({})", gp.staged.len())),
        );
        for (idx, f) in gp.staged.iter().enumerate() {
            let selected = idx == gp.list_state.selected;
            let path_unstage = f.path.clone();
            body = body.child(
                div()
                    .id(("git-staged", idx))
                    .debug_selector(move || format!("git-staged-{idx}"))
                    .w_full()
                    .px_2()
                    .py(px(4.0))
                    .rounded(px(5.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .when(selected, |d| {
                        d.bg(rgb(if focused { p.bg_selected } else { p.bg_hover }))
                    })
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.app.git_panel.list_state.selected = idx;
                        this.app.git_panel.preview_selected_diff();
                        this.after_widget_change(cx);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(12.0))
                            .text_color(rgb(p.success))
                            .text_ellipsis()
                            .child(f.path.clone()),
                    )
                    .child(
                        div()
                            .id(("git-unstage", idx))
                            .size(px(20.0))
                            .rounded(px(4.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(rgb(p.bg_elevated)))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.app.git_panel.unstage_file(&path_unstage);
                                this.after_widget_change(cx);
                                cx.notify();
                            }))
                            .child(icon("minus", 12.0, p.text_muted)),
                    ),
            );
        }
    }

    body = body.child(
        div()
            .px_1()
            .pt_1()
            .text_size(px(10.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(p.text_muted))
            .child(format!("CHANGES ({})", gp.unstaged.len())),
    );
    if gp.unstaged.is_empty() && gp.staged.is_empty() {
        body = body.child(
            div()
                .px_2()
                .py_3()
                .text_size(px(12.0))
                .text_color(rgb(p.text_muted))
                .child("Working tree clean"),
        );
    } else {
        for (u_idx, f) in gp.unstaged.iter().enumerate() {
            let global_idx = gp.staged.len() + u_idx;
            let selected = global_idx == gp.list_state.selected;
            let path_stage = f.path.clone();
            body = body.child(
                div()
                    .id(("git-unstaged", u_idx))
                    .debug_selector(move || format!("git-unstaged-{u_idx}"))
                    .w_full()
                    .px_2()
                    .py(px(4.0))
                    .rounded(px(5.0))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .when(selected, |d| {
                        d.bg(rgb(if focused { p.bg_selected } else { p.bg_hover }))
                    })
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.app.git_panel.list_state.selected = global_idx;
                        this.app.git_panel.preview_selected_diff();
                        this.after_widget_change(cx);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(12.0))
                            .text_color(rgb(p.warning))
                            .text_ellipsis()
                            .child(f.path.clone()),
                    )
                    .child(
                        div()
                            .id(("git-stage", u_idx))
                            .size(px(20.0))
                            .rounded(px(4.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(rgb(p.bg_elevated)))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.app.git_panel.stage_file(&path_stage);
                                this.after_widget_change(cx);
                                cx.notify();
                            }))
                            .child(icon("plus", 12.0, p.text_muted)),
                    ),
            );
        }
    }

    if !gp.snapshot.commits.is_empty() {
        body = body.child(
            div()
                .px_1()
                .pt_3()
                .text_size(px(10.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(p.text_muted))
                .child("RECENT COMMITS"),
        );
        for commit in gp.snapshot.commits.iter().take(5) {
            body = body.child(
                div()
                    .px_2()
                    .py(px(2.5))
                    .text_size(px(11.0))
                    .text_color(rgb(p.text_muted))
                    .text_ellipsis()
                    .child(format!("{}  {}", commit.hash, commit.message)),
            );
        }
    }

    div()
        .flex_1()
        .w_full()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .child(sidebar_header("SOURCE CONTROL", p, actions, cx))
        .child(div().id("git-switch-branch").debug_selector(|| "git-switch-branch".into()).px_3().py_2().cursor_pointer()
            .text_color(rgb(p.accent))
            .on_click(cx.listener(|this, _, _, cx| {
                this.app.git_panel.open_detailed_requested = true;
                this.after_widget_change(cx);
                cx.notify();
            }))
            .child(format!("⑂ {} · Switch branch", if gp.snapshot.branch.is_empty() { "Detached HEAD" } else { &gp.snapshot.branch })))
        .child(commit_box)
        .child(body)
        .into_any_element()
}

fn render_search_panel(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let focused = app.workspace.focus == FocusRegion::Sidebar;
    let sp = &app.search_panel;

    let mut hits = div()
        .id("search-hits")
        .flex_1()
        .w_full()
        .min_w_0()
        .min_h_0()
        .overflow_y_scroll()
        .p_2()
        .flex()
        .flex_col()
        .gap(px(2.0));

    if sp.hits.is_empty() {
        hits = hits.child(
            div()
                .p_2()
                .text_size(px(11.5))
                .text_color(rgb(p.text_muted))
                .child(if sp.query.is_empty() {
                    "Type to search project with ripgrep..."
                } else {
                    "No matches found"
                }),
        );
    } else {
        for (idx, hit) in sp.hits.iter().enumerate() {
            let sel = idx == sp.list_state.selected;
            let path = hit.path.clone();
            let line = hit.line;
            let col = hit.col;
            hits = hits.child(
                div()
                    .id(("search-hit", idx))
                    .px_2()
                    .py(px(5.0))
                    .rounded(px(5.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .when(sel && focused, |d| d.bg(rgb(p.bg_selected)))
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.app.search_panel.list_state.selected = idx;
                        this.app.search_panel.open_hit_requested = Some((path.clone(), line, col));
                        this.after_widget_change(cx);
                        cx.notify();
                    }))
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(p.accent))
                            .text_ellipsis()
                            .child(format!("{}:{}", hit.path, hit.line)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(11.5))
                            .text_color(rgb(p.text))
                            .text_ellipsis()
                            .child(hit.snippet.clone()),
                    ),
            );
        }
    }

    let search_actions = div().flex().items_center().gap(px(2.0)).child(
        div()
            .id("search-open-telescope")
            .debug_selector(|| "search-open-telescope".into())
            .px(px(6.0))
            .py(px(2.0))
            .rounded(px(4.0))
            .bg(rgb(p.bg_elevated))
            .text_size(px(10.0))
            .text_color(rgb(p.text_muted))
            .hover(|s| s.bg(rgb(p.bg_hover)).text_color(rgb(p.text_strong)))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.run_command("search_project", cx);
            }))
            .child("Live Grep"),
    );

    div()
        .flex_1()
        .w_full()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .child(sidebar_header("SEARCH", p, search_actions, cx))
        .child(
            div()
                .p_3()
                .border_b_1()
                .border_color(rgb(p.border_subtle))
                .child(
                    div()
                        .id("search-query-input")
                        .w_full()
                        .px(px(10.0))
                        .py(px(7.0))
                        .rounded(px(6.0))
                        .bg(rgb(p.bg_input))
                        .border_1()
                        .border_color(rgb(if focused && sp.focus_field == 1 {
                            p.accent
                        } else {
                            p.border
                        }))
                        .flex()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.app.workspace.focus = FocusRegion::Sidebar;
                            this.app.search_panel.focus_field = 1;
                            cx.notify();
                        }))
                        .child(icon("search", 13.0, p.text_muted))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(rgb(if sp.query.is_empty() && sp.focus_field != 1 {
                                    p.text_muted
                                } else {
                                    p.text_strong
                                }))
                                .child(if focused && sp.focus_field == 1 {
                                    format!("{}▏", sp.query)
                                } else if sp.query.is_empty() {
                                    "Search project...".to_string()
                                } else {
                                    sp.query.clone()
                                }),
                        ),
                ),
        )
        .child(hits)
}

fn render_ai_panel(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let app = &view.app;
    let p = gui_palette(&app.theme);
    let focused = app.workspace.focus == FocusRegion::Sidebar;
    let ai = &app.ai_panel;

    let tabs = [
        (AiSection::Agents, "Agents"),
        (AiSection::Context, "Context"),
        (AiSection::Actions, "Actions"),
    ];

    let mut tab_bar = div()
        .w_full()
        .p_2()
        .flex()
        .items_center()
        .gap_1()
        .border_b_1()
        .border_color(rgb(p.border_subtle));
    for (idx, (section, label)) in tabs.into_iter().enumerate() {
        let active = ai.section == section;
        tab_bar = tab_bar.child(
            div()
                .id(("ai-tab", idx))
                .flex_1()
                .py(px(5.0))
                .rounded(px(5.0))
                .text_center()
                .text_size(px(11.0))
                .font_weight(if active {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .bg(rgb(if active { p.bg_selected } else { p.bg_surface }))
                .text_color(rgb(if active { p.text_strong } else { p.text_muted }))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.ai_panel.set_section(section);
                    cx.notify();
                }))
                .child(label),
        );
    }

    let mut list = div()
        .id("ai-list")
        .flex_1()
        .w_full()
        .min_w_0()
        .min_h_0()
        .overflow_y_scroll()
        .p_2()
        .flex()
        .flex_col()
        .gap_1();

    match ai.section {
        AiSection::Agents => {
            for (idx, agent) in ai.agents.iter().enumerate() {
                let sel = idx == ai.list_state.selected;
                let installed = agent.installed;
                list = list.child(
                    div()
                        .id(("ai-agent", idx))
                        .px_2()
                        .py(px(6.0))
                        .rounded(px(5.0))
                        .flex()
                        .items_center()
                        .justify_between()
                        .when(sel && focused, |d| d.bg(rgb(p.bg_selected)))
                        .hover(|s| s.bg(rgb(p.bg_hover)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.app.ai_panel.list_state.selected = idx;
                            this.app.ai_panel.execute_selected(&mut this.app.editor_rpc);
                            this.after_widget_change(cx);
                            cx.notify();
                        }))
                        .child(
                            div()
                                .text_size(px(12.0))
                                .text_color(rgb(p.text_strong))
                                .child(agent.name),
                        )
                        .child(
                            div()
                                .px(px(6.0))
                                .py(px(1.5))
                                .rounded_full()
                                .bg(rgb(p.bg_elevated))
                                .text_size(px(10.0))
                                .text_color(rgb(if installed { p.success } else { p.text_muted }))
                                .child(if installed { "ready" } else { "missing" }),
                        ),
                );
            }
        }
        AiSection::Context => {
            for (idx, (label, _)) in AI_CONTEXT_ITEMS.iter().enumerate() {
                let sel = idx == ai.list_state.selected;
                list = list.child(
                    div()
                        .id(("ai-ctx", idx))
                        .px_2()
                        .py(px(6.0))
                        .rounded(px(5.0))
                        .text_size(px(12.0))
                        .text_color(rgb(p.text))
                        .when(sel && focused, |d| {
                            d.bg(rgb(p.bg_selected)).text_color(rgb(p.text_strong))
                        })
                        .hover(|s| s.bg(rgb(p.bg_hover)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.app.ai_panel.list_state.selected = idx;
                            this.app.ai_panel.execute_selected(&mut this.app.editor_rpc);
                            this.after_widget_change(cx);
                            cx.notify();
                        }))
                        .child(*label),
                );
            }
        }
        AiSection::Actions => {
            for (idx, (label, _)) in AI_ACTION_ITEMS.iter().enumerate() {
                let sel = idx == ai.list_state.selected;
                list = list.child(
                    div()
                        .id(("ai-act", idx))
                        .px_2()
                        .py(px(6.0))
                        .rounded(px(5.0))
                        .text_size(px(12.0))
                        .text_color(rgb(p.text))
                        .when(sel && focused, |d| {
                            d.bg(rgb(p.bg_selected)).text_color(rgb(p.text_strong))
                        })
                        .hover(|s| s.bg(rgb(p.bg_hover)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.app.ai_panel.list_state.selected = idx;
                            this.app.ai_panel.execute_selected(&mut this.app.editor_rpc);
                            this.after_widget_change(cx);
                            cx.notify();
                        }))
                        .child(*label),
                );
            }
        }
    }

    let mut session_btns = div().flex().flex_wrap().gap_1();
    for (idx, (label, lua)) in AI_SESSION_ACTIONS.iter().enumerate() {
        let lua_str = *lua;
        session_btns = session_btns.child(
            div()
                .id(("ai-sess", idx))
                .px_2()
                .py(px(3.0))
                .rounded(px(4.0))
                .bg(rgb(p.bg_elevated))
                .text_size(px(10.5))
                .text_color(rgb(p.text))
                .hover(|s| s.bg(rgb(p.bg_hover)).text_color(rgb(p.text_strong)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    let _ = this.app.editor_rpc.notify(
                        "nvim_exec_lua",
                        vec![rmpv::Value::from(lua_str), rmpv::Value::Array(vec![])],
                    );
                    if idx == 0 {
                        this.app.workspace.focus = FocusRegion::Editor;
                    }
                    cx.notify();
                }))
                .child(*label),
        );
    }

    let session_footer = div()
        .p_3()
        .border_t_1()
        .border_color(rgb(p.border_subtle))
        .bg(rgb(p.bg_surface))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_size(px(11.0))
                .text_color(rgb(p.text_muted))
                .child(format!(
                    "Session: {} ({})",
                    if ai.active_agent.is_empty() {
                        "none"
                    } else {
                        &ai.active_agent
                    },
                    ai.status
                )),
        )
        .child(session_btns);

    div()
        .flex_1()
        .w_full()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .child(sidebar_header("AI ASSISTANTS", p, div(), cx))
        .child(tab_bar)
        .child(list)
        .child(session_footer)
}

#[cfg(test)]
#[path = "git_tests.rs"]
mod git_tests;
