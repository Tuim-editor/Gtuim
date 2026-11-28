pub mod chrome;
pub mod drag_drop;
pub mod find_replace;
pub mod grid_canvas;
pub mod icons;
pub mod keymap;
pub mod modals;
mod modal_layout;
mod markdown_preview;
pub mod pdf_preview;
mod pdf_split;
pub mod image_preview;
#[cfg(test)]
mod modal_tests;
mod onboarding;
mod help;
mod welcome;
mod compilation;
mod plugin_center;
pub mod sidebar;
mod scroll;
mod profiles;
mod profile_panel;
pub mod practice;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{prelude::*, *};
use rmpv::Value;

use crate::nvim::helpers::handle_rpc_message;
use crate::nvim::process::NvimProcess;
use crate::nvim::rpc::{AsyncTag, RpcClient, RpcMessage};
use crate::tui::app::{App as TuimApp, FocusRegion, Mode, PaletteMode};
use crate::tui::capabilities::Capabilities;
use crate::tui::events;
use crate::tui::input::InputEvent;

use grid_canvas::{
    collect_system_fonts, font_weight_from_u16, resolve_configured_mono_font, resolve_mono_font,
    resolve_ui_font, update_nvim_buffer, update_shell_buffer, CellGridElement, GridBuffer,
    GridMetrics,
};
use icons::{gui_palette, icon};
use crate::tui::theme::blend_u32;

const EMBED_TUIM_INIT_LUA: &str = include_str!("../nvim/tuim_init.lua");
const EMBED_TUIM_SETTINGS_LUA: &str = include_str!("../nvim/tuim_settings.lua");
const EMBED_DASHBOARD_LUA: &str = include_str!("../nvim/dashboard.lua");
const EMBED_STORE_SEARCH_PY: &str = include_str!("../nvim/store_search.py");
const EMBED_PLUGIN_RECIPES: &str = include_str!("../nvim/plugin_recipes.json");
const EMBED_SOFTWARE_UPDATE_SH: &str = include_str!("../software_update.sh");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragKind {
    None,
    Sidebar,
    Drawer,
    VerticalTerminal,
    EditorMouse,
    DrawerTermSelect,
    VerticalTermSelect,
}

pub fn resolve_dirs() -> (String, String) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let data_home = std::env::var("XDG_DATA_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{}/.local/share", home));
    let data_dir = format!("{}/tuim", data_home);
    (data_dir, home)
}

pub fn write_embedded_runtime(data_dir: &str) -> Result<String, String> {
    fs::create_dir_all(data_dir).map_err(|e| format!("Failed to create {}: {}", data_dir, e))?;
    let lua_dir = format!("{}/lua", data_dir);
    fs::create_dir_all(&lua_dir).map_err(|e| format!("Failed to create {}: {}", lua_dir, e))?;

    let files = [
        (format!("{}/tuim_init.lua", data_dir), EMBED_TUIM_INIT_LUA, false),
        (
            format!("{}/lua/tuim_settings.lua", data_dir),
            EMBED_TUIM_SETTINGS_LUA,
            false,
        ),
        (
            format!("{}/tuim_settings.lua", data_dir),
            EMBED_TUIM_SETTINGS_LUA,
            false,
        ),
        (
            format!("{}/lua/dashboard.lua", data_dir),
            EMBED_DASHBOARD_LUA,
            false,
        ),
        (
            format!("{}/dashboard.lua", data_dir),
            EMBED_DASHBOARD_LUA,
            false,
        ),
        (
            format!("{}/store_search.py", data_dir),
            EMBED_STORE_SEARCH_PY,
            true,
        ),
        (
            format!("{}/plugin_recipes.json", data_dir),
            EMBED_PLUGIN_RECIPES,
            false,
        ),
        (
            format!("{}/software_update.sh", data_dir),
            EMBED_SOFTWARE_UPDATE_SH,
            true,
        ),
    ];

    for (path, content, executable) in files {
        let needs_write = match fs::read_to_string(&path) {
            Ok(existing) => existing != content,
            Err(_) => true,
        };
        if needs_write {
            fs::write(&path, content).map_err(|e| format!("Failed to write {}: {}", path, e))?;
        }
        if executable {
            if let Ok(meta) = fs::metadata(&path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&path, perms);
            }
        }
    }

    Ok(format!("{}/tuim_init.lua", data_dir))
}

fn attach_nvim_ui(rpc: &mut RpcClient, width: u16, height: u16) -> Result<(), String> {
    let opts = Value::Map(vec![
        (Value::from("rgb"), Value::from(true)),
        (Value::from("ext_linegrid"), Value::from(true)),
    ]);
    rpc.call(
        "nvim_ui_attach",
        vec![
            Value::from(width.max(1) as i64),
            Value::from(height.max(1) as i64),
            opts,
        ],
    )?;
    Ok(())
}

/// Split CLI arguments into an optional workspace root directory (when an existing
/// directory is passed, e.g. `gtuim "folder with spaces"`) and the list of non-directory
/// file paths to open in Neovim. Relative file paths are resolved against `base_dir`
/// when a workspace root directory is present so changing CWD never breaks sibling file args.
pub(crate) fn resolve_cli_workspace_args(
    args: &[String],
    base_dir: Option<&Path>,
) -> (Option<PathBuf>, Vec<String>) {
    let mut workspace_root: Option<PathBuf> = None;
    let mut raw_files: Vec<String> = Vec::new();

    for arg in args {
        if arg.is_empty() {
            continue;
        }
        let candidate = Path::new(arg);
        let resolved = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else if let Some(base) = base_dir {
            base.join(candidate)
        } else {
            candidate.to_path_buf()
        };
        if resolved.is_dir() {
            if workspace_root.is_none() {
                workspace_root = Some(fs::canonicalize(&resolved).unwrap_or(resolved));
            }
        } else {
            raw_files.push(arg.clone());
        }
    }

    let editor_files = if workspace_root.is_some() {
        raw_files
            .into_iter()
            .map(|f| {
                let p = Path::new(&f);
                if p.is_absolute() {
                    f
                } else if let Some(base) = base_dir {
                    base.join(p).to_string_lossy().to_string()
                } else {
                    f
                }
            })
            .collect()
    } else {
        raw_files
    };

    (workspace_root, editor_files)
}

/// Returns true only for tabs backed by an explicit Markdown file extension or filetype.
/// Unnamed `[No Name]` buffers with empty filetype must never trigger the Markdown preview toggle bar.
pub(crate) fn is_markdown_tab(tab: &crate::tui::app::TabInfo) -> bool {
    if tab.filetype.eq_ignore_ascii_case("markdown") {
        return true;
    }
    let name = tab.name.trim().to_ascii_lowercase();
    !name.is_empty()
        && [".md", ".markdown", ".mdown", ".mkd"]
            .iter()
            .any(|ext| name.ends_with(ext))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectiveWorkspaceLayout {
    pub viewport_w: f32,
    pub viewport_h: f32,
    pub header_h: f32,
    pub status_h: f32,
    pub tab_bar_h: f32,
    pub rail_w: f32,
    pub show_sidebar: bool,
    pub sidebar_w: f32,
    pub show_vterm: bool,
    pub vterm_w: f32,
    pub show_drawer: bool,
    pub drawer_h: f32,
    pub compact_header: bool,
    pub ultra_compact_header: bool,
    pub compact_rail: bool,
    pub compact_status: bool,
    pub main_col_w: f32,
    pub editor_avail_w: f32,
    pub editor_avail_h: f32,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn compute_effective_workspace_layout(
    viewport_w: f32,
    viewport_h: f32,
    is_zen: bool,
    want_sidebar: bool,
    want_drawer: bool,
    want_vterm: bool,
    show_tab_bar: bool,
    extra_bar_count: usize,
    desired_sidebar_px: f32,
    desired_drawer_px: f32,
    desired_vterm_px: f32,
    focus: FocusRegion,
) -> EffectiveWorkspaceLayout {
    let vp_w = if viewport_w.is_finite() && viewport_w > 0.0 {
        viewport_w
    } else {
        1380.0
    };
    let vp_h = if viewport_h.is_finite() && viewport_h > 0.0 {
        viewport_h
    } else {
        880.0
    };

    let header_h = if is_zen {
        0.0
    } else if vp_h < 360.0 {
        30.0
    } else {
        36.0
    };
    let status_h = if vp_h < 360.0 { 22.0 } else { 26.0 };
    let tab_bar_h = if !is_zen && show_tab_bar {
        if vp_h < 360.0 {
            26.0
        } else {
            32.0
        }
    } else {
        0.0
    };
    let compact_rail = vp_h < 380.0 || vp_w < 440.0;
    let rail_w = if is_zen {
        0.0
    } else if vp_w < 400.0 {
        34.0
    } else if vp_w < 520.0 {
        38.0
    } else {
        46.0
    };

    let avail_w = (vp_w - rail_w).max(0.0);
    let min_editor_w = if vp_w >= 700.0 {
        (avail_w * 0.40).clamp(240.0, 320.0).min(avail_w)
    } else if vp_w >= 460.0 {
        (avail_w * 0.36).clamp(150.0, 220.0).min(avail_w)
    } else {
        (avail_w * 0.52).clamp(130.0, 200.0).min(avail_w)
    };
    let side_budget = (avail_w - min_editor_w).max(0.0);

    let want_sb = !is_zen && want_sidebar;
    let want_vt = want_vterm;
    let (show_sidebar, sidebar_w, show_vterm, vterm_w) = match (want_sb, want_vt) {
        (true, true) => {
            let min_sb = 110.0f32;
            let min_vt = 120.0f32;
            if side_budget >= min_sb + min_vt {
                let desired_sum = (desired_sidebar_px + desired_vterm_px).max(1.0);
                if desired_sum <= side_budget {
                    (true, desired_sidebar_px, true, desired_vterm_px)
                } else {
                    let sb_share = (side_budget * (desired_sidebar_px / desired_sum))
                        .clamp(min_sb, (side_budget - min_vt).max(min_sb))
                        .min(desired_sidebar_px);
                    let vt_share = (side_budget - sb_share)
                        .max(min_vt)
                        .min(desired_vterm_px);
                    (true, sb_share.floor(), true, vt_share.floor())
                }
            } else if side_budget >= 96.0 {
                // Only room for one secondary pane alongside the editor; prioritize focused pane.
                if focus == FocusRegion::VerticalTerminal {
                    (false, 0.0, true, desired_vterm_px.min(side_budget).floor())
                } else if focus == FocusRegion::Sidebar {
                    (true, desired_sidebar_px.min(side_budget).floor(), false, 0.0)
                } else if vp_w >= 420.0 {
                    (true, desired_sidebar_px.min(side_budget).floor(), false, 0.0)
                } else {
                    (false, 0.0, false, 0.0)
                }
            } else {
                (false, 0.0, false, 0.0)
            }
        }
        (true, false) => {
            if vp_w < 400.0 && focus != FocusRegion::Sidebar {
                (false, 0.0, false, 0.0)
            } else if side_budget >= 96.0 {
                (true, desired_sidebar_px.min(side_budget).floor(), false, 0.0)
            } else {
                (false, 0.0, false, 0.0)
            }
        }
        (false, true) => {
            if vp_w < 400.0 && focus != FocusRegion::VerticalTerminal {
                (false, 0.0, false, 0.0)
            } else if side_budget >= 96.0 {
                (false, 0.0, true, desired_vterm_px.min(side_budget).floor())
            } else {
                (false, 0.0, false, 0.0)
            }
        }
        (false, false) => (false, 0.0, false, 0.0),
    };

    let sb_splitter = if show_sidebar { 1.0 } else { 0.0 };
    let vt_splitter = if show_vterm { 1.0 } else { 0.0 };
    let main_col_w = (avail_w - sidebar_w - sb_splitter).max(0.0);
    let editor_avail_w = (main_col_w - vterm_w - vt_splitter).max(0.0);

    let extra_bars_h = (extra_bar_count as f32) * 26.0;
    let avail_h = (vp_h - header_h - status_h).max(0.0);
    let content_h = (avail_h - tab_bar_h - extra_bars_h).max(0.0);
    let min_editor_h = (content_h * 0.44).clamp(72.0, 160.0).min(content_h);
    let max_drawer_h = (content_h - min_editor_h).max(0.0);

    let (show_drawer, drawer_h) = if want_drawer {
        if max_drawer_h >= 84.0 {
            (true, desired_drawer_px.min(max_drawer_h).floor())
        } else if focus == FocusRegion::Drawer && content_h >= 96.0 {
            let h = (content_h * 0.48).clamp(52.0, desired_drawer_px).floor();
            (true, h)
        } else {
            (false, 0.0)
        }
    } else {
        (false, 0.0)
    };
    let drawer_splitter = if show_drawer { 1.0 } else { 0.0 };
    let editor_avail_h = (content_h - drawer_h - drawer_splitter).max(0.0);

    EffectiveWorkspaceLayout {
        viewport_w: vp_w,
        viewport_h: vp_h,
        header_h,
        status_h,
        tab_bar_h,
        rail_w,
        show_sidebar,
        sidebar_w,
        show_vterm,
        vterm_w,
        show_drawer,
        drawer_h,
        compact_header: vp_w < 840.0,
        ultra_compact_header: vp_w < 540.0,
        compact_rail,
        compact_status: vp_w < 580.0,
        main_col_w,
        editor_avail_w,
        editor_avail_h,
    }
}

fn boot_tuim_app(files_to_open: &[String], cols: u16, rows: u16, profile_id: Option<&str>) -> Result<(TuimApp, profiles::Profiles), String> {
    let (data_dir, home_dir) = resolve_dirs();
    write_embedded_runtime(&data_dir)?;
    let mut profiles = profiles::Profiles::load(&data_dir)?;
    if let Some(id) = profile_id { profiles.select_for_window(id)?; }

    let launch_cwd = std::env::current_dir().ok();
    let (workspace_root, editor_files) =
        resolve_cli_workspace_args(files_to_open, launch_cwd.as_deref());
    if let Some(root_dir) = &workspace_root {
        let _ = std::env::set_current_dir(root_dir);
    }

    let env_map: HashMap<String, String> = std::env::vars().collect();
    let mut capabilities = Capabilities::detect(&env_map);
    capabilities.true_color = true;
    capabilities.mouse = true;
    capabilities.bracketed_paste = true;
    capabilities.nerd_fonts_recommended = true;

    let mut nvim_env_map = env_map;
    nvim_env_map.insert("NVIM_APPNAME".to_string(), "tuim".to_string());

    let editor_proc = NvimProcess::spawn(&nvim_env_map).map_err(|e| {
        format!(
            "Failed to launch embedded Neovim ({}). Ensure `nvim` is installed and in PATH.",
            e
        )
    })?;
    let editor_rpc = RpcClient::new(editor_proc);

    let mut app = TuimApp::new(
        cols,
        rows,
        editor_rpc,
        capabilities,
        data_dir.clone(),
        home_dir,
    );

    profile_panel::load_profile_settings(&mut app, &profiles)?;
    attach_nvim_ui(&mut app.editor_rpc, cols.max(20), rows.max(6))?;
    let _ = app
        .editor_rpc
        .call("nvim_command", vec![Value::from("set laststatus=0 noruler")]);
    let _ = app.editor_rpc.call(
        "nvim_command",
        vec![Value::from(
            "autocmd BufWritePost * let b:tuim_session_saved = 1",
        )],
    );

    let escaped_data = data_dir.replace('\'', "''");
    let always_split_headers = if app.workspace.tab_bar_visible { "false" } else { "true" };
    let file_vals: Vec<Value> = editor_files.iter().map(|s| Value::from(s.as_str())).collect();
    let editor_boot_lua = format!(
        "local files = select(1, ...)\n\
         vim.g.tuim_gui_split_headers = true\n\
         vim.g.tuim_always_split_headers = {1}\n\
         vim.g.tuim_gui_compile = true\n\
         vim.g.tuim_gui_onboarding = true\n\
         package.path = '{0}/lua/?.lua;' .. package.path\n\
         dofile('{0}/tuim_init.lua')\n\
         vim.o.ruler = false\n\
         pcall(function() vim.opt.mousescroll = 'ver:1,hor:1' end)\n\
         pcall(function() vim.opt.smoothscroll = true end)\n\
         if #files > 0 then\n\
             local orig_alpha = _G.tuim_alpha_start\n\
             _G.tuim_alpha_start = function()\n\
                 _G.tuim_alpha_start = orig_alpha\n\
             end\n\
             while #vim.api.nvim_win_get_config(0).relative > 0 do vim.cmd('close') end\n\
             vim.cmd('edit ' .. vim.fn.fnameescape(files[1]))\n\
             for i = 2, #files do\n\
                 vim.cmd('badd ' .. vim.fn.fnameescape(files[i]))\n\
             end\n\
         end\n\
         vim.schedule(function()\n\
             vim.schedule(function()\n\
                 vim.schedule(function()\n\
                     pcall(vim.rpcnotify, 1, 'tuim_startup_done')\n\
                 end)\n\
             end)\n\
         end)",
        escaped_data, always_split_headers
    );
    app.editor_rpc.call(
        "nvim_exec_lua",
        vec![
            Value::from(editor_boot_lua),
            Value::Array(vec![Value::Array(file_vals)]),
        ],
    ).map_err(|error| format!("Neovim runtime initialization failed: {error}"))?;

    let startup_deadline = Instant::now() + Duration::from_millis(500);
    let mut startup_done = false;
    while !startup_done && Instant::now() < startup_deadline {
        let _ = app.editor_rpc.pump_incoming();
        while let Some(msg) = app.editor_rpc.next_message() {
            if let RpcMessage::Notification { ref method, .. } = msg {
                if method == "tuim_startup_done" {
                    startup_done = true;
                }
            }
            handle_rpc_message(&mut app, msg);
        }
        if !startup_done {
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    app.maybe_open_first_run_onboarding();
    Ok((app, profiles))
}

pub struct GtuimView {
    pub app: TuimApp,
    pub(super) profiles: profiles::Profiles,
    practice_panels: HashMap<String, profile_panel::PracticeHost>,
    pub root_focus: FocusHandle,
    pub sidebar_px: f32,
    pub drawer_px: f32,
    pub vterm_px: f32,
    pub font_size: f32,
    pub font_weight: FontWeight,
    pub default_mono_font: SharedString,
    pub available_fonts: Vec<String>,
    pub mono_font: SharedString,
    pub ui_font: SharedString,
    pub find_replace: find_replace::FindReplaceState,
    pub editor_metrics: Rc<RefCell<GridMetrics>>,
    pub drawer_metrics: Rc<RefCell<GridMetrics>>,
    pub vterm_metrics: Rc<RefCell<GridMetrics>>,
    pub editor_buffer: Rc<RefCell<GridBuffer>>,
    pub drawer_buffer: Rc<RefCell<GridBuffer>>,
    pub vterm_buffer: Rc<RefCell<GridBuffer>>,
    pub editor_ctx_pos: Point<Pixels>,
    pub explorer_ctx_pos: Point<Pixels>,
    pub drag_kind: DragKind,
    pub explorer_drag: Option<drag_drop::ExplorerDragState>,
    pub split_drag: Option<drag_drop::SplitDragState>,
    pub last_editor_pane_bounds: Option<Bounds<Pixels>>,
    pub files_to_open: Vec<String>,
    last_title: String,
    last_editor_cell_size: (u32, u32),
    pub compile_scroll: ScrollHandle,
    pub compilation_scroll: ScrollHandle,
    pub markdown_preview: markdown_preview::MarkdownPreview,
    pub pdf_preview: pdf_preview::PdfPreview,
    pub image_preview: image_preview::ImagePreview,
    last_compile_output_version: u64,
    pub tab_scroll_start: usize,
    last_active_tab: usize,
    last_tab_bar_w: f32,
    last_known_tab_paths: Vec<String>,
    pub doc_cursor: Option<(usize, usize)>,
    editor_wheel: scroll::WheelAccumulator,
    drawer_wheel: scroll::WheelAccumulator,
    vterm_wheel: scroll::WheelAccumulator,
    _poll_task: Task<()>,
}

fn wait_for_backend_fds(fds: [i32; 4], timeout_ms: i32) {
    let mut pfds = [libc::pollfd {
        fd: -1,
        events: libc::POLLIN,
        revents: 0,
    }; 4];
    let mut count = 0usize;
    for fd in fds {
        if fd >= 0 {
            pfds[count] = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            count += 1;
        }
    }
    if count == 0 {
        std::thread::sleep(Duration::from_millis(timeout_ms.max(1) as u64));
        return;
    }
    unsafe {
        let _ = libc::poll(pfds.as_mut_ptr(), count as libc::nfds_t, timeout_ms);
    }
}

impl GtuimView {
    pub fn new(
        files_to_open: Vec<String>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Result<Self, String> {
        let (mut app, profiles) = boot_tuim_app(&files_to_open, 110, 34, None)?;
        cx.on_release(|this, cx| this.flush_practice_drafts(cx)).detach();
        cx.on_app_quit(|this, cx| { this.flush_practice_drafts(cx); async {} }).detach();
        let root_focus = cx.focus_handle();
        let window_handle = window.window_handle();
        let default_mono_font = resolve_mono_font(window);
        let available_fonts = collect_system_fonts(window);
        app.settings_widget.set_available_fonts(available_fonts.clone());
        let mono_font = resolve_configured_mono_font(
            &app.settings_widget.config.font_family,
            &default_mono_font,
            &available_fonts,
        );
        let font_weight = font_weight_from_u16(app.settings_widget.config.font_weight);
        let ui_font = resolve_ui_font(window);

        let poll_task = cx.spawn(async move |view, cx| loop {
            let fds_res = cx.update_window(window_handle, |_, _, cx| {
                view.read_with(cx, |this, _| this.pollable_fds())
            });
            let Ok(Ok((fds, timeout_ms))) = fds_res else {
                break;
            };

            cx.background_executor()
                .spawn(async move {
                    wait_for_backend_fds(fds, timeout_ms);
                })
                .await;

            if cx
                .update_window(window_handle, |_, window, cx| {
                    view.update(cx, |this, cx| this.poll_backend(window, cx))
                })
                .is_err()
            {
                break;
            }
        });

        let initial_font_size = app.settings_widget.config.font_size.clamp(9, 26) as f32;
        let mut view = Self {
            app,
            profiles,
            practice_panels: HashMap::new(),
            root_focus,
            sidebar_px: 256.0,
            drawer_px: 230.0,
            vterm_px: 400.0,
            font_size: initial_font_size,
            font_weight,
            default_mono_font,
            available_fonts,
            mono_font,
            ui_font,
            find_replace: find_replace::FindReplaceState::default(),
            editor_metrics: Rc::new(RefCell::new(GridMetrics::default())),
            drawer_metrics: Rc::new(RefCell::new(GridMetrics::default())),
            vterm_metrics: Rc::new(RefCell::new(GridMetrics::default())),
            editor_buffer: Rc::new(RefCell::new(GridBuffer::default())),
            drawer_buffer: Rc::new(RefCell::new(GridBuffer::default())),
            vterm_buffer: Rc::new(RefCell::new(GridBuffer::default())),
            editor_ctx_pos: point(px(120.0), px(120.0)),
            explorer_ctx_pos: point(px(80.0), px(120.0)),
            drag_kind: DragKind::None,
            explorer_drag: None,
            split_drag: None,
            last_editor_pane_bounds: None,
            files_to_open,
            last_title: String::new(),
            last_editor_cell_size: (0, 0),
            compile_scroll: ScrollHandle::new(),
            compilation_scroll: ScrollHandle::new(),
            markdown_preview: markdown_preview::MarkdownPreview::default(),
            pdf_preview: pdf_preview::PdfPreview::default(),
            image_preview: image_preview::ImagePreview::default(),
            last_compile_output_version: 0,
            tab_scroll_start: 0,
            last_active_tab: 0,
            last_tab_bar_w: 0.0,
            last_known_tab_paths: Vec::new(),
            doc_cursor: None,
            editor_wheel: scroll::WheelAccumulator::default(),
            drawer_wheel: scroll::WheelAccumulator::default(),
            vterm_wheel: scroll::WheelAccumulator::default(),
            _poll_task: poll_task,
        };
        view.sync_pdf_preview(cx);
        view.sync_image_preview(cx);
        Ok(view)
    }

    fn pollable_fds(&self) -> ([i32; 4], i32) {
        let fds = [
            self.app.editor_rpc.stdout_fd(),
            self.app.shell_terminal.fd(),
            self.app.vertical_terminal.fd(),
            self.app.task_runner.notifier_fd(),
        ];
        let timeout_ms = if self.app.notice.is_some()
            || self.app.bug_report.is_open
            || self.app.compile_widget.is_open
            || self.app.compile_widget.is_running()
            || self.app.workspace.drawer_visible
            || self.app.workspace.vertical_terminal_visible
        {
            25
        } else {
            100
        };
        (fds, timeout_ms)
    }

    fn has_overlay(&self) -> bool {
        self.app.workspace.palette_open
            || self.app.settings_widget.is_open
            || self.app.mason_widget.is_open
            || self.app.lazy_widget.is_open
            || self.app.git_detailed_widget.is_open
            || self.app.orchestrator.is_open
            || self.app.bug_report.is_open
            || self.app.compile_widget.is_open
            || self.app.onboarding.is_open
            || self.app.extension_shop.is_open
            || self.app.editor_context_menu.is_open
            || self.app.explorer.context_menu_open
            || self.app.menu_bar.open_menu.is_some()
    }

    fn sync_grid_dimensions(&mut self) -> bool {
        let mut changed = false;

        let em = *self.editor_metrics.borrow();
        let cell_key = (f32::from(em.cell_w).to_bits(), f32::from(em.cell_h).to_bits());
        if self.last_editor_cell_size != cell_key {
            self.last_editor_cell_size = cell_key;
            changed = true;
        }
        let ew = em.cols.max(10) as usize;
        let eh = em.rows.max(4) as usize;
        if self.app.ui_state.grid.width != ew || self.app.ui_state.grid.height != eh {
            self.app.ui_state.grid.resize(ew, eh);
            let _ = self.app.editor_rpc.notify(
                "nvim_ui_try_resize",
                vec![Value::from(ew as i64), Value::from(eh as i64)],
            );
            changed = true;
        }

        if self.app.workspace.drawer_visible && self.app.workspace.drawer_tab == 0 {
            let dm = *self.drawer_metrics.borrow();
            let dw = dm.cols.max(10);
            let dh = dm.rows.max(2);
            self.app.shell_terminal.set_theme(self.app.theme);
            if !self.app.shell_terminal.is_running() {
                self.app.shell_terminal.ensure_running(dw, dh);
                changed = true;
            } else if self.app.shell_terminal.screen().size() != (dh, dw) {
                self.app.shell_terminal.resize(dw, dh);
                changed = true;
            }
        }

        if self.app.workspace.vertical_terminal_visible {
            let vm = *self.vterm_metrics.borrow();
            let vw = vm.cols.max(10);
            let vh = vm.rows.max(2);
            self.app.vertical_terminal.set_theme(self.app.theme);
            if !self.app.vertical_terminal.is_running() {
                self.app.vertical_terminal.ensure_running(vw, vh);
                changed = true;
            } else if self.app.vertical_terminal.screen().size() != (vh, vw) {
                self.app.vertical_terminal.resize(vw, vh);
                changed = true;
            }
        }

        changed
    }

    fn sync_terminal_clipboard(&mut self, cx: &mut Context<Self>) {
        for term in [&mut self.app.shell_terminal, &mut self.app.vertical_terminal] {
            if let Some(text) = term.copy_requested.take() {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                let _ = self.app.editor_rpc.notify(
                    "nvim_exec_lua",
                    vec![
                        Value::from(r#"local text = ...; vim.fn.setreg('"', text)"#),
                        Value::Array(vec![Value::from(text)]),
                    ],
                );
            }
            if term.paste_requested {
                term.paste_requested = false;
                if let Some(item) = cx.read_from_clipboard() {
                    if let Some(text) = item.text() {
                        term.paste(&text);
                    }
                }
            }
        }
    }

    pub fn update_doc_cursor(&mut self, params: &[Value]) {
        if let (Some(ln), Some(col)) = (
            params.first().and_then(|v| v.as_u64()),
            params.get(1).and_then(|v| v.as_u64()),
        ) {
            self.doc_cursor = Some((ln.max(1) as usize, col.max(1) as usize));
            return;
        }
        if let Some(Value::Map(entries)) = params.first() {
            let mut ln = None;
            let mut col = None;
            for (k, v) in entries {
                if let Some(ks) = k.as_str() {
                    match ks {
                        "line" | "ln" | "row" => ln = v.as_u64().map(|n| n.max(1) as usize),
                        "col" | "column" => col = v.as_u64().map(|n| n.max(1) as usize),
                        _ => {}
                    }
                }
            }
            if let (Some(l), Some(c)) = (ln, col) {
                self.doc_cursor = Some((l, c));
            }
        }
    }

    pub fn effective_doc_cursor(&self) -> (usize, usize) {
        if let (Some(ln), Some(col)) = (self.app.doc_cursor_line, self.app.doc_cursor_col) {
            (ln.max(1), col.max(1))
        } else if let Some((ln, col)) = self.doc_cursor {
            (ln, col)
        } else {
            (
                self.app.ui_state.cursor_row + 1,
                self.app.ui_state.cursor_col + 1,
            )
        }
    }

    pub(crate) fn handle_rpc_with_gui_sync(&mut self, msg: RpcMessage) {
        let mut preserve_prev_bufnr: Option<i64> = None;
        let mut incoming_current_buf: Option<i64> = None;
        let mut refresh_explorer = false;
        let mut was_buffers_msg = false;

        if let RpcMessage::Notification { method, params } = &msg {
            match method.as_str() {
                "gtuim_code_activity" => {
                    let language = params.first().and_then(Value::as_str).unwrap_or("");
                    let saved = params.get(1).and_then(Value::as_bool).unwrap_or(false);
                    self.profiles.record(language, saved);
                }
                "gtuim_markdown_preview" => {
                    self.markdown_preview.update(params);
                }
                "gtuim_find_replace_state" => {
                    self.find_replace.update_from_rpc(params);
                }
                "tuim_cursor" | "tuim_cursor_pos" => {
                    self.update_doc_cursor(params);
                }
                "tuim_file_saved" | "tuim_fs_changed" => {
                    refresh_explorer = true;
                }
                "tuim_notice" => {
                    let level = params.first().and_then(|v| v.as_str()).unwrap_or("");
                    let text = params.get(1).and_then(|v| v.as_str()).unwrap_or("");
                    if level == "info"
                        && (text.starts_with("Saved") || text.starts_with("Wrote"))
                    {
                        refresh_explorer = true;
                    }
                }
                "tuim_buffers" => {
                    was_buffers_msg = true;
                    preserve_prev_bufnr =
                        self.app.tabs.get(self.app.active_tab).map(|t| t.bufnr);
                    incoming_current_buf = params.get(1).and_then(|v| v.as_i64());
                }
                _ => {}
            }
        }

        handle_rpc_message(&mut self.app, msg);

        if was_buffers_msg {
            if let Some(cur_buf) = incoming_current_buf {
                let current_is_listed = self.app.tabs.iter().any(|t| t.bufnr == cur_buf);
                if !current_is_listed {
                    if let Some(prev_buf) = preserve_prev_bufnr {
                        if let Some(idx) =
                            self.app.tabs.iter().position(|t| t.bufnr == prev_buf)
                        {
                            self.app.active_tab = idx;
                        }
                    }
                }
            }

            let new_paths: Vec<String> = self
                .app
                .tabs
                .iter()
                .filter(|t| !t.name.is_empty())
                .map(|t| t.name.clone())
                .collect();
            if new_paths != self.last_known_tab_paths {
                let added_or_renamed = new_paths
                    .iter()
                    .any(|p| !self.last_known_tab_paths.contains(p));
                self.last_known_tab_paths = new_paths;
                if added_or_renamed {
                    refresh_explorer = true;
                }
            }
        }

        if refresh_explorer {
            self.app.explorer.refresh();
            self.app.request_git_refresh();
        }
    }

    fn poll_backend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut dirty = self.profiles.flush_if_due();
        if self.profiles.open && self.profiles.active().practice_enabled {
            if let Some(host) = self.practice_panels.get(&self.profiles.active().id) {
                profile_panel::refresh_practice_day(host, cx);
            }
        }

        match self.app.editor_rpc.pump_incoming() {
            Ok(true) => {}
            _ => {
                self.app.should_quit = true;
            }
        }

        while let Some(msg) = self.app.editor_rpc.next_message() {
            self.handle_rpc_with_gui_sync(msg);
            dirty = true;
        }

        if self.app.shell_terminal.pump() {
            dirty = true;
        }
        if self.app.vertical_terminal.pump() {
            dirty = true;
        }
        if let Some(err) = self.app.shell_terminal.take_error() {
            self.app.set_notice("error", &err);
            dirty = true;
        }
        if let Some(err) = self.app.vertical_terminal.take_error() {
            self.app.set_notice("error", &err);
            dirty = true;
        }
        if self.app.poll_background_tasks() {
            dirty = true;
        }
        if self.sync_grid_dimensions() {
            dirty = true;
        }
        if self.app.toggle_pdf_preview_requested {
            self.app.toggle_pdf_preview_requested = false;
            self.toggle_pdf_preview(cx);
            dirty = true;
        }
        if self.app.open_pdf_in_split_requested {
            self.app.open_pdf_in_split_requested = false;
            self.open_pdf_in_split(cx);
            dirty = true;
        }
        if self.app.toggle_image_preview_requested {
            self.app.toggle_image_preview_requested = false;
            self.toggle_image_preview(cx);
            dirty = true;
        }
        if self.consume_find_replace_request(cx) {
            dirty = true;
        }
        self.sync_pdf_preview(cx);
        self.sync_image_preview(cx);

        self.sync_terminal_clipboard(cx);

        let _ = self.app.editor_rpc.flush_writes();
        self.app.shell_terminal.flush_input();
        self.app.vertical_terminal.flush_input();

        if self.app.restart_requested {
            self.app.restart_requested = false;
            let em = *self.editor_metrics.borrow();
            match boot_tuim_app(&self.files_to_open, em.cols.max(40), em.rows.max(12), Some(&self.profiles.active().id)) {
                Ok((new_app, _)) => {
                    self.app = new_app;
                    self.sync_profile_typography();
                    if self.markdown_preview.open {
                        self.markdown_preview.open = false;
                        self.toggle_markdown_preview(cx);
                    }
                    self.app.set_notice("info", "Restarted Neovim session");
                    dirty = true;
                }
                Err(e) => {
                    self.app.set_notice("error", &e);
                    dirty = true;
                }
            }
        }

        if self.app.should_quit {
            cx.quit();
            return;
        }

        self.sync_practice_widget(window, cx);
        let title = format!("Gtuim — {}", self.app.active_tab_display_name());
        if title != self.last_title {
            window.set_window_title(&title);
            self.last_title = title;
        }

        if dirty || self.app.invalidations.has_damage() {
            let _ = self.app.invalidations.consume_paint();
            cx.notify();
        }
    }

    pub fn sync_font_from_settings(&mut self) {
        let cfg = &self.app.settings_widget.config;
        self.font_size = cfg.font_size.clamp(9, 26) as f32;
        self.font_weight = font_weight_from_u16(cfg.font_weight);
        self.mono_font = resolve_configured_mono_font(
            &cfg.font_family,
            &self.default_mono_font,
            &self.available_fonts,
        );
    }

    pub fn adjust_font_size(&mut self, next: f32, cx: &mut Context<Self>) {
        let clamped = next.clamp(9.0, 26.0);
        self.font_size = clamped;
        let as_u8 = clamped.round() as u8;
        let was_dirty = self.app.settings_widget.is_dirty();
        self.app.settings_widget.config.font_size = as_u8;
        if !was_dirty {
            self.app.settings_widget.saved_config.font_size = as_u8;
        }
        cx.notify();
    }

    pub fn open_find_replace(&mut self, with_replace: bool, cx: &mut Context<Self>) {
        self.app.extension_shop.is_open = false;
        self.app.menu_bar.open_menu = None;
        self.app.workspace.focus = FocusRegion::Editor;
        self.find_replace.open(with_replace);
        if !self.find_replace.query.is_empty() {
            find_replace::send_sync_search(&mut self.app.editor_rpc, &self.find_replace, true);
            let _ = self.app.editor_rpc.flush_writes();
        }
        cx.notify();
    }

    pub fn close_find_replace(&mut self, cx: &mut Context<Self>) {
        if self.find_replace.is_open {
            self.find_replace.close();
            find_replace::send_clear_highlights(&mut self.app.editor_rpc);
            let _ = self.app.editor_rpc.flush_writes();
            self.app.workspace.focus = FocusRegion::Editor;
            cx.notify();
        }
    }

    pub fn switch_find_replace_to_vim(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.find_replace.clone();
        self.find_replace.close();
        self.app.workspace.focus = FocusRegion::Editor;
        find_replace::send_vim_handoff(&mut self.app.editor_rpc, &snapshot);
        let _ = self.app.editor_rpc.flush_writes();
        cx.notify();
    }

    fn consume_find_replace_request(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(with_replace) = self.app.open_find_replace_requested.take() {
            self.open_find_replace(with_replace, cx);
            true
        } else {
            false
        }
    }

    pub fn run_command(&mut self, cmd: &str, cx: &mut Context<Self>) {
        if cmd == "commands" {
            events::open_palette(&mut self.app, PaletteMode::Commands);
        } else if cmd == "toggle_pdf_preview" {
            self.toggle_pdf_preview(cx);
            return;
        } else if cmd == "toggle_image_preview" {
            self.toggle_image_preview(cx);
            return;
        } else {
            events::execute_command(&mut self.app, cmd);
        }
        if self.app.toggle_pdf_preview_requested {
            self.app.toggle_pdf_preview_requested = false;
            self.toggle_pdf_preview(cx);
        }
        if self.app.open_pdf_in_split_requested {
            self.app.open_pdf_in_split_requested = false;
            self.open_pdf_in_split(cx);
        }
        if self.app.toggle_image_preview_requested {
            self.app.toggle_image_preview_requested = false;
            self.toggle_image_preview(cx);
        }
        self.consume_find_replace_request(cx);
        self.sync_grid_dimensions();
        let _ = self.app.editor_rpc.flush_writes();
        cx.notify();
    }

    pub fn open_menu_action(&mut self, action: &str, cx: &mut Context<Self>) {
        self.run_command(action, cx);
    }

    pub fn send_tuim_key(&mut self, key: &str, cx: &mut Context<Self>) {
        if modals::handle_bug_report_tuim_key(self, key, cx) {
            return;
        }
        let prev_sb = self.app.sidebar_w;
        let prev_th = self.app.terminal_h;
        let prev_vt = self.app.vertical_terminal_w;
        let was_onboarding = self.app.onboarding.is_open;
        let was_settings = self.app.settings_widget.is_open;
        let was_plain_editor = self.app.workspace.focus == FocusRegion::Editor && !self.has_overlay();

        let prev_diagnostic = (self.app.compile_widget.selected_diagnostic, self.app.compile_widget.visited_diagnostic);
        events::handle_event(&mut self.app, InputEvent::Key(key.to_string()));
        let diagnostic = (self.app.compile_widget.selected_diagnostic, self.app.compile_widget.visited_diagnostic);
        if self.app.workspace.drawer_visible && self.app.workspace.drawer_tab == 3 && diagnostic != prev_diagnostic {
            self.compilation_scroll.scroll_to_item(self.app.compile_widget.selected_diagnostic);
        }

        if self.app.toggle_pdf_preview_requested {
            self.app.toggle_pdf_preview_requested = false;
            self.toggle_pdf_preview(cx);
        }

        if self.app.open_pdf_in_split_requested {
            self.app.open_pdf_in_split_requested = false;
            self.open_pdf_in_split(cx);
        }
        if self.app.toggle_image_preview_requested {
            self.app.toggle_image_preview_requested = false;
            self.toggle_image_preview(cx);
        }
        self.consume_find_replace_request(cx);

        if (was_onboarding && !self.app.onboarding.is_open) || was_settings {
            self.sync_font_from_settings();
        }

        if self.app.sidebar_w != prev_sb {
            let delta = (self.app.sidebar_w as f32 - prev_sb as f32) * 10.0;
            self.sidebar_px = (self.sidebar_px + delta).clamp(160.0, 640.0);
        }
        if self.app.terminal_h != prev_th {
            let delta = (self.app.terminal_h as f32 - prev_th as f32) * 20.0;
            self.drawer_px = (self.drawer_px + delta).clamp(100.0, 560.0);
        }
        if self.app.vertical_terminal_w != prev_vt {
            let delta = (self.app.vertical_terminal_w as f32 - prev_vt as f32) * 10.0;
            self.vterm_px = (self.vterm_px + delta).clamp(180.0, 760.0);
        }

        self.sync_terminal_clipboard(cx);
        let resized = self.sync_grid_dimensions();
        let _ = self.app.editor_rpc.flush_writes();
        self.app.shell_terminal.flush_input();
        self.app.vertical_terminal.flush_input();

        // If this was a pure Neovim keystroke with no chrome/overlay change, skip redundant
        // pre-reply paint; the kernel poll() watcher wakes in <0.5ms when Neovim emits redraw.
        if !was_plain_editor || self.has_overlay() || resized || self.app.invalidations.has_damage() {
            let _ = self.app.invalidations.consume_paint();
            cx.notify();
        }
    }

    pub fn select_buffer(&mut self, bufnr: i64, cx: &mut Context<Self>) {
        self.app.extension_shop.is_open = false;
        self.app.workspace.focus = FocusRegion::Editor;
        let lua = format!(
            "if _G.tuim_select_buffer then _G.tuim_select_buffer({}) else vim.api.nvim_set_current_buf({}) end",
            bufnr, bufnr
        );
        let _ = self
            .app
            .editor_rpc
            .notify("nvim_exec_lua", vec![Value::from(lua), Value::Array(vec![])]);
        let _ = self.app.editor_rpc.flush_writes();
        cx.notify();
    }

    pub fn close_buffer(&mut self, bufnr: i64, cx: &mut Context<Self>) {
        self.app.workspace.focus = FocusRegion::Editor;
        let lua = format!(
            "if _G.tuim_close_buffer then _G.tuim_close_buffer({}) else vim.cmd('confirm bdelete {}') end",
            bufnr, bufnr
        );
        let _ = self.app.editor_rpc.request_async(
            "nvim_exec_lua",
            vec![Value::from(lua), Value::Array(vec![])],
            AsyncTag::QuitConfirmation,
        );
        let _ = self.app.editor_rpc.flush_writes();
        cx.notify();
    }

    pub fn focus_split(&mut self, winid: i64, cx: &mut Context<Self>) {
        self.app.workspace.focus = FocusRegion::Editor;
        self.app.editor_context_menu.is_open = false;
        let lua = format!(
            "if _G.tuim_focus_split then _G.tuim_focus_split({}) elseif vim.api.nvim_win_is_valid({}) then vim.api.nvim_set_current_win({}) end",
            winid, winid, winid
        );
        let _ = self
            .app
            .editor_rpc
            .notify("nvim_exec_lua", vec![Value::from(lua), Value::Array(vec![])]);
        let _ = self.app.editor_rpc.flush_writes();
        cx.notify();
    }

    pub fn close_split(&mut self, winid: i64, bufnr: i64, cx: &mut Context<Self>) {
        self.app.workspace.focus = FocusRegion::Editor;
        self.app.editor_context_menu.is_open = false;
        let lua = format!(
            "if _G.tuim_close_split then _G.tuim_close_split({}, {}) elseif vim.api.nvim_win_is_valid({}) then pcall(vim.api.nvim_win_close, {}, false) end",
            winid, bufnr, winid, winid
        );
        let _ = self
            .app
            .editor_rpc
            .notify("nvim_exec_lua", vec![Value::from(lua), Value::Array(vec![])]);
        let _ = self.app.editor_rpc.flush_writes();
        cx.notify();
    }

    pub fn keep_only_split(&mut self, winid: i64, cx: &mut Context<Self>) {
        self.app.workspace.focus = FocusRegion::Editor;
        self.app.editor_context_menu.is_open = false;
        let lua = format!(
            "if _G.tuim_only_split then _G.tuim_only_split({}) else pcall(vim.cmd, 'only') end",
            winid
        );
        let _ = self
            .app
            .editor_rpc
            .notify("nvim_exec_lua", vec![Value::from(lua), Value::Array(vec![])]);
        let _ = self.app.editor_rpc.flush_writes();
        cx.notify();
    }

    pub fn move_split(
        &mut self,
        src_win: i64,
        kind: drag_drop::SplitMoveKind,
        cx: &mut Context<Self>,
    ) {
        self.split_drag = None;
        self.app.workspace.focus = FocusRegion::Editor;
        self.app.editor_context_menu.is_open = false;
        let (action, target_win) = kind.to_lua_args();
        let lua = "local src_win, action, target_win = ...\n\
            if _G.tuim_move_split then\n\
                _G.tuim_move_split(src_win, action, target_win)\n\
            end";
        let _ = self.app.editor_rpc.notify(
            "nvim_exec_lua",
            vec![
                Value::from(lua),
                Value::Array(vec![
                    Value::from(src_win),
                    Value::from(action),
                    Value::from(target_win),
                ]),
            ],
        );
        let _ = self.app.editor_rpc.flush_writes();
        cx.notify();
    }

    pub fn drop_explorer_item(
        &mut self,
        src_path: &str,
        dest_dir: &str,
        cx: &mut Context<Self>,
    ) {
        self.explorer_drag = None;
        self.app.workspace.focus = FocusRegion::Sidebar;
        match self.app.explorer.move_entry(src_path, dest_dir) {
            Ok(Some(new_path)) => {
                let name = std::path::Path::new(&new_path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("item");
                let dest_label = std::path::Path::new(dest_dir)
                    .strip_prefix(&self.app.explorer.root_dir)
                    .ok()
                    .and_then(|p| p.to_str())
                    .map(|s| s.trim_start_matches('/'))
                    .filter(|s| !s.is_empty())
                    .map(|s| format!("{}/", s))
                    .unwrap_or_else(|| "project root".to_string());
                self.app
                    .set_notice("info", &format!("Moved {} to {}", name, dest_label));
                self.after_widget_change(cx);
            }
            Ok(None) => {
                cx.notify();
            }
            Err(err) => {
                self.app.set_notice("warning", &err);
                cx.notify();
            }
        }
    }

    pub fn cycle_mode(&mut self, cx: &mut Context<Self>) {
        let next_mode = match self.app.mode {
            Mode::Ide => Mode::Normal,
            Mode::Normal => Mode::Zen,
            Mode::Zen => Mode::Ide,
        };
        if next_mode == Mode::Zen || self.app.mode == Mode::Zen {
            events::toggle_zen_mode(&mut self.app);
            if next_mode == Mode::Ide {
                self.app.mode = Mode::Ide;
            }
        } else {
            self.app.mode = next_mode;
        }
        let lua = match self.app.mode {
            Mode::Ide => "vim.g.tuim_zen_mode = false; vim.g.tuim_ide_mode = true; if _G.tuim_enable_ide_mode then _G.tuim_enable_ide_mode() end; if _G.tuim_notify_win_positions then _G.tuim_notify_win_positions() end",
            Mode::Zen => "vim.g.tuim_zen_mode = true; vim.g.tuim_ide_mode = false; if _G.tuim_disable_ide_mode then _G.tuim_disable_ide_mode() end; if _G.tuim_notify_win_positions then _G.tuim_notify_win_positions() end",
            Mode::Normal => "vim.g.tuim_zen_mode = false; vim.g.tuim_ide_mode = false; if _G.tuim_disable_ide_mode then _G.tuim_disable_ide_mode() end; if _G.tuim_notify_win_positions then _G.tuim_notify_win_positions() end",
        };
        let _ = self
            .app
            .editor_rpc
            .notify("nvim_exec_lua", vec![Value::from(lua), Value::Array(vec![])]);
        let _ = self.app.editor_rpc.flush_writes();
        cx.notify();
    }

    pub fn after_widget_change(&mut self, cx: &mut Context<Self>) {
        events::process_widget_side_effects(&mut self.app);
        self.sync_font_from_settings();
        self.consume_find_replace_request(cx);
        self.sync_grid_dimensions();
        let _ = self.app.editor_rpc.flush_writes();
        cx.notify();
    }

    pub fn preview_onboarding_theme(&mut self, theme_id: &str, cx: &mut Context<Self>) {
        self.app.onboarding.select_theme(theme_id);
        crate::tui::widgets::settings::SettingsWidget::preview_theme(
            &mut self.app.editor_rpc,
            theme_id,
        );
        let _ = self.app.editor_rpc.flush_writes();
        cx.notify();
    }

    pub fn finish_onboarding(&mut self, cx: &mut Context<Self>) {
        self.app.finish_onboarding();
        self.sync_font_from_settings();
        self.sync_grid_dimensions();
        let _ = self.app.editor_rpc.flush_writes();
        cx.notify();
    }

    pub fn cancel_onboarding(&mut self, cx: &mut Context<Self>) {
        self.app.cancel_onboarding();
        self.sync_font_from_settings();
        self.sync_grid_dimensions();
        let _ = self.app.editor_rpc.flush_writes();
        cx.notify();
    }

    pub fn diagnostics_json(&self) -> String {
        let em = *self.editor_metrics.borrow();
        self.app.metrics.to_diagnostics_json(
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            match self.app.mode {
                Mode::Ide => "ide",
                Mode::Zen => "zen",
                Mode::Normal => "normal",
            },
            &self.app.settings_widget.config.theme,
            true,
            true,
            true,
            true,
            em.cols,
            em.rows,
        )
    }

    fn handle_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if profile_panel::handle_key(self, ev, cx) { return; }
        if modals::handle_bug_report_key(self, ev, cx) { return; }
        let ks = &ev.keystroke;
        if !self.has_overlay()
            && self.find_replace.is_open
            && self.find_replace.focused
            && find_replace::handle_key_down(self, ks, cx)
        {
            return;
        }
        if !self.has_overlay() && self.app.workspace.focus == FocusRegion::Editor {
            let ctrl = ks.modifiers.control || ks.modifiers.platform;
            if ctrl && ks.modifiers.shift && !ks.modifiers.alt && ks.key.eq_ignore_ascii_case("h") {
                self.open_find_replace(true, cx);
                return;
            }
            if self.app.mode == Mode::Ide && ctrl && !ks.modifiers.shift && !ks.modifiers.alt {
                if ks.key.eq_ignore_ascii_case("f") {
                    self.run_command("find", cx);
                    return;
                }
                if ks.key.eq_ignore_ascii_case("h") {
                    self.run_command("replace", cx);
                    return;
                }
            }
        }
        if ks.modifiers.control && ks.modifiers.shift && !ks.modifiers.alt && !ks.modifiers.platform
            && ks.key.eq_ignore_ascii_case("s") && !self.has_overlay()
            && (self.app.settings_widget.config.keybindings.save_file_as.is_empty()
                || self
                    .app
                    .settings_widget
                    .config
                    .keybindings
                    .save_file_as
                    .eq_ignore_ascii_case("<C-S-s>"))
        {
            self.run_command("save_file_as", cx);
            return;
        }
        if ks.modifiers.control && ks.modifiers.shift && !ks.modifiers.alt && !ks.modifiers.platform
            && ks.key.eq_ignore_ascii_case("m") && !self.has_overlay()
            && self.app.workspace.focus == FocusRegion::Editor
        {
            self.toggle_markdown_preview(cx);
            return;
        }
        if ks.modifiers.control && ks.modifiers.shift && !ks.modifiers.alt
            && ks.key.eq_ignore_ascii_case("d") && !self.has_overlay()
        {
            self.toggle_pdf_preview(cx);
            return;
        }
        if ks.modifiers.control && ks.modifiers.shift && !ks.modifiers.alt
            && ks.key.eq_ignore_ascii_case("i") && !self.has_overlay()
        {
            self.toggle_image_preview(cx);
            return;
        }
        if ks.key == "escape"
            && (cx.has_active_drag() || self.explorer_drag.is_some() || self.split_drag.is_some())
        {
            cx.stop_active_drag(window);
            self.explorer_drag = None;
            self.split_drag = None;
            cx.notify();
            return;
        }
        if !self.has_overlay() && self.app.workspace.focus == FocusRegion::Editor {
            let handled = if self.image_preview.is_full_mode() {
                self.handle_image_key(ks, cx)
            } else {
                self.handle_pdf_key(ks, cx)
            };
            if handled {
                return;
            }
        }
        if ks.modifiers.control && !ks.modifiers.alt {
            match ks.key.as_str() {
                "=" | "+" => {
                    self.adjust_font_size(self.font_size + 1.0, cx);
                    return;
                }
                "-" => {
                    self.adjust_font_size(self.font_size - 1.0, cx);
                    return;
                }
                "0" => {
                    self.adjust_font_size(14.0, cx);
                    return;
                }
                _ => {}
            }
        }

        let is_paste_combo = ks.modifiers.control
            && ks.key.eq_ignore_ascii_case("v")
            && (ks.modifiers.shift
                || self.has_overlay()
                || self.app.workspace.focus == FocusRegion::Drawer
                || self.app.workspace.focus == FocusRegion::VerticalTerminal
                || self.app.workspace.focus == FocusRegion::Sidebar);
        if is_paste_combo {
            if let Some(item) = cx.read_from_clipboard() {
                if let Some(text) = item.text() {
                    events::handle_event(&mut self.app, InputEvent::Paste(text));
                    let _ = self.app.editor_rpc.flush_writes();
                    cx.notify();
                    return;
                }
            }
        }

        if let Some(tuim_key) = keymap::keystroke_to_tuim_key(ks) {
            if self.app.extension_shop.is_open {
                self.app.extension_shop.can_confirm_modal = true;
            }
            self.send_tuim_key(&tuim_key, cx);
        }
    }

    fn mouse_button_code(button: MouseButton, mods: Modifiers) -> u8 {
        let mut code: u8 = match button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            _ => 0,
        };
        if mods.shift {
            code |= 4;
        }
        if mods.alt {
            code |= 8;
        }
        if mods.control {
            code |= 16;
        }
        code
    }

    fn handle_root_mouse_move(
        &mut self,
        ev: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let vp = window.viewport_size();
        match self.drag_kind {
            DragKind::Sidebar => {
                let rail_w = 46.0;
                let max_w = (f32::from(vp.width) - 300.0).max(160.0);
                self.sidebar_px = (f32::from(ev.position.x) - rail_w).clamp(160.0, max_w);
                cx.notify();
            }
            DragKind::Drawer => {
                let status_h = 26.0;
                let max_h = (f32::from(vp.height) - 180.0).max(120.0);
                self.drawer_px =
                    (f32::from(vp.height) - status_h - f32::from(ev.position.y)).clamp(100.0, max_h);
                cx.notify();
            }
            DragKind::VerticalTerminal => {
                let max_w = (f32::from(vp.width) - 320.0).max(180.0);
                self.vterm_px = (f32::from(vp.width) - f32::from(ev.position.x)).clamp(180.0, max_w);
                cx.notify();
            }
            DragKind::EditorMouse => {
                let (col, row) = self.editor_metrics.borrow().pixel_to_cell(ev.position);
                let code = Self::mouse_button_code(MouseButton::Left, ev.modifiers);
                events::forward_mouse_to_nvim(&mut self.app.editor_rpc, col, row, code, true, true);
                let _ = self.app.editor_rpc.flush_writes();
            }
            DragKind::DrawerTermSelect => {
                let (col, row) = self.drawer_metrics.borrow().pixel_to_cell(ev.position);
                let code = Self::mouse_button_code(MouseButton::Left, ev.modifiers);
                self.app.shell_terminal.handle_mouse(col, row, code, true, true);
                cx.notify();
            }
            DragKind::VerticalTermSelect => {
                let (col, row) = self.vterm_metrics.borrow().pixel_to_cell(ev.position);
                let code = Self::mouse_button_code(MouseButton::Left, ev.modifiers);
                self.app
                    .vertical_terminal
                    .handle_mouse(col, row, code, true, true);
                cx.notify();
            }
            DragKind::None => {}
        }
    }

    fn handle_root_mouse_up(
        &mut self,
        ev: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.drag_kind {
            DragKind::EditorMouse => {
                let (col, row) = self.editor_metrics.borrow().pixel_to_cell(ev.position);
                let code = Self::mouse_button_code(ev.button, ev.modifiers);
                events::forward_mouse_to_nvim(
                    &mut self.app.editor_rpc,
                    col,
                    row,
                    code,
                    false,
                    false,
                );
                let _ = self.app.editor_rpc.flush_writes();
            }
            DragKind::DrawerTermSelect => {
                let (col, row) = self.drawer_metrics.borrow().pixel_to_cell(ev.position);
                let code = Self::mouse_button_code(ev.button, ev.modifiers);
                self.app
                    .shell_terminal
                    .handle_mouse(col, row, code, false, false);
                cx.notify();
            }
            DragKind::VerticalTermSelect => {
                let (col, row) = self.vterm_metrics.borrow().pixel_to_cell(ev.position);
                let code = Self::mouse_button_code(ev.button, ev.modifiers);
                self.app
                    .vertical_terminal
                    .handle_mouse(col, row, code, false, false);
                cx.notify();
            }
            _ => {}
        }
        self.drag_kind = DragKind::None;
    }

    fn render_editor_pane(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let editor_focused = self.app.workspace.focus == FocusRegion::Editor
            && !self.has_overlay()
            && !(self.find_replace.is_open && self.find_replace.focused);
        update_nvim_buffer(
            &mut self.editor_buffer.borrow_mut(),
            &self.app.ui_state,
            &self.app.theme,
            editor_focused,
            self.app.mode == Mode::Ide,
        );
        let element = CellGridElement::new(
            self.editor_buffer.clone(),
            self.editor_metrics.clone(),
            self.mono_font.clone(),
            px(self.font_size),
        )
        .with_font_weight(self.font_weight);

        let split_preview = self.split_drag.as_ref().and_then(|s| {
            s.target
                .as_ref()
                .map(|t| drag_drop::render_split_drop_preview(t, gui_palette(&self.app.theme)))
        });

        div()
            .id("nvim-editor-pane")
            .debug_selector(|| "nvim-editor-pane".into())
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .relative()
            .cursor_text()
            .on_drag_move::<drag_drop::SplitDrag>(cx.listener(
                |this, ev: &DragMoveEvent<drag_drop::SplitDrag>, window, cx| {
                    cx.set_active_drag_cursor_style(CursorStyle::ClosedHand, window);
                    this.last_editor_pane_bounds = Some(ev.bounds);
                    let drag = ev.drag(cx).clone();
                    let metrics = *this.editor_metrics.borrow();
                    let target = drag_drop::compute_split_drop_target(
                        drag.win_id,
                        &this.app.nvim_wins,
                        ev.bounds,
                        metrics,
                        ev.event.position,
                    );
                    let changed = match &this.split_drag {
                        Some(state) => state.source != drag || state.target != target,
                        None => true,
                    };
                    this.split_drag = Some(drag_drop::SplitDragState {
                        source: drag,
                        target,
                    });
                    if changed {
                        cx.notify();
                    }
                },
            ))
            .on_drop::<drag_drop::SplitDrag>(cx.listener(
                |this, drag: &drag_drop::SplitDrag, window, cx| {
                    let target = this
                        .split_drag
                        .as_ref()
                        .and_then(|s| s.target.clone())
                        .or_else(|| {
                            let bounds = this.last_editor_pane_bounds?;
                            let metrics = *this.editor_metrics.borrow();
                            drag_drop::compute_split_drop_target(
                                drag.win_id,
                                &this.app.nvim_wins,
                                bounds,
                                metrics,
                                window.mouse_position(),
                            )
                        });
                    this.split_drag = None;
                    if let Some(target) = target {
                        this.move_split(drag.win_id, target.kind, cx);
                    } else {
                        cx.notify();
                    }
                },
            ))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    window.focus(&this.root_focus);
                    this.app.workspace.focus = FocusRegion::Editor;
                    this.app.editor_context_menu.is_open = false;
                    this.find_replace.focused = false;
                    let (col, row) = this.editor_metrics.borrow().pixel_to_cell(ev.position);
                    let code = Self::mouse_button_code(MouseButton::Left, ev.modifiers);
                    events::forward_mouse_to_nvim(
                        &mut this.app.editor_rpc,
                        col,
                        row,
                        code,
                        true,
                        false,
                    );
                    this.drag_kind = DragKind::EditorMouse;
                    let _ = this.app.editor_rpc.flush_writes();
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    window.focus(&this.root_focus);
                    this.app.workspace.focus = FocusRegion::Editor;
                    this.editor_ctx_pos = ev.position;
                    this.app.editor_context_menu.open(0, 0);
                    cx.notify();
                }),
            )
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                if this.has_overlay() {
                    this.editor_wheel.reset();
                    return;
                }
                cx.stop_propagation();
                let metrics = *this.editor_metrics.borrow();
                let rows = this.editor_wheel.rows(ev.delta, f32::from(metrics.cell_h));
                let (col, row) = metrics.pixel_to_cell(ev.position);
                let button = if rows > 0 { 64 } else { 65 };
                for _ in 0..rows.unsigned_abs() {
                    events::forward_mouse_to_nvim(
                        &mut this.app.editor_rpc, col, row, button, true, false,
                    );
                }
                if rows != 0 {
                    let _ = this.app.editor_rpc.flush_writes();
                }
            }))
            .child(element)
            .children(chrome::render_split_headers(self, cx))
            .children(split_preview)
            .when(self.find_replace.is_open, |d| {
                d.child(find_replace::render_find_replace_bar(self, cx))
            })
            .when(self.app.welcome.is_some(), |d| d.child(welcome::render_welcome(self, cx)))
    }

    fn render_vertical_terminal_pane(
        &self,
        width_px: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = gui_palette(&self.app.theme);
        let focused =
            self.app.workspace.focus == FocusRegion::VerticalTerminal && !self.has_overlay();
        let (status_footer, scrollback) = {
            let mut buf = self.vterm_buffer.borrow_mut();
            update_shell_buffer(&mut buf, &self.app.vertical_terminal, &self.app.theme, focused);
            (buf.status_footer.clone(), buf.scrollback_lines)
        };
        let grid = CellGridElement::new(
            self.vterm_buffer.clone(),
            self.vterm_metrics.clone(),
            self.mono_font.clone(),
            px((self.font_size - 0.5).max(10.0)),
        )
        .with_font_weight(self.font_weight);

        div()
            .debug_selector(|| "vterm-pane".into())
            .w(px(width_px))
            .h_full()
            .min_w_0()
            .overflow_hidden()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(rgb(self.app.theme.bg_terminal))
            .child(
                div()
                    .h(px(30.0))
                    .w_full()
                    .flex_shrink_0()
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_between()
                    .bg(rgb(p.bg_panel))
                    .border_b_1()
                    .border_color(rgb(p.border_subtle))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon(
                                "terminal",
                                13.0,
                                if focused { p.accent } else { p.text_muted },
                            ))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(if focused {
                                        p.text_strong
                                    } else {
                                        p.text_muted
                                    }))
                                    .child("Terminal"),
                            )
                            .when(scrollback > 0, |d| {
                                d.child(
                                    div()
                                        .px_1()
                                        .rounded(px(3.0))
                                        .bg(rgb(p.bg_selected))
                                        .text_size(px(10.0))
                                        .text_color(rgb(p.text_strong))
                                        .child(format!("-{}", scrollback)),
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("btn-close-vterm")
                            .group("btn-close-vterm-grp")
                            .size(px(22.0))
                            .rounded(px(4.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(rgb(blend_u32(p.bg_panel, p.error, 22))))
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.run_command("toggle_vertical_terminal", cx);
                            }))
                            .child(
                                icon("x", 13.0, p.text_muted)
                                    .group_hover("btn-close-vterm-grp", move |s| {
                                        s.text_color(rgb(p.error))
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .id("vterm-canvas")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .cursor_text()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                            window.focus(&this.root_focus);
                            this.app.workspace.focus = FocusRegion::VerticalTerminal;
                            let (col, row) = this.vterm_metrics.borrow().pixel_to_cell(ev.position);
                            let code = Self::mouse_button_code(MouseButton::Left, ev.modifiers);
                            this.app
                                .vertical_terminal
                                .handle_mouse(col, row, code, true, false);
                            this.drag_kind = DragKind::VerticalTermSelect;
                            cx.notify();
                        }),
                    )
                    .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                        if this.has_overlay() {
                            this.vterm_wheel.reset();
                            return;
                        }
                        cx.stop_propagation();
                        let metrics = *this.vterm_metrics.borrow();
                        let rows = this.vterm_wheel.rows(ev.delta, f32::from(metrics.cell_h));
                        let (col, row) = metrics.pixel_to_cell(ev.position);
                        if rows != 0 {
                            let modifiers = Self::mouse_button_code(MouseButton::Left, ev.modifiers) & 28;
                            this.app.vertical_terminal.scroll_rows(col, row, rows, modifiers);
                            this.app.vertical_terminal.flush_input();
                            cx.notify();
                        }
                    }))
                    .child(grid),
            )
            .when_some(status_footer, |d, msg| {
                d.child(
                    div()
                        .px_3()
                        .py_1()
                        .bg(rgb(p.bg_surface))
                        .text_size(px(11.0))
                        .text_color(rgb(p.warning))
                        .child(msg),
                )
            })
    }

    fn render_bottom_drawer(
        &self,
        height_px: f32,
        layout: EffectiveWorkspaceLayout,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = gui_palette(&self.app.theme);
        let focused = self.app.workspace.focus == FocusRegion::Drawer && !self.has_overlay();

        let body: AnyElement = match self.app.workspace.drawer_tab {
            1 => chrome::render_console_panel(self, true).into_any_element(),
            2 => chrome::render_console_panel(self, false).into_any_element(),
            3 => compilation::render_compilation(self, cx).into_any_element(),
            _ => {
                let status_footer = {
                    let mut buf = self.drawer_buffer.borrow_mut();
                    update_shell_buffer(
                        &mut buf,
                        &self.app.shell_terminal,
                        &self.app.theme,
                        focused,
                    );
                    buf.status_footer.clone()
                };
                let grid = CellGridElement::new(
                    self.drawer_buffer.clone(),
                    self.drawer_metrics.clone(),
                    self.mono_font.clone(),
                    px((self.font_size - 0.5).max(10.0)),
                )
                .with_font_weight(self.font_weight);
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("bterm-canvas")
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .cursor_text()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                                    window.focus(&this.root_focus);
                                    this.app.workspace.focus = FocusRegion::Drawer;
                                    let (col, row) =
                                        this.drawer_metrics.borrow().pixel_to_cell(ev.position);
                                    let code =
                                        Self::mouse_button_code(MouseButton::Left, ev.modifiers);
                                    this.app
                                        .shell_terminal
                                        .handle_mouse(col, row, code, true, false);
                                    this.drag_kind = DragKind::DrawerTermSelect;
                                    cx.notify();
                                }),
                            )
                            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                                if this.has_overlay() {
                                    this.drawer_wheel.reset();
                                    return;
                                }
                                cx.stop_propagation();
                                let metrics = *this.drawer_metrics.borrow();
                                let rows = this.drawer_wheel.rows(ev.delta, f32::from(metrics.cell_h));
                                let (col, row) = metrics.pixel_to_cell(ev.position);
                                if rows != 0 {
                                    let modifiers = Self::mouse_button_code(MouseButton::Left, ev.modifiers) & 28;
                                    this.app.shell_terminal.scroll_rows(col, row, rows, modifiers);
                                    this.app.shell_terminal.flush_input();
                                    cx.notify();
                                }
                            }))
                            .child(grid),
                    )
                    .when_some(status_footer, |d, msg| {
                        d.child(
                            div()
                                .px_3()
                                .py_1()
                                .bg(rgb(p.bg_surface))
                                .text_size(px(11.0))
                                .text_color(rgb(p.warning))
                                .child(msg),
                        )
                    })
                    .into_any_element()
            }
        };

        div()
            .debug_selector(|| "bottom-drawer-pane".into())
            .h(px(height_px))
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(rgb(self.app.theme.bg_terminal))
            .child(chrome::render_drawer_header(self, layout, cx))
            .child(body)
    }
}

impl Focusable for GtuimView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.root_focus.clone()
    }
}

impl Render for GtuimView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !cx.has_active_drag() {
            self.explorer_drag = None;
            self.split_drag = None;
        }
        // Occluding overlays keep wheel events away from the grid handlers entirely.
        // Drop any sub-row gesture remainder here as well, so it cannot leak across
        // a quickly opened/dismissed menu.
        if self.has_overlay() {
            self.editor_wheel.reset();
            self.drawer_wheel.reset();
            self.vterm_wheel.reset();
        }
        if self.app.compile_widget.is_open
            && self.last_compile_output_version != self.app.compile_widget.output_version
        {
            self.last_compile_output_version = self.app.compile_widget.output_version;
            self.compile_scroll.scroll_to_bottom();
        }
        let p = gui_palette(&self.app.theme);
        let is_zen = self.app.mode == Mode::Zen;
        let show_md_toggle = !is_zen
            && !self.markdown_preview.open
            && self
                .app
                .tabs
                .get(self.app.active_tab)
                .is_some_and(is_markdown_tab);
        let show_pdf_toggle =
            !is_zen && !self.pdf_preview.open && pdf_preview::active_pdf_path(&self.app).is_some();
        let show_image_toggle = !is_zen
            && !self.image_preview.open
            && image_preview::active_image_path(&self.app).is_some();
        let extra_bar_count = usize::from(show_md_toggle)
            + usize::from(show_pdf_toggle)
            + usize::from(show_image_toggle);

        let vp = window.viewport_size();
        let layout = compute_effective_workspace_layout(
            f32::from(vp.width),
            f32::from(vp.height),
            is_zen,
            self.app.workspace.sidebar_visible,
            self.app.workspace.drawer_visible,
            self.app.workspace.vertical_terminal_visible,
            self.app.workspace.tab_bar_visible,
            extra_bar_count,
            self.sidebar_px,
            self.drawer_px,
            self.vterm_px,
            self.app.workspace.focus,
        );

        // Keep the active tab scrolled into view whenever active_tab changes or tab strip resizes.
        if self.app.tabs.is_empty() {
            self.tab_scroll_start = 0;
            self.last_active_tab = 0;
            self.last_tab_bar_w = layout.main_col_w;
        } else {
            let max_idx = self.app.tabs.len().saturating_sub(1);
            let active = self.app.active_tab.min(max_idx);
            self.tab_scroll_start = self.tab_scroll_start.min(max_idx);
            let width_changed = (layout.main_col_w - self.last_tab_bar_w).abs() > 1.0;
            if active != self.last_active_tab || width_changed {
                let widths = chrome::estimate_tab_widths(&self.app.tabs);
                let (new_start, _, _) = chrome::compute_visible_tab_range(
                    &widths,
                    active,
                    self.tab_scroll_start,
                    layout.main_col_w,
                    chrome::TAB_OVERFLOW_CONTROLS_W,
                    true,
                );
                self.tab_scroll_start = new_start;
                self.last_active_tab = active;
                self.last_tab_bar_w = layout.main_col_w;
            }
        }

        let overlays = modals::render_overlays(self, layout, cx);

        let full_image = self.image_preview.is_full_mode();
        let full_pdf = !full_image && self.pdf_preview.is_full_mode();
        let editor_and_vterm = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .w_full()
            .overflow_hidden()
            .flex()
            .when(full_image, |d| {
                d.child(image_preview::render(self, window, cx))
            })
            .when(full_pdf, |d| {
                d.child(pdf_preview::render(self, window, cx))
            })
            .when(!full_image && !full_pdf, |d| {
                d.child(self.render_editor_pane(cx))
            })
            .when(self.markdown_preview.open, |d| d.child(markdown_preview::render(self, cx)))
            .when(self.pdf_preview.is_split_mode(), |d| {
                d.child(pdf_preview::render(self, window, cx))
            })
            .when(self.image_preview.is_split_mode(), |d| {
                d.child(image_preview::render(self, window, cx))
            })
            .when(layout.show_vterm, |d| {
                d.child(
                    div()
                        .id("vterm-splitter")
                        .w(px(1.0))
                        .h_full()
                        .flex_shrink_0()
                        .bg(rgb(p.border))
                        .hover(|s| s.w(px(3.0)).bg(rgb(p.accent)))
                        .cursor_col_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.drag_kind = DragKind::VerticalTerminal;
                                cx.notify();
                            }),
                        ),
                )
                .child(self.render_vertical_terminal_pane(layout.vterm_w, cx))
            });

        let main_column = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .h_full()
            .overflow_hidden()
            .flex()
            .flex_col()
            .when(!is_zen && self.app.workspace.tab_bar_visible, |d| {
                d.child(chrome::render_tab_bar(self, layout, cx))
            })
            .when(show_md_toggle, |d| d.child(markdown_preview::render_toggle(self, cx)))
            .when(show_pdf_toggle, |d| {
                d.child(pdf_preview::render_toggle(self, cx))
            })
            .when(show_image_toggle, |d| {
                d.child(image_preview::render_toggle(self, cx))
            })
            .child(editor_and_vterm)
            .when(layout.show_drawer, |d| {
                d.child(
                    div()
                        .id("drawer-splitter")
                        .h(px(1.0))
                        .w_full()
                        .flex_shrink_0()
                        .bg(rgb(p.border))
                        .hover(|s| s.h(px(3.0)).bg(rgb(p.accent)))
                        .cursor_row_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.drag_kind = DragKind::Drawer;
                                cx.notify();
                            }),
                        ),
                )
                .child(self.render_bottom_drawer(layout.drawer_h, layout, cx))
            });

        let workspace_row = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .w_full()
            .overflow_hidden()
            .flex()
            .when(!is_zen, |d| {
                d.child(sidebar::render_activity_rail(self, layout, cx))
            })
            .when(layout.show_sidebar, |d| {
                d.child(sidebar::render_sidebar(self, layout, cx)).child(
                    div()
                        .id("sidebar-splitter")
                        .w(px(1.0))
                        .h_full()
                        .flex_shrink_0()
                        .bg(rgb(p.border))
                        .hover(|s| s.w(px(3.0)).bg(rgb(p.accent)))
                        .cursor_col_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.drag_kind = DragKind::Sidebar;
                                cx.notify();
                            }),
                        ),
                )
            })
            .child(main_column);

        div()
            .id("gtuim-root")
            .track_focus(&self.root_focus)
            .font_family(self.ui_font.clone())
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(p.bg_editor))
            .text_color(rgb(p.text))
            .on_key_down(cx.listener(Self::handle_key_down))
            .on_mouse_move(cx.listener(Self::handle_root_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::handle_root_mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::handle_root_mouse_up))
            .when(!is_zen, |d| {
                d.child(chrome::render_header_bar(self, layout, cx))
            })
            .child(workspace_row)
            .child(chrome::render_status_bar(self, layout, cx))
            .when(self.profiles.open && !is_zen, |d| d.child(profile_panel::render(self, layout, cx)))
            .children(overlays)
    }
}

pub fn run_gui(files_to_open: Vec<String>) {
    Application::new()
        .with_assets(icons::Assets)
        .run(move |cx: &mut App| {
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(1380.0), px(880.0)), cx);
            let files = files_to_open.clone();
            if let Err(error) = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(720.0), px(460.0))),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Gtuim — GPUI Workspace".into()),
                        ..Default::default()
                    }),
                    app_id: Some("gtuim".into()),
                    ..Default::default()
                },
                move |window, cx| {
                    let view = cx.new(|cx| match GtuimView::new(files, window, cx) {
                        Ok(view_state) => view_state,
                        Err(err) => {
                            eprintln!("Gtuim could not start: {}", err);
                            std::process::exit(1);
                        }
                    });
                    window.focus(&view.read(cx).root_focus);
                    view
                },
            ) {
                eprintln!("Could not open GPUI window: {}", error);
                cx.quit();
            }
            cx.activate(true);
        });
}
