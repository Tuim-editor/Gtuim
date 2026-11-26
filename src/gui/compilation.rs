//! Persistent compilation drawer: actionable locations above complete bounded output.
use super::{
    icons::{gui_palette, icon},
    GtuimView,
};
use crate::tui::{
    app::{FocusRegion, Mode},
    events,
    widgets::{compile::CompileStage, diagnostics},
};
use gpui::{prelude::*, *};

pub(super) fn render_compilation(
    view: &GtuimView,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let p = gui_palette(&view.app.theme);
    let compile = &view.app.compile_widget;
    let errors = diagnostics::collect(&compile.lines);
    let stage = match compile.stage {
        CompileStage::Ready => "Ready".to_string(),
        CompileStage::Running => "Compiling…".to_string(),
        CompileStage::Succeeded => "Succeeded".to_string(),
        CompileStage::Failed(code) => format!("Failed ({code})"),
        CompileStage::Stopped => "Stopped".to_string(),
        CompileStage::StartError => "Could not start".to_string(),
    };
    let status_color = match compile.stage {
        CompileStage::Failed(_) | CompileStage::StartError => p.error,
        CompileStage::Succeeded => p.success,
        _ => p.text_muted,
    };
    let mut list = div()
        .id("compilation-list")
        .debug_selector(|| "compilation-list".into())
        .track_scroll(&view.compilation_scroll)
        .flex_1()
        .min_h_0()
        .w_full()
        .min_w_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .p_2()
        .gap_1()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                window.focus(&this.root_focus);
                this.app.workspace.focus = FocusRegion::Drawer;
                cx.notify();
            }),
        );
    for (index, diagnostic) in errors.iter().enumerate() {
        let selected = index == compile.selected_diagnostic;
        list = list.child(
            div()
                .id(("compilation-diagnostic", index))
                .debug_selector(move || format!("compilation-diagnostic-{index}"))
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .px_3()
                .py_2()
                .rounded(px(5.0))
                .bg(rgb(if selected {
                    p.bg_selected
                } else {
                    p.bg_surface
                }))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .flex()
                .items_start()
                .gap_2()
                .on_click(cx.listener(move |this, _, window, cx| {
                    window.focus(&this.root_focus);
                    events::visit_compilation_diagnostic(&mut this.app, index);
                    let _ = this.app.editor_rpc.flush_writes();
                    cx.notify();
                }))
                .child(icon(
                    "file-code",
                    14.0,
                    if selected { p.accent } else { p.text_muted },
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .truncate()
                                .text_size(px(11.0))
                                .text_color(rgb(p.accent))
                                .child(format!(
                                    "{}:{}:{}",
                                    diagnostic.path, diagnostic.line, diagnostic.column
                                )),
                        )
                        .child(
                            div()
                                .debug_selector(move || format!("compilation-message-{index}"))
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(12.0))
                                .text_color(rgb(p.text))
                                .child(diagnostic.message.clone()),
                        ),
                ),
        );
    }
    if compile.lines.is_empty() {
        list = list.child(div().w_full().min_w_0().whitespace_normal().p_3().text_size(px(12.0)).text_color(rgb(p.text_muted))
            .child("Run Tools → Compile to build your project. Compiler locations will appear here; click one or press Enter to visit its source."));
    } else {
        list = list.child(
            div()
                .w_full()
                .flex_shrink_0()
                .pt_2()
                .text_size(px(10.0))
                .text_color(rgb(p.text_muted))
                .child("BUILD OUTPUT"),
        );
        for (index, line) in compile.lines.iter().enumerate() {
            list = list.child(
                div()
                    .id(("compilation-output", index))
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .child(super::modals::render_compile_line(line, p, p.bg_panel)),
            );
        }
    }
    if let Some(line) = compile.active_partial_line() {
        list = list.child(super::modals::render_compile_line(&line, p, p.bg_panel));
    }
    let hint = if view.app.mode == Mode::Ide {
        "↑/↓ · Ctrl+N/P · Enter: visit · Esc: editor · Ctrl+G: close"
    } else {
        "j/k · gg/G · Enter: visit · Esc: editor · q: close"
    };
    div()
        .debug_selector(|| "compilation-split".into())
        .flex_1()
        .min_h_0()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .px_3()
                .py_1()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(11.0))
                        .text_color(rgb(status_color))
                        .child(format!(
                            "{stage} · {} locations · {}",
                            errors.len(),
                            compile.command
                        )),
                )
                .child(
                    div()
                        .id("compilation-edit-command")
                        .px_2()
                        .py_1()
                        .rounded(px(4.0))
                        .text_size(px(11.0))
                        .text_color(rgb(p.text_muted))
                        .hover(|s| s.bg(rgb(p.bg_hover)))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.run_command("compile", cx);
                        }))
                        .child("Command…"),
                )
                .child(
                    div()
                        .id("compilation-rerun")
                        .px_2()
                        .py_1()
                        .rounded(px(4.0))
                        .text_size(px(11.0))
                        .text_color(rgb(p.accent))
                        .hover(|s| s.bg(rgb(p.bg_hover)))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.app.compile_widget.is_running() {
                                this.app.compile_widget.stop();
                            } else if this.app.compile_widget.command.trim().is_empty() {
                                this.run_command("compile", cx);
                            } else {
                                this.app.compile_widget.start();
                            }
                            cx.notify();
                        }))
                        .child(if compile.is_running() {
                            "Stop"
                        } else {
                            "Run again"
                        }),
                ),
        )
        .when(!compile.error_message.is_empty(), |d| {
            d.child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_3()
                    .py_1()
                    .whitespace_normal()
                    .text_size(px(11.0))
                    .text_color(rgb(p.error))
                    .child(compile.error_message.clone()),
            )
        })
        .child(list)
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .px_3()
                .py_1()
                .text_size(px(10.0))
                .text_color(rgb(p.text_muted))
                .truncate()
                .child(hint),
        )
}

#[cfg(test)]
mod tests {
    #[gpui::test]
    fn compilation_split_is_non_modal_and_readable_in_zen(cx: &mut gpui::TestAppContext) {
        use gpui::{px, size};
        for mode in [
            crate::tui::app::Mode::Ide,
            crate::tui::app::Mode::Normal,
            crate::tui::app::Mode::Zen,
        ] {
            for (width, height) in [(1000.0, 700.0), (480.0, 400.0)] {
                let (view, window) = cx.add_window_view(|_, cx| {
                    let mut view = crate::gui::modal_tests::fixture(cx);
                    view.app.extension_shop.is_open = false;
                    view.app.mode = mode;
                    view.app.workspace.drawer_visible = true;
                    view.app.workspace.drawer_tab = 3;
                    view.app.compile_widget.lines.push(
                        crate::tui::widgets::compile::parse_ansi_line(
                            "src/main.c:12:4: error: incompatible types",
                        ),
                    );
                    view
                });
                window.simulate_resize(size(px(width), px(height)));
                window.run_until_parked();
                let split = window
                    .debug_bounds("compilation-split")
                    .expect("compilation visible in every mode");
                let message = window
                    .debug_bounds("compilation-message-0")
                    .expect("diagnostic text");
                let editor = window
                    .debug_bounds("nvim-editor-pane")
                    .expect("main editor stays visible");
                assert!(editor.size.height > px(50.0));
                assert!(message.size.width > px(100.0));
                assert!(
                    message.size.height < px(if width < 640.0 { 90.0 } else { 50.0 }),
                    "diagnostic must wrap as readable prose, not a ribbon: {message:?}"
                );
                assert!(editor.bottom() <= split.top());
                let row = window.debug_bounds("compilation-diagnostic-0").unwrap();
                window.simulate_mouse_down(
                    row.center(),
                    gpui::MouseButton::Left,
                    gpui::Modifiers::default(),
                );
                window.simulate_mouse_up(
                    row.center(),
                    gpui::MouseButton::Left,
                    gpui::Modifiers::default(),
                );
                window.run_until_parked();
                view.read_with(window, |this, _| {
                    assert_eq!(
                        this.app.workspace.focus,
                        crate::tui::app::FocusRegion::Editor
                    );
                    assert_eq!(this.app.compile_widget.visited_diagnostic, Some(0));
                });
                let close = window.debug_bounds("drawer-close").unwrap();
                window.simulate_mouse_down(
                    close.center(),
                    gpui::MouseButton::Left,
                    gpui::Modifiers::default(),
                );
                window.simulate_mouse_up(
                    close.center(),
                    gpui::MouseButton::Left,
                    gpui::Modifiers::default(),
                );
                window.run_until_parked();
                view.read_with(window, |this, _| {
                    assert!(!this.app.workspace.drawer_visible);
                    assert!(!this.app.compile_widget.lines.is_empty());
                });
            }
        }
    }
}
