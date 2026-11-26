use gpui::{prelude::*, *};
use super::{icons::gui_palette, GtuimView};
use super::modals::modal_button;
use crate::tui::widgets::extension_shop::{ExtensionShop, PluginInfo};

pub fn lifecycle_label(plugin: &PluginInfo) -> String {
    let state = match plugin.lifecycle.as_str() {
        "not_installed" => "Not installed",
        "downloaded" => "Downloaded",
        "dependencies_ready" => "Dependencies ready",
        "initialized" => "Initialized",
        "verified" => "Verified",
        "needs_attention" => "Needs attention",
        "pending_restart" => "Restart required",
        _ => ExtensionShop::plugin_status(plugin),
    };
    format!("{} · {}", if plugin.supported { "Supported recipe" } else { "Community / best effort" }, state)
}

pub fn render_health(view: &GtuimView, cx: &mut Context<GtuimView>) -> AnyElement {
    let Some(plugin) = view.app.extension_shop.selected_plugin() else { return div().into_any_element(); };
    let p = gui_palette(&view.app.theme);
    let installed = plugin.installed && !ExtensionShop::is_pending_removal(plugin);
    let recipe = crate::tui::widgets::plugin_center::recipe_for(&plugin.full_name);
    let version = if !plugin.pinned_commit.is_empty() {
        format!("Version policy ({}): {}", plugin.pin_source, plugin.pinned_commit)
    } else if !plugin.installed_commit.is_empty() {
        format!("Installed revision: {}", plugin.installed_commit)
    } else { "Revision will be recorded after installation".to_string() };
    let mut panel = div().debug_selector(|| "plugin-health-panel".into())
        .w_full().min_w_0().flex_shrink_0().whitespace_normal()
        .mt_3().p_3().rounded(px(6.0)).bg(rgb(p.bg_surface))
        .flex().flex_col().gap_2()
        .child(div().text_size(px(11.0)).text_color(rgb(p.text_muted)).child(version))
        .when(!plugin.pinned_commit.is_empty() && !plugin.installed_commit.is_empty(), |d| {
            d.child(div().text_size(px(11.0)).text_color(rgb(p.text_muted))
                .child(format!("Installed revision: {}", plugin.installed_commit)))
        })
        .child(div().text_size(px(12.0)).text_color(rgb(if plugin.lifecycle == "needs_attention" { p.warning } else { p.text }))
            .child(if plugin.health_message.is_empty() {
                "No runtime verification recorded. Restart Neovim after changing plugins; refresh health afterward.".to_string()
            } else { plugin.health_message.clone() }));
    if let Some(recipe) = recipe {
        panel = panel.child(div().text_size(px(11.0)).text_color(rgb(p.text_muted))
            .child("Lifecycle: Downloaded → Dependencies ready → Initialized → Verified"));
        if !recipe.requirements.is_empty() {
            panel = panel.child(div().text_size(px(11.0)).child(format!("Requirements: {}",
                recipe.requirements.iter().map(|r| format!("{} ({})", r.label, r.command)).collect::<Vec<_>>().join(", "))));
        }
        panel = panel.child(div().text_size(px(11.0)).text_color(rgb(p.text_muted))
            .child(format!("Verification: {}", recipe.verification)));
    }
    if !plugin.missing_requirements.is_empty() {
        panel = panel.child(div().text_size(px(12.0)).text_color(rgb(p.warning))
            .child(format!("Missing: {}. Install these tools, then refresh health.", plugin.missing_requirements.join(", "))));
    }
    if installed {
        panel = panel.child(div().w_full().min_w_0().flex_shrink_0().flex().flex_wrap().gap_2()
            .child(modal_button("center-health", "Refresh health (V)", false, false, p, cx.listener(|this, _, _, cx| {
                this.app.extension_shop.selected_backend_action("verify"); this.after_widget_change(cx); cx.notify();
            })))
            .child(modal_button("center-lua", "Advanced Lua (L)", false, false, p, cx.listener(|this, _, _, cx| {
                this.app.extension_shop.open_selected_lua(); this.after_widget_change(cx); cx.notify();
            })))
            .child(modal_button("center-pin", if plugin.pin_source == "user" { "Remove user pin" } else { "Pin installed revision" },
                false, false, p, cx.listener(|this, _, _, cx| {
                    let action = if this.app.extension_shop.selected_plugin().is_some_and(|p| p.pin_source == "user") { "unpin" } else { "pin" };
                    this.app.extension_shop.selected_backend_action(action); this.after_widget_change(cx); cx.notify();
                }))));
    }
    panel.into_any_element()
}

pub fn render_form(view: &GtuimView, cx: &mut Context<GtuimView>) -> AnyElement {
    let form = view.app.extension_shop.recipe_form.as_ref().expect("recipe form open");
    let p = gui_palette(&view.app.theme);
    let mut fields = super::modal_layout::scroll_column("recipe-fields")
        .flex().flex_col().gap_2();
    for (index, field) in form.recipe.fields.iter().enumerate() {
        let value = form.values.get(&field.key).unwrap_or(&field.default);
        let text = value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string());
        let selected = index == form.selected;
        let mut control = div().debug_selector(move || format!("recipe-control-{}", index))
            .max_w(px(260.0)).flex().flex_wrap().items_center().gap_2();
        if field.kind == "integer" || field.kind == "enum" {
            control = control.child(div().id(("recipe-minus", index)).flex_shrink_0().whitespace_nowrap().px_2().py_1()
                .rounded(px(4.0)).bg(rgb(p.bg_elevated)).cursor_pointer()
                .child(if field.kind == "integer" { format!("−{}", field.step()) } else { "−".into() })
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if let Some(form) = this.app.extension_shop.recipe_form.as_mut() { form.adjust(index, -1); }
                    cx.notify();
                })));
        }
        control = control.child(div().debug_selector(move || format!("recipe-value-{}", index))
            .max_w(px(180.0)).whitespace_normal().text_size(px(12.0)).text_color(rgb(p.text_strong))
            .child(if field.kind == "boolean" {
                if value.as_bool().unwrap_or(false) { "✓ On".to_string() } else { "○ Off".to_string() }
            } else if field.kind == "string" && selected { format!("{}▏", text) } else { text }));
        if field.kind != "string" {
            control = control.child(div().id(("recipe-plus", index)).flex_shrink_0().whitespace_nowrap().px_2().py_1()
                .rounded(px(4.0)).bg(rgb(p.bg_elevated)).cursor_pointer()
                .child(if field.kind == "boolean" { "Toggle".to_string() }
                    else if field.kind == "integer" { format!("+{}", field.step()) } else { "+".into() })
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if let Some(form) = this.app.extension_shop.recipe_form.as_mut() { form.adjust(index, 1); }
                    cx.notify();
                })));
        }
        fields = fields.child(super::modal_layout::list_card().id(("recipe-field", index))
            .debug_selector(move || format!("recipe-field-{}", index)).p_3().rounded(px(6.0))
            .border_1().border_color(rgb(if selected { p.accent } else { p.border_subtle }))
            .bg(rgb(p.bg_surface)).flex().flex_col().gap_1().cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(form) = this.app.extension_shop.recipe_form.as_mut() { form.select(index); }
                cx.notify();
            }))
            .child(div().w_full().min_w_0().flex().flex_wrap().gap_2().items_center().justify_between()
                .child(div().debug_selector(move || format!("recipe-label-{}", index))
                    .flex_basis(px(160.0)).flex_grow().min_w_0().whitespace_normal()
                    .text_size(px(12.0)).child(field.label.clone())).child(control))
            .child(div().w_full().min_w_0().whitespace_normal().text_size(px(10.5)).text_color(rgb(p.text_muted)).child(field.key.clone())));
    }
    let mut panel = div().w_full().flex_1().min_w_0().min_h_0().whitespace_normal().p_4().flex().flex_col().gap_3()
        .child(div().text_size(px(17.0)).font_weight(FontWeight::BOLD).child(format!("Configure {}", form.recipe.name)))
        .child(div().text_size(px(11.5)).text_color(rgb(p.text_muted))
            .child("Options are separate from your Lua; explicit Lua overrides take precedence. Restart Neovim to apply."))
        .child(fields)
        .when(!form.error.is_empty(), |d| d.child(div().text_size(px(12.0)).text_color(rgb(p.error)).child(form.error.clone())))
        .child(div().text_size(px(10.5)).text_color(rgb(p.text_muted))
            .child("Tab / ↑↓ select · ←→ adjust · type to edit text · Ctrl+S save · Esc cancel"));
    if form.discard_confirm {
        panel = panel.child(div().w_full().min_w_0().flex_shrink_0().p_3().rounded(px(5.0)).bg(rgb(p.bg_elevated)).flex().flex_col().gap_2()
            .child("Discard unsaved recipe options?")
            .child(div().w_full().min_w_0().flex_shrink_0().flex().flex_wrap().gap_2()
                .child(modal_button("recipe-discard", "Discard (Y)", false, true, p, cx.listener(|this, _, _, cx| {
                    this.app.extension_shop.recipe_form = None; cx.notify();
                })))
                .child(modal_button("recipe-keep", "Keep editing (N)", true, false, p, cx.listener(|this, _, _, cx| {
                    if let Some(form) = this.app.extension_shop.recipe_form.as_mut() { form.discard_confirm = false; }
                    cx.notify();
                })))));
    } else {
        panel = panel.child(div().debug_selector(|| "recipe-actions".into())
            .w_full().min_w_0().flex_shrink_0().flex().flex_wrap().gap_2()
            .child(modal_button("recipe-save", "Save options", true, false, p, cx.listener(|this, _, _, cx| {
                this.app.extension_shop.save_recipe_form(); this.after_widget_change(cx); cx.notify();
            })))
            .child(modal_button("recipe-reset", "Use defaults", false, false, p, cx.listener(|this, _, _, cx| {
                if let Some(form) = this.app.extension_shop.recipe_form.as_mut() { form.reset_defaults(); }
                cx.notify();
            })))
            .child(modal_button("recipe-cancel", "Cancel", false, false, p, cx.listener(|this, _, _, cx| {
                this.app.extension_shop.close_recipe_form(); cx.notify();
            }))));
    }
    panel.into_any_element()
}
