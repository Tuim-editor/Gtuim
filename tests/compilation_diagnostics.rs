use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rmpv::Value;

use gtuim::nvim::helpers::handle_rpc_message;
use gtuim::nvim::process::NvimProcess;
use gtuim::nvim::rpc::RpcClient;
use gtuim::tui::app::{App, FocusRegion};
use gtuim::tui::capabilities::Capabilities;
use gtuim::tui::events;
use gtuim::tui::input::InputEvent;
use gtuim::tui::widgets::compile::{parse_ansi_line, CompileStage};
use gtuim::tui::widgets::diagnostics;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

struct Harness {
    app: App,
    root: PathBuf,
    work: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Harness {
    fn new(settings_json: Option<&str>, files: &[(&str, &[u8])], open_files: &[&str]) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "gtuim-comp-diag-test-{}-{}",
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&root);
        let work = root.join("work");
        let xdg_data = root.join("xdg_data");
        let xdg_cfg = root.join("xdg_cfg");
        let xdg_state = root.join("xdg_state");
        let xdg_cache = root.join("xdg_cache");
        let data_dir = xdg_data.join("tuim");
        for d in [&work, &data_dir, &xdg_cfg, &xdg_state, &xdg_cache] {
            fs::create_dir_all(d).unwrap();
        }
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
            .call(
                "nvim_ui_attach",
                vec![Value::from(110), Value::from(34), opts],
            )
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

        let mut h = Self { app, root, work };
        h.pump_for(Duration::from_millis(250));
        h
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

    fn buf_lines(&mut self) -> Vec<String> {
        self.eval_lua("return vim.api.nvim_buf_get_lines(0, 0, -1, false)")
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap_or("").to_string())
            .collect()
    }

    fn nvim_mode(&mut self) -> String {
        let res = self.app.editor_rpc.call("nvim_get_mode", vec![]).unwrap();
        if let Value::Map(entries) = res {
            for (k, v) in entries {
                if k.as_str() == Some("mode") {
                    return v.as_str().unwrap_or("").to_string();
                }
            }
        }
        String::new()
    }
}

#[test]
fn jump_into_same_modified_buffer_preserves_edits_and_moves_cursor() {
    let mut h = Harness::new(
        Some(r#"{"mode":"ide","onboarding_completed":true}"#),
        &[("main.c", b"line 1\nline 2\nline 3 target\nline 4\n")],
        &["main.c"],
    );
    h.type_text("EDITED ");
    assert_eq!(h.buf_lines()[0], "EDITED line 1");
    assert_eq!(
        h.eval_lua("return vim.bo.modified"),
        Value::from(true),
        "buffer must be modified before jump"
    );

    h.app.compile_widget.cwd = h.work.to_string_lossy().into();
    h.app.compile_widget.lines = vec![parse_ansi_line("main.c:3:8: error: type mismatch")];
    h.app.notice = None;

    events::visit_compilation_diagnostic(&mut h.app, 0);
    h.pump_for(Duration::from_millis(120));

    assert!(
        h.app.notice.is_none(),
        "jumping to diagnostic in already-open modified buffer must not fail with E37; got notice: {:?}",
        h.app.notice
    );
    assert_eq!(
        h.buf_lines()[0],
        "EDITED line 1",
        "unsaved edits must be preserved"
    );
    let cursor = h.eval_lua("return vim.api.nvim_win_get_cursor(0)");
    assert_eq!(
        cursor.as_array().unwrap(),
        &vec![Value::from(3), Value::from(7)],
        "cursor must move to diagnostic line 3 col 8 (0-based 7)"
    );
}

#[test]
fn jump_skips_non_editor_nofile_and_help_windows_in_favor_of_code_split() {
    let mut h = Harness::new(
        Some(r#"{"mode":"normal","onboarding_completed":true}"#),
        &[
            ("a.c", b"one\ntwo\n"),
            ("target.c", b"alpha\nbeta\ngamma\n"),
        ],
        &["a.c"],
    );
    let code_win = h.eval_lua("return vim.api.nvim_get_current_win()");
    // Open a huge non-editor scratch/nofile split so it has strictly larger area than code_win.
    h.eval_lua(
        "vim.cmd('vsplit'); local b = vim.api.nvim_create_buf(false, true); \
         vim.bo[b].buftype = 'nofile'; vim.api.nvim_win_set_buf(0, b); \
         vim.api.nvim_win_set_width(0, 80)",
    );
    let nofile_win = h.eval_lua("return vim.api.nvim_get_current_win()");
    assert_ne!(code_win, nofile_win);

    h.app.compile_widget.cwd = h.work.to_string_lossy().into();
    h.app.compile_widget.lines = vec![parse_ansi_line("target.c:2:3: error: bad")];

    events::visit_compilation_diagnostic(&mut h.app, 0);
    h.pump_for(Duration::from_millis(120));

    let cur_win = h.eval_lua("return vim.api.nvim_get_current_win()");
    assert_eq!(
        cur_win, code_win,
        "diagnostic jump must target the editor window, not a larger nofile split"
    );
    assert!(h
        .eval_lua("return vim.api.nvim_buf_get_name(0)")
        .as_str()
        .unwrap()
        .ends_with("target.c"));
}

#[test]
fn jump_in_ide_mode_preserves_insert_mode_and_cursor() {
    let mut h = Harness::new(
        Some(r#"{"mode":"ide","onboarding_completed":true}"#),
        &[
            ("first.c", b"first\n"),
            ("second.c", b"row1\nrow2\nrow3 here\n"),
        ],
        &["first.c"],
    );
    assert_eq!(h.nvim_mode(), "i");
    h.app.compile_widget.cwd = h.work.to_string_lossy().into();
    h.app.compile_widget.lines = vec![parse_ansi_line("second.c:3:6: error: oops")];

    events::visit_compilation_diagnostic(&mut h.app, 0);
    h.pump_for(Duration::from_millis(150));

    assert!(h
        .eval_lua("return vim.api.nvim_buf_get_name(0)")
        .as_str()
        .unwrap()
        .ends_with("second.c"));
    let cursor = h.eval_lua("return vim.api.nvim_win_get_cursor(0)");
    assert_eq!(
        cursor.as_array().unwrap(),
        &vec![Value::from(3), Value::from(5)]
    );
    assert_eq!(
        h.nvim_mode(),
        "i",
        "IDE mode should remain in Insert mode after jumping to a diagnostic"
    );
}

#[test]
fn diagnostics_parser_handles_tsc_msvc_parens_and_rejects_noise() {
    // 1. MSVC / tsc --pretty false with parentheses inside error message
    let msvc = diagnostics::parse_location(
        "src/app.ts(10,5): error TS2304: Cannot find name 'foo' (did you mean 'bar'?)",
    )
    .expect("MSVC/tsc diagnostic with parens in message must parse");
    assert_eq!((msvc.path.as_str(), msvc.line, msvc.column), ("src/app.ts", 10, 5));

    // 2. tsc --pretty (enabled by FORCE_COLOR=1 / CLICOLOR_FORCE=1)
    let tsc_pretty = diagnostics::parse_location(
        "src/app.ts:10:5 - error TS2304: Cannot find name 'x'.",
    )
    .expect("tsc --pretty diagnostic must parse");
    assert_eq!(
        (tsc_pretty.path.as_str(), tsc_pretty.line, tsc_pretty.column),
        ("src/app.ts", 10, 5)
    );
    assert!(
        !tsc_pretty.message.starts_with("5 -"),
        "column must not leak into message: {:?}",
        tsc_pretty.message
    );

    // 3. Noise lines must NOT become fake diagnostics
    for noise in [
        "12:34:56 INFO starting build",
        "12:34:56",
        "http://localhost:8080/health",
        "https://example.com:443/api",
        "::1",
    ] {
        assert!(
            diagnostics::parse_location(noise).is_none(),
            "noise line must not parse as diagnostic: {noise:?} -> {:?}",
            diagnostics::parse_location(noise)
        );
    }
}

#[test]
fn plugin_center_overlay_is_not_hijacked_by_compilation_keys() {
    let mut h = Harness::new(
        Some(r#"{"mode":"normal","onboarding_completed":true}"#),
        &[("a.c", b"a\n"), ("b.c", b"b\n")],
        &["a.c"],
    );
    h.app.compile_widget.cwd = h.work.to_string_lossy().into();
    h.app.compile_widget.lines = vec![
        parse_ansi_line("a.c:1:1: error: first"),
        parse_ansi_line("b.c:1:1: error: second"),
    ];
    h.app.workspace.drawer_visible = true;
    h.app.workspace.drawer_tab = 3;
    h.app.workspace.focus = FocusRegion::Drawer;

    events::execute_command(&mut h.app, "extensions");
    assert!(h.app.extension_shop.is_open);

    // Pressing Esc must close Plugin Center rather than being stolen by compilation_key
    h.send_key("<Esc>");
    assert!(
        !h.app.extension_shop.is_open,
        "Esc must close Plugin Center even when Compilation split previously had focus"
    );
}

#[test]
fn paste_while_compilation_focused_does_not_corrupt_editor_buffer() {
    let mut h = Harness::new(
        Some(r#"{"mode":"ide","onboarding_completed":true}"#),
        &[("clean.txt", b"untouched\n")],
        &["clean.txt"],
    );
    h.app.workspace.drawer_visible = true;
    h.app.workspace.drawer_tab = 3;
    h.app.workspace.focus = FocusRegion::Drawer;

    events::handle_event(&mut h.app, InputEvent::Paste("CORRUPTED".into()));
    let _ = h.app.editor_rpc.flush_writes();
    h.pump_for(Duration::from_millis(120));

    assert_eq!(
        h.buf_lines(),
        vec!["untouched"],
        "pasting while Compilation split is focused must not paste into editor buffer"
    );
}

#[test]
fn ide_mode_jk_in_compilation_does_not_scroll_output_panel() {
    let mut h = Harness::new(
        Some(r#"{"mode":"ide","onboarding_completed":true}"#),
        &[("a.c", b"a\n")],
        &["a.c"],
    );
    for i in 0..10 {
        h.app.output_panel.push_line(format!("log {i}"));
    }
    h.app.output_panel.list_state.selected = 0;
    h.app.workspace.drawer_visible = true;
    h.app.workspace.drawer_tab = 3;
    h.app.workspace.focus = FocusRegion::Drawer;

    h.send_key("j");
    h.send_key("j");
    assert_eq!(
        h.app.output_panel.list_state.selected, 0,
        "IDE mode 'j' in Compilation tab must not fall through and scroll output_panel"
    );
}

#[test]
fn dismissing_compile_modal_while_job_runs_does_not_kill_job() {
    let mut h = Harness::new(
        Some(r#"{"mode":"ide","onboarding_completed":true}"#),
        &[("a.c", b"a\n")],
        &["a.c"],
    );
    let cmd = "sleep 2".to_string();
    h.app.compile_widget.open_with(
        h.work.to_string_lossy().into(),
        cmd.clone(),
        cmd,
        "long".into(),
    );
    h.app.compile_widget.start();
    assert!(h.app.compile_widget.is_running());

    // Simulate user clicking "Command..." while job is still running, then pressing Esc to dismiss modal.
    h.app.compile_widget.is_open = true;
    h.send_key("<Esc>");
    assert!(!h.app.compile_widget.is_open);
    let still_running = h.app.compile_widget.is_running();
    h.app.compile_widget.stop();
    assert!(
        still_running,
        "dismissing the Compile modal with Esc must not kill a running background compile job"
    );
}

#[test]
fn background_compile_failure_does_not_hijack_focused_terminal_tab() {
    let mut h = Harness::new(
        Some(r#"{"mode":"ide","onboarding_completed":true}"#),
        &[("a.c", b"a\n")],
        &["a.c"],
    );
    let cmd = "sleep 0.15; printf 'a.c:1:1: error: boom\\n'; exit 1".to_string();
    h.app.compile_widget.open_with(
        h.work.to_string_lossy().into(),
        cmd.clone(),
        cmd,
        "bg".into(),
    );
    h.app.compile_widget.start();
    h.app.poll_background_tasks();
    assert_eq!(h.app.workspace.drawer_tab, 3);

    // User switches to Terminal tab and starts working in the shell while compile runs.
    h.app.workspace.drawer_tab = 0;
    h.app.workspace.focus = FocusRegion::Drawer;

    for _ in 0..80 {
        h.app.poll_background_tasks();
        if !h.app.compile_widget.is_running() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(h.app.compile_widget.stage, CompileStage::Failed(1));
    assert_eq!(
        h.app.workspace.drawer_tab, 0,
        "background compile failure must not rip Terminal tab away while Terminal owns focus"
    );
}
