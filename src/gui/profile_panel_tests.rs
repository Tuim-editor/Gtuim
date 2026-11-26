use super::{handle_key, import_picture, profiles};
use gpui::{px, size, KeyDownEvent, Keystroke, Modifiers, MouseButton, TestAppContext};

#[test]
fn local_picture_is_validated_resized_copied_and_persistent() {
    let root = std::env::temp_dir().join(format!(
        "gtuim-picture-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut profiles = profiles::Profiles::load(&root).unwrap();
    let source = root.join("source.png");
    image::RgbaImage::from_pixel(640, 480, image::Rgba([20, 200, 80, 255]))
        .save(&source)
        .unwrap();
    let name = import_picture(&source, &profiles.data_dir()).unwrap();
    profiles.set_avatar_for("default", name).unwrap();
    std::fs::remove_file(source).unwrap();
    let saved = profiles.avatar_path().expect("profile owns its image copy");
    let image = image::open(saved).unwrap();
    assert_eq!((image.width(), image.height()), (256, 192));
    drop(profiles);
    let mut profiles = profiles::Profiles::load(&root).unwrap();
    assert!(profiles.avatar_path().is_some());
    profiles.next_avatar().unwrap();
    assert!(profiles.avatar_path().is_none());
    let bad = root.join("bad.png");
    std::fs::write(&bad, b"not an image").unwrap();
    assert!(import_picture(&bad, &profiles.data_dir()).is_err());
    drop(profiles);
    std::fs::remove_dir_all(root).unwrap();
}

#[gpui::test]
fn profile_panel_is_clickable_and_guest_practice_is_opt_in(cx: &mut TestAppContext) {
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = crate::gui::modal_tests::fixture(cx);
        view.app.extension_shop.is_open = false;
        view
    });
    window.simulate_resize(size(px(1000.0), px(900.0)));
    window.run_until_parked();
    let button = window.debug_bounds("btn-profile").unwrap();
    window.simulate_mouse_down(button.center(), MouseButton::Left, Modifiers::default());
    window.simulate_mouse_up(button.center(), MouseButton::Left, Modifiers::default());
    window.run_until_parked();
    assert!(window.debug_bounds("profile-panel").is_some());
    assert!(window.debug_bounds("profile-calendar").is_some());
    assert!(window.debug_bounds("profile-enable-practice").is_some());
    view.read_with(window, |this, _| {
        assert!(!this.profiles.active().practice_enabled)
    });
    let name = window.debug_bounds("profile-name").unwrap();
    window.simulate_mouse_down(name.center(), MouseButton::Left, Modifiers::default());
    window.simulate_mouse_up(name.center(), MouseButton::Left, Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| assert!(this.profiles.editing_name));
    view.update(window, |this, cx| {
        this.profiles.name_draft = "Local Coder".into();
        let ev = KeyDownEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
            is_held: false,
        };
        assert!(handle_key(this, &ev, cx));
        assert_eq!(this.profiles.active().display_name, "Local Coder");
        cx.notify();
    });
    window.run_until_parked();
    let date = profiles::local_day_key();
    view.update(window, |this, cx| {
        this.profiles.selected_day = Some(date.clone());
        cx.notify();
    });
    window.run_until_parked();
    assert!(window.debug_bounds("profile-day-detail").is_some());
    assert_eq!(date, crate::gui::practice::LocalDate::today().to_string());
    assert!(
        window.debug_bounds("practice-panel").is_none(),
        "guest must not initialize practice"
    );
    let enable = window.debug_bounds("profile-enable-practice").unwrap();
    window.simulate_mouse_down(enable.center(), MouseButton::Left, Modifiers::default());
    window.simulate_mouse_up(enable.center(), MouseButton::Left, Modifiers::default());
    window.run_until_parked();
    assert!(window.debug_bounds("profile-practice-widget").is_some());
    let first_path = view.read_with(window, |this, cx| {
        let panel = &this.practice_panels[&this.profiles.active().id].panel;
        assert_eq!(panel.read(cx).profile_dir(), this.profiles.practice_dir());
        panel.read(cx).profile_dir().to_path_buf()
    });
    view.update(window, |this, cx| {
        // Switch only the storage selection here; the live-RPC switch has its own test.
        let id = this.profiles.create().unwrap();
        this.profiles.select_for_window(&id).unwrap();
        assert!(!this.profiles.active().practice_enabled);
        cx.notify();
    });
    window.run_until_parked();
    // GPUI 0.2.2 retains old debug selector bounds after unmounting; inspect host state.
    view.read_with(window, |this, _| {
        assert!(!this.profiles.active().practice_enabled);
        assert!(!this
            .practice_panels
            .contains_key(&this.profiles.active().id));
    });
    window.update(|window, cx| {
        view.update(cx, |this, cx| {
            this.profiles.enable_practice().unwrap();
            this.sync_practice_widget(window, cx);
            cx.notify();
        });
    });
    window.run_until_parked();
    view.read_with(window, |this, cx| {
        let panel = &this.practice_panels[&this.profiles.active().id].panel;
        assert_ne!(panel.read(cx).profile_dir(), first_path);
        assert_eq!(panel.read(cx).profile_dir(), this.profiles.practice_dir());
    });
}

#[gpui::test]
fn live_switch_preserves_unsaved_buffer_and_routes_settings_and_queued_activity(
    cx: &mut TestAppContext,
) {
    use crate::nvim::{process::NvimProcess, rpc::RpcClient};
    use crate::tui::widgets::settings::SettingsConfig;
    use rmpv::Value;
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = crate::gui::modal_tests::fixture(cx);
        view.app.extension_shop.is_open = false;
        view
    });
    view.update(window, |this, cx| {
        let home = this.profiles.data_dir().join("test-home");
        std::fs::create_dir_all(&home).unwrap();
        let mut env: std::collections::HashMap<String, String> = std::env::vars().collect();
        env.insert("HOME".into(), home.to_string_lossy().into_owned());
        for variable in ["XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"] {
            env.insert(variable.into(), home.join(variable).to_string_lossy().into_owned());
        }
        this.app.editor_rpc = RpcClient::new(NvimProcess::spawn(&env).expect("test requires nvim"));
        super::load_profile_settings(&mut this.app, &this.profiles).unwrap();
        // Execute the production settings loader, but not the plugin bootstrap.
        let lua = include_str!("../nvim/tuim_init.lua");
        let loader = lua.split_once("_G.tuim_load_settings = function()").unwrap().1
            .split_once("-- Count actual normal-file").unwrap().0;
        let activity = lua.split_once("-- Count actual normal-file change batches, not renders, cursor movement or duplicate save notices.\n").unwrap().1.split_once("local M = {}").unwrap().0;
        let script = format!("local function ide_startinsert() end; _G.tuim_enable_ide_mode=function()end; _G.tuim_disable_ide_mode=function()end; _G.tuim_apply_theme=function()end; _G.tuim_load_settings = function(){}\n{}", loader, activity);
        this.app.editor_rpc.call("nvim_exec_lua", vec![Value::from(script), Value::Array(vec![])]).unwrap();
        let file = this.profiles.data_dir().join("unsaved.rs");
        std::fs::write(&file, "baseline\n").unwrap();
        this.app.editor_rpc.call("nvim_exec_lua", vec![Value::from("local path=...; vim.cmd('edit '..vim.fn.fnameescape(path)); vim.cmd('doautocmd TextChanged')"), Value::Array(vec![Value::from(file.to_string_lossy().into_owned())])]).unwrap();
        while let Some(msg) = this.app.editor_rpc.next_message() { this.handle_rpc_with_gui_sync(msg); }
        assert!(this.profiles.active().activity.is_empty(), "opening a file is not an edit");
        this.app.editor_rpc.call("nvim_exec_lua", vec![Value::from("vim.api.nvim_buf_set_lines(0,0,-1,false,{'unsaved profile work'}); vim.bo.modified=true; vim.bo.filetype='rust'"), Value::Array(vec![])]).unwrap();
        let second = this.profiles.create().unwrap();
        let path = this.profiles.data_dir().parent().unwrap().join(&second).join("settings.json");
        let mut config = SettingsConfig::default(); config.font_size = 21; config.tabstop = 8;
        config.save(&path.to_string_lossy()).unwrap();
        let outgoing_path = this.profiles.settings_path();
        let mut external_config = SettingsConfig::load(&outgoing_path.to_string_lossy()).unwrap();
        external_config.font_size = 17; external_config.tabstop = 2;
        external_config.save(&outgoing_path.to_string_lossy()).unwrap();
        this.app.settings_widget.config.font_size = 23; // Unconfirmed/stale local draft.
        this.app.editor_rpc.notify("nvim_exec_lua", vec![Value::from("vim.cmd('doautocmd TextChanged')"), Value::Array(vec![])]).unwrap();
        this.switch_profile(&second, cx).unwrap();
        assert!(!this.app.editor_rpc.wants_write(), "settings notification must be flushed immediately");
        assert_eq!(SettingsConfig::load(&outgoing_path.to_string_lossy()).unwrap().font_size, 17, "switch must not overwrite external settings or commit unconfirmed edits");
        assert_eq!(this.font_size, 21.0);
        assert!(this.profiles.active().activity.is_empty(), "queued event belongs to outgoing profile");
        let tabstop = this.app.editor_rpc.call("nvim_eval", vec![Value::from("&tabstop")]).unwrap();
        assert_eq!(tabstop.as_i64(), Some(8));
        let line = this.app.editor_rpc.call("nvim_eval", vec![Value::from("getline(1)")]).unwrap();
        assert_eq!(line.as_str(), Some("unsaved profile work"));
        let modified = this.app.editor_rpc.call("nvim_eval", vec![Value::from("&modified")]).unwrap();
        assert_eq!(modified.as_i64(), Some(1));
        this.switch_profile("default", cx).unwrap();
        assert_eq!(this.font_size, 17.0);
        assert_eq!(this.profiles.active().activity[&profiles::local_day_key()].edits, 1);
        let active_path = this.app.editor_rpc.call("nvim_eval", vec![Value::from("g:tuim_settings_path")]).unwrap();
        assert_eq!(active_path.as_str(), this.profiles.settings_path().to_str());
    });
}
