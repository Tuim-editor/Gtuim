//! Native project welcome surface; Neovim remains the authority on its visibility.
use super::{
    icons::{gui_palette, icon, tuim_logo},
    GtuimView,
};
use crate::tui::{
    app::{FocusRegion, PaletteMode},
    events, workspace,
};
use gpui::{prelude::*, *};
use std::path::Path;

pub(super) fn render_welcome(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let p = gui_palette(&view.app.theme);
    let state = view.app.welcome.as_ref().expect("visible welcome state");
    let project = Path::new(&state.project_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&state.project_path)
        .to_owned();
    let mut content = div()
        .debug_selector(|| "welcome-content".into())
        .w_full()
        .max_w(px(520.0))
        .min_w_0()
        .flex_shrink_0()
        .my_auto()
        .p_5()
        .flex()
        .flex_col()
        .gap_5()
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .gap_3()
                .child(tuim_logo(40.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(23.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_strong))
                                .child("Gtuim"),
                        )
                        .child(
                            div()
                                .w_full()
                                .truncate()
                                .text_size(px(12.0))
                                .text_color(rgb(p.text_muted))
                                .child(format!("Your workspace · {project}")),
                        ),
                ),
        )
        .child(
            div()
                .w_full()
                .truncate()
                .text_size(px(11.0))
                .text_color(rgb(p.text_muted))
                .child(state.project_path.clone()),
        );
    let mut actions = div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap_2();
    for (id, title, glyph) in [
        ("new_file", "New File", "file-plus"),
        ("find_file", "Open File", "folder-open"),
        ("search_project", "Search Project", "search"),
    ] {
        let command = workspace::COMMANDS.iter().find(|c| c.id == id).unwrap();
        let key = workspace::format_shortcut_display(
            view.app
                .settings_widget
                .config
                .keybindings
                .get_by_field(command.key_field),
        );
        actions = actions.child(
            div()
                .id(SharedString::from(format!("welcome-{id}")))
                .debug_selector(move || format!("welcome-action-{id}"))
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .px_3()
                .py_3()
                .rounded(px(7.0))
                .border_1()
                .border_color(rgb(p.border_subtle))
                .bg(rgb(p.bg_surface))
                .hover(|s| s.bg(rgb(p.bg_hover)).border_color(rgb(p.border)))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_3()
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.root_focus);
                    this.app.workspace.focus = FocusRegion::Editor;
                    events::execute_command(&mut this.app, id);
                    cx.notify();
                }))
                .child(icon(glyph, 17.0, p.accent))
                .child(
                    div()
                        .debug_selector(move || format!("welcome-label-{id}"))
                        .flex_1()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(13.0))
                        .text_color(rgb(p.text_strong))
                        .child(title),
                )
                .when(!key.is_empty(), |d| {
                    d.child(
                        div()
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .px_2()
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(rgb(p.bg_elevated))
                            .text_size(px(11.0))
                            .text_color(rgb(p.text_muted))
                            .child(key),
                    )
                }),
        );
    }
    content = content.child(actions);
    let mut recent = div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(p.text_muted))
                .child("RECENT IN THIS PROJECT"),
        );
    if state.recent_files.is_empty() {
        recent = recent.child(
            div()
                .debug_selector(|| "welcome-empty".into())
                .w_full()
                .min_w_0()
                .whitespace_normal()
                .py_2()
                .text_size(px(12.0))
                .text_color(rgb(p.text_muted))
                .child("Files you open in this project will appear here."),
        );
    }
    for (index, path) in state.recent_files.iter().enumerate() {
        let relative = Path::new(path)
            .strip_prefix(&state.project_path)
            .unwrap_or(Path::new(path))
            .to_string_lossy()
            .into_owned();
        let path = path.clone();
        recent = recent.child(
            div()
                .id(("welcome-recent", index))
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .px_2()
                .py_2()
                .rounded(px(5.0))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_2()
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.root_focus);
                    this.app.explorer.open_file_requested = Some(path.clone());
                    events::process_widget_side_effects(&mut this.app);
                    cx.notify();
                }))
                .child(icon("file-code", 14.0, p.text_muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.0))
                        .text_color(rgb(p.text))
                        .child(relative),
                ),
        );
    }
    let mut links = div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .border_t_1()
        .border_color(rgb(p.border_subtle))
        .pt_3()
        .flex()
        .flex_wrap()
        .gap_2();
    for (id, label) in [
        ("commands", "Command Palette"),
        ("help", "Help & Quickstart"),
        ("settings", "Settings"),
    ] {
        links = links.child(
            div()
                .id(SharedString::from(format!("welcome-link-{id}")))
                .px_2()
                .py_1()
                .rounded(px(4.0))
                .text_size(px(11.0))
                .text_color(rgb(p.text_muted))
                .hover(|s| s.bg(rgb(p.bg_hover)).text_color(rgb(p.text_strong)))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.root_focus);
                    if id == "commands" {
                        events::open_palette(&mut this.app, PaletteMode::Commands);
                    } else {
                        events::execute_command(&mut this.app, id);
                    }
                    cx.notify();
                }))
                .child(label),
        );
    }
    div()
        .id("native-welcome")
        .debug_selector(|| "native-welcome".into())
        .absolute()
        .inset_0()
        .min_w_0()
        .min_h_0()
        .bg(rgb(p.bg_editor))
        .cursor_default()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .items_center()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .child(content.child(recent).child(links))
}

#[cfg(test)]
mod tests {
    #[gpui::test]
    fn welcome_actions_remain_readable_in_small_workspaces(cx: &mut gpui::TestAppContext) {
        use gpui::{px, size};
        for (width, height) in [(1440.0, 900.0), (640.0, 480.0), (300.0, 220.0)] {
            let (_, window) = cx.add_window_view(|_, cx| {
                let mut view = crate::gui::modal_tests::fixture(cx);
                view.app.extension_shop.is_open = false;
                view.app.welcome = Some(crate::tui::app::WelcomeState {
                    project_path: "/tmp/project".into(),
                    recent_files: vec![],
                });
                view
            });
            window.simulate_resize(size(px(width), px(height)));
            window.run_until_parked();
            let surface = window
                .debug_bounds("native-welcome")
                .expect("native welcome");
            let content = window
                .debug_bounds("welcome-content")
                .expect("welcome content");
            let label = window
                .debug_bounds("welcome-label-search_project")
                .expect("readable action");
            assert!(content.size.width <= surface.size.width);
            assert!(label.size.width >= px(50.0), "label collapsed: {label:?}");
            assert!(label.size.height < px(60.0), "label is a vertical ribbon");
            assert!(window.debug_bounds("welcome-empty").is_some());
        }
    }
}
