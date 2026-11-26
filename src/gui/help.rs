//! Native help uses the same command registry and bindings as the palette.
use super::{
    icons::{gui_palette, icon, GuiPalette},
    modal_layout::{list_card, scroll_column},
    modals::{backdrop, modal_button, modal_card},
    GtuimView,
};
use crate::tui::{app::Mode, events, widgets::settings::Keybindings, workspace};
use gpui::{prelude::*, *};

const GROUPS: &[(&str, &[&str])] = &[
    (
        "Files & editing",
        &[
            "new_file",
            "find_file",
            "save_file",
            "save_file_as",
            "close_buffer",
            "switch_buffers",
            "search_project",
        ],
    ),
    (
        "Workspace",
        &[
            "toggle_explorer",
            "terminal",
            "terminal_right",
            "split_right",
            "split_down",
            "focus_next",
            "focus_prev",
            "zen",
        ],
    ),
    (
        "Build & diagnostics",
        &["compile", "compilation", "next_error", "previous_error"],
    ),
    (
        "Preferences & tools",
        &[
            "settings",
            "keyboard_shortcuts",
            "language_tools",
            "extensions",
            "report_bug",
        ],
    ),
];

// Keep behavior text separate from configurable global shortcut labels below.
const BEHAVIOR_SECTIONS: &[(&str, &str)] = &[
    ("Welcome & workspace", "Starting without a file shows the native welcome screen: New File, Open File, Search Project, and recent files from this project. It is not an editable buffer. Opening a file or picker reveals the editor; the welcome screen returns when you return to the dashboard. The activity rail opens workspace tools; pane dividers resize the layout."),
    ("Saving & closing files", "A dot on a tab marks unsaved changes. Save writes the current file; an unnamed buffer asks for a path. Save As writes to a different path and updates the tab and Explorer. Closing a dirty buffer asks whether to save, discard, or cancel. Cancel preserves your buffer. Existing files retain their trailing-newline policy."),
    ("Compile your project", "Choose Tools → Compile, check the working directory and build command, then Run. A started build opens the non-modal Compilation split below the editor. It contains clickable compiler locations followed by retained build output. Failed builds reveal the split unless doing so would replace a terminal you are actively using. Open it yourself with Toggle Compilation split; its current shortcut is listed below."),
    ("Visit an error or warning", "Click a compiler location, or select one and press Enter. Its file opens at the reported line and column in the largest editor pane—not a terminal, help window, or floating picker. Existing modified buffers are reused without discarding edits. Focus returns to the editor while Compilation stays visible. Next/Previous compilation diagnostic visits locations directly from the editor; the first Next visits the first location."),
    ("Compilation: Normal & Zen controls", "With the Compilation split focused, use j/k or arrow keys to select a location, gg/G for the first/last location, and Enter to visit it. Page Up/Down move ten locations. Esc returns focus to the editor. q or Ctrl+G closes the split. Vim-style navigation is available in both Normal and Zen modes; Zen can still show terminals and Compilation."),
    ("Compilation: IDE & mouse controls", "With Compilation focused, use Up/Down or Ctrl+P/Ctrl+N to select a location and Enter to visit it. Page Down / Page Up move down/up ten locations; Alt+V also moves up ten. Home/End or Alt+< / Alt+> select the first/last location. These keys navigate the list, not the editor; Ctrl+N/P do not create a file or open a picker here. Click a location to visit it, scroll to read output, or click the Compilation tab to focus the split. Esc returns to the editor; Ctrl+G or the close button closes the split. Compilation is read-only: typing and paste cannot change source text."),
    ("Close, stop & rerun a build", "Closing Compilation hides it and retains its output; it does not stop a running build. Closing the compile-command dialog also leaves the build running. Use Stop or Ctrl+C while Compilation is focused to stop it explicitly. Run again starts a fresh build with the same command; Command… opens the command dialog. If no command is configured, Run again opens that dialog first. Reopen the split using Tools → Compilation Split, the command palette, or your configured shortcut."),
    ("Compiler output & location limits", "Common GCC/Clang, Rust, Go, Java, TypeScript/MSVC-style locations and Python traceback locations are recognized. Relative filenames resolve from the build working directory. Compiler formats and column conventions vary; not every tool's output is navigable. Raw output remains readable even without recognized locations. Output is bounded to the latest 2,500 lines, so older output and its locations can be dropped during a very large build."),
];

fn binding_label(kb: &Keybindings, field: &str) -> String {
    let label = workspace::format_shortcut_display(kb.get_by_field(field));
    if label.is_empty() {
        "Not assigned".into()
    } else {
        label
    }
}

fn heading(title: &str, p: GuiPalette) -> Div {
    div()
        .w_full()
        .min_w_0()
        .whitespace_normal()
        .text_size(px(12.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(p.text_strong))
        .child(title.to_owned())
}

pub(super) fn render_help(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let p = gui_palette(&view.app.theme);
    let kb = &view.app.settings_widget.config.keybindings;
    let mut body = scroll_column("help-body").p_4().flex().flex_col().gap_4();
    let mode = match view.app.mode {
        Mode::Normal => "Vim mode · Press i to insert text and Esc for Normal-mode motions. Workspace shortcuts can override Vim keys; check your bindings below.",
        Mode::Zen => "Zen · Workspace chrome is hidden for focus. Editing uses Vim-style Normal-mode controls. Use Toggle Zen to restore chrome; terminals and the Compilation split can still be opened.",
        _ => "IDE mode · Click in the editor to type. Drag to select text; right-click for editing actions. Vim Normal-mode motions are not typing shortcuts in this mode.",
    };
    body = body.child(
        list_card()
            .p_3()
            .rounded(px(7.0))
            .bg(rgb(p.bg_selected))
            .flex()
            .flex_col()
            .gap_2()
            .child(heading("Your editing mode", p))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .whitespace_normal()
                    .text_size(px(12.0))
                    .text_color(rgb(p.text))
                    .child(mode),
            ),
    );
    let mut start = list_card()
        .flex()
        .flex_col()
        .gap_2()
        .child(heading("A quick start", p));
    for (title, text) in [
        ("01  Open a file", "Choose a file in Explorer, or run Find file from the command palette. New file creates an unnamed buffer."),
        ("02  Edit, then save", "Save file writes your changes; an unnamed buffer asks for a path. Save file as… writes to a different path. A dot on a tab means unsaved changes."),
        ("03  Make room for your work", "Use the activity rail for tools and the top bar for splits and terminals. Drag pane dividers to resize your workspace."),
    ] {
        start = start.child(list_card().p_3().rounded(px(7.0)).bg(rgb(p.bg_surface))
            .border_1().border_color(rgb(p.border_subtle)).flex().flex_col().gap_1()
            .child(heading(title, p)).child(div().w_full().min_w_0().whitespace_normal()
                .text_size(px(12.0)).text_color(rgb(p.text_muted)).child(text)));
    }
    body = body.child(start);
    let mut behaviors = list_card()
        .flex()
        .flex_col()
        .gap_2()
        .child(heading("How Gtuim behaves", p));
    for (index, &(title, text)) in BEHAVIOR_SECTIONS.iter().enumerate() {
        behaviors = behaviors.child(
            list_card()
                .debug_selector(move || format!("help-behavior-{index}"))
                .p_3()
                .rounded(px(7.0))
                .bg(rgb(p.bg_surface))
                .border_1()
                .border_color(rgb(p.border_subtle))
                .flex()
                .flex_col()
                .gap_2()
                .child(heading(title, p))
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(12.0))
                        .text_color(rgb(p.text_muted))
                        .child(text),
                ),
        );
    }
    body = body.child(behaviors)
        .child(modal_button("help-commands", format!("Commands · {}", binding_label(kb, "commands")), false, false, p,
            cx.listener(|this, _, _, cx| {
                this.cancel_onboarding(cx);
                events::open_palette(&mut this.app, crate::tui::app::PaletteMode::Commands);
                cx.notify();
            })))
        .child(div().w_full().min_w_0().whitespace_normal()
        .text_size(px(11.0)).text_color(rgb(p.text_muted))
        .child("Current shortcuts · These reflect your saved configuration, not factory defaults. Click an action to run it. Unassigned actions remain available from menus and the command palette."));
    for &(title, ids) in GROUPS {
        let mut group = list_card()
            .flex()
            .flex_col()
            .gap_1()
            .child(heading(title, p));
        for &id in ids {
            let Some(command) = workspace::COMMANDS.iter().find(|c| c.id == id) else {
                continue;
            };
            let shortcut = binding_label(kb, command.key_field);
            group = group.child(
                div()
                    .id(SharedString::from(format!("help-action-{id}")))
                    .debug_selector(move || format!("help-row-{id}"))
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_3()
                    .py_2()
                    .rounded(px(5.0))
                    .bg(rgb(p.bg_surface))
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.cancel_onboarding(cx);
                        events::execute_command(&mut this.app, id);
                        let _ = this.app.editor_rpc.flush_writes();
                        cx.notify();
                    }))
                    .child(
                        div()
                            .debug_selector(move || format!("help-label-{id}"))
                            .flex_1()
                            .min_w_0()
                            .whitespace_normal()
                            .text_size(px(12.0))
                            .text_color(rgb(p.text))
                            .child(command.label),
                    )
                    .child(
                        div()
                            .debug_selector(move || format!("help-key-{id}"))
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .px_2()
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(rgb(p.bg_elevated))
                            .text_size(px(11.0))
                            .text_color(rgb(p.text_muted))
                            .child(shortcut),
                    ),
            );
        }
        body = body.child(group);
    }
    backdrop(
        "help-backdrop",
        cx.listener(|this, _, _, cx| this.cancel_onboarding(cx)),
    )
    .child(
        modal_card(p, 760.0, 680.0)
            .debug_selector(|| "help-dialog".into())
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .p_4()
                    .border_b_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icon("book-open", 20.0, p.accent))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(heading("Help & Quickstart", p))
                            .child(
                                div()
                                    .whitespace_normal()
                                    .text_size(px(11.0))
                                    .text_color(rgb(p.text_muted))
                                    .child("Your workspace, at a glance"),
                            ),
                    )
                    .child(modal_button(
                        "help-close",
                        "Close",
                        false,
                        false,
                        p,
                        cx.listener(|this, _, _, cx| this.cancel_onboarding(cx)),
                    )),
            )
            .child(body)
            .child(
                div()
                    .debug_selector(|| "help-footer".into())
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(rgb(p.border))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(p.text_muted))
                            .child("Esc to close"),
                    )
                    .child(modal_button(
                        "help-keybindings",
                        "Keyboard shortcuts",
                        false,
                        false,
                        p,
                        cx.listener(|this, _, _, cx| {
                            this.cancel_onboarding(cx);
                            events::execute_command(&mut this.app, "keyboard_shortcuts");
                            cx.notify();
                        }),
                    )),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::{binding_label, BEHAVIOR_SECTIONS, GROUPS};
    use crate::tui::{widgets::settings::Keybindings, workspace};
    #[test]
    fn help_uses_live_bindings_without_default_fallbacks() {
        let mut kb = Keybindings::vim_safe_preset();
        assert_eq!(binding_label(&kb, "save_file"), "F2");
        kb.save_file.clear();
        assert_eq!(binding_label(&kb, "save_file"), "Not assigned");
        kb.save_file = "<M-s>".into();
        assert_eq!(
            binding_label(&kb, "save_file"),
            workspace::format_shortcut_display("<M-s>")
        );
    }
    #[test]
    fn help_keyboard_cannot_apply_settings_or_advance_setup() {
        use crate::tui::{
            input::InputEvent,
            widgets::onboarding::{OnboardingKeyAction, OnboardingWidget},
        };
        let mut help = OnboardingWidget::new();
        help.is_open = true;
        help.help_only = true;
        let initial = help.draft.clone();
        for key in ["<C-s>", "<CR>", "1", "<Tab>"] {
            assert_eq!(
                help.handle_key(&InputEvent::Key(key.into())),
                OnboardingKeyAction::None
            );
        }
        assert_eq!(help.draft, initial);
        assert_eq!(
            help.handle_key(&InputEvent::Key("<Esc>".into())),
            OnboardingKeyAction::Cancel
        );
        help.open(&initial, false);
        assert!(!help.help_only, "setup must reset read-only help mode");
    }

    #[gpui::test]
    fn native_help_scrolls_above_visible_footer_in_small_windows(cx: &mut gpui::TestAppContext) {
        use gpui::{px, size};
        for (width, height) in [(1440.0, 900.0), (640.0, 480.0), (300.0, 220.0)] {
            let (_, window) = cx.add_window_view(|_, cx| {
                let mut view = crate::gui::modal_tests::fixture(cx);
                view.app.extension_shop.is_open = false;
                view.app.open_onboarding(false);
                view.app.onboarding.help_only = true;
                view
            });
            window.simulate_resize(size(px(width), px(height)));
            window.run_until_parked();
            let dialog = window.debug_bounds("help-dialog").expect("native help");
            let body = window
                .debug_bounds("help-body")
                .expect("scrolling help body");
            let footer = window.debug_bounds("help-footer").expect("help actions");
            assert!(dialog.size.width <= px(width));
            assert!(dialog.size.height <= px(height));
            assert!(body.bottom() <= footer.top() + px(1.0));
            assert!(
                footer.bottom() <= dialog.bottom() + px(1.0),
                "{width}x{height}: footer {footer:?}, dialog {dialog:?}"
            );
            assert!(footer.size.height > px(20.0));
            for (id, row_selector, label_selector, key_selector) in [
                (
                    "focus_next",
                    "help-row-focus_next",
                    "help-label-focus_next",
                    "help-key-focus_next",
                ),
                (
                    "save_file_as",
                    "help-row-save_file_as",
                    "help-label-save_file_as",
                    "help-key-save_file_as",
                ),
                (
                    "keyboard_shortcuts",
                    "help-row-keyboard_shortcuts",
                    "help-label-keyboard_shortcuts",
                    "help-key-keyboard_shortcuts",
                ),
            ] {
                let row = window.debug_bounds(row_selector).expect("action row");
                let label = window.debug_bounds(label_selector).expect("action label");
                let key = window.debug_bounds(key_selector).expect("shortcut badge");
                assert!(
                    label.size.width >= px(60.0),
                    "{width}: collapsed label {id}: {label:?}"
                );
                assert!(label.right() <= key.left(), "label overlaps shortcut");
                assert!(key.right() <= row.right(), "shortcut outside row");
                if width >= 640.0 {
                    assert!(
                        row.size.height < px(55.0),
                        "{id} must not wrap into a vertical ribbon: {row:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn compilation_help_covers_workflow_and_uses_live_shortcuts() {
        let actions = GROUPS
            .iter()
            .find(|(title, _)| *title == "Build & diagnostics")
            .unwrap()
            .1;
        assert_eq!(
            actions,
            &["compile", "compilation", "next_error", "previous_error"]
        );
        let mut kb = Keybindings::default();
        assert_eq!(binding_label(&kb, "compilation"), "F4");
        assert_eq!(binding_label(&kb, "next_error"), "F8");
        assert_eq!(binding_label(&kb, "previous_error"), "Shift+F8");
        kb.compilation = "<M-b>".into();
        assert_eq!(
            binding_label(&kb, "compilation"),
            workspace::format_shortcut_display("<M-b>")
        );
        kb.next_error.clear();
        assert_eq!(binding_label(&kb, "next_error"), "Not assigned");
        let text = BEHAVIOR_SECTIONS
            .iter()
            .map(|(_, text)| *text)
            .collect::<Vec<_>>()
            .join(" ");
        for required in [
            "working directory",
            "line and column",
            "modified buffers",
            "Ctrl+P/Ctrl+N",
            "gg/G",
            "does not stop",
            "Ctrl+C",
            "2,500",
        ] {
            assert!(text.contains(required), "missing help behavior: {required}");
        }
    }

    #[test]
    fn help_actions_exist_in_the_command_registry() {
        for &(_, ids) in GROUPS {
            for id in ids {
                assert!(
                    workspace::COMMANDS.iter().any(|c| c.id == *id),
                    "unknown help action: {id}"
                );
            }
        }
    }
}
