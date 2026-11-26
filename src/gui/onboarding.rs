//! Zed-inspired native GPUI Welcome & First-Run Setup modal.
use gpui::{prelude::*, *};

use crate::gui::icons::{gui_palette, icon, tuim_logo, GuiPalette};
use crate::gui::modal_layout::{list_card, scroll_column};
use crate::gui::modals::{backdrop, modal_button, modal_card};
use crate::gui::GtuimView;
use crate::tui::theme::blend_u32;
use crate::tui::widgets::onboarding::{theme_swatch, ControlPreset, OnboardingStep};
use crate::tui::workspace;

pub fn render_onboarding(view: &GtuimView, cx: &mut Context<GtuimView>) -> impl IntoElement {
    let app = &view.app;
    let ob = &app.onboarding;
    let p = gui_palette(&app.theme);

    let title = if ob.is_first_run {
        "Welcome to Gtuim"
    } else {
        "Gtuim Setup & Quickstart"
    };
    let subtitle = "Choose your theme, controls, and workspace defaults — everything previews live.";

    let header = div()
        .debug_selector(|| "onboarding-header".into())
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .px_5()
        .py_4()
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
                .gap_3()
                .child(
                    div()
                        .size(px(36.0))
                        .flex_shrink_0()
                        .rounded(px(9.0))
                        .bg(rgb(blend_u32(p.bg_surface, p.accent, 18)))
                        .border_1()
                        .border_color(rgb(blend_u32(p.border, p.accent, 45)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(tuim_logo(24.0)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .flex_wrap()
                                .gap_2()
                                .child(
                                    div()
                                        .text_size(px(16.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(p.text_strong))
                                        .child(title),
                                )
                                .child(pill_badge(
                                    format!("v{}", env!("CARGO_PKG_VERSION")),
                                    p.bg_elevated,
                                    p.text_muted,
                                )),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(12.0))
                                .text_color(rgb(p.text_muted))
                                .child(subtitle),
                        ),
                ),
        )
        .child(
            div()
                .id("onboarding-close-x")
                .flex_shrink_0()
                .size(px(28.0))
                .rounded(px(6.0))
                .flex()
                .items_center()
                .justify_center()
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cancel_onboarding(cx);
                }))
                .child(icon("x", 15.0, p.text_muted)),
        );

    let mut step_bar = div()
        .debug_selector(|| "onboarding-steps".into())
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .px_5()
        .py(px(10.0))
        .bg(rgb(p.bg_rail))
        .border_b_1()
        .border_color(rgb(p.border))
        .flex()
        .items_center()
        .flex_wrap()
        .gap_2();

    for (idx, step) in OnboardingStep::ALL.iter().enumerate() {
        let active = ob.step == *step;
        let completed = idx < ob.step.index();
        let step_val = *step;
        let badge_bg = if active {
            p.accent
        } else if completed {
            blend_u32(p.bg_surface, p.success, 24)
        } else {
            p.bg_elevated
        };
        let badge_fg = if active {
            p.accent_fg
        } else if completed {
            p.success
        } else {
            p.text_muted
        };

        step_bar = step_bar.child(
            div()
                .id(("onboarding-step", idx))
                .debug_selector(move || format!("onboarding-step-{idx}"))
                .flex_shrink_0()
                .whitespace_nowrap()
                .px_3()
                .py(px(6.0))
                .rounded(px(7.0))
                .border_1()
                .border_color(rgb(if active { p.accent } else { p.border_subtle }))
                .bg(rgb(if active {
                    p.bg_selected
                } else {
                    p.bg_surface
                }))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_2()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.onboarding.step = step_val;
                    cx.notify();
                }))
                .child(
                    div()
                        .size(px(18.0))
                        .flex_shrink_0()
                        .rounded(px(99.0))
                        .bg(rgb(badge_bg))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(10.5))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(badge_fg))
                        .child(if completed {
                            "✓".to_string()
                        } else {
                            format!("{}", idx + 1)
                        }),
                )
                .child(icon(
                    step.icon(),
                    13.5,
                    if active { p.accent } else { p.text_muted },
                ))
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(if active {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(rgb(if active { p.text_strong } else { p.text }))
                        .child(step.label()),
                ),
        );
    }

    let body = match ob.step {
        OnboardingStep::Theme => render_theme_step(view, p, cx).into_any_element(),
        OnboardingStep::Controls => render_controls_step(view, p, cx).into_any_element(),
        OnboardingStep::Guide => render_guide_step(view, p).into_any_element(),
        OnboardingStep::Setup => render_setup_step(view, p, cx).into_any_element(),
    };

    let footer = render_footer(view, p, cx);

    backdrop(
        "onboarding-backdrop",
        cx.listener(|this, _, _, cx| {
            this.cancel_onboarding(cx);
        }),
    )
    .child(
        modal_card(p, 920.0, 660.0)
            .child(header)
            .child(step_bar)
            .child(body)
            .child(footer),
    )
}

fn render_theme_step(
    view: &GtuimView,
    p: GuiPalette,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let ob = &view.app.onboarding;
    let themes = view.app.settings_widget.theme_options();

    let mut theme_grid = div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_wrap()
        .gap_2();

    for (idx, (label, id)) in themes.into_iter().enumerate() {
        let selected = ob.draft.theme == id;
        let sw = theme_swatch(&id);
        let theme_id = id.clone();
        let is_system = id == "system";

        theme_grid = theme_grid.child(
            div()
                .id(("onboarding-theme", idx))
                .debug_selector(move || format!("onboarding-theme-{idx}"))
                .flex_basis(px(195.0))
                .flex_grow()
                .min_w_0()
                .p(px(10.0))
                .rounded(px(8.0))
                .bg(rgb(if selected {
                    p.bg_selected
                } else {
                    p.bg_surface
                }))
                .border_1()
                .border_color(rgb(if selected {
                    p.accent
                } else {
                    p.border_subtle
                }))
                .hover(|s| s.bg(rgb(p.bg_hover)).border_color(rgb(p.accent)))
                .cursor_pointer()
                .flex()
                .flex_col()
                .gap_2()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.preview_onboarding_theme(&theme_id, cx);
                }))
                // Mini Zed-style editor mockup swatch
                .child(
                    div()
                        .w_full()
                        .h(px(46.0))
                        .rounded(px(6.0))
                        .overflow_hidden()
                        .border_1()
                        .border_color(rgb(blend_u32(sw.bg_editor, sw.text, 16)))
                        .bg(rgb(sw.bg_editor))
                        .flex()
                        .child(
                            // Mini sidebar
                            div()
                                .w(px(28.0))
                                .h_full()
                                .flex_shrink_0()
                                .bg(rgb(sw.bg_sidebar))
                                .border_r_1()
                                .border_color(rgb(blend_u32(sw.bg_sidebar, sw.text, 12)))
                                .p(px(5.0))
                                .flex()
                                .flex_col()
                                .gap(px(4.0))
                                .child(
                                    div()
                                        .w(px(14.0))
                                        .h(px(3.0))
                                        .rounded(px(2.0))
                                        .bg(rgb(sw.accent)),
                                )
                                .child(
                                    div()
                                        .w(px(16.0))
                                        .h(px(3.0))
                                        .rounded(px(2.0))
                                        .bg(rgb(blend_u32(sw.bg_sidebar, sw.text, 35))),
                                )
                                .child(
                                    div()
                                        .w(px(11.0))
                                        .h(px(3.0))
                                        .rounded(px(2.0))
                                        .bg(rgb(blend_u32(sw.bg_sidebar, sw.text, 25))),
                                ),
                        )
                        .child(
                            // Mini editor lines
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .p(px(6.0))
                                .flex()
                                .flex_col()
                                .justify_between()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.0))
                                        .child(
                                            div()
                                                .w(px(22.0))
                                                .h(px(4.0))
                                                .rounded(px(2.0))
                                                .bg(rgb(sw.keyword)),
                                        )
                                        .child(
                                            div()
                                                .w(px(36.0))
                                                .h(px(4.0))
                                                .rounded(px(2.0))
                                                .bg(rgb(sw.accent)),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.0))
                                        .pl(px(8.0))
                                        .child(
                                            div()
                                                .w(px(26.0))
                                                .h(px(4.0))
                                                .rounded(px(2.0))
                                                .bg(rgb(sw.text)),
                                        )
                                        .child(
                                            div()
                                                .w(px(32.0))
                                                .h(px(4.0))
                                                .rounded(px(2.0))
                                                .bg(rgb(sw.string)),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(4.0))
                                        .pl(px(8.0))
                                        .child(
                                            div()
                                                .w(px(18.0))
                                                .h(px(4.0))
                                                .rounded(px(2.0))
                                                .bg(rgb(sw.keyword)),
                                        )
                                        .child(
                                            div()
                                                .w(px(24.0))
                                                .h(px(4.0))
                                                .rounded(px(2.0))
                                                .bg(rgb(blend_u32(sw.bg_editor, sw.text, 45))),
                                        ),
                                ),
                        ),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_strong))
                                .child(label),
                        )
                        .when(selected, |d| {
                            d.child(pill_badge("Active", p.accent, p.accent_fg))
                        })
                        .when(!selected && is_system, |d| {
                            d.child(pill_badge("Auto", p.bg_elevated, p.text_muted))
                        }),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(10.5))
                        .text_color(rgb(p.text_muted))
                        .child(sw.subtitle),
                ),
        );
    }

    let font_size = ob.draft.font_size;
    let separator = ob.draft.separator.clone();

    let appearance_bar = list_card()
        .p_3()
        .rounded(px(8.0))
        .bg(rgb(p.bg_surface))
        .border_1()
        .border_color(rgb(p.border_subtle))
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex_basis(px(240.0))
                .flex_grow()
                .min_w_0()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap_2()
                .child(
                    div()
                        .flex_basis(px(140.0))
                        .flex_grow()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_strong))
                                .child("Editor Font Size"),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(11.0))
                                .text_color(rgb(p.text_muted))
                                .child("Zoom anytime with Ctrl+= / Ctrl+- / Ctrl+0"),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(stepper_btn(
                            "onboarding-font-dec",
                            "-",
                            p,
                            cx.listener(|this, _, _, cx| {
                                this.app.onboarding.adjust_font_size(-1);
                                this.font_size = this.app.onboarding.draft.font_size as f32;
                                cx.notify();
                            }),
                        ))
                        .child(
                            div()
                                .flex_shrink_0()
                                .whitespace_nowrap()
                                .px_3()
                                .py(px(4.0))
                                .rounded(px(5.0))
                                .bg(rgb(p.bg_input))
                                .border_1()
                                .border_color(rgb(p.border))
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_strong))
                                .child(format!("{font_size} px")),
                        )
                        .child(stepper_btn(
                            "onboarding-font-inc",
                            "+",
                            p,
                            cx.listener(|this, _, _, cx| {
                                this.app.onboarding.adjust_font_size(1);
                                this.font_size = this.app.onboarding.draft.font_size as f32;
                                cx.notify();
                            }),
                        )),
                ),
        )
        .child(
            div()
                .flex_basis(px(240.0))
                .flex_grow()
                .min_w_0()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap_2()
                .child(
                    div()
                        .flex_basis(px(120.0))
                        .flex_grow()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_strong))
                                .child("Split Separator"),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(11.0))
                                .text_color(rgb(p.text_muted))
                                .child("Divider weight between splits"),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap_1()
                        .children(["thin", "thick", "double"].into_iter().enumerate().map(|(idx, sep)| {
                            let active = separator == sep;
                            let label = match sep {
                                "thin" => "Thin",
                                "thick" => "Thick",
                                _ => "Double",
                            };
                            segment_btn(
                                ("onboarding-sep", idx),
                                label,
                                active,
                                p,
                                cx.listener(move |this, _, _, cx| {
                                    this.app.onboarding.draft.separator = sep.to_string();
                                    cx.notify();
                                }),
                            )
                        })),
                ),
        );

    scroll_column("onboarding-body")
        .p_5()
        .flex()
        .flex_col()
        .gap_4()
        .child(section_heading(
            "Choose Your Theme",
            "Click any theme to preview it live across the entire editor and workspace chrome.",
            p,
        ))
        .child(theme_grid)
        .child(section_heading(
            "Display & Readability",
            "Adjust text size and split dividers to match your screen.",
            p,
        ))
        .child(appearance_bar)
}

fn render_controls_step(
    view: &GtuimView,
    p: GuiPalette,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let ob = &view.app.onboarding;

    let modes = [
        (
            "ide",
            "IDE Mode",
            "Default",
            "sparkles",
            "Click and type immediately like VS Code or Zed. Use your configured Save shortcut; Ctrl+Z undoes, Ctrl+F finds, and Ctrl+C/V copy and paste.",
        ),
        (
            "normal",
            "Normal Mode (Vim)",
            "Modal",
            "terminal",
            "Classic modal Neovim controls: press i to insert, Esc for Normal commands, full Vim motions (hjkl, w, dd, ciw, :w).",
        ),
        (
            "zen",
            "Zen Mode",
            "Focused",
            "maximize",
            "Distraction-free editing with panels hidden and Vim-style controls: i to insert, Esc for Normal motions. Use the Toggle Zen command or your configured shortcut to restore chrome.",
        ),
    ];

    let mut mode_grid = div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_wrap()
        .gap_3();

    for (idx, (id, title, badge, ic, desc)) in modes.into_iter().enumerate() {
        let selected = ob.draft.mode == id;
        mode_grid = mode_grid.child(
            div()
                .id(("onboarding-mode", idx))
                .debug_selector(move || format!("onboarding-mode-{idx}"))
                .flex_basis(px(230.0))
                .flex_grow()
                .min_w_0()
                .p_3()
                .rounded(px(8.0))
                .bg(rgb(if selected {
                    p.bg_selected
                } else {
                    p.bg_surface
                }))
                .border_1()
                .border_color(rgb(if selected {
                    p.accent
                } else {
                    p.border_subtle
                }))
                .hover(|s| s.bg(rgb(p.bg_hover)).border_color(rgb(p.accent)))
                .cursor_pointer()
                .flex()
                .flex_col()
                .gap_2()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.onboarding.select_mode(id);
                    cx.notify();
                }))
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(icon(ic, 15.0, if selected { p.accent } else { p.text_muted }))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .whitespace_normal()
                                        .text_size(px(13.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(p.text_strong))
                                        .child(title),
                                ),
                        )
                        .child(pill_badge(
                            if selected { "Selected" } else { badge },
                            if selected { p.accent } else { p.bg_elevated },
                            if selected { p.accent_fg } else { p.text_muted },
                        )),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(11.5))
                        .text_color(rgb(p.text_muted))
                        .child(desc),
                ),
        );
    }

    let mut preset_grid = div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_wrap()
        .gap_3();

    for (idx, preset) in ControlPreset::SELECTABLE.into_iter().enumerate() {
        let selected = ob.preset == preset;
        preset_grid = preset_grid.child(
            div()
                .id(("onboarding-preset", idx))
                .debug_selector(move || format!("onboarding-preset-{idx}"))
                .flex_basis(px(230.0))
                .flex_grow()
                .min_w_0()
                .p_3()
                .rounded(px(8.0))
                .bg(rgb(if selected {
                    p.bg_selected
                } else {
                    p.bg_surface
                }))
                .border_1()
                .border_color(rgb(if selected {
                    p.accent
                } else {
                    p.border_subtle
                }))
                .hover(|s| s.bg(rgb(p.bg_hover)).border_color(rgb(p.accent)))
                .cursor_pointer()
                .flex()
                .flex_col()
                .gap_2()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.app.onboarding.select_preset(preset);
                    cx.notify();
                }))
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_strong))
                                .child(preset.label()),
                        )
                        .child(pill_badge(
                            if selected { "Selected" } else { preset.badge() },
                            if selected { p.accent } else { p.bg_elevated },
                            if selected { p.accent_fg } else { p.text_muted },
                        )),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(11.5))
                        .text_color(rgb(p.text_muted))
                        .child(preset.summary()),
                ),
        );
    }

    let kb = &ob.draft.keybindings;
    let preview_items = [
        ("Command Palette", kb.commands.as_str()),
        ("Explorer", kb.toggle_explorer.as_str()),
        ("Terminal", kb.toggle_terminal.as_str()),
        ("Find File", kb.find_file.as_str()),
        ("Search Project", kb.search_project.as_str()),
        ("New File", kb.new_file.as_str()),
        ("Save File", kb.save_file.as_str()),
        ("Zen Mode", kb.toggle_zen.as_str()),
        ("Next Pane", kb.focus_next.as_str()),
        ("Quit", kb.quit.as_str()),
    ];

    let mut shortcut_chips = div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_wrap()
        .gap_2();

    for (label, raw) in preview_items {
        let disp = workspace::format_shortcut_display(raw);
        let disp = if disp.is_empty() {
            "Unbound".to_string()
        } else {
            disp
        };
        shortcut_chips = shortcut_chips.child(
            div()
                .flex_shrink_0()
                .px(px(10.0))
                .py(px(5.0))
                .rounded(px(6.0))
                .bg(rgb(p.bg_panel))
                .border_1()
                .border_color(rgb(p.border_subtle))
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .whitespace_nowrap()
                        .text_size(px(11.5))
                        .text_color(rgb(p.text_muted))
                        .child(label),
                )
                .child(kbd_pill(disp, p)),
        );
    }

    let preview_card = list_card()
        .p_3()
        .rounded(px(8.0))
        .bg(rgb(p.bg_surface))
        .border_1()
        .border_color(rgb(p.border_subtle))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap_2()
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(p.text_strong))
                        .child(format!("Active Shortcuts — {} Preset", ob.preset.label())),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(p.text_muted))
                        .child("You can rebind any command later in Settings or with F2 in Command Palette."),
                ),
        )
        .child(shortcut_chips);

    scroll_column("onboarding-body")
        .p_5()
        .flex()
        .flex_col()
        .gap_4()
        .child(section_heading(
            "Editing Style",
            "Pick how typing and commands behave inside the editor. You can switch anytime from the status bar or Settings.",
            p,
        ))
        .child(mode_grid)
        .child(section_heading(
            "Control Preset (Keymap)",
            "Choose your default workspace keyboard shortcuts.",
            p,
        ))
        .child(preset_grid)
        .child(preview_card)
}

fn render_guide_step(view: &GtuimView, p: GuiPalette) -> impl IntoElement {
    let ob = &view.app.onboarding;
    let kb = &ob.draft.keybindings;

    // An intentionally unbound command must not masquerade as a default shortcut.
    let fmt = |raw: &str| {
        let d = workspace::format_shortcut_display(raw);
        if d.is_empty() { "Not assigned".to_string() } else { d }
    };

    let k_cmd = fmt(&kb.commands);
    let k_exp = fmt(&kb.toggle_explorer);
    let k_term = fmt(&kb.toggle_terminal);
    let k_find = fmt(&kb.find_file);
    let k_grep = fmt(&kb.search_project);
    let k_new = fmt(&kb.new_file);
    let k_save = fmt(&kb.save_file);
    let k_zen = fmt(&kb.toggle_zen);
    let k_next = fmt(&kb.focus_next);

    let edit_step_text = match ob.draft.mode.as_str() {
        "normal" => "You selected Normal (Vim) mode: press i to start typing, Esc to return to Normal mode for motions (hjkl, w, dd, ciw), and : for Neovim commands. Drag with the mouse to select text or right-click for editor & AI actions.",
        "zen" => "You selected Zen mode: panels start hidden for distraction-free focus. Editing uses Vim-style controls: i to insert and Esc for Normal-mode motions. Run Toggle Zen mode to reveal workspace chrome. Terminals and the Compilation split remain available.",
        _ => "You selected IDE mode (default): click anywhere in the editor and start typing right away. Use Ctrl+Z / Ctrl+Y to undo/redo, Ctrl+F to find, drag to select text, and right-click for copy, paste, or AI actions.",
    };

    let quickstart_steps = [
        (
            "1",
            "Open or Create a File",
            format!(
                "Click Explorer in the activity rail to browse files. Use Find file to search filenames, or New file to start an unnamed buffer. Your selected shortcuts are listed below."
            ),
        ),
        ("2", "Edit & Save", format!("{edit_step_text} Use File → Save to write changes, or Save As to choose a different path.")),
        (
            "3",
            "Use the Command Palette",
            format!(
                "Click the top search bar to open the command palette. Choose an action to run it. Open Keyboard shortcuts to inspect or change your bindings."
            ),
        ),
        (
            "4",
            "Terminals, Splits & Focus",
            format!(
                "Use the header controls for terminals and editor splits. Focus next workspace region cycles between panes. Toggle Zen mode hides or reveals workspace chrome. Check your configured shortcuts below."
            ),
        ),
        ("5", "Compile & Visit Errors", "Choose Tools → Compile to run a build. The Compilation split keeps the editor visible; click a compiler location or select it and press Enter to open its source. Closing the split retains output and does not stop the build. Help & Quickstart explains mode-specific controls and shows your current Compilation/Next/Previous diagnostic shortcuts.".to_string()),
    ];

    let mut workflow_col = div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap_2();

    for (idx, (num, heading, body)) in quickstart_steps.into_iter().enumerate() {
        workflow_col = workflow_col.child(
            list_card()
                .debug_selector(move || format!("onboarding-guide-card-{idx}"))
                .p_3()
                .rounded(px(8.0))
                .bg(rgb(p.bg_surface))
                .border_1()
                .border_color(rgb(p.border_subtle))
                .flex()
                .items_start()
                .gap_3()
                .child(
                    div()
                        .size(px(22.0))
                        .flex_shrink_0()
                        .rounded(px(99.0))
                        .bg(rgb(blend_u32(p.bg_surface, p.accent, 22)))
                        .text_size(px(11.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(p.accent))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(num),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_strong))
                                .child(heading),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(11.5))
                                .text_color(rgb(p.text_muted))
                                .child(body),
                        ),
                ),
        );
    }

    let workspace_areas = [
        (
            "files",
            "Explorer & Project Search",
            format!(
                "Browse and manage files in Explorer. Use the Search sidebar or Search project command to search text across the project."
            ),
        ),
        (
            "git-branch",
            "Git & Worktree Orchestrator",
            "Inspect modified files, diffs, and commit history from the Git rail tab, or open Tools → Worktree Orchestrator for parallel branches."
                .to_string(),
        ),
        (
            "sparkles",
            "AI Assistants & Right-Click Actions",
            "Launch AI coding agents from the Sparkles tab in the left rail, or highlight code and right-click to Explain, Fix, or Send Selection to AI."
                .to_string(),
        ),
        (
            "blocks",
            "Plugins, LSPs & Compile",
            "Install language servers via Language Tools (Mason), manage Neovim themes and extensions in Plugin Center, and run builds from Tools → Compile."
                .to_string(),
        ),
    ];

    let mut tour_grid = div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_wrap()
        .gap_2();

    for (ic, heading, body) in workspace_areas {
        tour_grid = tour_grid.child(
            div()
                .flex_basis(px(270.0))
                .flex_grow()
                .min_w_0()
                .p_3()
                .rounded(px(8.0))
                .bg(rgb(p.bg_surface))
                .border_1()
                .border_color(rgb(p.border_subtle))
                .flex()
                .items_start()
                .gap(px(10.0))
                .child(
                    div()
                        .size(px(28.0))
                        .flex_shrink_0()
                        .rounded(px(7.0))
                        .bg(rgb(p.bg_elevated))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon(ic, 14.5, p.accent)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_strong))
                                .child(heading),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(11.5))
                                .text_color(rgb(p.text_muted))
                                .child(body),
                        ),
                ),
        );
    }

    let cheat_rows = [
        ("Command Palette", k_cmd),
        ("New File", k_new),
        ("Focus Next Region", k_next),
        ("Toggle Explorer", k_exp),
        ("Toggle Terminal", k_term),
        ("Find File", k_find),
        ("Search Project", k_grep),
        ("Save File", k_save),
        ("Toggle Zen Mode", k_zen),
        ("Save File As", {
            let key = workspace::format_shortcut_display(&kb.save_file_as);
            if key.is_empty() { "Not assigned".to_string() } else { key }
        }),
    ];

    let mut cheat_grid = div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_wrap()
        .gap_2();

    for (label, key) in cheat_rows {
        cheat_grid = cheat_grid.child(
            div()
                .flex_basis(px(190.0))
                .flex_grow()
                .min_w_0()
                .px_3()
                .py(px(6.0))
                .rounded(px(6.0))
                .bg(rgb(p.bg_panel))
                .border_1()
                .border_color(rgb(p.border_subtle))
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(11.5))
                        .text_color(rgb(p.text))
                        .child(label),
                )
                .child(kbd_pill(key, p)),
        );
    }

    let cheat_card = list_card()
        .p_3()
        .rounded(px(8.0))
        .bg(rgb(p.bg_surface))
        .border_1()
        .border_color(rgb(p.border_subtle))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(p.text_strong))
                .child("Quick Reference (Reflects Your Selected Preset)"),
        )
        .child(cheat_grid);

    scroll_column("onboarding-body")
        .p_5()
        .flex()
        .flex_col()
        .gap_4()
        .child(section_heading(
            "Getting Started in Gtuim",
            "Gtuim pairs a native GPUI desktop interface with an embedded Neovim core. Here is how everything fits together.",
            p,
        ))
        .child(workflow_col)
        .child(section_heading(
            "Workspace Tour",
            "Explore the built-in tools in the left activity rail, top bar, and editor splits.",
            p,
        ))
        .child(tour_grid)
        .child(cheat_card)
}

fn render_setup_step(
    view: &GtuimView,
    p: GuiPalette,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let ob = &view.app.onboarding;
    let draft = &ob.draft;

    let numbers_val = draft.numbers.clone();
    let tabstop_val = draft.tabstop;

    let formatting_card = list_card()
        .debug_selector(|| "onboarding-pref-numbers".into())
        .p_3()
        .rounded(px(8.0))
        .bg(rgb(p.bg_surface))
        .border_1()
        .border_color(rgb(p.border_subtle))
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex_basis(px(260.0))
                .flex_grow()
                .min_w_0()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap_2()
                .child(
                    div()
                        .flex_basis(px(120.0))
                        .flex_grow()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_strong))
                                .child("Line Numbers"),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(11.0))
                                .text_color(rgb(p.text_muted))
                                .child("Gutter numbering style"),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap_1()
                        .children(
                            [("relative", "Relative"), ("absolute", "Absolute"), ("none", "Off")]
                                .into_iter()
                                .enumerate()
                                .map(|(idx, (val, label))| {
                                    let active = numbers_val == val;
                                    segment_btn(
                                        ("onboarding-num", idx),
                                        label,
                                        active,
                                        p,
                                        cx.listener(move |this, _, _, cx| {
                                            this.app.onboarding.draft.numbers = val.to_string();
                                            cx.notify();
                                        }),
                                    )
                                }),
                        ),
                ),
        )
        .child(
            div()
                .flex_basis(px(220.0))
                .flex_grow()
                .min_w_0()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap_2()
                .child(
                    div()
                        .flex_basis(px(100.0))
                        .flex_grow()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(p.text_strong))
                                .child("Tab Width"),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .text_size(px(11.0))
                                .text_color(rgb(p.text_muted))
                                .child("Indent column width"),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap_1()
                        .children([2u8, 4u8, 8u8].into_iter().map(|sz| {
                            let active = tabstop_val == sz;
                            segment_btn(
                                ("onboarding-tab", sz as usize),
                                format!("{sz}"),
                                active,
                                p,
                                cx.listener(move |this, _, _, cx| {
                                    this.app.onboarding.draft.tabstop = sz;
                                    cx.notify();
                                }),
                            )
                        })),
                ),
        );

    let toggles_grid = div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_wrap()
        .gap_2()
        .child(toggle_card(
            "onboarding-tog-clip",
            "System Clipboard",
            "Share copied and yanked text with other desktop apps.",
            draft.clipboard,
            p,
            cx.listener(|this, _, _, cx| {
                this.app.onboarding.draft.clipboard = !this.app.onboarding.draft.clipboard;
                cx.notify();
            }),
        ))
        .child(toggle_card(
            "onboarding-tog-ac",
            "Autocomplete",
            "Show completion popups as you type code.",
            draft.autocomplete,
            p,
            cx.listener(|this, _, _, cx| {
                this.app.onboarding.draft.autocomplete = !this.app.onboarding.draft.autocomplete;
                cx.notify();
            }),
        ))
        .child(toggle_card(
            "onboarding-tog-ai",
            "Autoindent",
            "Automatically match indentation on new lines.",
            draft.autoindent,
            p,
            cx.listener(|this, _, _, cx| {
                this.app.onboarding.draft.autoindent = !this.app.onboarding.draft.autoindent;
                cx.notify();
            }),
        ))
        .child(toggle_card(
            "onboarding-tog-nf",
            "Nerd Font Icons",
            "Display filetype and status glyphs in Neovim UI.",
            draft.nerd_fonts,
            p,
            cx.listener(|this, _, _, cx| {
                this.app.onboarding.draft.nerd_fonts = !this.app.onboarding.draft.nerd_fonts;
                cx.notify();
            }),
        ));

    let tools_grid = div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_wrap()
        .gap_2()
        .child(toggle_card(
            "onboarding-tog-mason",
            "Language Tools (Mason)",
            "Enable LSP servers, formatters, linters, and debuggers.",
            draft.mason_enabled,
            p,
            cx.listener(|this, _, _, cx| {
                this.app.onboarding.draft.mason_enabled = !this.app.onboarding.draft.mason_enabled;
                cx.notify();
            }),
        ))
        .child(toggle_card(
            "onboarding-tog-lazy",
            "Plugin Manager (Lazy.nvim)",
            "Load bundled and user Neovim plugins at startup.",
            draft.lazy_enabled,
            p,
            cx.listener(|this, _, _, cx| {
                this.app.onboarding.draft.lazy_enabled = !this.app.onboarding.draft.lazy_enabled;
                cx.notify();
            }),
        ));

    let quick_launch = list_card()
        .p_3()
        .rounded(px(8.0))
        .bg(rgb(p.bg_surface))
        .border_1()
        .border_color(rgb(p.border_subtle))
        .flex()
        .items_center()
        .justify_between()
        .flex_wrap()
        .gap_3()
        .child(
            div()
                .flex_basis(px(240.0))
                .flex_grow()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(p.text_strong))
                        .child("Save & Jump Straight Into Tools"),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(11.0))
                        .text_color(rgb(p.text_muted))
                        .child("Install language servers for your stack or browse themes & extensions."),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .child(modal_button(
                    "onboarding-launch-mason",
                    "Language Tools (Mason)",
                    false,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| {
                        this.app.onboarding.open_mason_after = true;
                        this.finish_onboarding(cx);
                    }),
                ))
                .child(modal_button(
                    "onboarding-launch-shop",
                    "Plugin Center",
                    false,
                    false,
                    p,
                    cx.listener(|this, _, _, cx| {
                        this.app.onboarding.open_shop_after = true;
                        this.finish_onboarding(cx);
                    }),
                )),
        );

    let theme_label = view
        .app
        .settings_widget
        .theme_options()
        .into_iter()
        .find(|(_, id)| *id == draft.theme)
        .map(|(label, _)| label)
        .unwrap_or_else(|| draft.theme.clone());
    let mode_label = match draft.mode.as_str() {
        "normal" => "Normal (Vim)",
        "zen" => "Zen",
        _ => "IDE (Default)",
    };

    let summary_card = list_card()
        .p_3()
        .rounded(px(8.0))
        .bg(rgb(blend_u32(p.bg_surface, p.accent, 8)))
        .border_1()
        .border_color(rgb(blend_u32(p.border, p.accent, 35)))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_size(px(12.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(p.text_strong))
                .child("Summary of Your Workspace Setup"),
        )
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(summary_pill("Theme", theme_label, p))
                .child(summary_pill("Editing", mode_label.to_string(), p))
                .child(summary_pill("Controls", ob.preset.label().to_string(), p))
                .child(summary_pill("Font", format!("{} px", draft.font_size), p))
                .child(summary_pill("Numbers", draft.numbers.clone(), p))
                .child(summary_pill("Tab", format!("{} cols", draft.tabstop), p)),
        );

    scroll_column("onboarding-body")
        .p_5()
        .flex()
        .flex_col()
        .gap_4()
        .child(section_heading(
            "Editor Essentials",
            "Fine-tune line numbers, indentation, clipboard, and completion behavior.",
            p,
        ))
        .child(formatting_card)
        .child(toggles_grid)
        .child(section_heading(
            "Extensions & Language Support",
            "Keep Mason and Lazy enabled for full IDE intelligence, or disable for a minimal setup.",
            p,
        ))
        .child(tools_grid)
        .child(quick_launch)
        .child(summary_card)
}

fn render_footer(
    view: &GtuimView,
    p: GuiPalette,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let ob = &view.app.onboarding;
    let step_num = ob.step.index() + 1;
    let total = OnboardingStep::ALL.len();
    let is_first = ob.step == OnboardingStep::Theme;
    let is_last = ob.step == OnboardingStep::Setup;

    let hint = if !ob.status_msg.is_empty() {
        ob.status_msg.clone()
    } else {
        format!("Step {step_num} of {total} · Reopen anytime from Help → Welcome & Setup")
    };

    div()
        .debug_selector(|| "onboarding-footer".into())
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .px_5()
        .py_3()
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
                .flex_basis(px(220.0))
                .flex_grow()
                .min_w_0()
                .flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .child(if is_first {
                    modal_button(
                        "onboarding-skip",
                        if ob.is_first_run { "Skip for now" } else { "Cancel" },
                        false,
                        false,
                        p,
                        cx.listener(|this, _, _, cx| {
                            this.cancel_onboarding(cx);
                        }),
                    )
                    .into_any_element()
                } else {
                    modal_button(
                        "onboarding-back",
                        "← Back",
                        false,
                        false,
                        p,
                        cx.listener(|this, _, _, cx| {
                            this.app.onboarding.prev_step();
                            cx.notify();
                        }),
                    )
                    .into_any_element()
                })
                .child(
                    div()
                        .flex_basis(px(160.0))
                        .flex_grow()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(11.0))
                        .text_color(rgb(p.text_muted))
                        .child(hint),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .flex_wrap()
                .justify_end()
                .gap_2()
                .when(ob.has_changes(), |d| {
                    d.child(modal_button(
                        "onboarding-reset",
                        "Defaults",
                        false,
                        false,
                        p,
                        cx.listener(|this, _, _, cx| {
                            this.app.onboarding.reset_to_defaults();
                            let theme = this.app.onboarding.draft.theme.clone();
                            this.font_size = this.app.onboarding.draft.font_size as f32;
                            this.preview_onboarding_theme(&theme, cx);
                        }),
                    ))
                })
                .when(!is_last, |d| {
                    d.child(modal_button(
                        "onboarding-apply-early",
                        "Save & Finish",
                        false,
                        false,
                        p,
                        cx.listener(|this, _, _, cx| {
                            this.finish_onboarding(cx);
                        }),
                    ))
                })
                .child(
                    div()
                        .id("onboarding-primary-btn")
                        .debug_selector(|| "onboarding-primary-btn".into())
                        .flex_shrink_0()
                        .whitespace_nowrap()
                        .px_4()
                        .py(px(6.0))
                        .rounded(px(6.0))
                        .bg(rgb(p.accent))
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(p.accent_fg))
                        .hover(|s| s.opacity(0.9))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap_2()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if is_last {
                                this.finish_onboarding(cx);
                            } else {
                                this.app.onboarding.next_step();
                                cx.notify();
                            }
                        }))
                        .child(if is_last {
                            if ob.is_first_run {
                                "Start Coding"
                            } else {
                                "Save & Close"
                            }
                        } else {
                            "Next"
                        })
                        .child(icon(
                            if is_last { "check" } else { "arrow-right" },
                            13.0,
                            p.accent_fg,
                        )),
                ),
        )
}

fn section_heading(title: &'static str, subtitle: &'static str, p: GuiPalette) -> impl IntoElement {
    div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .w_full()
                .min_w_0()
                .whitespace_normal()
                .text_size(px(13.5))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(p.text_strong))
                .child(title),
        )
        .child(
            div()
                .w_full()
                .min_w_0()
                .whitespace_normal()
                .text_size(px(11.5))
                .text_color(rgb(p.text_muted))
                .child(subtitle),
        )
}

fn pill_badge(label: impl Into<SharedString>, bg: u32, fg: u32) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .whitespace_nowrap()
        .px(px(7.0))
        .py(px(2.0))
        .rounded(px(99.0))
        .bg(rgb(bg))
        .text_size(px(10.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(fg))
        .child(label.into())
}

fn kbd_pill(label: impl Into<SharedString>, p: GuiPalette) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .whitespace_nowrap()
        .px(px(6.0))
        .py(px(2.0))
        .rounded(px(4.0))
        .bg(rgb(p.bg_elevated))
        .border_1()
        .border_color(rgb(p.border))
        .text_size(px(10.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(p.text_strong))
        .child(label.into())
}

fn stepper_btn(
    id: &'static str,
    label: &'static str,
    p: GuiPalette,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_shrink_0()
        .whitespace_nowrap()
        .size(px(26.0))
        .rounded(px(5.0))
        .bg(rgb(p.bg_elevated))
        .border_1()
        .border_color(rgb(p.border))
        .hover(|s| s.bg(rgb(p.bg_hover)).border_color(rgb(p.accent)))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(13.0))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(p.text_strong))
        .on_click(handler)
        .child(label)
}

fn segment_btn(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    p: GuiPalette,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_shrink_0()
        .whitespace_nowrap()
        .px(px(10.0))
        .py(px(4.0))
        .rounded(px(5.0))
        .bg(rgb(if active { p.accent } else { p.bg_elevated }))
        .border_1()
        .border_color(rgb(if active { p.accent } else { p.border }))
        .hover(|s| s.opacity(0.9))
        .cursor_pointer()
        .text_size(px(11.5))
        .font_weight(if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::MEDIUM
        })
        .text_color(rgb(if active { p.accent_fg } else { p.text }))
        .on_click(handler)
        .child(label.into())
}

fn toggle_card(
    id: &'static str,
    title: &'static str,
    desc: &'static str,
    enabled: bool,
    p: GuiPalette,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_basis(px(260.0))
        .flex_grow()
        .min_w_0()
        .p_3()
        .rounded(px(8.0))
        .bg(rgb(p.bg_surface))
        .border_1()
        .border_color(rgb(if enabled {
            blend_u32(p.border, p.accent, 35)
        } else {
            p.border_subtle
        }))
        .hover(|s| s.bg(rgb(p.bg_hover)))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_between()
        .flex_wrap()
        .gap_2()
        .on_click(handler)
        .child(
            div()
                .flex_basis(px(170.0))
                .flex_grow()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(p.text_strong))
                        .child(title),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(11.0))
                        .text_color(rgb(p.text_muted))
                        .child(desc),
                ),
        )
        .child(pill_badge(
            if enabled { "ON" } else { "OFF" },
            if enabled { p.accent } else { p.bg_elevated },
            if enabled { p.accent_fg } else { p.text_muted },
        ))
}

fn summary_pill(label: &'static str, value: String, p: GuiPalette) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .px(px(9.0))
        .py(px(4.0))
        .rounded(px(6.0))
        .bg(rgb(p.bg_panel))
        .border_1()
        .border_color(rgb(p.border_subtle))
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(
            div()
                .whitespace_nowrap()
                .text_size(px(11.0))
                .text_color(rgb(p.text_muted))
                .child(label),
        )
        .child(
            div()
                .whitespace_nowrap()
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(p.text_strong))
                .child(value),
        )
}
