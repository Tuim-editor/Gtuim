//! Real isolated embedded-Neovim regressions for window-local line-number settings.
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gtuim::nvim::process::NvimProcess;
use gtuim::nvim::rpc::RpcClient;
use gtuim::tui::widgets::settings::{SettingsConfig, SettingsWidget};
use rmpv::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Harness {
    rpc: RpcClient,
    root: PathBuf,
}

impl Harness {
    fn new(mode: &str, file_startup: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "gtuim-line-numbers-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        let mut env: HashMap<String, String> = std::env::vars().collect();
        for kind in ["DATA", "CONFIG", "STATE", "CACHE"] {
            let path = root.join(kind.to_lowercase());
            fs::create_dir_all(&path).unwrap();
            env.insert(format!("XDG_{kind}_HOME"), path.to_string_lossy().into());
        }
        env.insert("HOME".into(), root.to_string_lossy().into());
        env.insert("NVIM_APPNAME".into(), "tuim".into());
        env.insert("TUIM_DISABLE_PLUGINS".into(), "1".into());
        let settings = root.join("settings.json");
        fs::write(
            &settings,
            format!(r#"{{"numbers":"{mode}","mode":"normal","clipboard":false}}"#),
        )
        .unwrap();
        let file = root.join("source.txt");
        fs::write(&file, "one\ntwo\nthree\n".repeat(100)).unwrap();
        let args = if file_startup {
            vec![file.to_string_lossy().into()]
        } else {
            vec![]
        };
        let process = NvimProcess::spawn_with_args(&env, &args).expect("installed nvim --embed");
        let rpc = RpcClient::new(process);
        let mut h = Self { rpc, root };
        h.rpc
            .call(
                "nvim_ui_attach",
                vec![
                    Value::from(100),
                    Value::from(30),
                    Value::Map(vec![
                        (Value::from("rgb"), Value::from(true)),
                        (Value::from("ext_linegrid"), Value::from(true)),
                    ]),
                ],
            )
            .unwrap();
        let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/nvim");
        h.lua(
            r#"
            local runtime, settings, root = ...
            vim.g.tuim_settings_path = settings
            vim.g.tuim_gui_onboarding = true
            vim.g.tuim_gui_split_headers = true
            vim.cmd('cd ' .. vim.fn.fnameescape(root))
            package.path = runtime .. '/?.lua;' .. package.path
            dofile(runtime .. '/tuim_init.lua')
        "#,
            vec![
                Value::from(runtime.to_string_lossy().to_string()),
                Value::from(settings.to_string_lossy().to_string()),
                Value::from(h.root.to_string_lossy().to_string()),
            ],
        );
        if !file_startup {
            assert_eq!(h.lua("return vim.wait(1000, function() return vim.bo.filetype == 'tuim_dashboard' end, 10)", vec![]), Value::from(true));
        }
        h
    }

    fn lua(&mut self, script: &str, args: Vec<Value>) -> Value {
        self.rpc
            .call(
                "nvim_exec_lua",
                vec![Value::from(script), Value::Array(args)],
            )
            .unwrap_or_else(|e| panic!("Lua failed: {e}\n{script}"))
    }

    fn assert_current(&mut self, mode: &str, label: &str) {
        let actual = self.lua("return {vim.wo.number, vim.wo.relativenumber}", vec![]);
        let expected = Value::Array(vec![
            Value::from(mode != "none"),
            Value::from(mode == "relative"),
        ]);
        assert_eq!(actual, expected, "{label} ({mode})");
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.rpc.process.child.kill();
        let _ = self.rpc.process.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn saved_numbers_survive_dashboard_to_files_splits_and_new_buffers() {
    for mode in ["relative", "absolute", "none"] {
        let mut h = Harness::new(mode, false);
        h.assert_current("none", "startup dashboard must not have numbers");
        h.lua("vim.cmd('edit source.txt')", vec![]);
        h.assert_current(mode, "file after startup dashboard");
        h.lua("vim.cmd('vsplit'); vim.cmd('enew')", vec![]);
        h.assert_current(mode, "new buffer in split");
        h.lua("vim.cmd('wincmd p')", vec![]);
        h.assert_current(mode, "original file split");
        h.lua("_G.tuim_alpha_start()", vec![]);
        h.assert_current("none", "reopened dashboard");
        h.lua("vim.cmd('edit source.txt')", vec![]);
        h.assert_current(mode, "returning to file from dashboard");
        // The separately embedded fallback dashboard must have identical behavior.
        let dashboard = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/nvim/dashboard.lua");
        h.lua(
            "dofile(...); _G.tuim_show_dashboard()",
            vec![Value::from(dashboard.to_string_lossy().to_string())],
        );
        h.assert_current("none", "fallback dashboard");
        h.lua("vim.cmd('edit source.txt')", vec![]);
        h.assert_current(mode, "file after fallback dashboard");
    }
}

#[test]
fn saved_numbers_apply_to_direct_file_startup() {
    for mode in ["relative", "absolute", "none"] {
        let mut h = Harness::new(mode, true);
        h.assert_current(mode, "command-line file startup");
        h.lua("vim.cmd('split'); vim.cmd('enew')", vec![]);
        h.assert_current(mode, "new split after direct startup");
    }
}

#[test]
fn profile_reload_updates_all_editor_windows_but_not_special_or_float_windows() {
    let mut h = Harness::new("relative", false);
    h.lua(
        r#"
        vim.cmd('edit source.txt')
        _G.number_test_file = vim.api.nvim_get_current_win()
        vim.cmd('vsplit')
        _G.number_test_split = vim.api.nvim_get_current_win()
        vim.cmd('vnew')
        vim.bo.buftype = 'nofile'
        vim.bo.filetype = 'tuim_dashboard'
        _G.number_test_special = vim.api.nvim_get_current_win()
        local buf = vim.api.nvim_win_get_buf(_G.number_test_file)
        _G.number_test_float = vim.api.nvim_open_win(buf, false, {
            relative = 'editor', row = 1, col = 1, width = 20, height = 5, style = 'minimal',
        })
    "#,
        vec![],
    );
    for mode in ["absolute", "none", "relative"] {
        fs::write(
            h.root.join("profile.json"),
            format!(r#"{{"numbers":"{mode}","mode":"normal","clipboard":false}}"#),
        )
        .unwrap();
        let path = h.root.join("profile.json").to_string_lossy().to_string();
        h.lua(
            "vim.g.tuim_settings_path = ...; _G.tuim_load_settings()",
            vec![Value::from(path)],
        );
        // Do not enter these windows: entry callbacks could hide a failure to
        // apply settings immediately to non-current splits.
        for win in ["number_test_file", "number_test_split"] {
            let options = h.lua(
                "local w = _G[...]; return {vim.wo[w].number, vim.wo[w].relativenumber}",
                vec![Value::from(win)],
            );
            assert_eq!(
                options,
                Value::Array(vec![
                    Value::from(mode != "none"),
                    Value::from(mode == "relative")
                ]),
                "{win} after profile reload ({mode})"
            );
        }
        for win in ["number_test_special", "number_test_float"] {
            let options = h.lua(
                "local w = _G[...]; return {vim.wo[w].number, vim.wo[w].relativenumber}",
                vec![Value::from(win)],
            );
            assert_eq!(
                options,
                Value::Array(vec![Value::from(false), Value::from(false)]),
                "{win} after profile reload"
            );
        }
    }
    // A new/default profile without a numbers key must not inherit 'none'.
    h.lua("_G.tuim_apply_numbers('none')", vec![]);
    fs::write(
        h.root.join("profile.json"),
        r#"{"mode":"normal","clipboard":false}"#,
    )
    .unwrap();
    h.lua(
        "_G.tuim_load_settings(); vim.api.nvim_set_current_win(_G.number_test_file)",
        vec![],
    );
    h.assert_current(
        "relative",
        "profile missing numbers uses application default",
    );
    // The no-file profile path must reset settings too.
    h.lua("_G.tuim_apply_numbers('none'); vim.g.tuim_settings_path = 'missing-profile.json'; _G.tuim_load_settings()", vec![]);
    h.assert_current("relative", "new profile without settings file");
}

#[test]
fn live_settings_save_and_sync_updates_all_splits_and_persists_all_modes() {
    let mut h = Harness::new("relative", false);
    h.lua(
        r#"
        vim.cmd('edit source.txt')
        _G.number_test_file = vim.api.nvim_get_current_win()
        vim.cmd('vsplit')
        _G.number_test_split = vim.api.nvim_get_current_win()
        vim.cmd('vnew')
        _G.tuim_alpha_start()
        _G.number_test_special = vim.api.nvim_get_current_win()
    "#,
        vec![],
    );
    let path = h.root.join("settings.json").to_string_lossy().to_string();
    let mut config = SettingsConfig::default();
    config.mode = "normal".into();
    config.clipboard = false;
    let mut settings = SettingsWidget::new(&path, config);
    for mode in ["absolute", "none", "relative"] {
        settings.config.numbers = mode.into();
        settings.save_and_sync(&mut h.rpc);
        // RPC request is ordered after save_and_sync's asynchronous notification.
        for win in ["number_test_file", "number_test_split"] {
            let options = h.lua(
                "local w = _G[...]; return {vim.wo[w].number, vim.wo[w].relativenumber}",
                vec![Value::from(win)],
            );
            assert_eq!(
                options,
                Value::Array(vec![
                    Value::from(mode != "none"),
                    Value::from(mode == "relative")
                ]),
                "live settings {mode} in {win}"
            );
        }
        h.assert_current(
            "none",
            "live settings must leave current dashboard unnumbered",
        );
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["numbers"], mode);
        h.lua("_G.tuim_load_settings()", vec![]);
        h.assert_current("none", "reloading saved settings on dashboard");
    }
}

#[test]
fn special_buffers_remain_unnumbered_and_cursor_scroll_does_not_reapply_options() {
    let mut h = Harness::new("relative", false);
    h.lua("vim.cmd('edit source.txt')", vec![]);
    h.assert_current("relative", "file before special buffers");
    for setup in [
        "vim.cmd('enew'); vim.bo.buftype = 'nofile'",
        "vim.cmd('enew'); vim.fn.termopen({'/bin/sh', '-c', 'printf terminal'})",
        "vim.cmd('enew'); vim.bo.buftype = 'nofile'; vim.bo.filetype = 'tuim_dashboard'",
        "vim.cmd('enew'); vim.bo.filetype = 'netrw'",
        "vim.cmd('help help')",
        "vim.fn.setqflist({{filename = 'source.txt', lnum = 1, text = 'test'}}); vim.cmd('copen')",
    ] {
        h.lua(setup, vec![]);
        h.assert_current("none", setup);
        h.lua("vim.cmd('edit source.txt')", vec![]);
        h.assert_current("relative", "return to file from special buffer");
    }
    assert_eq!(h.lua(r#"
        local hot = {CursorMoved=true, CursorMovedI=true, WinScrolled=true, TextChanged=true, TextChangedI=true}
        for _, ac in ipairs(vim.api.nvim_get_autocmds({group='TuimLineNumbers'})) do
            assert(not hot[ac.event], 'line-number handler on hot path: ' .. ac.event)
        end
        _G.number_option_writes = 0
        vim.api.nvim_create_autocmd('OptionSet', {
            pattern={'number', 'relativenumber'}, callback=function() _G.number_option_writes = _G.number_option_writes + 1 end,
        })
        for i=1,30 do vim.cmd('normal! j'); vim.cmd('normal! \005') end
        return _G.number_option_writes
    "#, vec![]), Value::from(0));
    h.assert_current("relative", "after repeated cursor/scroll changes");
}
