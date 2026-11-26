//! Regressions for Gemini's profile peer review; no practice implementation is modified.
use super::{calendar_color, handle_key, panel_width};
use crate::tui::{
    app::Mode,
    theme::{blend_u32, contrast_ratio_u32, readable_u32, Theme},
    widgets::settings::{SettingsConfig, SettingsWidget},
};
use gpui::{px, size, KeyDownEvent, Keystroke, Modifiers, MouseButton, TestAppContext};

fn key(value: &str) -> KeyDownEvent {
    KeyDownEvent {
        keystroke: Keystroke::parse(value).unwrap(),
        is_held: false,
    }
}

#[gpui::test]
fn overlays_and_zen_take_precedence_over_profile_rename(cx: &mut TestAppContext) {
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = crate::gui::modal_tests::fixture(cx);
        view.app.extension_shop.is_open = false;
        view
    });
    view.update(window, |this, cx| {
        for palette in [false, true] {
            this.profiles.open = true;
            this.profiles.editing_name = true;
            this.profiles.name_draft = "Unchanged".into();
            this.app.settings_widget.is_open = !palette;
            this.app.workspace.palette_open = palette;
            assert!(!handle_key(this, &key("escape"), cx));
            assert!(!handle_key(this, &key("x"), cx));
            assert_eq!(this.profiles.name_draft, "Unchanged");
            assert!(
                this.profiles.open,
                "modal Escape must not dismiss the panel behind it"
            );
            assert!(!this.profiles.editing_name);
        }
        this.app.settings_widget.is_open = false;
        this.app.workspace.palette_open = false;
        this.app.mode = Mode::Zen;
        this.profiles.editing_name = true;
        assert!(!handle_key(this, &key("escape"), cx));
        assert!(!this.profiles.open);
        assert!(!this.profiles.editing_name);
        this.app.mode = Mode::Ide;
        this.profiles.open = true;
        this.profiles.editing_name = true;
        this.profiles.name_draft = "Two".into();
        let mut space = key("space");
        space.keystroke.key_char = None;
        assert!(handle_key(this, &space, cx));
        let mut control = key("x");
        control.keystroke.key_char = Some("\n\t".into());
        assert!(handle_key(this, &control, cx));
        assert_eq!(this.profiles.name_draft, "Two ");
    });
}

#[gpui::test]
fn close_and_disable_restore_root_focus_and_flush_drafts(cx: &mut TestAppContext) {
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = crate::gui::modal_tests::fixture(cx);
        view.app.extension_shop.is_open = false;
        view
    });
    window.simulate_resize(size(px(1200.0), px(900.0)));
    window.update(|window, cx| {
        view.update(cx, |this, cx| {
            this.set_profile_panel_open(true, window, cx);
            this.set_profile_practice_enabled(true, window, cx).unwrap();
            let panel = this.practice_panels[&this.profiles.active().id]
                .panel
                .clone();
            panel.update(cx, |panel, cx| panel.set_solution_text("# close flush", cx));
            panel.read(cx).focus(window);
            assert!(!this.root_focus.is_focused(window));
        })
    });
    window.run_until_parked();
    let close = window.debug_bounds("profile-close").unwrap();
    window.simulate_mouse_down(close.center(), MouseButton::Left, Modifiers::default());
    window.simulate_mouse_up(close.center(), MouseButton::Left, Modifiers::default());
    window.run_until_parked();
    window.update(|window, cx| {
        view.update(cx, |this, cx| {
            assert!(this.root_focus.is_focused(window));
            assert!(!this.profiles.open);
            let saved =
                std::fs::read_to_string(this.profiles.practice_dir().join("practice_state.json"))
                    .unwrap();
            assert!(saved.contains("# close flush"));
            this.set_profile_panel_open(true, window, cx);
            let panel = this.practice_panels[&this.profiles.active().id]
                .panel
                .clone();
            panel.read(cx).focus(window);
            this.set_profile_practice_enabled(false, window, cx)
                .unwrap();
            assert!(this.root_focus.is_focused(window));
            assert!(!this.profiles.active().practice_enabled);
            this.set_profile_practice_enabled(true, window, cx).unwrap();
            panel.read(cx).focus(window);
            this.app.settings_widget.is_open = true;
            this.sync_practice_widget(window, cx);
            assert!(
                this.root_focus.is_focused(window),
                "modal keys must not remain focused in practice underneath"
            );
        })
    });
}

#[gpui::test]
fn rendering_does_not_construct_practice_and_font_restore_repopulates_settings(
    cx: &mut TestAppContext,
) {
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = crate::gui::modal_tests::fixture(cx);
        view.app.extension_shop.is_open = false;
        view.profiles.open = true;
        view.profiles.enable_practice().unwrap();
        view
    });
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert!(
            this.practice_panels.is_empty(),
            "render must be side-effect free"
        )
    });
    window.update(|window, cx| {
        view.update(cx, |this, cx| {
            this.sync_practice_widget(window, cx);
            assert_eq!(this.practice_panels.len(), 1);
            let mut config = SettingsConfig::default();
            config.font_size = 22;
            config.font_family = "Fira Code".into();
            this.app.settings_widget = SettingsWidget::new("", config);
            assert!(this.app.settings_widget.available_fonts.is_empty());
            this.sync_profile_typography();
            assert!(this
                .app
                .settings_widget
                .available_fonts
                .iter()
                .any(|font| font == "Fira Code"));
            assert_eq!(this.font_size, 22.0);
            assert_eq!(this.mono_font.as_ref(), "Fira Code");
        })
    });
}

#[test]
fn practice_layout_and_semantic_colors_adapt_to_viewport_and_light_theme() {
    assert_eq!(panel_width(1200.0, false), 340.0);
    assert_eq!(panel_width(1200.0, true), 640.0);
    assert_eq!(panel_width(500.0, true), 488.0);
    for bg in [0x101010, 0xffffff] {
        let mut theme = Theme::default();
        theme.bg_editor = bg;
        let p = crate::gui::icons::gui_palette(&theme);
        assert_eq!(calendar_color(p, 0), blend_u32(p.bg_surface, p.success, 12));
        assert_eq!(calendar_color(p, 100), p.success);
        assert_ne!(calendar_color(p, 0), calendar_color(p, 5));
        assert!(
            contrast_ratio_u32(readable_u32(p.success, p.bg_surface, 3.0), p.bg_surface) >= 2.99
        );
    }
}
