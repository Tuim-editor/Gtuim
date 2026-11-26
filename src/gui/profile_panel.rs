//! Clickable local profile surface, deliberately independent of editor buffers.
#[cfg(test)]
#[path = "profile_review_tests.rs"]
mod review_tests;
#[cfg(test)]
#[path = "profile_panel_tests.rs"]
mod tests;
use super::{
    icons::gui_palette,
    profiles::{self, AVATARS},
    GtuimView,
};
use crate::tui::theme::{blend_u32, readable_u32};
use crate::tui::{
    app::{App as TuimApp, Mode},
    widgets::settings::{SettingsConfig, SettingsWidget},
};
use gpui::{prelude::*, *};
use rmpv::Value;
use std::collections::BTreeMap;

pub(super) fn panel_width(viewport_width: f32, practice_enabled: bool) -> f32 {
    (viewport_width - 12.0).clamp(180.0, if practice_enabled { 640.0 } else { 340.0 })
}
fn calendar_color(p: super::icons::GuiPalette, count: u64) -> u32 {
    let strength = match count {
        0 => 12,
        1..=4 => 40,
        5..=19 => 60,
        20..=49 => 80,
        _ => 100,
    };
    blend_u32(p.bg_surface, p.success, strength)
}

pub(super) struct PracticeHost {
    panel: Entity<super::practice::PracticePanel>,
    _subscription: Subscription,
}

pub(super) fn refresh_practice_day(host: &PracticeHost, cx: &mut Context<GtuimView>) {
    host.panel.update(cx, |panel, cx| panel.refresh_daily(cx));
}

pub(super) fn load_profile_settings(
    app: &mut TuimApp,
    profiles: &profiles::Profiles,
) -> Result<(), String> {
    let path = profiles.settings_path().to_string_lossy().into_owned();
    let config = SettingsConfig::load(&path)?;
    app.editor_rpc.call(
        "nvim_exec_lua",
        vec![
            Value::from("vim.g.tuim_settings_path = ..."),
            Value::Array(vec![Value::from(path.clone())]),
        ],
    )?;
    app.mode = match config.mode.as_str() {
        "zen" => Mode::Zen,
        "normal" => Mode::Normal,
        _ => Mode::Ide,
    };
    app.workspace.tab_bar_visible = config.show_tab_bar;
    app.workspace.sidebar_visible = app.mode != Mode::Zen;
    if app.mode == Mode::Zen {
        app.workspace.drawer_visible = false;
    }
    app.settings_widget = SettingsWidget::new(&path, config);
    // Plugins and runtime remain shared; only the settings file is profile-owned.
    app.settings_widget.data_dir = app.data_dir.clone();
    Ok(())
}

impl GtuimView {
    pub(super) fn flush_practice_drafts(&mut self, cx: &mut App) {
        for host in self.practice_panels.values() {
            host.panel.update(cx, |panel, _| panel.flush_pending_save());
        }
    }
    pub(super) fn set_profile_panel_open(
        &mut self,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !open {
            self.flush_practice_drafts(cx);
        }
        self.profiles.open = open && self.app.mode != Mode::Zen;
        self.profiles.editing_name = false;
        self.app.menu_bar.open_menu = None;
        window.focus(&self.root_focus);
        self.sync_practice_widget(window, cx);
        cx.notify();
    }
    pub(super) fn set_profile_practice_enabled(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if enabled {
            self.profiles.enable_practice()?;
        } else {
            self.flush_practice_drafts(cx);
            self.profiles.disable_practice()?;
            if let Some(host) = self.practice_panels.get(&self.profiles.active().id) {
                host.panel
                    .update(cx, |panel, cx| panel.cancel_submission(cx));
            }
        }
        window.focus(&self.root_focus);
        self.sync_practice_widget(window, cx);
        cx.notify();
        Ok(())
    }
    pub(super) fn sync_profile_typography(&mut self) {
        self.app
            .settings_widget
            .set_available_fonts(self.available_fonts.clone());
        self.sync_font_from_settings();
    }
    pub(super) fn sync_practice_widget(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.app.mode == Mode::Zen && self.profiles.open {
            self.flush_practice_drafts(cx);
            self.profiles.open = false;
            self.profiles.editing_name = false;
            window.focus(&self.root_focus);
        }
        if self.has_overlay() {
            self.profiles.editing_name = false;
            if self
                .practice_panels
                .values()
                .any(|host| host.panel.read(cx).focus_handle(cx).is_focused(window))
            {
                self.flush_practice_drafts(cx);
                window.focus(&self.root_focus);
            }
        }
        if !self.profiles.open || !self.profiles.active().practice_enabled || self.has_overlay() {
            return;
        }
        let id = self.profiles.active().id.clone();
        if !self.practice_panels.contains_key(&id) {
            let dir = self.profiles.practice_dir();
            let panel = cx.new(|cx| {
                let mut panel = super::practice::PracticePanel::new(dir, cx);
                panel.set_show_close_button(false, cx);
                panel
            });
            let subscription = cx.subscribe_in(&panel, window, |this, _, event, window, cx| {
                if matches!(event, super::practice::PracticeEvent::CloseRequested) {
                    this.set_profile_panel_open(false, window, cx);
                }
                cx.notify();
            });
            self.practice_panels.insert(
                id.clone(),
                PracticeHost {
                    panel,
                    _subscription: subscription,
                },
            );
        }
        let panel = self.practice_panels[&id].panel.clone();
        panel.update(cx, |panel, cx| {
            panel.sync_appearance(
                self.app.theme.clone(),
                self.mono_font.clone(),
                self.ui_font.clone(),
                self.font_size,
                cx,
            );
            panel.refresh_daily(cx);
        });
    }

    pub(super) fn choose_profile_picture(&mut self, cx: &mut Context<Self>) {
        let selected_profile = self.profiles.active().id.clone();
        let directory = self.profiles.data_dir();
        let selection = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose profile picture (PNG, JPEG or WebP)".into()),
        });
        cx.spawn(async move |view, cx| {
            let result = match selection.await {
                Ok(Ok(Some(paths))) => {
                    let Some(path) = paths.into_iter().next() else {
                        return;
                    };
                    cx.background_executor()
                        .spawn(async move { import_picture(&path, &directory) })
                        .await
                }
                Ok(Ok(None)) => return,
                Ok(Err(e)) => Err(format!("Picture picker: {e}")),
                Err(e) => Err(format!("Picture picker: {e}")),
            };
            let _ = view.update(cx, |this, cx| {
                match result.and_then(|name| this.profiles.set_avatar_for(&selected_profile, name))
                {
                    Ok(()) => this.profiles.error = None,
                    Err(e) => this.profiles.error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn switch_profile(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.profiles.active().id == id {
            return Ok(());
        }
        self.flush_practice_drafts(cx);
        // A synchronous barrier puts all earlier edit notifications in the queue.
        self.app
            .editor_rpc
            .call("nvim_eval", vec![Value::from("1")])?;
        while let Some(msg) = self.app.editor_rpc.next_message() {
            self.handle_rpc_with_gui_sync(msg);
        }
        let old = self.profiles.active().id.clone();
        self.profiles.switch(id)?;
        if let Err(e) = load_profile_settings(&mut self.app, &self.profiles) {
            let _ = self.profiles.switch(&old);
            return Err(e);
        }
        self.sync_profile_typography();
        let _ = self.app.editor_rpc.notify(
            "nvim_exec_lua",
            vec![
                Value::from("if _G.tuim_load_settings then _G.tuim_load_settings() end"),
                Value::Array(vec![]),
            ],
        );
        self.app.editor_rpc.flush_writes()?;
        self.app.workspace.tab_bar_visible = self.app.settings_widget.config.show_tab_bar;
        self.profiles.error = None;
        Ok(())
    }
}

fn import_picture(path: &std::path::Path, directory: &std::path::Path) -> Result<String, String> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 10 * 1024 * 1024 {
        return Err("Choose an image smaller than 10 MiB".into());
    }
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP)
    ) {
        return Err("Choose a PNG, JPEG or WebP image".into());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let picture = reader
        .decode()
        .map_err(|e| format!("Invalid profile picture: {e}"))?
        .thumbnail(256, 256);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let name = format!("avatar-{stamp}.png");
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    picture
        .save_with_format(directory.join(&name), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(name)
}

pub(super) fn handle_key(
    view: &mut GtuimView,
    ev: &KeyDownEvent,
    cx: &mut Context<GtuimView>,
) -> bool {
    if view.app.mode == Mode::Zen {
        view.profiles.open = false;
        view.profiles.editing_name = false;
        return false;
    }
    if view.has_overlay() {
        view.profiles.editing_name = false;
        return false;
    }
    if !view.profiles.open {
        return false;
    }
    let key = ev.keystroke.key.as_str();
    if view.profiles.editing_name {
        match key {
            "escape" => view.profiles.editing_name = false,
            "enter" => {
                let name = view.profiles.name_draft.clone();
                match view.profiles.rename(&name) {
                    Ok(()) => {
                        view.profiles.editing_name = false;
                        view.profiles.error = None;
                    }
                    Err(e) => view.profiles.error = Some(e),
                }
            }
            "backspace" => {
                view.profiles.name_draft.pop();
            }
            _ => {
                if !ev.keystroke.modifiers.control
                    && !ev.keystroke.modifiers.platform
                    && !ev.keystroke.modifiers.alt
                {
                    let text = ev
                        .keystroke
                        .key_char
                        .as_deref()
                        .or_else(|| (key == "space").then_some(" "));
                    if let Some(text) = text.filter(|s| !s.chars().any(char::is_control)) {
                        if view.profiles.name_draft.chars().count() + text.chars().count() <= 48 {
                            view.profiles.name_draft.push_str(text);
                        }
                    }
                }
            }
        }
        cx.stop_propagation();
        cx.notify();
        return true;
    }
    if key == "escape" {
        view.flush_practice_drafts(cx);
        view.profiles.open = false;
        cx.stop_propagation();
        cx.notify();
        return true;
    }
    false
}

pub(super) fn render(
    view: &GtuimView,
    layout: super::EffectiveWorkspaceLayout,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let p = gui_palette(&view.app.theme);
    let profile = view.profiles.active();
    let panel_width = panel_width(layout.viewport_w, profile.practice_enabled);
    let mut panel = div()
        .w_full()
        .flex_shrink_0()
        .p_4()
        .flex()
        .flex_col()
        .gap_3()
        .text_size(px(12.0))
        .child(
            div()
                .flex()
                .justify_between()
                .items_center()
                .child(div().font_weight(FontWeight::BOLD).child("LOCAL PROFILES"))
                .child(
                    div()
                        .id("profile-close")
                        .debug_selector(|| "profile-close".into())
                        .cursor_pointer()
                        .px_2()
                        .child("×")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.set_profile_panel_open(false, window, cx)
                        })),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .id("profile-avatar")
                        .debug_selector(|| "profile-avatar".into())
                        .cursor_pointer()
                        .text_size(px(32.0))
                        .text_color(rgb(readable_u32(p.success, p.bg_surface, 3.0)))
                        .when_some(view.profiles.avatar_path(), |d, path| {
                            d.child(
                                img(path)
                                    .w(px(42.0))
                                    .h(px(42.0))
                                    .object_fit(ObjectFit::Contain),
                            )
                        })
                        .when(view.profiles.avatar_path().is_none(), |d| {
                            d.child(AVATARS[profile.avatar % AVATARS.len()])
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.choose_profile_picture(cx))),
                )
                .child(
                    div()
                        .id("profile-name")
                        .debug_selector(|| "profile-name".into())
                        .cursor_pointer()
                        .min_w_0()
                        .flex_1()
                        .whitespace_normal()
                        .text_size(px(18.0))
                        .child(if view.profiles.editing_name {
                            format!("{}│", view.profiles.name_draft)
                        } else {
                            profile.display_name.clone()
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.profiles.name_draft = this.profiles.active().display_name.clone();
                            this.profiles.editing_name = true;
                            window.focus(&this.root_focus);
                            cx.notify();
                        })),
                ),
        )
        .child(
            div()
                .text_size(px(10.0))
                .text_color(rgb(p.text_muted))
                .child(if view.profiles.editing_name {
                    "Type a name · Enter saves · Esc cancels"
                } else {
                    "Click name to rename · Click picture to choose image"
                }),
        );
    panel = panel.child(
        div()
            .id("profile-avatar-fallback")
            .cursor_pointer()
            .text_color(rgb(p.text_muted))
            .child("Use / cycle built-in avatar")
            .on_click(cx.listener(|this, _, _, cx| {
                if let Err(e) = this.profiles.next_avatar() {
                    this.profiles.error = Some(e);
                }
                cx.notify();
            })),
    );
    for entry in view.profiles.list() {
        let id = entry.id.clone();
        let active = id == profile.id;
        panel = panel.child(
            div()
                .id(SharedString::from(format!("profile-switch-{id}")))
                .cursor_pointer()
                .px_2()
                .py_1()
                .rounded(px(4.0))
                .when(active, |d| d.bg(rgb(p.bg_elevated)))
                .hover(|s| s.bg(rgb(p.bg_hover)))
                .child(format!(
                    "{}  {}{}",
                    AVATARS[entry.avatar % AVATARS.len()],
                    entry.display_name,
                    if active { "  ✓" } else { "" }
                ))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Err(e) = this.switch_profile(&id, cx) {
                        this.profiles.error = Some(e);
                    }
                    window.focus(&this.root_focus);
                    this.sync_practice_widget(window, cx);
                    cx.notify();
                })),
        );
    }
    panel = panel.child(
        div()
            .id("profile-create")
            .debug_selector(|| "profile-create".into())
            .cursor_pointer()
            .py_2()
            .text_color(rgb(p.accent))
            .child("+ Create local profile")
            .on_click(cx.listener(|this, _, window, cx| {
                let result = this
                    .profiles
                    .create()
                    .and_then(|id| this.switch_profile(&id, cx));
                match result {
                    Ok(()) => {
                        this.profiles.name_draft = this.profiles.active().display_name.clone();
                        this.profiles.editing_name = true;
                        window.focus(&this.root_focus);
                    }
                    Err(e) => this.profiles.error = Some(e),
                }
                this.sync_practice_widget(window, cx);
                cx.notify();
            })),
    );
    if let Some(error) = &view.profiles.error {
        panel = panel.child(
            div()
                .text_color(rgb(readable_u32(p.error, p.bg_surface, 3.0)))
                .whitespace_normal()
                .child(error.clone()),
        );
    }
    let edits: u64 = profile.activity.values().map(|d| d.edits).sum();
    let saves: u64 = profile.activity.values().map(|d| d.saves).sum();
    let mut languages: BTreeMap<String, u64> = BTreeMap::new();
    for day in profile.activity.values() {
        for (lang, count) in &day.languages {
            *languages.entry(lang.clone()).or_default() += count;
        }
    }
    let mut languages: Vec<_> = languages.into_iter().collect();
    languages.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    panel = panel.child(div().border_t_1().border_color(rgb(p.border)).pt_3().font_weight(FontWeight::BOLD).child("CODE ACTIVITY"))
        .child(format!("{edits} edit batches · {saves} saves · {} active days", profile.activity.len()))
        .child(div().text_size(px(10.0)).text_color(rgb(p.text_muted)).whitespace_normal().child("Local editor changes, not Git commits or lines of code. Dates use your local calendar day."));
    // 26 weeks, Monday-first columns, with future days omitted.
    let today = profiles::today();
    let end_week_start = today - (today + 3).rem_euclid(7);
    let start = end_week_start - 25 * 7;
    let mut calendar = div()
        .debug_selector(|| "profile-calendar".into())
        .flex()
        .gap(px(2.0));
    let day_size = ((panel_width - 32.0 - 50.0) / 26.0).clamp(2.0, 8.0);
    for week in 0..26 {
        let mut column = div().flex().flex_col().gap(px(2.0));
        for weekday in 0..7 {
            let day = start + week * 7 + weekday;
            if day > today {
                continue;
            }
            let date = profiles::day_key(day);
            let count = profile
                .activity
                .get(&date)
                .map(|d| d.edits + d.saves)
                .unwrap_or(0);
            let color = calendar_color(p, count);
            column = column.child(
                div()
                    .id(SharedString::from(format!("activity-{date}")))
                    .w(px(day_size))
                    .h(px(day_size))
                    .rounded(px(2.0))
                    .bg(rgb(color))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.profiles.selected_day = Some(date.clone());
                        cx.notify();
                    })),
            );
        }
        calendar = calendar.child(column);
    }
    panel = panel
        .child(
            div()
                .text_color(rgb(p.text_muted))
                .child("Last 26 weeks · click a day"),
        )
        .child(calendar);
    let date = view
        .profiles
        .selected_day
        .clone()
        .unwrap_or_else(|| profiles::day_key(today));
    let day = profile.activity.get(&date).cloned().unwrap_or_default();
    panel = panel
        .child(
            div()
                .debug_selector(|| "profile-day-detail".into())
                .child(format!("{date}: {} edits, {} saves", day.edits, day.saves)),
        )
        .child(div().font_weight(FontWeight::BOLD).child("LANGUAGES"));
    if languages.is_empty() {
        panel = panel.child(
            div()
                .text_color(rgb(p.text_muted))
                .child("Start editing a file to build your stats."),
        );
    }
    for (language, count) in languages {
        let percent = count * 100 / edits.max(1);
        panel = panel.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(format!("{language} · {percent}% · {count} edits"))
                .child(
                    div()
                        .h(px(3.0))
                        .w(relative(percent as f32 / 100.0))
                        .bg(rgb(p.success)),
                ),
        );
    }
    panel = panel.child(
        div()
            .border_t_1()
            .border_color(rgb(p.border))
            .pt_3()
            .font_weight(FontWeight::BOLD)
            .child("PRACTICE"),
    );
    if profile.practice_enabled {
        if let Some(host) = view.practice_panels.get(&profile.id) {
            panel = panel.child(
                div()
                    .debug_selector(|| "profile-practice-widget".into())
                    .w_full()
                    .h(px(680.0))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .child(host.panel.clone()),
            );
        }
        panel = panel.child(
            div()
                .id("profile-disable-practice")
                .debug_selector(|| "profile-disable-practice".into())
                .cursor_pointer()
                .text_color(rgb(p.text_muted))
                .child("Disable practice (keep progress)")
                .on_click(cx.listener(|this, _, window, cx| {
                    if let Err(e) = this.set_profile_practice_enabled(false, window, cx) {
                        this.profiles.error = Some(e);
                        cx.notify();
                    }
                })),
        );
    } else {
        panel = panel
            .child(
                div()
                    .text_color(rgb(p.text_muted))
                    .whitespace_normal()
                    .child("Optional daily practice. Enable it for this profile to get started."),
            )
            .child(
                div()
                    .id("profile-enable-practice")
                    .debug_selector(|| "profile-enable-practice".into())
                    .cursor_pointer()
                    .py_2()
                    .text_color(rgb(p.accent))
                    .child("Enable practice for this profile")
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Err(e) = this.set_profile_practice_enabled(true, window, cx) {
                            this.profiles.error = Some(e);
                            cx.notify();
                        }
                    })),
            );
    }
    div()
        .id("profile-panel")
        .debug_selector(|| "profile-panel".into())
        .absolute()
        .right_0()
        .top(px(layout.header_h))
        .bottom(px(layout.status_h))
        .w(px(panel_width))
        .overflow_y_scroll()
        .bg(rgb(p.bg_surface))
        .border_l_1()
        .border_color(rgb(p.border))
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .child(panel)
}
