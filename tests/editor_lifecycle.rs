use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rmpv::Value;

use gtuim::nvim::helpers::handle_rpc_message;
use gtuim::nvim::process::NvimProcess;
use gtuim::nvim::rpc::{AsyncTag, RpcClient};
use gtuim::tui::app::{App, Mode};
use gtuim::tui::capabilities::Capabilities;
use gtuim::tui::events;
use gtuim::tui::input::InputEvent;
use gtuim::tui::widgets::menu_bar::MENUS;
use gtuim::tui::widgets::settings::Keybindings;
use gtuim::tui::workspace::COMMANDS;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[test]
fn gui_search_replace_uses_live_neovim_and_preserves_regex_context() {
    use gtuim::gui::find_replace::{self, FindReplaceState};
    let mut h = TestHarness::new(None, &[("search.txt", b"foo foo\nbar foobar bar\nalpha beta alpha\n")], &["search.txt"]);
    let mut state = FindReplaceState::default();
    state.query = "foo".into();
    state.replacement = "X&Y".into();
    state.case_sensitive = true;
    find_replace::send_replace_all(&mut h.app.editor_rpc, &state);
    h.pump_for(Duration::from_millis(100));
    let lines = h.eval_lua("return vim.api.nvim_buf_get_lines(0, 0, -1, false)");
    assert_eq!(lines.as_array().unwrap()[0], Value::from("X&Y X&Y"));
    assert_eq!(lines.as_array().unwrap()[1], Value::from("bar X&Ybar bar"));
    h.eval_lua("vim.api.nvim_buf_set_lines(0, 0, -1, false, {'bar foobar bar', 'alpha beta alpha'}); vim.api.nvim_win_set_cursor(0, {1,0}); return true");
    state.query = "(foo)@<=bar".into();
    state.replacement = "BAZ".into();
    state.use_regex = true;
    find_replace::send_replace_current(&mut h.app.editor_rpc, &state);
    h.pump_for(Duration::from_millis(100));
    assert_eq!(h.eval_lua("return vim.api.nvim_buf_get_lines(0, 0, 1, false)[1]"), Value::from("bar fooBAZ bar"));
    h.eval_lua("vim.api.nvim_win_set_cursor(0, {2,6}); return true");
    state.query = "alpha|beta".into();
    state.replacement = "BETA".into();
    find_replace::send_replace_current(&mut h.app.editor_rpc, &state);
    h.pump_for(Duration::from_millis(100));
    assert_eq!(h.eval_lua("return vim.api.nvim_buf_get_lines(0, 1, 2, false)[1]"), Value::from("alpha BETA alpha"));
    h.eval_lua("vim.api.nvim_buf_set_lines(0, 0, -1, false, {'beta alpha beta'}); vim.api.nvim_win_set_cursor(0, {1,5}); return true");
    state.replacement = "CHANGED".into();
    find_replace::send_replace_current(&mut h.app.editor_rpc, &state);
    h.pump_for(Duration::from_millis(100));
    assert_eq!(h.eval_lua("return vim.api.nvim_buf_get_lines(0, 0, 1, false)[1]"), Value::from("beta CHANGED beta"));
    assert_eq!(fs::read(h.work.join("search.txt")).unwrap(), b"foo foo\nbar foobar bar\nalpha beta alpha\n");
}

#[test]
fn explorer_deletion_does_not_close_similarly_named_unsaved_buffers() {
    let mut h = TestHarness::new(None, &[
        ("notes.txt", b"delete me\n"), ("notes.txt.bak", b"keep me\n"),
    ], &["notes.txt.bak"]);
    h.type_text("UNSAVED ");
    let backup = h.eval_lua("return vim.api.nvim_get_current_buf()").as_i64().unwrap();
    let path = h.work.join("notes.txt");
    h.app.explorer = gtuim::tui::widgets::explorer::FileExplorer::new_at(h.work.to_string_lossy().into());
    h.app.explorer.list_state.selected = h.app.explorer.entries.iter()
        .position(|entry| Path::new(&entry.path) == path).unwrap();
    h.app.explorer.begin_delete();
    h.app.explorer.confirm_delete();
    events::process_widget_side_effects(&mut h.app);
    h.pump_for(Duration::from_millis(100));
    assert!(!path.exists());
    assert_eq!(h.eval_lua(&format!("return vim.api.nvim_buf_is_valid({backup})")), Value::from(true));
    assert_eq!(h.eval_lua(&format!("return vim.bo[{backup}].modified")), Value::from(true));
    assert_eq!(fs::read(h.work.join("notes.txt.bak")).unwrap(), b"keep me\n");
}

#[test]
fn compilation_split_failed_build_navigation_and_safe_source_jump() {
    use gtuim::tui::app::FocusRegion;
    use gtuim::tui::widgets::compile::CompileStage;
    let mut h = TestHarness::new(None, &[
        ("keep.txt", b"keep unsaved work\n"),
        ("src with spaces.c", b"first\nsecond\nthird line here\nlast\n"),
        ("other.c", b"other\nwarning here\n"),
    ], &["keep.txt"]);
    h.type_text("UNSAVED ");
    let keep_buffer = h.eval_lua("return vim.api.nvim_get_current_buf()");
    let command = "printf 'src with spaces.c:3:5: error: broken type\\nother.c:2:1: warning: unused name\\n'; exit 1";
    h.app.compile_widget.open_with(h.work.to_string_lossy().into(), command.into(), command.into(), "test".into());
    h.app.compile_widget.start();
    for _ in 0..100 {
        h.app.poll_background_tasks();
        h.pump_for(Duration::from_millis(15));
        if !h.app.compile_widget.is_running() { break; }
    }
    assert_eq!(h.app.compile_widget.stage, CompileStage::Failed(1));
    assert!(!h.app.compile_widget.is_open, "compile command modal must become a non-modal split");
    assert!(h.app.workspace.drawer_visible);
    assert_eq!(h.app.workspace.drawer_tab, 3);
    assert_eq!(gtuim::tui::widgets::diagnostics::collect(&h.app.compile_widget.lines).len(), 2);
    h.app.workspace.focus = FocusRegion::Editor;
    h.send_key("<F8>");
    h.pump_for(Duration::from_millis(50));
    assert_eq!(h.app.compile_widget.selected_diagnostic, 0, "first next-error must visit the first error");
    h.send_key("<F8>");
    h.pump_for(Duration::from_millis(50));
    assert_eq!(h.app.compile_widget.selected_diagnostic, 1);
    h.send_key("<S-F8>");
    h.pump_for(Duration::from_millis(50));
    assert_eq!(h.app.compile_widget.selected_diagnostic, 0);
    for (mode, next, previous) in [(Mode::Ide, "<C-n>", "<C-p>"), (Mode::Normal, "j", "k"), (Mode::Zen, "j", "k")] {
        h.app.mode = mode;
        h.app.workspace.focus = FocusRegion::Drawer;
        h.app.compile_widget.selected_diagnostic = 0;
        h.send_key(next);
        assert_eq!(h.app.compile_widget.selected_diagnostic, 1);
        h.send_key(previous);
        assert_eq!(h.app.compile_widget.selected_diagnostic, 0);
        h.send_key("<CR>");
        h.pump_for(Duration::from_millis(100));
        assert_eq!(h.app.workspace.focus, FocusRegion::Editor);
        assert!(h.eval_lua("return vim.api.nvim_buf_get_name(0)").as_str().unwrap().ends_with("src with spaces.c"));
        let cursor = h.eval_lua("return vim.api.nvim_win_get_cursor(0)");
        assert_eq!(cursor.as_array().unwrap(), &vec![Value::from(3), Value::from(4)]);
    }
    assert_eq!(h.eval_lua(&format!("return vim.bo[{}].modified", keep_buffer.as_i64().unwrap())), Value::from(true));
    assert_eq!(fs::read(h.work.join("keep.txt")).unwrap(), b"keep unsaved work\n");
    h.app.workspace.focus = FocusRegion::Drawer;
    h.send_key("q");
    assert!(!h.app.workspace.drawer_visible);
    events::execute_command(&mut h.app, "compilation");
    assert!(h.app.workspace.drawer_visible);
    assert!(!h.app.compile_widget.lines.is_empty(), "closing split must retain output");
}

#[test]
fn native_welcome_tracks_dashboard_files_pickers_and_project_recents() {
    let mut h = TestHarness::new(None, &[("recent file.txt", b"recent\n"), ("other.txt", b"other\n")], &[]);
    assert!(h.pump_until(Duration::from_secs(2), |h| h.app.welcome.is_some()), "empty startup shows welcome");
    let dashboard_lines = h.buf_lines();
    h.type_text("must not edit the welcome surface");
    assert_eq!(h.buf_lines(), dashboard_lines);
    let recent = h.work.join("recent file.txt").to_string_lossy().to_string();
    let missing = h.work.join("missing.txt").to_string_lossy().to_string();
    h.eval_lua(&format!("vim.v.oldfiles = {{{recent:?}, {recent:?}, {missing:?}, '/etc/hosts'}}; _G.tuim_alpha_start()"));
    assert!(h.pump_until(Duration::from_secs(2), |h| h.app.welcome.is_some()));
    let welcome = h.app.welcome.as_ref().unwrap();
    assert_eq!(welcome.project_path, h.work.to_string_lossy());
    assert_eq!(welcome.recent_files.iter().filter(|p| **p == recent).count(), 1);
    assert!(!welcome.recent_files.contains(&missing));
    assert!(!welcome.recent_files.iter().any(|p| p == "/etc/hosts"));
    h.eval_lua("_G.test_welcome_float = vim.api.nvim_open_win(vim.api.nvim_create_buf(false, true), true, {relative='editor', width=20, height=4, row=1, col=1})");
    assert!(h.pump_until(Duration::from_secs(2), |h| h.app.welcome.is_none()));
    h.eval_lua("vim.api.nvim_win_close(_G.test_welcome_float, true)");
    assert!(h.pump_until(Duration::from_secs(2), |h| h.app.welcome.is_some()));
    h.app.explorer.open_file_requested = Some(recent);
    events::process_widget_side_effects(&mut h.app);
    assert!(h.pump_until(Duration::from_secs(2), |h| h.app.welcome.is_none()));
    assert_eq!(h.buf_lines(), vec!["recent"]);
    h.eval_lua("_G.tuim_alpha_start()");
    assert!(h.pump_until(Duration::from_secs(2), |h| h.app.welcome.is_some()));
    events::execute_command(&mut h.app, "new_file");
    assert!(h.pump_until(Duration::from_secs(2), |h| h.app.welcome.is_none()));
    h.type_text("a real new buffer");
    assert_eq!(h.buf_lines(), vec!["a real new buffer"]);
}

#[test]
fn gui_help_is_native_read_only_and_does_not_open_a_neovim_float() {
    let mut h = TestHarness::new(None, &[("help.txt", b"keep this text\n")], &["help.txt"]);
    let windows = h.eval_lua("return #vim.api.nvim_list_wins()");
    let buffer = h.eval_lua("return vim.api.nvim_get_current_buf()");
    let config = h.app.settings_widget.config.clone();
    events::execute_command(&mut h.app, "help");
    assert!(h.pump_until(Duration::from_secs(2), |h| h.app.onboarding.is_open && h.app.onboarding.help_only));
    assert_eq!(h.eval_lua("return #vim.api.nvim_list_wins()"), windows);
    assert_eq!(h.eval_lua("return vim.api.nvim_get_current_buf()"), buffer);
    h.send_key("<C-s>");
    h.send_key("<CR>");
    assert!(h.app.onboarding.is_open);
    h.send_key("<Esc>");
    assert!(!h.app.onboarding.is_open);
    assert_eq!(h.app.settings_widget.config, config);
    assert_eq!(h.buf_lines(), vec!["keep this text"]);
}


struct TestHarness {
    app: App,
    root: PathBuf,
    work: PathBuf,
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl TestHarness {
    fn new(settings_json: Option<&str>, files: &[(&str, &[u8])], open_files: &[&str]) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("gtuim-editor-test-{}-{}", std::process::id(), id));
        let _ = fs::remove_dir_all(&root);
        let work = root.join("work");
        let xdg_data = root.join("xdg_data");
        let xdg_cfg = root.join("xdg_cfg");
        let xdg_state = root.join("xdg_state");
        let xdg_cache = root.join("xdg_cache");
        let data_dir = xdg_data.join("tuim");
        fs::create_dir_all(&work).unwrap();
        fs::create_dir_all(&data_dir).unwrap();
        fs::create_dir_all(&xdg_cfg).unwrap();
        fs::create_dir_all(&xdg_state).unwrap();
        fs::create_dir_all(&xdg_cache).unwrap();

        if let Some(json) = settings_json {
            fs::write(data_dir.join("settings.json"), json).unwrap();
        }
        for (rel, bytes) in files {
            let p = work.join(rel);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&p, bytes).unwrap();
        }

        let mut env_map: HashMap<String, String> = std::env::vars().collect();
        env_map.insert("XDG_DATA_HOME".into(), xdg_data.to_string_lossy().into());
        env_map.insert("XDG_CONFIG_HOME".into(), xdg_cfg.to_string_lossy().into());
        env_map.insert("XDG_STATE_HOME".into(), xdg_state.to_string_lossy().into());
        env_map.insert("XDG_CACHE_HOME".into(), xdg_cache.to_string_lossy().into());
        env_map.insert("NVIM_APPNAME".into(), "tuim".into());
        env_map.insert("TUIM_DISABLE_PLUGINS".into(), "1".into());

        let proc = NvimProcess::spawn(&env_map).expect("spawn nvim --embed");
        let rpc = RpcClient::new(proc);
        let caps = Capabilities::detect(&env_map);
        let mut app = App::new(
            110,
            34,
            rpc,
            caps,
            data_dir.to_string_lossy().into(),
            root.to_string_lossy().into(),
        );
        app.explorer.root_dir = work.to_string_lossy().into();
        app.explorer.refresh();

        let opts = Value::Map(vec![
            (Value::from("rgb"), Value::from(true)),
            (Value::from("ext_linegrid"), Value::from(true)),
        ]);
        app.editor_rpc
            .call("nvim_ui_attach", vec![Value::from(110), Value::from(34), opts])
            .expect("ui attach");

        let init_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/nvim/tuim_init.lua");
        let lua_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/nvim");
        let open_vals: Vec<Value> = open_files
            .iter()
            .map(|f| Value::from(work.join(f).to_string_lossy().to_string()))
            .collect();
        let boot_lua = format!(
            "local work, files = select(1, ...)\n\
             vim.cmd('cd ' .. vim.fn.fnameescape(work))\n\
             vim.g.tuim_gui_split_headers = true\n\
             vim.g.tuim_gui_compile = true\n\
             vim.g.tuim_gui_onboarding = true\n\
             package.path = '{}/?.lua;' .. package.path\n\
             dofile('{}')\n\
             if #files > 0 then\n\
                 _G.tuim_alpha_start = function() end\n\
                 vim.cmd('edit ' .. vim.fn.fnameescape(files[1]))\n\
                 for i = 2, #files do vim.cmd('badd ' .. vim.fn.fnameescape(files[i])) end\n\
             end",
            lua_dir.to_string_lossy().replace('\'', "''"),
            init_path.to_string_lossy().replace('\'', "''"),
        );
        app.editor_rpc
            .call(
                "nvim_exec_lua",
                vec![
                    Value::from(boot_lua),
                    Value::Array(vec![
                        Value::from(work.to_string_lossy().to_string()),
                        Value::Array(open_vals),
                    ]),
                ],
            )
            .expect("boot tuim_init.lua");

        let mut harness = Self { app, root, work };
        harness.pump_for(Duration::from_millis(250));
        harness
    }

    fn pump_for(&mut self, dur: Duration) {
        let deadline = Instant::now() + dur;
        while Instant::now() < deadline {
            let _ = self.app.editor_rpc.flush_writes();
            let _ = self.app.editor_rpc.pump_incoming();
            while let Some(msg) = self.app.editor_rpc.next_message() {
                handle_rpc_message(&mut self.app, msg);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn pump_until<F>(&mut self, timeout: Duration, mut pred: F) -> bool
    where
        F: FnMut(&Self) -> bool,
    {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let _ = self.app.editor_rpc.flush_writes();
            let _ = self.app.editor_rpc.pump_incoming();
            while let Some(msg) = self.app.editor_rpc.next_message() {
                handle_rpc_message(&mut self.app, msg);
            }
            if pred(self) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        pred(self)
    }

    fn send_key(&mut self, key: &str) {
        events::handle_event(&mut self.app, InputEvent::Key(key.to_string()));
        let _ = self.app.editor_rpc.flush_writes();
    }

    fn type_text(&mut self, text: &str) {
        for ch in text.chars() {
            self.send_key(&ch.to_string());
        }
        self.pump_for(Duration::from_millis(80));
    }

    fn eval_lua(&mut self, code: &str) -> Value {
        let res = self
            .app
            .editor_rpc
            .call("nvim_exec_lua", vec![Value::from(code), Value::Array(vec![])])
            .expect("eval_lua");
        self.pump_for(Duration::from_millis(30));
        res
    }

    fn nvim_mode(&mut self) -> (String, bool) {
        let res = self.app.editor_rpc.call("nvim_get_mode", vec![]).expect("get_mode");
        let mut mode = String::new();
        let mut blocking = false;
        if let Value::Map(entries) = res {
            for (k, v) in entries {
                match k.as_str() {
                    Some("mode") => mode = v.as_str().unwrap_or("").to_string(),
                    Some("blocking") => blocking = v.as_bool().unwrap_or(false),
                    _ => {}
                }
            }
        }
        (mode, blocking)
    }

    fn buf_lines(&mut self) -> Vec<String> {
        let val = self.eval_lua("return vim.api.nvim_buf_get_lines(0, 0, -1, false)");
        val.as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap_or("").to_string())
            .collect()
    }
}

#[test]
fn ide_mode_default_sync_without_mode_field_and_zz_inserts_text() {
    // Reproduces workspace audit finding: settings.json has onboarding_completed
    // without an explicit "mode" field; Rust defaults to Mode::Ide and Neovim
    // must match so ZZ types text instead of quitting.
    let mut h = TestHarness::new(
        Some(r#"{"onboarding_completed":true}"#),
        &[("README.md", b"# Hello\n")],
        &["README.md"],
    );
    assert_eq!(h.app.mode, Mode::Ide);
    let (mode, blocking) = h.nvim_mode();
    assert_eq!(mode, "i", "Neovim should start in Insert mode for IDE mode");
    assert!(!blocking);

    h.type_text("ZZ");
    h.pump_for(Duration::from_millis(120));
    let lines = h.buf_lines();
    assert_eq!(lines[0], "ZZ# Hello");
    assert!(!h.app.should_quit);
}

#[test]
fn floating_picker_preserves_active_tab_and_restores_ide_insert_mode() {
    let mut h = TestHarness::new(
        Some(r#"{"mode":"ide","onboarding_completed":true}"#),
        &[("README.md", b"# Doc\n"), ("main.rs", b"fn main() {}\n")],
        &["README.md", "main.rs"],
    );
    // Switch to main.rs (second tab)
    let main_bufnr = h.app.tabs.iter().find(|t| t.name.ends_with("main.rs")).unwrap().bufnr;
    h.eval_lua(&format!("_G.tuim_select_buffer({})", main_bufnr));
    assert!(h.pump_until(Duration::from_millis(500), |h| {
        h.app.tabs.get(h.app.active_tab).map_or(false, |t| t.name.ends_with("main.rs"))
    }));

    // Simulate a Telescope-like floating prompt opening and running stopinsert on close
    h.eval_lua(
        "local b = vim.api.nvim_create_buf(false, true)\n\
         vim.bo[b].buftype = 'prompt'\n\
         vim.bo[b].filetype = 'TelescopePrompt'\n\
         _G._test_float = vim.api.nvim_open_win(b, true, { relative='editor', width=30, height=5, row=2, col=2 })\n\
         _G.tuim_notify_buffers()",
    );
    h.pump_for(Duration::from_millis(120));
    // Active tab MUST NOT jump back to tab 0 (README.md) while float is open!
    assert!(
        h.app.tabs[h.app.active_tab].name.ends_with("main.rs"),
        "Active tab jumped to {} while float was open",
        h.app.tabs[h.app.active_tab].name
    );

    // Close float with stopinsert (like Telescope actions.close) and verify IDE snaps back to Insert
    h.eval_lua("vim.cmd('stopinsert'); vim.api.nvim_win_close(_G._test_float, true)");
    h.pump_for(Duration::from_millis(120));
    let (mode, blocking) = h.nvim_mode();
    assert_eq!(mode, "i", "IDE mode must return to Insert after closing float");
    assert!(!blocking);
}

#[test]
fn ctrl_h_replace_marks_dirty_immediately_and_immediate_ctrl_s_saves() {
    let mut h = TestHarness::new(
        Some(r#"{"mode":"ide","onboarding_completed":true}"#),
        &[(
            "search_undo.txt",
            b"Line 1: needle alpha\nLine 2: needle beta\n",
        )],
        &["search_undo.txt"],
    );
    assert!(!h.app.tabs[h.app.active_tab].changed);

    // Ctrl+H -> needle/REPLACED/g -> Enter
    h.send_key("<C-h>");
    h.pump_for(Duration::from_millis(80));
    h.type_text("needle/REPLACED/g");
    h.send_key("<CR>");

    // Dirty dot MUST appear immediately without pressing Right or another key!
    assert!(
        h.pump_until(Duration::from_millis(800), |h| h.app.tabs[h.app.active_tab].changed),
        "Dirty marker did not appear immediately after Ctrl+H replace"
    );
    let (mode_after_replace, blocking) = h.nvim_mode();
    assert!(!blocking, "Neovim must not stall in hit-enter/wait_return after replace");
    assert_eq!(mode_after_replace, "i", "IDE mode should return to Insert after replace");

    // Immediate Ctrl+S MUST write replacements to disk and clear dirty dot!
    h.send_key("<C-s>");
    assert!(
        h.pump_until(Duration::from_millis(800), |h| !h.app.tabs[h.app.active_tab].changed),
        "Dirty marker did not clear after immediate Ctrl+S"
    );
    let disk = fs::read_to_string(h.work.join("search_undo.txt")).unwrap();
    assert_eq!(disk, "Line 1: REPLACED alpha\nLine 2: REPLACED beta\n");
    assert!(
        h.app.notice.as_ref().map_or(false, |n| n.message.contains("Saved")),
        "Expected Saved notice, got {:?}",
        h.app.notice
    );
}

#[test]
fn dirty_named_close_save_completes_without_hit_enter_stall() {
    let mut h = TestHarness::new(
        Some(r#"{"mode":"ide","onboarding_completed":true}"#),
        &[("dirty_a.txt", b"alpha\n"), ("dirty_b.txt", b"beta\n")],
        &["dirty_a.txt", "dirty_b.txt"],
    );
    h.type_text("SAVED_");
    assert!(h.pump_until(Duration::from_millis(500), |h| h.app.tabs[h.app.active_tab].changed));

    events::execute_command(&mut h.app, "close_buffer");
    h.pump_for(Duration::from_millis(120));
    assert!(h.app.editor_rpc.has_pending_tag(AsyncTag::QuitConfirmation));

    // Answer 'y' (Save)
    h.send_key("y");
    assert!(
        h.pump_until(Duration::from_millis(1000), |h| {
            !h.app.editor_rpc.has_pending_tag(AsyncTag::QuitConfirmation)
                && !h.app.tabs.iter().any(|t| t.name.ends_with("dirty_a.txt"))
        }),
        "Dirty named tab stalled instead of saving and closing cleanly; tabs={:?}",
        h.app.tabs
    );
    let disk = fs::read_to_string(h.work.join("dirty_a.txt")).unwrap();
    assert_eq!(disk, "SAVED_alpha\n");
    let (_, blocking) = h.nvim_mode();
    assert!(!blocking);
}

#[test]
fn dirty_unnamed_close_cancel_preserves_unnamed_and_yes_prompts_for_path() {
    let mut h = TestHarness::new(
        Some(r#"{"mode":"ide","onboarding_completed":true}"#),
        &[("basic.txt", b"hello\n")],
        &["basic.txt"],
    );
    events::execute_command(&mut h.app, "new_file");
    assert!(h.pump_until(Duration::from_millis(600), |h| h.app.tabs[h.app.active_tab].name.is_empty()));
    assert_eq!(h.app.tabs[h.app.active_tab].filetype, "");

    h.type_text("UNNAMED_BODY");
    assert!(h.pump_until(Duration::from_millis(500), |h| h.app.tabs[h.app.active_tab].changed));

    // Close -> Cancel ('c'): must keep unnamed identity and never create ./Untitled
    events::execute_command(&mut h.app, "close_buffer");
    h.pump_for(Duration::from_millis(120));
    h.send_key("c");
    assert!(h.pump_until(Duration::from_millis(800), |h| {
        !h.app.editor_rpc.has_pending_tag(AsyncTag::QuitConfirmation)
    }));
    assert_eq!(h.app.tabs[h.app.active_tab].name, "");
    assert_eq!(h.app.active_tab_display_name(), "[No Name]");
    assert!(h.app.tabs[h.app.active_tab].changed);
    assert!(!h.work.join("Untitled").exists());

    // Close -> Yes ('y') -> Esc (cancel path prompt): must still not create ./Untitled
    events::execute_command(&mut h.app, "close_buffer");
    h.pump_for(Duration::from_millis(120));
    h.send_key("y");
    h.pump_for(Duration::from_millis(180));
    assert!(!h.work.join("Untitled").exists(), "Must never silently write ./Untitled");
    h.send_key("<Esc>");
    h.pump_for(Duration::from_millis(180));
    assert_eq!(h.app.tabs[h.app.active_tab].name, "");
    assert!(h.app.tabs[h.app.active_tab].changed);
    assert!(!h.work.join("Untitled").exists());

    // Close -> Yes ('y') -> enter path + Return: writes chosen path and closes tab
    events::execute_command(&mut h.app, "close_buffer");
    h.pump_for(Duration::from_millis(120));
    h.send_key("y");
    h.pump_for(Duration::from_millis(180));
    h.type_text("chosen_unnamed.txt");
    h.send_key("<CR>");
    assert!(
        h.pump_until(Duration::from_millis(1000), |h| {
            h.work.join("chosen_unnamed.txt").exists()
                && !h.app.tabs.iter().any(|t| t.name.is_empty())
        }),
        "Expected chosen_unnamed.txt to be written and unnamed tab closed"
    );
    assert!(!h.work.join("Untitled").exists());
    assert_eq!(fs::read_to_string(h.work.join("chosen_unnamed.txt")).unwrap(), "UNNAMED_BODY\n");
}

#[test]
fn save_file_as_wiring_shortcut_no_ctrl_s_literal_and_syncs_chrome() {
    assert!(COMMANDS.iter().any(|c| c.id == "save_file_as"));
    assert!(MENUS[0].items.iter().any(|&(_, act)| act == "save_file_as"));
    assert_eq!(Keybindings::default().save_file_as, "<C-S-s>");

    let mut h = TestHarness::new(
        Some(r#"{"mode":"ide","onboarding_completed":true}"#),
        &[("basic.txt", b"original\n")],
        &["basic.txt"],
    );
    // Trigger via shortcut <C-S-s>
    h.send_key("<C-S-s>");
    h.pump_for(Duration::from_millis(180));
    h.type_text("copied_via_save_as.txt");
    h.send_key("<CR>");
    assert!(
        h.pump_until(Duration::from_millis(1000), |h| {
            h.work.join("copied_via_save_as.txt").exists()
                && h.app.tabs[h.app.active_tab].name.ends_with("copied_via_save_as.txt")
        }),
        "Save As failed to write or sync tab name; tabs={:?}",
        h.app.tabs
    );
    let saved = fs::read_to_string(h.work.join("copied_via_save_as.txt")).unwrap();
    assert_eq!(saved, "original\n", "Must not contain literal ^S (0x13)");
    assert!(!saved.contains('\x13'));
    assert!(!h.app.tabs[h.app.active_tab].changed);

    // Manual :saveas must also update active tab/chrome immediately
    h.eval_lua("vim.cmd('silent saveas manual_saveas.txt')");
    assert!(
        h.pump_until(Duration::from_millis(800), |h| {
            h.work.join("manual_saveas.txt").exists()
                && h.app.tabs[h.app.active_tab].name.ends_with("manual_saveas.txt")
        }),
        "Manual :saveas left active tab stale: {:?}",
        h.app.tabs
    );
}

#[test]
fn lossless_final_newline_policy_preserves_missing_lf_and_crlf() {
    let mut h = TestHarness::new(
        Some(r#"{"mode":"ide","onboarding_completed":true}"#),
        &[
            ("no_eol_lf.txt", b"first\nsecond"),
            ("no_eol_crlf.txt", b"first\r\nsecond"),
            ("with_eol_crlf.txt", b"first\r\nsecond\r\n"),
        ],
        &["no_eol_lf.txt", "no_eol_crlf.txt", "with_eol_crlf.txt"],
    );

    // Edit and save no_eol_lf.txt
    h.type_text("X_");
    h.send_key("<C-s>");
    assert!(h.pump_until(Duration::from_millis(800), |h| !h.app.tabs[h.app.active_tab].changed));
    assert_eq!(fs::read(h.work.join("no_eol_lf.txt")).unwrap(), b"X_first\nsecond");

    // Switch to no_eol_crlf.txt, edit, save
    let b2 = h.app.tabs.iter().find(|t| t.name.ends_with("no_eol_crlf.txt")).unwrap().bufnr;
    h.eval_lua(&format!("_G.tuim_select_buffer({})", b2));
    h.pump_for(Duration::from_millis(120));
    h.type_text("Y_");
    h.send_key("<C-s>");
    assert!(h.pump_until(Duration::from_millis(800), |h| !h.app.tabs[h.app.active_tab].changed));
    assert_eq!(fs::read(h.work.join("no_eol_crlf.txt")).unwrap(), b"Y_first\r\nsecond");

    // Switch to with_eol_crlf.txt, edit, save
    let b3 = h.app.tabs.iter().find(|t| t.name.ends_with("with_eol_crlf.txt")).unwrap().bufnr;
    h.eval_lua(&format!("_G.tuim_select_buffer({})", b3));
    h.pump_for(Duration::from_millis(120));
    h.type_text("Z_");
    h.send_key("<C-s>");
    assert!(h.pump_until(Duration::from_millis(800), |h| !h.app.tabs[h.app.active_tab].changed));
    assert_eq!(fs::read(h.work.join("with_eol_crlf.txt")).unwrap(), b"Z_first\r\nsecond\r\n");
}

#[test]
fn ide_undo_breakpoints_cursor_restore_and_doc_cursor_coords() {
    let mut content = String::new();
    for i in 1..=120 {
        if i == 4 {
            content.push_str(&"a".repeat(600));
            content.push('\n');
        } else {
            content.push_str(&format!("line {}\n", i));
        }
    }
    let mut h = TestHarness::new(
        Some(r#"{"mode":"ide","onboarding_completed":true}"#),
        &[("doc.txt", content.as_bytes())],
        &["doc.txt"],
    );

    // 1. Document cursor coordinates on line 100 and end of long wrapped line 4
    h.eval_lua("vim.api.nvim_win_set_cursor(0, {100, 0}); _G.tuim_notify_cursor()");
    assert!(h.pump_until(Duration::from_millis(500), |h| {
        h.app.doc_cursor_line == Some(100) && h.app.doc_cursor_col == Some(1)
    }));
    h.eval_lua("vim.api.nvim_win_set_cursor(0, {4, 599}); _G.tuim_notify_cursor()");
    assert!(h.pump_until(Duration::from_millis(500), |h| {
        h.app.doc_cursor_line == Some(4) && h.app.doc_cursor_col == Some(600)
    }));

    // 2. IDE undo breakpoints in new buffer
    events::execute_command(&mut h.app, "new_file");
    assert!(h.pump_until(Duration::from_millis(500), |h| h.app.tabs[h.app.active_tab].name.is_empty()));
    h.type_text("FIRST ");
    h.type_text("SECOND ");
    h.send_key("<CR>");
    h.pump_for(Duration::from_millis(60));
    h.type_text("THIRD ");
    h.send_key("<C-z>");
    h.pump_for(Duration::from_millis(120));
    let after_undo_1 = h.buf_lines();
    assert_eq!(
        after_undo_1.first().map(|s| s.as_str()),
        Some("FIRST SECOND "),
        "Single Ctrl+Z should only undo THIRD, not all typed text; got {:?}",
        after_undo_1
    );

    // 3. Cursor position after unnamed save prompt cancel (must NOT shift 1 char left)
    events::execute_command(&mut h.app, "new_file");
    h.pump_for(Duration::from_millis(120));
    h.type_text("Second line");
    h.send_key("<C-s>");
    h.pump_for(Duration::from_millis(180));
    h.send_key("<Esc>");
    h.pump_for(Duration::from_millis(180));
    h.type_text("!");
    let lines = h.buf_lines();
    assert_eq!(lines[0], "Second line!", "Cursor shifted left after save prompt cancel!");
}
