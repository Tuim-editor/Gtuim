//! Headless tests render the production Gtuim modal tree. The backend is an inert
//! pipe process: no Neovim bootstrap, network, user runtime, or desktop is touched.
use super::{DragKind, GtuimView, GridBuffer, GridMetrics};
use crate::{nvim::{process::NvimProcess, rpc::RpcClient}, tui::{app::App as TuimApp,
    capabilities::Capabilities, widgets::extension_shop::PluginInfo}};
use gpui::{point, px, size, SharedString, Task, TestAppContext};
use std::{cell::RefCell, collections::HashMap, process::{Command, Stdio}, rc::Rc};

pub(super) fn fixture(cx: &mut gpui::Context<GtuimView>) -> GtuimView {
    use std::os::unix::io::AsRawFd;
    let mut child = Command::new("cat").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    for fd in [stdin.as_raw_fd(), stdout.as_raw_fd()] {
        unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL, 0);
            if flags >= 0 {
                let _ = libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            }
        }
    }
    let process = NvimProcess { stdin, stdout, child };
    // Deliberately absent paths: constructors only read defaults here.
    let path = format!("/tmp/gtuim-inert-modal-tests-{}", std::process::id());
    let mut app = TuimApp::new(100, 30, RpcClient::new(process),
        Capabilities::detect(&HashMap::new()), path.clone(), path.clone());
    app.extension_shop.config_dir = path;
    app.extension_shop.is_open = true;
    for i in 0..6 {
        app.extension_shop.plugins.push(PluginInfo {
            name: format!("catppuccin-{i}"), full_name: "catppuccin/nvim".into(),
            description: "Bundled | Enabled | Themes: catppuccin-latte, catppuccin-frappe, catppuccin-macchiato, catppuccin-mocha; ".repeat(12),
            installed: true, enabled: true, lifecycle: "dependencies_ready".into(),
            ..Default::default()
        });
    }
    static PROFILE_FIXTURE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let profiles = super::profiles::Profiles::load(format!("{}-profiles-{}", app.data_dir,
        PROFILE_FIXTURE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed))).unwrap();
    GtuimView {
        profiles,
        practice_panels: HashMap::new(),
        app, root_focus: cx.focus_handle(), sidebar_px: 256.0, drawer_px: 230.0,
        vterm_px: 400.0, font_size: 14.0,
        font_weight: gpui::FontWeight::NORMAL,
        default_mono_font: SharedString::from("monospace"),
        available_fonts: vec!["Fira Code".into(), "JetBrains Mono".into(), "monospace".into()],
        mono_font: SharedString::from("monospace"), ui_font: SharedString::from("monospace"),
        find_replace: super::find_replace::FindReplaceState::default(),
        editor_metrics: Rc::new(RefCell::new(GridMetrics::default())),
        drawer_metrics: Rc::new(RefCell::new(GridMetrics::default())),
        vterm_metrics: Rc::new(RefCell::new(GridMetrics::default())),
        editor_buffer: Rc::new(RefCell::new(GridBuffer::default())),
        drawer_buffer: Rc::new(RefCell::new(GridBuffer::default())),
        vterm_buffer: Rc::new(RefCell::new(GridBuffer::default())),
        editor_ctx_pos: point(px(120.0), px(120.0)), explorer_ctx_pos: point(px(80.0), px(120.0)),
        drag_kind: DragKind::None, explorer_drag: None, split_drag: None,
        last_editor_pane_bounds: None, files_to_open: Vec::new(), last_title: String::new(),
        last_editor_cell_size: (0, 0),
        compile_scroll: gpui::ScrollHandle::new(),
        compilation_scroll: gpui::ScrollHandle::new(),
        markdown_preview: super::markdown_preview::MarkdownPreview::default(),
        pdf_preview: super::pdf_preview::PdfPreview::default(),
        last_compile_output_version: 0,
        tab_scroll_start: 0,
        last_active_tab: 0,
        last_tab_bar_w: 0.0,
        last_known_tab_paths: Vec::new(),
        doc_cursor: None,
        editor_wheel: Default::default(), drawer_wheel: Default::default(), vterm_wheel: Default::default(),
        _poll_task: Task::ready(()),
    }
}

#[gpui::test]
fn markdown_preview_renders_beside_editor_and_toggle_closes_it(cx: &mut TestAppContext) {
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.markdown_preview.open = true;
        view.markdown_preview.update(&[
            rmpv::Value::from("live.md"), rmpv::Value::from(true),
            rmpv::Value::from("# Live preview\n\nUnsaved **Markdown** text.\n\n- item"),
            rmpv::Value::from(false),
        ]);
        view
    });
    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();
    let preview = window.debug_bounds("markdown-preview-pane").expect("native preview pane");
    let editor = window.debug_bounds("nvim-editor-pane").expect("editor alongside preview");
    assert!(preview.size.width >= px(180.0));
    assert!(editor.right() <= preview.left() + px(1.0));
    let toggle = window.debug_bounds("markdown-preview-toggle").expect("preview close button");
    window.simulate_mouse_down(toggle.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(toggle.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| assert!(!this.markdown_preview.open));
}

#[gpui::test]
fn markdown_preview_preserves_editor_space_in_narrow_windows_with_terminal(cx: &mut TestAppContext) {
    for show_terminal in [false, true] {
        let (_, window) = cx.add_window_view(|_, cx| {
            let mut view = fixture(cx);
            view.app.extension_shop.is_open = false;
            view.app.workspace.vertical_terminal_visible = show_terminal;
            view.markdown_preview.open = true;
            view.markdown_preview.update(&[
                rmpv::Value::from("live.md"), rmpv::Value::from(true),
                rmpv::Value::from("# A long heading that should wrap without overlapping\n\nPreview"),
                rmpv::Value::from(false),
            ]);
            view
        });
        window.simulate_resize(size(px(720.0), px(460.0)));
        window.run_until_parked();
        let editor = window.debug_bounds("nvim-editor-pane").expect("source editor");
        let preview = window.debug_bounds("markdown-preview-pane").expect("preview");
        assert!(editor.size.width > px(0.0), "editor must not collapse with terminal={show_terminal}: {editor:?}");
        assert!(preview.size.width > px(0.0));
        assert!(editor.right() <= preview.left() + px(1.0));
        assert!(preview.right() <= px(720.0));
    }
}

#[gpui::test]
fn markdown_preview_zen_hides_toggle_and_preserves_clipboard_shortcut(cx: &mut TestAppContext) {
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.tabs[0].name = "README.md".into();
        view.app.mode = crate::tui::app::Mode::Zen;
        view
    });
    window.simulate_resize(size(px(720.0), px(460.0)));
    window.run_until_parked();
    assert!(window.debug_bounds("markdown-preview-toggle").is_none());
    window.update(|window, cx| view.update(cx, |this, cx| {
        let event = gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse("ctrl-shift-v").unwrap(),
            is_held: false,
        };
        this.handle_key_down(&event, window, cx);
        assert!(!this.markdown_preview.open, "paste must not toggle preview");
        let toggle = gpui::KeyDownEvent {
            keystroke: gpui::Keystroke::parse("ctrl-shift-m").unwrap(),
            is_held: false,
        };
        this.handle_key_down(&toggle, window, cx);
        assert!(this.markdown_preview.open, "new shortcut opens preview");
    }));
}

#[gpui::test]
fn pdf_preview_full_mode_split_toggle_controls_and_raw_reopen(cx: &mut TestAppContext) {
    use super::pdf_preview::{PdfFitMode, PdfViewMode, RenderedPng};

    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.tabs[0].name = "/tmp/sample-doc.pdf".into();
        view.app.tabs[0].relative_name = "sample-doc.pdf".into();
        view.pdf_preview.seed_test_page(
            "/tmp/sample-doc.pdf",
            3,
            RenderedPng {
                bytes: vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a],
                width_px: 600,
                height_px: 800,
            },
        );
        view
    });

    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();

    let pane = window.debug_bounds("pdf-preview-pane").expect("pdf preview pane");
    let toolbar = window.debug_bounds("pdf-preview-toolbar").expect("pdf toolbar");
    let surface = window.debug_bounds("pdf-page-surface").expect("pdf page surface");
    assert!(
        window.debug_bounds("nvim-editor-pane").is_none(),
        "full PDF mode replaces raw binary editor"
    );
    assert!(toolbar.bottom() <= pane.bottom());
    assert!(surface.size.width >= px(120.0) && surface.size.height >= px(120.0));

    // Zoom in and reset
    let zoom_in = window.debug_bounds("pdf-zoom-in").expect("zoom-in button");
    window.simulate_mouse_down(zoom_in.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(zoom_in.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| assert!((this.pdf_preview.zoom - 1.25).abs() < 0.01));

    let zoom_reset = window.debug_bounds("pdf-zoom-reset").expect("zoom-reset pill");
    window.simulate_mouse_down(zoom_reset.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(zoom_reset.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| assert!((this.pdf_preview.zoom - 1.0).abs() < 0.01));

    // Toggle fit mode
    let fit_btn = window.debug_bounds("pdf-fit-toggle").expect("fit toggle");
    window.simulate_mouse_down(fit_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(fit_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| assert_eq!(this.pdf_preview.fit_mode, PdfFitMode::ActualSize));

    // Switch Full -> Split mode: both editor and PDF render side-by-side
    let mode_btn = window.debug_bounds("pdf-mode-toggle").expect("mode toggle");
    window.simulate_mouse_down(mode_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(mode_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| assert_eq!(this.pdf_preview.mode, PdfViewMode::Split));
    window.simulate_resize(size(px(1001.0), px(700.0)));
    window.run_until_parked();
    let editor = window.debug_bounds("nvim-editor-pane").expect("editor beside split PDF");
    let split_pdf = window.debug_bounds("pdf-preview-pane").expect("split PDF pane");
    assert!(editor.right() <= split_pdf.left() + px(1.0));

    // Close preview: button stays inside split pane and reveals Raw PDF reopen bar
    let close_btn = window.debug_bounds("pdf-close").expect("close button");
    assert!(
        close_btn.right() <= split_pdf.right() + px(1.0),
        "close button must remain inside narrow split pane: close={close_btn:?}, pane={split_pdf:?}"
    );
    window.simulate_mouse_down(close_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(close_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert!(!this.pdf_preview.open);
        assert_eq!(
            this.pdf_preview.dismissed_for_path.as_deref(),
            Some("/tmp/sample-doc.pdf")
        );
    });
    window.simulate_resize(size(px(1002.0), px(700.0)));
    window.run_until_parked();
    let toggle_bar = window
        .debug_bounds("pdf-preview-toggle")
        .expect("raw PDF reopen bar appears after closing preview");
    assert!(toggle_bar.size.height > px(20.0));
}

#[gpui::test]
fn pdf_preview_empty_and_error_states_render_inside_pane(cx: &mut TestAppContext) {
    use super::pdf_preview::PdfViewMode;

    for with_error in [false, true] {
        let (_, window) = cx.add_window_view(|_, cx| {
            let mut view = fixture(cx);
            view.app.extension_shop.is_open = false;
            view.pdf_preview.open = true;
            view.pdf_preview.mode = PdfViewMode::Split;
            if with_error {
                view.pdf_preview.path = Some("/tmp/broken.pdf".into());
                view.pdf_preview.error = Some("Could not render PDF: Syntax Error".into());
            }
            view
        });
        window.simulate_resize(size(px(900.0), px(620.0)));
        window.run_until_parked();
        let pane = window.debug_bounds("pdf-preview-pane").expect("pdf pane");
        let selector = if with_error {
            "pdf-preview-error"
        } else {
            "pdf-preview-empty"
        };
        let body = window.debug_bounds(selector).expect(selector);
        assert!(body.top() >= pane.top() && body.bottom() <= pane.bottom() + px(1.0));
        assert!(body.size.height > px(40.0));
    }
}

#[gpui::test]
fn pdf_preview_auto_opens_real_pdf_and_navigates_pages_with_keyboard(cx: &mut TestAppContext) {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let pdf_path = std::env::temp_dir().join(format!("gtuim-gui-e2e-{}.pdf", std::process::id()));
    std::fs::write(&pdf_path, super::pdf_preview::tests::build_sample_two_page_pdf())
        .expect("write e2e sample pdf");
    let _cleanup = Cleanup(pdf_path.clone());
    let pdf_str = pdf_path.to_string_lossy().to_string();

    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.tabs[0].name = pdf_str.clone();
        view.app.tabs[0].relative_name = "gtuim-gui-e2e.pdf".into();
        view.sync_pdf_preview(cx);
        view
    });

    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();

    view.read_with(window, |this, _| {
        assert!(this.pdf_preview.open);
        assert_eq!(this.pdf_preview.page_count(), 2);
        assert_eq!(this.pdf_preview.current_page, 1);
        assert!(this.pdf_preview.error.is_none(), "unexpected pdf error: {:?}", this.pdf_preview.error);
        assert!(this.pdf_preview.current_rendered_page().is_some());
    });
    let surface = window.debug_bounds("pdf-page-surface").expect("real rendered PDF surface");
    assert!(surface.size.width >= px(120.0) && surface.size.height >= px(120.0));

    // Advance to page 2 and return to page 1 via keyboard handler
    view.update(window, |this, cx| {
        let right = gpui::Keystroke {
            modifiers: gpui::Modifiers::default(),
            key: "right".into(),
            key_char: None,
        };
        assert!(this.handle_pdf_key(&right, cx));
    });
    window.run_until_parked();
    view.read_with(window, |this, _| assert_eq!(this.pdf_preview.current_page, 2));

    view.update(window, |this, cx| {
        let left = gpui::Keystroke {
            modifiers: gpui::Modifiers::default(),
            key: "left".into(),
            key_char: None,
        };
        assert!(this.handle_pdf_key(&left, cx));
    });
    window.run_until_parked();
    view.read_with(window, |this, _| assert_eq!(this.pdf_preview.current_page, 1));

    let _ = std::fs::remove_file(&pdf_path);
}

#[gpui::test]
fn pdf_preview_companion_maximize_survives_sync_and_split_coexists_with_vterm(
    cx: &mut TestAppContext,
) {
    use super::pdf_preview::PdfViewMode;

    struct Cleanup(Vec<std::path::PathBuf>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for p in &self.0 {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    let stem = format!("gtuim-companion-{}", std::process::id());
    let tex_path = std::env::temp_dir().join(format!("{stem}.tex"));
    let pdf_path = std::env::temp_dir().join(format!("{stem}.pdf"));
    std::fs::write(&tex_path, "\\documentclass{article}\n").expect("write tex");
    std::fs::write(&pdf_path, super::pdf_preview::tests::build_sample_two_page_pdf())
        .expect("write companion pdf");
    let _cleanup = Cleanup(vec![tex_path.clone(), pdf_path.clone()]);
    let tex_str = tex_path.to_string_lossy().to_string();

    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.tabs[0].name = tex_str.clone();
        view.app.tabs[0].relative_name = format!("{stem}.tex");
        view.toggle_pdf_preview(cx);
        view
    });

    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();

    view.read_with(window, |this, _| {
        assert!(this.pdf_preview.open);
        assert_eq!(this.pdf_preview.mode, PdfViewMode::Split);
        assert!(this.pdf_preview.pinned);
    });

    // Maximize companion PDF while `.tex` buffer remains active; next sync_pdf_preview must NOT hide it.
    window.refresh().expect("refresh companion split layout");
    window.run_until_parked();
    let pdf_split_pane = window.debug_bounds("pdf-preview-pane").expect("split pdf pane");
    let mode_btn = window.debug_bounds("pdf-mode-toggle").expect("maximize button");
    assert!(
        mode_btn.right() <= pdf_split_pane.right() + px(1.0),
        "mode button outside split pane: mode={mode_btn:?}, pane={pdf_split_pane:?}"
    );
    window.simulate_mouse_down(mode_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(mode_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();

    view.update(window, |this, cx| {
        assert_eq!(this.pdf_preview.mode, PdfViewMode::Full);
        assert!(this.pdf_preview.pinned);
        this.sync_pdf_preview(cx);
    });
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert!(this.pdf_preview.open, "maximized companion PDF must stay open across sync");
        assert_eq!(this.pdf_preview.mode, PdfViewMode::Full);
    });

    // Switch back to Split with vertical terminal open on a narrow window: editor must NOT collapse to 0px.
    view.update(window, |this, cx| {
        this.toggle_pdf_view_mode(cx);
        this.app.workspace.vertical_terminal_visible = true;
        this.vterm_px = 300.0;
        cx.notify();
    });
    window.simulate_resize(size(px(960.0), px(620.0)));
    window.run_until_parked();

    let editor = window.debug_bounds("nvim-editor-pane").expect("editor pane");
    let pdf_pane = window.debug_bounds("pdf-preview-pane").expect("split pdf pane");
    assert!(editor.size.width > px(80.0), "editor must not collapse with split PDF + vterm: {editor:?}");
    assert!(pdf_pane.size.width > px(80.0), "split pdf must stay readable: {pdf_pane:?}");
    assert!(editor.right() <= pdf_pane.left() + px(1.0));
}

#[gpui::test]
fn pdf_preview_continuous_wheel_scroll_crosses_pages_and_tracks_indicator(
    cx: &mut TestAppContext,
) {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    let pdf_path = std::env::temp_dir().join(format!("gtuim-cont-scroll-{}.pdf", std::process::id()));
    std::fs::write(&pdf_path, super::pdf_preview::tests::build_sample_multipage_pdf(4))
        .expect("write 4-page pdf");
    let _cleanup = Cleanup(pdf_path.clone());
    let pdf_str = pdf_path.to_string_lossy().to_string();

    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.tabs[0].name = pdf_str.clone();
        view.app.tabs[0].relative_name = "gtuim-cont-scroll.pdf".into();
        view.sync_pdf_preview(cx);
        view
    });

    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();

    view.read_with(window, |this, _| {
        assert_eq!(this.pdf_preview.page_count(), 4);
        assert_eq!(this.pdf_preview.current_page, 1);
        assert!(this.pdf_preview.rendered_page(1).is_some());
    });

    // Verify vertical page stacking: page 2 sits directly below page 1.
    let slot1 = window.debug_bounds("pdf-page-slot-1").expect("page 1 slot");
    let slot2 = window.debug_bounds("pdf-page-slot-2").expect("page 2 slot");
    assert!(
        slot2.top() >= slot1.bottom() - px(1.0),
        "page 2 must stack vertically below page 1: slot1={slot1:?}, slot2={slot2:?}"
    );

    // Scroll continuously downward with wheel events across page 1 -> page 2 -> page 3.
    let scroll_bounds = window.debug_bounds("pdf-preview-scroll").expect("pdf scroll container");
    for _ in 0..4 {
        window.simulate_event(gpui::ScrollWheelEvent {
            position: scroll_bounds.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-260.0))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        });
        window.run_until_parked();
    }

    let offset_after_p2 = view.read_with(window, |this, _| {
        assert_eq!(
            this.pdf_preview.current_page, 2,
            "continuous scroll past page 1 must update indicator to page 2"
        );
        assert!(
            this.pdf_preview.rendered_page(2).is_some(),
            "page 2 must be lazily rendered on scroll"
        );
        this.pdf_preview.scroll.offset().y
    });
    assert!(offset_after_p2 < px(-500.0));

    for _ in 0..4 {
        window.simulate_event(gpui::ScrollWheelEvent {
            position: scroll_bounds.center(),
            delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-260.0))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        });
        window.run_until_parked();
    }

    let offset_after_p3 = view.read_with(window, |this, _| {
        assert_eq!(
            this.pdf_preview.current_page, 3,
            "continuous scroll past page 2 must update indicator to page 3"
        );
        assert!(
            this.pdf_preview.rendered_page(3).is_some(),
            "page 3 must be lazily rendered on scroll"
        );
        this.pdf_preview.scroll.offset().y
    });
    assert!(offset_after_p3 < offset_after_p2);

    // Page navigation button scrolls back up to page 2 cleanly.
    let prev_btn = window.debug_bounds("pdf-prev-page").expect("prev page button");
    window.simulate_mouse_down(prev_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(prev_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();

    view.read_with(window, |this, _| {
        assert_eq!(this.pdf_preview.current_page, 2);
        assert!(this.pdf_preview.scroll.offset().y > offset_after_p3);
    });
}

#[gpui::test]
fn pdf_preview_large_document_virtualizes_pages_and_bounds_lazy_renders(
    cx: &mut TestAppContext,
) {
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    let pdf_path = std::env::temp_dir().join(format!("gtuim-large-25p-{}.pdf", std::process::id()));
    std::fs::write(&pdf_path, super::pdf_preview::tests::build_sample_multipage_pdf(25))
        .expect("write 25-page pdf");
    let _cleanup = Cleanup(pdf_path.clone());
    let pdf_str = pdf_path.to_string_lossy().to_string();

    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.tabs[0].name = pdf_str.clone();
        view.app.tabs[0].relative_name = "gtuim-large-25p.pdf".into();
        view.sync_pdf_preview(cx);
        view
    });

    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();

    view.read_with(window, |this, _| {
        assert_eq!(this.pdf_preview.page_count(), 25);
        assert_eq!(this.pdf_preview.current_page, 1);
        assert!(
            this.pdf_preview.cached_page_count() <= 6,
            "must lazily render only visible window + prefetch, got {}",
            this.pdf_preview.cached_page_count()
        );
        assert!(this.pdf_preview.rendered_page(20).is_none());
    });
    assert!(
        window.debug_bounds("pdf-page-slot-20").is_none(),
        "far-off page 20 must remain virtualized until scrolled into view"
    );

    // Jump to page 20: scrolls down, materializes page 20, lazily renders it, stays bounded.
    view.update(window, |this, cx| {
        this.pdf_go_to_page(20, cx);
    });
    window.run_until_parked();
    window.refresh().expect("refresh after page 20 jump");
    window.run_until_parked();

    let (cur_p, off, max_off, cached) = view.read_with(window, |this, _| {
        (
            this.pdf_preview.current_page,
            this.pdf_preview.scroll.offset(),
            this.pdf_preview.scroll.max_offset(),
            this.pdf_preview.cached_page_count(),
        )
    });
    assert_eq!(cur_p, 20);
    assert!(off.y < px(-5000.0), "off={off:?}, max_off={max_off:?}");
    assert!(
        view.read_with(window, |this, _| this.pdf_preview.rendered_page(20).is_some()),
        "page 20 must render after jumping to it"
    );
    assert!(cached <= 16);
    assert!(
        window.debug_bounds("pdf-page-slot-20").is_some(),
        "slot-20 missing: off={off:?}, max_off={max_off:?}, slot19={:?}, surface={:?}",
        window.debug_bounds("pdf-page-slot-19"),
        window.debug_bounds("pdf-page-surface"),
    );
}

#[gpui::test]
fn pdf_preview_pinned_split_not_replaced_by_active_pdf_tab_on_sync(cx: &mut TestAppContext) {
    use super::pdf_preview::PdfViewMode;

    struct Cleanup(Vec<std::path::PathBuf>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for p in &self.0 {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    let pinned_path = std::env::temp_dir().join(format!("gtuim-pinned-a-{}.pdf", std::process::id()));
    let other_path = std::env::temp_dir().join(format!("gtuim-other-b-{}.pdf", std::process::id()));
    std::fs::write(&pinned_path, super::pdf_preview::tests::build_sample_two_page_pdf())
        .expect("write pinned pdf");
    std::fs::write(&other_path, super::pdf_preview::tests::build_sample_two_page_pdf())
        .expect("write other pdf");
    let _cleanup = Cleanup(vec![pinned_path.clone(), other_path.clone()]);
    let pinned_str = pinned_path.to_string_lossy().to_string();
    let other_str = other_path.to_string_lossy().to_string();

    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.tabs[0].name = other_str.clone();
        view.open_pdf_path(pinned_str.clone(), PdfViewMode::Split, true, cx);
        view
    });

    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();

    view.update(window, |this, cx| {
        this.sync_pdf_preview(cx);
    });
    window.run_until_parked();

    view.read_with(window, |this, _| {
        assert!(this.pdf_preview.open);
        assert_eq!(this.pdf_preview.mode, PdfViewMode::Split);
        assert!(this.pdf_preview.pinned);
        assert_eq!(this.pdf_preview.path.as_deref(), Some(pinned_str.as_str()));
    });
}

#[gpui::test]
fn production_plugin_errors_and_trust_dialog_keep_actions_visible(cx: &mut TestAppContext) {
    for confirm in [false, true] {
        let (_, window) = cx.add_window_view(|_, cx| {
            let mut view = fixture(cx);
            let shop = &mut view.app.extension_shop;
            shop.status_msg = "Long backend error with full diagnostics/path/".repeat(80);
            if confirm {
                shop.plugins[0].full_name = format!("{}/{}", "a".repeat(39), "b".repeat(100));
                shop.plugins[0].installed = false;
                shop.detail_open = true;
                shop.handle_key(&crate::tui::input::InputEvent::Key("<CR>".into()));
            }
            view
        });
        window.simulate_resize(size(px(720.0), px(460.0)));
        window.run_until_parked();
        let card = window.debug_bounds("modal-card").unwrap();
        let footer = window.debug_bounds("shop-footer").unwrap();
        let status = window.debug_bounds("shop-status").unwrap();
        assert!(status.size.height <= px(54.0));
        assert!(footer.bottom() <= card.bottom() + px(1.0));
        if confirm {
            let confirm_card = window.debug_bounds("shop-confirm-card").unwrap();
            let body = window.debug_bounds("shop-confirm-body").unwrap();
            let actions = window.debug_bounds("shop-confirm-actions").unwrap();
            assert!(confirm_card.top() >= card.top());
            assert!(confirm_card.bottom() <= card.bottom() + px(1.0));
            assert!(confirm_card.right() <= card.right());
            assert!(body.size.height > px(0.0), "trust warning must have a readable scroll viewport");
            assert!(body.bottom() <= actions.top());
            assert!(actions.bottom() <= confirm_card.bottom());
        } else {
            let list = window.debug_bounds("shop-plugins").unwrap();
            assert!(list.size.height > px(60.0), "backend errors must not collapse plugin list");
            assert!(list.bottom() <= footer.top());
        }
    }
}

#[gpui::test]
fn production_popups_and_notices_stay_inside_viewport_edges(cx: &mut TestAppContext) {
    let cases = [("editor", "ed-ctx-list"), ("explorer", "exp-ctx-list"),
        ("menu", "menu-dd-list"), ("notice", "notice-toast"), ("settings-dropdown", "settings-dropdown")];
    let mut failures = Vec::new();
    for (mode, selector) in cases {
        let (_, window) = cx.add_window_view(|_, cx| {
            let mut view = fixture(cx);
            view.app.extension_shop.is_open = false;
            match mode {
                "editor" => {
                    view.app.editor_context_menu.is_open = true;
                    view.editor_ctx_pos = point(px(710.0), px(450.0));
                }
                "explorer" => {
                    view.app.explorer.context_menu_open = true;
                    view.explorer_ctx_pos = point(px(710.0), px(450.0));
                }
                "menu" => view.app.menu_bar.open_menu = Some(0),
                "settings-dropdown" => {
                    view.app.settings_widget.is_open = true;
                    view.app.settings_widget.active_tab = 1;
                    view.app.settings_widget.selected_row = 0;
                    view.app.settings_widget.dropdown_open = true;
                }
                _ => view.app.set_notice("error", &"long-notice-detail/".repeat(300)),
            }
            view
        });
        window.simulate_resize(size(px(720.0), px(460.0)));
        window.run_until_parked();
        let bounds = window.debug_bounds(selector).expect(selector);
        if mode == "notice" {
            assert!(bounds.size.width >= px(160.0), "notice must not collapse into a ribbon: {bounds:?}");
        }
        let container = if mode == "settings-dropdown" { window.debug_bounds("modal-card").unwrap() }
            else { gpui::Bounds::new(point(px(0.0), px(0.0)), size(px(720.0), px(460.0))) };
        if bounds.left() < container.left() || bounds.top() < container.top()
            || bounds.right() > container.right() + px(1.0) || bounds.bottom() > container.bottom() + px(1.0) {
            failures.push(format!("{mode} popup clipped: {bounds:?}, container {container:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[gpui::test]
fn production_submenus_bound_long_text_and_keep_scroll_viewports_visible(cx: &mut TestAppContext) {
    use crate::{git_snapshot::{GitCommitEntry, WorktreeEntry}, tui::{app::PaletteMode,
        widgets::{bug_report::BugStage, lazy::LazyPlugin, mason::MasonPkg}}};
    let mut failures = Vec::new();
    macro_rules! check {
        ($condition:expr, $($message:tt)*) => {
            if !$condition { failures.push(format!($($message)*)); }
        };
    }
    let cases = [
        ("palette", "palette-list", Some("palette-buf-0"), None),
        ("settings", "settings-rows", Some("settings-row-0"), Some("settings-footer")),
        ("mason", "mason-list", Some("mason-item-0"), Some("mason-footer")),
        ("lazy", "lazy-list", Some("lazy-item-0"), Some("lazy-footer")),
        ("git", "git-det-list", Some("git-det-commit-0"), Some("git-det-footer")),
        ("orchestrator", "orch-list", Some("orch-item-0"), Some("orch-footer")),
        ("bug-form", "bug-form-body", None, Some("bug-footer")),
        ("bug-consent", "bug-consent-body", None, Some("bug-footer")),
        ("bug-submitting", "bug-submitting-body", None, Some("bug-footer")),
        ("bug-success", "bug-success-body", None, Some("bug-footer")),
        ("bug-failure", "bug-failure-body", None, Some("bug-footer")),
    ];
    for (width, height) in [(720.0, 460.0), (1380.0, 880.0)] {
        for (mode, selector, row_selector, footer_selector) in cases {
            let (_, window) = cx.add_window_view(|_, cx| {
                let mut view = fixture(cx);
                let app = &mut view.app;
                let long = "long-description-or-branch-without-spaces/".repeat(16);
                app.extension_shop.is_open = false;
                match mode {
                    "palette" => {
                        app.workspace.palette_open = true;
                        app.workspace.palette_mode = PaletteMode::Buffers;
                        app.tabs[0].name = long.clone(); app.tabs[0].relative_name = long;
                    }
                    "settings" => { app.settings_widget.is_open = true; }
                    "mason" => {
                        app.mason_widget.is_open = true;
                        app.mason_widget.packages = vec![MasonPkg { name: long.clone(), desc: long, installed: true, ..Default::default() }];
                        app.mason_widget.filtered = vec![0];
                    }
                    "lazy" => {
                        app.lazy_widget.is_open = true;
                        app.lazy_widget.plugins = vec![LazyPlugin { name: long.clone(), repo: long.clone(), loaded: true, ..Default::default() }];
                        app.lazy_widget.filtered = vec![0]; app.lazy_widget.status_msg = long;
                    }
                    "git" => {
                        app.git_detailed_widget.is_open = true;
                        app.git_detailed_widget.snapshot.commits = vec![GitCommitEntry { hash: "abcdef0".into(), author: long.clone(), date: long.clone(), message: long.clone() }];
                        app.git_detailed_widget.status_msg = long;
                    }
                    "orchestrator" => {
                        app.orchestrator.is_open = true;
                        app.orchestrator.worktrees = vec![WorktreeEntry { path: long.clone(), head: "abcdef0".into(), branch: long.clone(), is_bare: false }];
                        app.orchestrator.status_msg = long;
                    }
                    _ => {
                        app.bug_report.is_open = true;
                        app.bug_report.stage = match mode {
                            "bug-form" => BugStage::Form, "bug-consent" => BugStage::Consent,
                            "bug-submitting" => BugStage::Submitting, "bug-success" => BugStage::Success,
                            _ => BugStage::Failure,
                        };
                        app.bug_report.summary = long.clone(); app.bug_report.description = long.clone();
                        app.bug_report.issue_url = long.clone(); app.bug_report.error_message = long;
                    }
                }
                view
            });
            window.simulate_resize(size(px(width), px(height)));
            window.run_until_parked();
            let card = window.debug_bounds("modal-card").expect(mode);
            let list = window.debug_bounds(selector).expect(selector);
            check!(list.right() <= card.right() + px(1.0), "{mode}: list too wide at {width}: {list:?}, card {card:?}");
            check!(list.bottom() <= card.bottom() + px(1.0), "{mode}: list below dialog at {height}: {list:?}, card {card:?}");
            check!(list.size.height > px(0.0), "{mode}: scroll viewport collapsed");
            if let Some(selector) = row_selector {
                let row = window.debug_bounds(selector).expect(selector);
                check!(row.right() <= card.right() + px(1.0), "{mode}: long row overflows at {width}: {row:?}, card {card:?}");
            }
            let badge_selector = match mode {
                "mason" => Some("mason-badge-0"), "lazy" => Some("lazy-badge-0"),
                "orchestrator" => Some("orch-head-0"), _ => None,
            };
            if let Some(selector) = badge_selector {
                let badge = window.debug_bounds(selector).expect(selector);
                check!(badge.size.width >= px(32.0), "{mode}: short status collapsed: {badge:?}");
                check!(badge.size.height <= px(28.0), "{mode}: short status must stay on one line: {badge:?}");
            }
            if mode == "git" {
                let meta = window.debug_bounds("git-det-meta-0").unwrap();
                check!(meta.size.width >= px(150.0), "git metadata needs a readable column: {meta:?}");
            }
            if let Some(selector) = footer_selector {
                let footer = window.debug_bounds(selector).expect(selector);
                check!(footer.right() <= card.right() + px(1.0), "{mode}: footer wider than dialog");
                check!(footer.bottom() <= card.bottom() + px(1.0), "{mode}: footer below dialog");
                check!(list.bottom() <= footer.top() + px(1.0), "{mode}: scroll viewport overlaps footer");
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[gpui::test]
fn production_recipe_form_and_health_details_fit_small_windows(cx: &mut TestAppContext) {
    use crate::tui::widgets::plugin_center::{recipe_for, RecipeForm};
    for form_open in [false, true] {
        let (_, window) = cx.add_window_view(|_, cx| {
            let mut view = fixture(cx);
            view.app.extension_shop.detail_open = true;
            view.app.extension_shop.plugins[0].full_name = "numToStr/Comment.nvim".into();
            view.app.extension_shop.plugins[0].health_message = "Verification failed: ".to_string() + &"long/path/".repeat(60);
            if form_open {
                view.app.extension_shop.recipe_form = Some(RecipeForm::new(recipe_for("numToStr/Comment.nvim").unwrap(), Default::default()));
            }
            view
        });
        window.simulate_resize(size(px(720.0), px(460.0)));
        window.run_until_parked();
        let card = window.debug_bounds("modal-card").unwrap();
        let footer = window.debug_bounds("shop-footer").unwrap();
        if form_open {
            let fields = window.debug_bounds("recipe-fields").unwrap();
            let row = window.debug_bounds("recipe-field-0").unwrap();
            let actions = window.debug_bounds("recipe-actions").unwrap();
            assert!(row.right() <= card.right());
            let label = window.debug_bounds("recipe-label-0").unwrap();
            let value = window.debug_bounds("recipe-value-0").unwrap();
            assert!(label.size.width >= px(120.0) && label.size.height <= px(40.0),
                "recipe label needs readable space: {label:?}");
            assert!(value.size.width >= px(24.0) && value.size.height <= px(28.0),
                "recipe boolean must stay readable: {value:?}");
            assert!(row.size.height <= px(110.0), "ordinary recipe field should remain compact: {row:?}");
            assert!(fields.right() <= card.right());
            assert!(fields.bottom() <= actions.top());
            assert!(actions.bottom() <= footer.top(), "form buttons must not disappear below dialog: actions={actions:?}, fields={fields:?}, footer={footer:?}, card={card:?}");
        } else {
            let details = window.debug_bounds("plugin-details").unwrap();
            let health = window.debug_bounds("plugin-health-panel").unwrap();
            assert!(details.right() <= card.right());
            assert!(health.right() <= card.right());
            assert!(details.bottom() <= footer.top());
        }
    }
}

// Containment alone accepts a one-character-wide, hundreds-of-lines-tall
// control. These normal-content fixtures guard readability as well as bounds.
#[gpui::test]
fn settings_values_are_readable_and_normal_rows_stay_compact(cx: &mut TestAppContext) {
    for (width, height) in [(720.0, 460.0), (1380.0, 880.0), (1920.0, 1200.0)] {
        for tab in [0, 1, 3, 4] {
            let (_, window) = cx.add_window_view(|_, cx| {
                let mut view = fixture(cx);
                view.app.extension_shop.is_open = false;
                view.app.settings_widget.is_open = true;
                view.app.settings_widget.active_tab = tab;
                view.app.settings_widget.config.theme = "gruber-darker (Installed)".into();
                view
            });
            window.simulate_resize(size(px(width), px(height)));
            window.run_until_parked();
            let row = window.debug_bounds("settings-row-0").unwrap();
            let value = window.debug_bounds("settings-value-0").unwrap();
            assert!(value.size.width >= px(if tab == 1 { 150.0 } else { 32.0 }),
                "value collapsed at {width}, tab {tab}: {value:?}");
            assert!(value.size.height <= px(40.0), "ordinary value must fit one line: {value:?}");
            // At narrow widths the theme control intentionally moves below the
            // label/description, rather than stealing their reading width.
            assert!(row.size.height <= px(if width < 800.0 { 145.0 } else { 110.0 }),
                "ordinary setting must not become a giant row at {width}, tab {tab}: {row:?}, value={value:?}");
            assert!(value.right() <= row.right() - px(8.0), "value needs trailing padding");
            if width >= 800.0 {
                let copy = window.debug_bounds("settings-copy-0").unwrap();
                assert!(value.top() < copy.bottom() && copy.top() < value.bottom(),
                    "normal-sized settings should retain a horizontal layout: {copy:?}, {value:?}");
            }
            if tab == 0 {
                let mode = window.debug_bounds("settings-value-1").unwrap();
                assert!(mode.size.width >= px(40.0) && mode.size.height <= px(28.0),
                    "Mode dropdown should not wrap into a vertical ribbon: {mode:?}");
            }
        }
    }
}

#[gpui::test]
fn unusually_long_setting_values_wrap_in_a_readable_control(cx: &mut TestAppContext) {
    let (_, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.settings_widget.is_open = true;
        view.app.settings_widget.active_tab = 1;
        view.app.settings_widget.config.theme = "long-custom-theme-name-".repeat(10);
        view
    });
    window.simulate_resize(size(px(720.0), px(460.0)));
    window.run_until_parked();
    let row = window.debug_bounds("settings-row-0").unwrap();
    let value = window.debug_bounds("settings-value-0").unwrap();
    assert!(value.size.width >= px(200.0) && value.size.width <= px(260.0));
    assert!(value.size.height > px(28.0), "exceptional values wrap, rather than overflow or truncate");
    assert!(value.right() <= row.right() - px(8.0));
}

#[gpui::test]
fn plugin_status_is_readable_and_normal_cards_stay_compact(cx: &mut TestAppContext) {
    for (width, height) in [(720.0, 460.0), (1380.0, 880.0), (1920.0, 1200.0)] {
        let (_, window) = cx.add_window_view(|_, cx| {
            let mut view = fixture(cx);
            for plugin in &mut view.app.extension_shop.plugins {
                plugin.description = "Bundled | Enabled | Required by: neovim/nvim-lspconfig".into();
            }
            view
        });
        window.simulate_resize(size(px(width), px(height)));
        window.run_until_parked();
        let row = window.debug_bounds("shop-item-0").unwrap();
        let badge = window.debug_bounds("shop-badge-0").unwrap();
        let name = window.debug_bounds("shop-name-0").unwrap();
        assert!(name.size.width >= px(100.0), "name must retain readable space: {name:?}");
        assert!(name.size.height <= px(28.0), "ordinary plugin names should stay on one line: {name:?}");
        if width >= 800.0 {
            assert!(badge.top() < name.bottom() && name.top() < badge.bottom(),
                "normal-sized plugin headings should retain name/status alignment: {name:?}, {badge:?}");
        }
        assert!(badge.size.width >= px(200.0), "status squeezed into a ribbon: {badge:?}");
        assert!(badge.size.height <= px(28.0), "status should be a single-line badge: {badge:?}");
        assert!(row.size.height <= px(140.0), "normal plugin card became excessively tall: {row:?}, badge={badge:?}, heading={:?}, name={:?}, description={:?}", window.debug_bounds("shop-heading-0"), window.debug_bounds("shop-name-0"), window.debug_bounds("shop-description-0"));
        assert!(badge.right() <= row.right() - px(8.0), "badge needs trailing padding");
    }
}

#[gpui::test]
fn production_plugin_center_keeps_long_rows_badges_and_footer_inside(cx: &mut TestAppContext) {
    for (width, height) in [(720.0, 460.0), (1380.0, 880.0), (1920.0, 1200.0)] {
        let (_, window) = cx.add_window_view(|_, cx| fixture(cx));
        window.simulate_resize(size(px(width), px(height)));
        window.run_until_parked();
        let card = window.debug_bounds("modal-card").expect("modal card");
        let list = window.debug_bounds("shop-plugins").expect("plugin list");
        let row = window.debug_bounds("shop-item-0").expect("first plugin");
        let badge = window.debug_bounds("shop-badge-0").expect("status badge");
        let description = window.debug_bounds("shop-description-0").expect("description");
        let footer = window.debug_bounds("shop-footer").expect("footer");
        assert!(card.size.width <= px(width * 0.92 + 1.0));
        for (name, bounds) in [("list", list), ("row", row), ("badge", badge), ("description", description)] {
            assert!(bounds.right() <= card.right() + px(1.0), "{name} cropped at {width}x{height}: {bounds:?}, card={card:?}");
        }
        assert!(description.size.height > px(32.0), "description wraps instead of disappearing");
        assert!(list.bottom() <= footer.top() + px(1.0));
        assert!(footer.bottom() <= card.bottom() + px(1.0));
        assert!(footer.size.height >= px(32.0), "footer actions stay usable");
        window.simulate_event(gpui::ScrollWheelEvent {
            position: point(list.left() + px(40.0), list.top() + px(40.0)),
            delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-180.0))),
            touch_phase: gpui::TouchPhase::Moved,
            ..Default::default()
        });
        window.run_until_parked();
        let moved_row = window.debug_bounds("shop-item-0").unwrap();
        assert!(moved_row.top() < row.top(), "wrapped content remains vertically scrollable");
    }
}

#[gpui::test]
fn split_headers_render_only_for_multiple_splits_and_keep_close_action_accessible(cx: &mut TestAppContext) {
    use crate::tui::{app::NvimWin, layout::Rect};

    // Single window must not render any split header overlay.
    let (_, single_win) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.nvim_wins = vec![NvimWin {
            id: 1000,
            bufnr: 1,
            row: 0,
            col: 0,
            width: 80,
            height: 24,
            active: true,
            name: "main.rs".into(),
            relative_name: "src/main.rs".into(),
            ..Default::default()
        }];
        view
    });
    single_win.simulate_resize(size(px(1200.0), px(760.0)));
    single_win.run_until_parked();
    assert!(single_win.debug_bounds("split-header-0").is_none());

    // Multi-split (top-left, bottom-left, narrow right) must align inside the editor pane,
    // keep close buttons inside each header, and hide headers occluded by a Neovim float.
    let (_, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.nvim_wins = vec![
            NvimWin {
                id: 1001,
                bufnr: 1,
                row: 0,
                col: 0,
                width: 45,
                height: 12,
                active: true,
                name: "very_long_split_filename_that_should_truncate_cleanly.rs".into(),
                relative_name: "src/gui/very_long_split_filename_that_should_truncate_cleanly.rs".into(),
                modified: true,
                ..Default::default()
            },
            NvimWin {
                id: 1002,
                bufnr: 2,
                row: 13,
                col: 0,
                width: 45,
                height: 11,
                active: false,
                name: "chrome.rs".into(),
                relative_name: "src/gui/chrome.rs".into(),
                readonly: true,
                ..Default::default()
            },
            NvimWin {
                id: 1003,
                bufnr: 3,
                row: 0,
                col: 46,
                width: 10,
                height: 24,
                active: false,
                name: "narrow.rs".into(),
                relative_name: "narrow.rs".into(),
                ..Default::default()
            },
        ];
        view
    });
    window.simulate_resize(size(px(1200.0), px(760.0)));
    window.run_until_parked();

    let pane = window.debug_bounds("nvim-editor-pane").expect("editor pane");
    let h0 = window.debug_bounds("split-header-0").expect("top-left split header");
    let h1 = window.debug_bounds("split-header-1").expect("bottom-left split header");
    let h2 = window.debug_bounds("split-header-2").expect("narrow right split header");

    for (label, h, close_sel) in [
        ("top-left", h0, "split-close-0"),
        ("bottom-left", h1, "split-close-1"),
        ("narrow-right", h2, "split-close-2"),
    ] {
        assert!(h.left() >= pane.left(), "{label} escaped left: {h:?} vs {pane:?}");
        assert!(h.top() >= pane.top(), "{label} escaped top: {h:?} vs {pane:?}");
        assert!(h.right() <= pane.right() + px(1.0), "{label} escaped right: {h:?} vs {pane:?}");
        let close = window.debug_bounds(close_sel).expect("close button");
        assert!(close.left() >= h.left() && close.right() <= h.right() + px(1.0),
            "{label} close button must stay inside header: {close:?} vs {h:?}");
        assert!(close.size.width >= px(14.0) && close.size.height >= px(14.0),
            "{label} close button must stay clickable: {close:?}");
    }
    assert!(h0.right() < h2.left(), "side-by-side split headers must not overlap: {h0:?} vs {h2:?}");
    assert!(h0.bottom() < h1.top(), "stacked split headers must not overlap: {h0:?} vs {h1:?}");
    assert!(window.debug_bounds("split-only-0").is_some(), "wide split shows keep-only action");
    assert!(window.debug_bounds("split-only-2").is_none(), "narrow split hides secondary action to keep close accessible");

    // When a floating window covers row 0 cols 0..40, top-left header hides while others stay visible.
    let (_, float_win) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.nvim_wins = vec![
            NvimWin { id: 1001, bufnr: 1, row: 0, col: 0, width: 45, height: 12, active: true, name: "a.rs".into(), ..Default::default() },
            NvimWin { id: 1002, bufnr: 2, row: 13, col: 0, width: 45, height: 11, active: false, name: "b.rs".into(), ..Default::default() },
            NvimWin { id: 1003, bufnr: 3, row: 0, col: 46, width: 30, height: 24, active: false, name: "c.rs".into(), ..Default::default() },
        ];
        view.app.nvim_floats = vec![Rect::new(0, 0, 40, 8)];
        view
    });
    float_win.simulate_resize(size(px(1200.0), px(760.0)));
    float_win.run_until_parked();
    assert!(float_win.debug_bounds("split-header-0").is_none(), "float-occluded header must hide");
    assert!(float_win.debug_bounds("split-header-1").is_some(), "unoccluded lower header stays visible");
    assert!(float_win.debug_bounds("split-header-2").is_some(), "unoccluded right header stays visible");
}

#[gpui::test]
fn compile_modal_keeps_header_command_output_and_footer_contained(cx: &mut TestAppContext) {
    use crate::tui::widgets::compile::{parse_ansi_line, CompileStage};

    for (width, height) in [(720.0, 460.0), (1380.0, 880.0)] {
        let (_, window) = cx.add_window_view(|_, cx| {
            let mut view = fixture(cx);
            view.app.extension_shop.is_open = false;
            let cw = &mut view.app.compile_widget;
            cw.open_with(
                "/home/roubs/Project/very/deep/workspace/path/Gtuim".into(),
                "cargo build --workspace --all-targets --message-format=short".into(),
                "cargo build".into(),
                "proj\0rust".into(),
            );
            cw.stage = CompileStage::Failed(101);
            for i in 0..24 {
                cw.lines.push(parse_ansi_line(&format!(
                    "\x1b[1;31merror[E0425]\x1b[0m: line {i} {}",
                    "long/compiler/diagnostic/path/segment/".repeat(6)
                )));
            }
            view
        });
        window.simulate_resize(size(px(width), px(height)));
        window.run_until_parked();

        let card = window.debug_bounds("modal-card").expect("compile modal card");
        let header = window.debug_bounds("compile-header").expect("compile header");
        let badge = window.debug_bounds("compile-status-badge").expect("status badge");
        let cmd_bar = window.debug_bounds("compile-command-bar").expect("command bar");
        let output = window.debug_bounds("compile-output").expect("output viewport");
        let footer = window.debug_bounds("compile-footer").expect("compile footer");

        assert!(header.bottom() <= cmd_bar.top() + px(1.0));
        assert!(cmd_bar.bottom() <= output.top() + px(1.0));
        assert!(output.bottom() <= footer.top() + px(1.0));
        assert!(footer.bottom() <= card.bottom() + px(1.0));
        assert!(output.size.height >= px(100.0), "output viewport must stay readable at {width}x{height}: {output:?}");
        assert!(badge.right() <= header.right() + px(1.0), "status badge must stay inside header");
        assert!(badge.size.height <= px(28.0), "status badge must stay single-line");
    }
}

#[gpui::test]
fn onboarding_modal_fits_and_scrolls_across_all_steps_and_sizes(cx: &mut TestAppContext) {
    use crate::tui::widgets::onboarding::OnboardingStep;

    for (width, height) in [(720.0, 460.0), (1380.0, 880.0), (1920.0, 1200.0)] {
        for step in OnboardingStep::ALL {
            let (_, window) = cx.add_window_view(|_, cx| {
                let mut view = fixture(cx);
                view.app.extension_shop.is_open = false;
                let cfg = view.app.settings_widget.config.clone();
                view.app.onboarding.open(&cfg, true);
                view.app.onboarding.step = step;
                view
            });
            window.simulate_resize(size(px(width), px(height)));
            window.run_until_parked();

            let card = window.debug_bounds("modal-card").expect("onboarding modal card");
            let header = window.debug_bounds("onboarding-header").expect("onboarding header");
            let steps = window.debug_bounds("onboarding-steps").expect("onboarding steps");
            let body = window.debug_bounds("onboarding-body").expect("onboarding body");
            let footer = window.debug_bounds("onboarding-footer").expect("onboarding footer");
            let primary = window
                .debug_bounds("onboarding-primary-btn")
                .expect("onboarding primary button");

            assert!(
                card.size.width <= px(width * 0.92 + 1.0),
                "card too wide at {width}x{height} on {step:?}: {card:?}"
            );
            assert!(
                card.size.height <= px(height * 0.88 + 1.0),
                "card too tall at {width}x{height} on {step:?}: {card:?}"
            );
            assert!(header.bottom() <= steps.top() + px(1.0));
            assert!(steps.bottom() <= body.top() + px(1.0));
            assert!(body.bottom() <= footer.top() + px(1.0));
            assert!(footer.bottom() <= card.bottom() + px(1.0));
            assert!(
                body.size.height >= px(90.0),
                "body viewport collapsed at {width}x{height} on {step:?}: {body:?}"
            );
            assert!(
                body.right() <= card.right() + px(1.0),
                "body overflowed right at {width}x{height} on {step:?}: {body:?} vs {card:?}"
            );
            assert!(
                primary.right() <= card.right() + px(1.0) && primary.size.width >= px(52.0),
                "primary button clipped or collapsed at {width}x{height} on {step:?}: {primary:?}"
            );
            assert!(
                primary.size.height <= px(36.0),
                "primary button wrapped unexpectedly at {width}x{height} on {step:?}: {primary:?}"
            );

            let key_selector = match step {
                OnboardingStep::Theme => "onboarding-theme-0",
                OnboardingStep::Controls => "onboarding-mode-0",
                OnboardingStep::Guide => "onboarding-guide-card-0",
                OnboardingStep::Setup => "onboarding-pref-numbers",
            };
            let item = window.debug_bounds(key_selector).expect(key_selector);
            assert!(
                item.right() <= card.right() + px(1.0),
                "{key_selector} overflowed at {width}x{height}: {item:?} vs {card:?}"
            );
            assert!(
                item.size.width >= px(140.0),
                "{key_selector} collapsed at {width}x{height}: {item:?}"
            );

            if step == OnboardingStep::Theme && width < 800.0 {
                window.simulate_event(gpui::ScrollWheelEvent {
                    position: point(body.left() + px(40.0), body.top() + px(40.0)),
                    delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(-180.0))),
                    touch_phase: gpui::TouchPhase::Moved,
                    ..Default::default()
                });
                window.run_until_parked();
                let moved = window.debug_bounds("onboarding-theme-0").unwrap();
                assert!(
                    moved.top() < item.top(),
                    "onboarding body must scroll vertically on compact viewports"
                );
            }
        }
    }
}

#[gpui::test]
fn explorer_drag_and_drop_moves_file_out_of_parent_and_into_other_folder(cx: &mut TestAppContext) {
    use crate::tui::widgets::explorer::FileExplorer;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "gtuim-gui-dnd-exp-{}-{}",
        std::process::id(),
        nanos
    ));
    let dir_a = root.join("alpha");
    let dir_b = root.join("beta");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    let child_file = dir_a.join("dragme.rs");
    let root_file = root.join("root.rs");
    std::fs::write(&child_file, "// drag me").unwrap();
    std::fs::write(&root_file, "// root").unwrap();

    let (view_entity, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.workspace.sidebar_visible = true;
        view.app.workspace.overview = false;
        view.app.activity_bar.active_tab = 0;
        let mut exp = FileExplorer::new_at(root.to_string_lossy().to_string());
        exp.expanded.insert(dir_a.to_string_lossy().to_string());
        exp.refresh();
        view.app.explorer = exp;
        view
    });
    window.simulate_resize(size(px(1200.0), px(760.0)));
    window.run_until_parked();

    // Expected sorted order: 0: alpha/, 1: alpha/dragme.rs, 2: beta/, 3: root.rs
    let child_bounds = window.debug_bounds("exp-row-1").expect("dragme.rs row");
    let beta_bounds = window.debug_bounds("exp-row-2").expect("beta folder row");
    let start = child_bounds.center();

    // Drag alpha/dragme.rs to beta/
    window.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_move(
        point(start.x, start.y + px(12.0)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    window.run_until_parked();

    // While dragging a nested file, the root drop banner appears.
    assert!(
        window.debug_bounds("exp-drop-root").is_some(),
        "root drop bar must appear when dragging a nested file"
    );

    let beta_center = beta_bounds.center();
    window.simulate_mouse_move(
        beta_center,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    window.simulate_mouse_up(
        beta_center,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    window.run_until_parked();

    let in_beta = dir_b.join("dragme.rs");
    assert!(!child_file.exists(), "source file should have moved");
    assert!(in_beta.exists(), "file should now be inside beta/");
    view_entity.read_with(window, |view, _| {
        assert!(view.explorer_drag.is_none());
        assert!(view.app.explorer.expanded.contains(dir_b.to_str().unwrap()));
        // After move: 0: alpha/, 1: beta/, 2: beta/dragme.rs, 3: root.rs
        assert_eq!(view.app.explorer.entries[2].name, "dragme.rs");
    });

    // Now drag beta/dragme.rs out to the root drop bar to move it to project root.
    let drag_bounds = window
        .debug_bounds("exp-row-2")
        .expect("dragme.rs row in beta");
    let drag_start = drag_bounds.center();

    window.simulate_mouse_down(
        drag_start,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    window.simulate_mouse_move(
        point(drag_start.x, drag_start.y - px(12.0)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    window.run_until_parked();

    let root_bar = window
        .debug_bounds("exp-drop-root")
        .expect("root drop bar while dragging out of beta");
    let drop_pt = root_bar.center();
    window.simulate_mouse_move(
        drop_pt,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    window.simulate_mouse_up(
        drop_pt,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    window.run_until_parked();

    assert!(!in_beta.exists(), "file should have moved out of beta/");
    assert!(
        root.join("dragme.rs").exists(),
        "file should now live in project root"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[gpui::test]
fn split_pane_drag_and_drop_previews_bottom_and_dispatches_move_rpc(cx: &mut TestAppContext) {
    use crate::tui::app::NvimWin;

    let (view_entity, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.workspace.sidebar_visible = false;
        view.app.nvim_wins = vec![
            NvimWin {
                id: 1001,
                bufnr: 1,
                row: 0,
                col: 0,
                width: 48,
                height: 28,
                active: false,
                name: "left.rs".into(),
                ..Default::default()
            },
            NvimWin {
                id: 1002,
                bufnr: 2,
                row: 0,
                col: 49,
                width: 48,
                height: 28,
                active: true,
                name: "right.rs".into(),
                ..Default::default()
            },
        ];
        view
    });
    window.simulate_resize(size(px(1200.0), px(760.0)));
    window.run_until_parked();

    let pane = window.debug_bounds("nvim-editor-pane").expect("editor pane");
    let right_hdr = window
        .debug_bounds("split-header-1")
        .expect("right split header");
    let grab_pt = point(right_hdr.left() + px(40.0), right_hdr.center().y);
    let bottom_pt = point(right_hdr.center().x, pane.bottom() - px(40.0));

    // Grab right split header and drag straight down to the bottom of the editor pane.
    window.simulate_mouse_down(grab_pt, gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_move(
        point(grab_pt.x, grab_pt.y + px(14.0)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    window.run_until_parked();
    window.simulate_mouse_move(
        bottom_pt,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    window.run_until_parked();

    // Preview overlay must render in the bottom half of the editor pane.
    let preview = window
        .debug_bounds("split-drop-preview")
        .expect("bottom drop preview overlay");
    assert!(
        preview.top() >= pane.center().y - px(4.0),
        "bottom preview must sit in lower half: preview={preview:?}, pane={pane:?}"
    );
    assert!(
        preview.bottom() <= pane.bottom() + px(1.0),
        "bottom preview must stay inside pane: preview={preview:?}, pane={pane:?}"
    );
    view_entity.read_with(window, |view, _| {
        let state = view.split_drag.as_ref().expect("split drag active");
        assert_eq!(state.source.win_id, 1002);
        assert_eq!(
            state.target.as_ref().map(|t| t.kind),
            Some(crate::gui::drag_drop::SplitMoveKind::DockBottom)
        );
    });

    // Drop to execute the move.
    window.simulate_mouse_up(
        bottom_pt,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    window.run_until_parked();

    assert!(
        window.debug_bounds("split-drop-preview").is_none(),
        "preview must clear after drop"
    );
    view_entity.update(window, |view, _| {
        assert!(view.split_drag.is_none());
        let mut found_move = false;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while !found_move && std::time::Instant::now() < deadline {
            let _ = view.app.editor_rpc.pump_incoming();
            while let Some(msg) = view.app.editor_rpc.next_message() {
                if let crate::nvim::rpc::RpcMessage::Notification { method, params } = msg {
                    if method == "nvim_exec_lua" {
                        let code = params.first().and_then(|v| v.as_str()).unwrap_or("");
                        if code.contains("tuim_move_split") {
                            let args = params
                                .get(1)
                                .and_then(|v| v.as_array())
                                .expect("lua args");
                            assert_eq!(args[0].as_i64(), Some(1002));
                            assert_eq!(args[1].as_str(), Some("dock_bottom"));
                            assert_eq!(args[2].as_i64(), Some(0));
                            found_move = true;
                        }
                    }
                }
            }
            if !found_move {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        assert!(found_move, "dropping split must dispatch _G.tuim_move_split RPC");
    });

    // Pressing Escape cancels an in-progress drag cleanly and prevents any drop RPC.
    window.simulate_mouse_down(grab_pt, gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_move(
        bottom_pt,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    window.run_until_parked();
    view_entity.read_with(window, |view, cx| {
        assert!(cx.has_active_drag());
        assert!(view.split_drag.as_ref().and_then(|s| s.target.as_ref()).is_some());
    });
    window.simulate_event(gpui::KeyDownEvent {
        keystroke: gpui::Keystroke::parse("escape").unwrap(),
        is_held: false,
    });
    window.run_until_parked();
    view_entity.read_with(window, |view, cx| {
        assert!(
            !cx.has_active_drag(),
            "active drag should be stopped by Escape"
        );
        assert!(
            view.split_drag.is_none(),
            "split_drag should be cleared by Escape"
        );
    });
    window.simulate_mouse_up(
        bottom_pt,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    window.run_until_parked();
    view_entity.update(window, |view, _| {
        std::thread::sleep(std::time::Duration::from_millis(20));
        let _ = view.app.editor_rpc.pump_incoming();
        while let Some(msg) = view.app.editor_rpc.next_message() {
            if let crate::nvim::rpc::RpcMessage::Notification { method, params } = msg {
                if method == "nvim_exec_lua" {
                    let code = params.first().and_then(|v| v.as_str()).unwrap_or("");
                    assert!(
                        !code.contains("tuim_move_split"),
                        "cancelled drag must not dispatch tuim_move_split"
                    );
                }
            }
        }
    });
}

#[gpui::test]
fn hiding_tab_bar_from_view_menu_replaces_it_with_single_pane_header(cx: &mut TestAppContext) {
    use crate::tui::{app::NvimWin, widgets::menu_bar::MENUS};

    // Confirm View menu exposes toggle_tab_bar.
    let view_menu_idx = MENUS
        .iter()
        .position(|m| m.title == "View")
        .expect("View menu exists");
    let toggle_item_idx = MENUS[view_menu_idx]
        .items
        .iter()
        .position(|(_, action)| *action == "toggle_tab_bar")
        .expect("Toggle Tab Bar item in View menu");

    // 1) Default state (tab bar visible, single window): tab bar renders, split header does not.
    let (_, shown_win) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.workspace.tab_bar_visible = true;
        view.app.nvim_wins = vec![NvimWin {
            id: 1000,
            bufnr: 1,
            row: 0,
            col: 0,
            width: 80,
            height: 24,
            active: true,
            name: "chrome.rs".into(),
            relative_name: "src/gui/chrome.rs".into(),
            ..Default::default()
        }];
        view
    });
    shown_win.simulate_resize(size(px(1200.0), px(760.0)));
    shown_win.run_until_parked();
    assert!(shown_win.debug_bounds("editor-tab-bar").is_some());
    assert!(shown_win.debug_bounds("split-header-0").is_none());

    // 2) Hidden tab bar state (single window): tab bar is absent and pane header takes its place.
    let (hidden_entity, hidden_win) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.workspace.tab_bar_visible = false;
        view.app.nvim_wins = vec![NvimWin {
            id: 1000,
            bufnr: 1,
            row: 0,
            col: 0,
            width: 80,
            height: 24,
            active: true,
            name: "chrome.rs".into(),
            relative_name: "src/gui/chrome.rs".into(),
            ..Default::default()
        }];
        view
    });
    hidden_win.simulate_resize(size(px(1200.0), px(760.0)));
    hidden_win.run_until_parked();
    assert!(hidden_win.debug_bounds("editor-tab-bar").is_none());
    let hdr = hidden_win
        .debug_bounds("split-header-0")
        .expect("pane header must replace hidden tab bar");
    let pane = hidden_win
        .debug_bounds("nvim-editor-pane")
        .expect("editor pane");
    assert!(hdr.left() >= pane.left() && hdr.top() >= pane.top());
    assert!(hidden_win.debug_bounds("split-close-0").is_some());
    assert!(hidden_win.debug_bounds("split-only-0").is_some());

    // 3) Clicking View -> Show Tab Bar toggles tab_bar_visible back to true.
    hidden_entity.update(hidden_win, |view, cx| {
        view.app.menu_bar.open_menu = Some(view_menu_idx);
        cx.notify();
    });
    hidden_win.run_until_parked();
    let item_sel: &'static str = match toggle_item_idx {
        4 => "menu-dd-item-4",
        _ => panic!("unexpected View menu index {toggle_item_idx}"),
    };
    let item_bounds = hidden_win
        .debug_bounds(item_sel)
        .expect("View menu Toggle Tab Bar item");
    let click_pt = item_bounds.center();
    hidden_win.simulate_mouse_down(click_pt, gpui::MouseButton::Left, gpui::Modifiers::default());
    hidden_win.simulate_mouse_up(click_pt, gpui::MouseButton::Left, gpui::Modifiers::default());
    hidden_win.run_until_parked();
    hidden_entity.read_with(hidden_win, |view, _| {
        assert!(view.app.workspace.tab_bar_visible);
        assert!(view.app.settings_widget.config.show_tab_bar);
    });
    assert!(hidden_win.debug_bounds("editor-tab-bar").is_some());
}

#[gpui::test]
fn tiny_window_480x320_and_300x220_keep_editor_header_settings_accessible_and_recover_sizes(
    cx: &mut TestAppContext,
) {
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.workspace.sidebar_visible = true;
        view.app.workspace.drawer_visible = true;
        view.app.workspace.vertical_terminal_visible = true;
        view.sidebar_px = 256.0;
        view.drawer_px = 230.0;
        view.vterm_px = 400.0;
        view
    });

    // 1) 480x320 with all panes open: header controls never collide, editor keeps visible rows/cols,
    // right vertical terminal stays on-screen, and Settings gear in Activity Rail stays visible.
    window.simulate_resize(size(px(480.0), px(320.0)));
    window.run_until_parked();

    let hdr_left = window.debug_bounds("hdr-left").expect("header left");
    let hdr_right = window.debug_bounds("hdr-right").expect("header right");
    assert!(
        hdr_left.right() <= hdr_right.left() + px(1.0),
        "header controls collided at 480x320: left={hdr_left:?}, right={hdr_right:?}"
    );
    assert!(hdr_right.right() <= px(481.0), "header right clipped at 480x320: {hdr_right:?}");

    let editor = window.debug_bounds("nvim-editor-pane").expect("editor pane at 480x320");
    assert!(editor.size.width >= px(130.0), "editor too narrow at 480x320: {editor:?}");
    assert!(editor.size.height >= px(70.0), "editor collapsed vertically at 480x320: {editor:?}");
    assert!(editor.right() <= px(481.0));

    if let Some(vterm) = window.debug_bounds("vterm-pane") {
        assert!(vterm.right() <= px(481.0), "vterm pushed off-screen at 480x320: {vterm:?}");
    }

    let settings_btn = window.debug_bounds("rail-settings").expect("settings gear at 480x320");
    assert!(
        settings_btn.bottom() <= px(320.0),
        "settings gear clipped off-screen at 480x320: {settings_btn:?}"
    );

    // 2) 300x220: editor stays visible on-screen, header does not clip, Settings gear stays visible.
    window.simulate_resize(size(px(300.0), px(220.0)));
    window.run_until_parked();

    let hdr_left_300 = window.debug_bounds("hdr-left").expect("header left at 300x220");
    let hdr_right_300 = window.debug_bounds("hdr-right").expect("header right at 300x220");
    assert!(
        hdr_left_300.right() <= hdr_right_300.left() + px(1.0),
        "header collided at 300x220: left={hdr_left_300:?}, right={hdr_right_300:?}"
    );
    assert!(hdr_right_300.right() <= px(301.0), "header clipped at 300x220: {hdr_right_300:?}");

    let editor_300 = window.debug_bounds("nvim-editor-pane").expect("editor at 300x220");
    assert!(editor_300.size.width >= px(120.0), "editor pushed off-screen at 300x220: {editor_300:?}");
    assert!(editor_300.size.height >= px(80.0), "editor collapsed at 300x220: {editor_300:?}");

    let settings_btn_300 = window.debug_bounds("rail-settings").expect("settings gear at 300x220");
    assert!(
        settings_btn_300.bottom() <= px(220.0),
        "settings gear clipped at 300x220: {settings_btn_300:?}"
    );

    // Open Settings modal at 300x220: category tab "General" must not wrap mid-word,
    // and System Clipboard toggle control must remain visible inside settings-rows.
    view.update(window, |this, cx| {
        this.app.settings_widget.is_open = true;
        this.app.settings_widget.active_tab = 0;
        cx.notify();
    });
    window.run_until_parked();

    let tab0 = window.debug_bounds("settings-tab-0").expect("General tab at 300x220");
    assert!(
        tab0.size.height <= px(26.0),
        "General tab wrapped mid-word at 300x220: {tab0:?}"
    );
    let rows_box = window.debug_bounds("settings-rows").expect("settings rows at 300x220");
    let val0 = window.debug_bounds("settings-value-0").expect("System Clipboard value at 300x220");
    assert!(
        val0.bottom() <= rows_box.bottom() + px(1.0) && val0.right() <= rows_box.right() + px(1.0),
        "System Clipboard toggle clipped at 300x220: val={val0:?}, rows={rows_box:?}"
    );

    // 3) Enlarge back to 1440x900: preferred pane sizes recover without corruption.
    view.update(window, |this, cx| {
        this.app.settings_widget.is_open = false;
        cx.notify();
    });
    window.simulate_resize(size(px(1440.0), px(900.0)));
    window.run_until_parked();

    view.read_with(window, |this, _| {
        assert_eq!(this.sidebar_px, 256.0);
        assert_eq!(this.drawer_px, 230.0);
        assert_eq!(this.vterm_px, 400.0);
    });
    let sb = window.debug_bounds("sidebar-pane").expect("sidebar recovered");
    let vt = window.debug_bounds("vterm-pane").expect("vterm recovered");
    let dr = window.debug_bounds("bottom-drawer-pane").expect("drawer recovered");
    assert!((f32::from(sb.size.width) - 256.0).abs() <= 2.0, "sidebar size not recovered: {sb:?}");
    assert!((f32::from(vt.size.width) - 400.0).abs() <= 2.0, "vterm size not recovered: {vt:?}");
    assert!((f32::from(dr.size.height) - 230.0).abs() <= 2.0, "drawer size not recovered: {dr:?}");
}

#[gpui::test]
fn tab_bar_overflow_scrolls_keeps_active_eighth_tab_visible_and_exposes_controls(
    cx: &mut TestAppContext,
) {
    use crate::tui::app::{PaletteMode, TabInfo};

    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.tabs = (1..=9)
            .map(|i| TabInfo {
                bufnr: i,
                name: format!("/tmp/project/file_number_{i}_long_name.txt"),
                relative_name: if i == 8 {
                    "created with space.txt".into()
                } else {
                    format!("file_number_{i}_long_name.txt")
                },
                changed: i == 8,
                is_preview: false,
                ..Default::default()
            })
            .collect();
        view.app.active_tab = 7; // 8th tab active
        view
    });

    window.simulate_resize(size(px(900.0), px(600.0)));
    window.run_until_parked();

    let bar = window.debug_bounds("editor-tab-bar").expect("tab bar");
    let controls = window
        .debug_bounds("tab-overflow-controls")
        .expect("overflow controls for 9 tabs");
    let active_tab = window
        .debug_bounds("tab-7")
        .expect("active 8th tab must auto-scroll into view");
    assert!(
        active_tab.left() >= bar.left() - px(1.0) && active_tab.right() <= controls.left() + px(1.0),
        "active 8th tab clipped or overlapped controls: tab={active_tab:?}, controls={controls:?}, bar={bar:?}"
    );
    assert!(controls.right() <= bar.right() + px(1.0));

    // Clicking scroll-left shifts visible window earlier.
    let before_start = view.read_with(window, |this, _| this.tab_scroll_start);
    assert!(before_start > 0);
    let left_btn = window.debug_bounds("tab-scroll-left").expect("scroll left button");
    window.simulate_mouse_down(left_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(left_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();
    let after_start = view.read_with(window, |this, _| this.tab_scroll_start);
    assert_eq!(after_start, before_start - 1);

    // Clicking overflow menu opens Buffers palette.
    let overflow_btn = window.debug_bounds("tab-overflow-menu").expect("overflow menu button");
    window.simulate_mouse_down(overflow_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(overflow_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert!(this.app.workspace.palette_open);
        assert_eq!(this.app.workspace.palette_mode, PaletteMode::Buffers);
    });
}

#[gpui::test]
fn command_palette_never_clips_bottom_row_and_includes_save_as(cx: &mut TestAppContext) {
    use crate::tui::app::PaletteMode;

    for (w, h) in [(480.0, 320.0), (1440.0, 900.0), (2560.0, 1440.0)] {
        let (_, window) = cx.add_window_view(|_, cx| {
            let mut view = fixture(cx);
            view.app.extension_shop.is_open = false;
            view.app.workspace.palette_open = true;
            view.app.workspace.palette_mode = PaletteMode::Commands;
            view.app.workspace.palette_query.clear();
            view.app.workspace.palette_index = 0;
            view.app.workspace.palette_scroll = 0;
            view
        });
        window.simulate_resize(size(px(w), px(h)));
        window.run_until_parked();

        let card = window.debug_bounds("modal-card").expect("palette card");
        let list = window.debug_bounds("palette-list").expect("palette list");
        let selectors = [
            "palette-cmd-0",
            "palette-cmd-1",
            "palette-cmd-2",
            "palette-cmd-3",
            "palette-cmd-4",
            "palette-cmd-5",
            "palette-cmd-6",
            "palette-cmd-7",
            "palette-cmd-8",
            "palette-cmd-9",
            "palette-cmd-10",
            "palette-cmd-11",
        ];
        let mut rendered_count = 0usize;
        for (idx, sel) in selectors.iter().enumerate() {
            if let Some(row) = window.debug_bounds(sel) {
                rendered_count += 1;
                assert!(
                    row.top() >= list.top() - px(1.0) && row.bottom() <= list.bottom() + px(1.0),
                    "palette row {idx} clipped at {w}x{h}: row={row:?}, list={list:?}, card={card:?}"
                );
            }
        }
        assert!(rendered_count >= 1, "at least one palette row must render at {w}x{h}");
    }

    // Filtering "save" exposes Save file as... in palette.
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.workspace.palette_open = true;
        view.app.workspace.palette_mode = PaletteMode::Commands;
        view.app.workspace.palette_query = "save".into();
        view
    });
    window.simulate_resize(size(px(1200.0), px(760.0)));
    window.run_until_parked();
    view.read_with(window, |this, _| {
        let cmds = super::modals::palette_filtered_commands(&this.app);
        assert!(cmds.iter().any(|c| c.id == "save_file"));
        assert!(cmds.iter().any(|c| c.id == "save_file_as"));
    });
}

#[gpui::test]
fn top_menu_switches_in_single_click_and_file_menu_includes_save_as(cx: &mut TestAppContext) {
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.menu_bar.open_menu = Some(0); // File menu open
        view
    });

    window.simulate_resize(size(px(1280.0), px(800.0)));
    window.run_until_parked();

    // File menu includes Save As... and menu items stay single-line (never collapse into vertical ribbons).
    let file_items = super::modals::file_menu_items_with_save_as(
        0,
        crate::tui::widgets::menu_bar::MENUS[0].items,
    );
    assert!(file_items.iter().any(|&(_, act)| act == "save_file_as"));
    let item0 = window.debug_bounds("menu-dd-item-0").expect("first File menu item");
    assert!(
        item0.size.height <= px(32.0) && item0.size.width >= px(160.0),
        "menu item must stay readable single-line row: {item0:?}"
    );

    // Single click on Edit (top-menu-1) switches straight from File (0) to Edit (1).
    let edit_btn = window.debug_bounds("top-menu-1").expect("Edit top menu button");
    window.simulate_mouse_down(edit_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(edit_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();

    view.read_with(window, |this, _| {
        assert_eq!(
            this.app.menu_bar.open_menu,
            Some(1),
            "clicking Edit while File menu is open must switch in a single click"
        );
    });
}

#[gpui::test]
fn narrow_explorer_truncates_long_filename_without_overlapping_git_badge(
    cx: &mut TestAppContext,
) {
    use crate::tui::widgets::explorer::ExplorerEntry;

    let (_, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.workspace.sidebar_visible = true;
        view.app.activity_bar.active_tab = 0;
        view.sidebar_px = 160.0;
        view.app.explorer.root_dir = "/tmp/proj".into();
        view.app.explorer.entries = vec![ExplorerEntry {
            path: "/tmp/proj/unicode_日本語_café_very_long_filename.txt".into(),
            name: "unicode_日本語_café_very_long_filename.txt".into(),
            is_dir: false,
            depth: 0,
            expanded: false,
        }];
        view.app
            .explorer
            .git_badges
            .insert("unicode_日本語_café_very_long_filename.txt".into(), '?');
        view
    });

    window.simulate_resize(size(px(900.0), px(600.0)));
    window.run_until_parked();

    let sidebar = window.debug_bounds("sidebar-pane").expect("sidebar");
    let name = window.debug_bounds("exp-name-0").expect("explorer filename");
    let badge = window.debug_bounds("exp-badge-0").expect("explorer git badge");
    assert!(
        name.right() <= badge.left() + px(1.0),
        "git badge overlapped filename: name={name:?}, badge={badge:?}"
    );
    assert!(
        badge.right() <= sidebar.right() + px(1.0),
        "git badge escaped narrowed sidebar: badge={badge:?}, sidebar={sidebar:?}"
    );
}

#[gpui::test]
fn footer_uses_doc_cursor_telescope_preserves_tab_unnamed_hides_md_and_save_refreshes_explorer(
    cx: &mut TestAppContext,
) {
    use crate::nvim::rpc::RpcMessage;
    use crate::tui::app::TabInfo;
    use rmpv::Value;

    let tmp_dir = std::env::temp_dir().join(format!("gtuim-gui-sync-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let existing = tmp_dir.join("README.md");
    let txt = tmp_dir.join("deep space.txt");
    std::fs::write(&existing, "# readme\n").unwrap();
    std::fs::write(&txt, "hello\n").unwrap();

    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.explorer =
            crate::tui::widgets::explorer::FileExplorer::new_at(tmp_dir.to_string_lossy().into());
        view.app.ui_state.cursor_row = 20;
        view.app.ui_state.cursor_col = 7;
        view.app.tabs = vec![
            TabInfo {
                bufnr: 1,
                name: existing.to_string_lossy().into(),
                relative_name: "README.md".into(),
                filetype: "markdown".into(),
                changed: false,
                is_preview: false,
            },
            TabInfo {
                bufnr: 2,
                name: txt.to_string_lossy().into(),
                relative_name: "deep space.txt".into(),
                filetype: "text".into(),
                changed: false,
                is_preview: false,
            },
            TabInfo {
                bufnr: 3,
                name: String::new(),
                relative_name: String::new(),
                filetype: String::new(),
                changed: false,
                is_preview: false,
            },
        ];
        view.app.active_tab = 1; // deep space.txt
        view
    });

    window.simulate_resize(size(px(1100.0), px(700.0)));
    window.run_until_parked();

    // 1) Non-markdown active buffer must NOT show Markdown preview bar.
    assert!(window.debug_bounds("markdown-preview-toggle").is_none());

    // 2) Document cursor notification overrides grid coords (21, 8).
    view.update(window, |this, cx| {
        this.handle_rpc_with_gui_sync(RpcMessage::Notification {
            method: "tuim_cursor".into(),
            params: vec![Value::from(500_000u64), Value::from(25_006u64)],
        });
        assert_eq!(this.effective_doc_cursor(), (500_000, 25_006));

        // 3) TelescopePrompt unlisted current_buf (99) must NOT reset active_tab to 0 (README.md).
        let buf_list = Value::Array(vec![
            Value::Map(vec![
                (Value::from("bufnr"), Value::from(1)),
                (Value::from("name"), Value::from(existing.to_string_lossy().as_ref())),
                (Value::from("relative_name"), Value::from("README.md")),
                (Value::from("changed"), Value::from(false)),
            ]),
            Value::Map(vec![
                (Value::from("bufnr"), Value::from(2)),
                (Value::from("name"), Value::from(txt.to_string_lossy().as_ref())),
                (Value::from("relative_name"), Value::from("deep space.txt")),
                (Value::from("changed"), Value::from(false)),
            ]),
        ]);
        this.handle_rpc_with_gui_sync(RpcMessage::Notification {
            method: "tuim_buffers".into(),
            params: vec![buf_list, Value::from(99)],
        });
        assert_eq!(
            this.app.active_tab, 1,
            "unlisted TelescopePrompt buffer must preserve active tab index 1"
        );
        cx.notify();
    });
    window.run_until_parked();
    assert!(
        window.debug_bounds("markdown-preview-toggle").is_none(),
        "Markdown preview bar must not appear for deep space.txt while Telescope is open"
    );

    // 4) Unnamed [No Name] buffer must NOT show Markdown preview bar.
    view.update(window, |this, cx| {
        this.app.tabs.push(TabInfo {
            bufnr: 3,
            name: String::new(),
            relative_name: String::new(),
            filetype: String::new(),
            changed: false,
            is_preview: false,
        });
        this.app.active_tab = 2;
        cx.notify();
    });
    window.run_until_parked();
    assert!(
        window.debug_bounds("markdown-preview-toggle").is_none(),
        "unnamed [No Name] buffer must not show spurious Markdown preview bar"
    );

    // 5) Explorer auto-refreshes when a newly saved file appears on disk and tuim_file_saved arrives.
    let saved_copy = tmp_dir.join("saved_as_copy.txt");
    std::fs::write(&saved_copy, "copied\n").unwrap();
    view.update(window, |this, _| {
        assert!(!this.app.explorer.entries.iter().any(|e| e.name == "saved_as_copy.txt"));
        this.handle_rpc_with_gui_sync(RpcMessage::Notification {
            method: "tuim_file_saved".into(),
            params: vec![Value::from(saved_copy.to_string_lossy().as_ref())],
        });
        assert!(
            this.app.explorer.entries.iter().any(|e| e.name == "saved_as_copy.txt"),
            "Explorer must automatically show newly saved / Save As files"
        );
    });

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn cli_directory_arg_resolves_workspace_root_without_passing_dir_to_neovim() {
    let base = std::env::temp_dir().join(format!("gtuim-cli-root-test-{}", std::process::id()));
    let spaced_dir = base.join("folder with spaces");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&spaced_dir).unwrap();
    let note = base.join("note.txt");
    std::fs::write(&note, "hi").unwrap();

    let (root, files) =
        super::resolve_cli_workspace_args(&["folder with spaces".into()], Some(&base));
    assert_eq!(root, Some(std::fs::canonicalize(&spaced_dir).unwrap()));
    assert!(files.is_empty(), "directory arg must not be passed to Neovim edit/netrw");

    let (root2, files2) = super::resolve_cli_workspace_args(
        &["folder with spaces".into(), "note.txt".into()],
        Some(&base),
    );
    assert_eq!(root2, Some(std::fs::canonicalize(&spaced_dir).unwrap()));
    assert_eq!(files2, vec![note.to_string_lossy().to_string()]);

    let (root3, files3) =
        super::resolve_cli_workspace_args(&["does_not_exist.txt".into()], Some(&base));
    assert!(root3.is_none());
    assert_eq!(files3, vec!["does_not_exist.txt".to_string()]);

    let _ = std::fs::remove_dir_all(&base);
}

#[gpui::test]
fn gui_find_replace_bar_lifecycle_and_vim_mode_choice(cx: &mut TestAppContext) {
    use super::find_replace::FindReplaceField;
    use crate::nvim::rpc::RpcMessage;
    use crate::tui::app::Mode;
    use rmpv::Value;

    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.mode = Mode::Ide;
        view.app.settings_widget.config.search_replace_mode = "gui".into();
        view
    });

    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();

    // Ctrl+F in IDE mode opens GUI Find bar when search_replace_mode == "gui".
    window.update(|window, cx| {
        view.update(cx, |this, cx| {
            let ev = gpui::KeyDownEvent {
                keystroke: gpui::Keystroke::parse("ctrl-f").unwrap(),
                is_held: false,
            };
            this.handle_key_down(&ev, window, cx);
        });
    });
    window.run_until_parked();

    let bar = window.debug_bounds("find-replace-bar").expect("GUI find bar visible");
    let editor = window.debug_bounds("nvim-editor-pane").expect("editor pane");
    assert!(bar.left() >= editor.left() && bar.right() <= editor.right() + px(1.0));
    assert!(window.debug_bounds("replace-row").is_none());

    // Type query, toggle regex/case/word chips, expand replace via Tab, type replacement.
    window.update(|window, cx| {
        view.update(cx, |this, cx| {
            for ch in ["f", "o", "o"] {
                this.handle_key_down(
                    &gpui::KeyDownEvent {
                        keystroke: gpui::Keystroke {
                            modifiers: gpui::Modifiers::default(),
                            key: ch.into(),
                            key_char: Some(ch.into()),
                        },
                        is_held: false,
                    },
                    window,
                    cx,
                );
            }
            this.handle_key_down(
                &gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("tab").unwrap(),
                    is_held: false,
                },
                window,
                cx,
            );
            for ch in ["b", "a", "r"] {
                this.handle_key_down(
                    &gpui::KeyDownEvent {
                        keystroke: gpui::Keystroke {
                            modifiers: gpui::Modifiers::default(),
                            key: ch.into(),
                            key_char: Some(ch.into()),
                        },
                        is_held: false,
                    },
                    window,
                    cx,
                );
            }
            this.handle_rpc_with_gui_sync(RpcMessage::Notification {
                method: "gtuim_find_replace_state".into(),
                params: vec![Value::from(2), Value::from(5), Value::from("")],
            });
            cx.notify();
        });
    });
    window.run_until_parked();

    view.read_with(window, |this, _| {
        assert_eq!(this.find_replace.query, "foo");
        assert_eq!(this.find_replace.replacement, "bar");
        assert!(this.find_replace.show_replace);
        assert_eq!(this.find_replace.active_field, FindReplaceField::Replace);
        assert_eq!(this.find_replace.current_match, 2);
        assert_eq!(this.find_replace.match_count, 5);
    });
    assert!(window.debug_bounds("replace-row").is_some());
    assert!(window.debug_bounds("find-count").is_some());

    // Clicking Aa toggles case-sensitive flag; clicking Vim switches out of GUI bar.
    let case_btn = window.debug_bounds("find-case-btn").expect("case chip");
    window.simulate_mouse_down(case_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(case_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| assert!(this.find_replace.case_sensitive));

    let vim_btn = window.debug_bounds("find-vim-btn").expect("Vim handoff button");
    window.simulate_mouse_down(vim_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(vim_btn.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| assert!(!this.find_replace.is_open));

    // When search_replace_mode == "vim", Ctrl+F / "find" / "replace" stay in Vim mode,
    // while Ctrl+Shift+H (find_replace_gui) still offers the GUI bar on demand.
    window.update(|window, cx| {
        view.update(cx, |this, cx| {
            this.app.settings_widget.config.search_replace_mode = "vim".into();
            this.run_command("find", cx);
            assert!(!this.find_replace.is_open, "vim mode must keep find in Vim");
            this.run_command("replace", cx);
            assert!(!this.find_replace.is_open, "vim mode must keep replace in Vim");
            this.app.mode = Mode::Normal;
            this.handle_key_down(
                &gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("ctrl-shift-h").unwrap(),
                    is_held: false,
                },
                window,
                cx,
            );
            assert!(this.find_replace.is_open && this.find_replace.show_replace);
        });
    });
}

#[gpui::test]
fn settings_appearance_configures_font_family_weight_and_size_live(cx: &mut TestAppContext) {
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app
            .settings_widget
            .set_available_fonts(view.available_fonts.clone());
        view.app.settings_widget.is_open = true;
        view.app.settings_widget.active_tab = 1;
        view
    });

    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();

    let card = window.debug_bounds("modal-card").expect("settings card");
    let preview = window
        .debug_bounds("settings-font-preview")
        .expect("live font preview card");
    assert!(preview.right() <= card.right() + px(1.0));
    assert!(window.debug_bounds("settings-row-3").is_some());
    assert!(window.debug_bounds("settings-row-4").is_some());
    assert!(window.debug_bounds("settings-row-5").is_some());

    // Step font size up via inline '+' button in Settings Appearance.
    let inc = window.debug_bounds("settings-font-inc").expect("font size + button");
    window.simulate_mouse_down(inc.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(inc.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert_eq!(this.app.settings_widget.config.font_size, 15);
        assert_eq!(this.font_size, 15.0);
    });

    // Open Font Family dropdown (row 3) and pick Fira Code.
    view.update(window, |this, cx| {
        this.app.settings_widget.selected_row = 3;
        this.app.settings_widget.activate_selected_row();
        cx.notify();
    });
    window.run_until_parked();
    let fira_idx = view.read_with(window, |this, _| {
        this.app
            .settings_widget
            .font_family_options()
            .iter()
            .position(|(_, id)| id == "Fira Code")
            .unwrap()
    });
    assert_eq!(fira_idx, 1, "Fira Code should be first monospace font after Auto");
    let opt = window
        .debug_bounds("settings-dd-opt-1")
        .expect("Fira Code dropdown option");
    window.simulate_mouse_down(opt.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(opt.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();

    view.read_with(window, |this, _| {
        assert_eq!(this.app.settings_widget.config.font_family, "Fira Code");
        assert_eq!(this.mono_font.as_ref(), "Fira Code");
    });

    // Change Font Weight (row 4 -> Bold 700, index 4) and verify live weight sync.
    view.update(window, |this, cx| {
        this.app.settings_widget.selected_row = 4;
        this.app.settings_widget.activate_selected_row();
        this.app.settings_widget.select_dropdown_option(4);
        this.sync_font_from_settings();
        cx.notify();
    });
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert_eq!(this.app.settings_widget.config.font_weight, 700);
        assert_eq!(this.font_weight, gpui::FontWeight::BOLD);
    });
}

#[gpui::test]
fn explorer_context_menu_hides_rename_and_delete_when_no_target(cx: &mut TestAppContext) {
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.explorer.context_menu_open = true;
        view.app.explorer.context_target = None;
        view.explorer_ctx_pos = point(px(80.0), px(120.0));
        view
    });

    window.simulate_resize(size(px(800.0), px(600.0)));
    window.run_until_parked();
    assert!(window.debug_bounds("exp-ctx-item-0").is_some());
    assert!(window.debug_bounds("exp-ctx-item-1").is_some());
    assert!(
        window.debug_bounds("exp-ctx-item-2").is_none(),
        "Rename must be hidden for root/whitespace context menu"
    );
    assert!(
        window.debug_bounds("exp-ctx-item-3").is_none(),
        "Delete must be hidden for root/whitespace context menu"
    );

    view.update(window, |this, cx| {
        this.app.explorer.context_target = Some("/tmp/sample.txt".into());
        cx.notify();
    });
    window.run_until_parked();
    assert!(window.debug_bounds("exp-ctx-item-2").is_some());
    assert!(window.debug_bounds("exp-ctx-item-3").is_some());
}

fn click_debug_target(window: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = window
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing debug bounds for {selector}"));
    window.simulate_mouse_down(bounds.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.simulate_mouse_up(bounds.center(), gpui::MouseButton::Left, gpui::Modifiers::default());
    window.run_until_parked();
}

#[gpui::test]
fn bug_report_empty_summary_blocks_both_mouse_and_keyboard_consent(cx: &mut TestAppContext) {
    use crate::tui::widgets::bug_report::{BugStage, DEFAULT_BUG_REPORT_REPO};

    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.bug_report.is_open = true;
        view.app.bug_report.report_repo = DEFAULT_BUG_REPORT_REPO.into();
        view.app.bug_report.summary = "   ".into();
        view
    });
    window.simulate_resize(size(px(900.0), px(620.0)));
    window.run_until_parked();

    // Mouse click on Review Draft must not bypass required summary validation.
    click_debug_target(window, "bug-send");
    view.read_with(window, |this, _| {
        assert_eq!(this.app.bug_report.stage, BugStage::Form);
        assert_eq!(this.app.bug_report.focus, 1);
    });

    // Keyboard Ctrl+S also stays on Form when summary is blank.
    view.update(window, |this, cx| {
        this.send_tuim_key("<C-s>", cx);
    });
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert_eq!(this.app.bug_report.stage, BugStage::Form);
    });

    // Once summary is present, Review Draft advances to Consent and Back returns to Form.
    view.update(window, |this, cx| {
        this.app.bug_report.summary = "Tab closes unexpectedly".into();
        cx.notify();
    });
    window.run_until_parked();
    click_debug_target(window, "bug-send");
    view.read_with(window, |this, _| {
        assert_eq!(this.app.bug_report.stage, BugStage::Consent);
    });

    click_debug_target(window, "bug-back");
    view.read_with(window, |this, _| {
        assert_eq!(this.app.bug_report.stage, BugStage::Form);
        assert_eq!(this.app.bug_report.summary, "Tab closes unexpectedly");
    });
    assert_eq!(window.opened_url(), None);
}

#[gpui::test]
fn bug_report_gui_open_draft_uses_gpui_open_url_redacts_and_supports_copy_fallbacks(
    cx: &mut TestAppContext,
) {
    use crate::tui::widgets::bug_report::{BugStage, DEFAULT_BUG_REPORT_REPO, MAX_DRAFT_URL_BYTES};

    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.bug_report.is_open = true;
        view.app.bug_report.stage = BugStage::Consent;
        view.app.bug_report.home_dir = "/home/alice".into();
        view.app.bug_report.report_repo = DEFAULT_BUG_REPORT_REPO.into();
        view.app.bug_report.category_idx = 2; // Git
        view.app.bug_report.summary = "Crash in /home/alice/work ghp_secret123".into();
        view.app.bug_report.description = "Repro with token=mysecret&foo=1".into();
        view
    });
    window.simulate_resize(size(px(900.0), px(620.0)));
    window.run_until_parked();

    click_debug_target(window, "bug-confirm");

    let opened = window.opened_url().expect("GPUI open_url should be invoked");
    assert!(opened.starts_with("https://github.com/Tuim-editor/Gtuim/issues/new?title="));
    assert!(opened.len() <= MAX_DRAFT_URL_BYTES);
    assert!(!opened.contains("ghp_secret123"));
    assert!(!opened.contains("mysecret"));
    assert!(!opened.contains("alice"));

    view.read_with(window, |this, _| {
        assert_eq!(this.app.bug_report.stage, BugStage::Success);
        assert!(!this.app.bug_report.needs_clipboard);
        assert_eq!(this.app.bug_report.issue_url, opened);
    });
    assert!(window.debug_bounds("bug-clipboard-callout").is_none());
    assert!(window.debug_bounds("bug-draft-url").is_some());

    // Copy full redacted Markdown fallback works from Success.
    click_debug_target(window, "bug-copy");
    let copied = window
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("full report should be copied to GPUI clipboard");
    assert!(copied.contains("## Summary"));
    assert!(copied.contains("## Category\n\nGit"));
    assert!(copied.contains("~/work"));
    assert!(copied.contains("ghp_[REDACTED]"));
    assert!(copied.contains("token=[REDACTED]"));
    assert!(!copied.contains("ghp_secret123"));
    assert!(!copied.contains("mysecret"));
    assert!(!copied.contains("/home/alice"));

    // Copy Draft URL copies the exact issue composer URL.
    click_debug_target(window, "bug-copy-url");
    let copied_url = window
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("draft URL should be copied");
    assert_eq!(copied_url, opened);
}

#[gpui::test]
fn bug_report_gui_oversized_report_copies_full_markdown_before_opening_bounded_url(
    cx: &mut TestAppContext,
) {
    use crate::tui::widgets::bug_report::{BugStage, DEFAULT_BUG_REPORT_REPO, MAX_DRAFT_URL_BYTES};

    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.bug_report.is_open = true;
        view.app.bug_report.stage = BugStage::Consent;
        view.app.bug_report.home_dir = "/home/bob".into();
        view.app.bug_report.report_repo = DEFAULT_BUG_REPORT_REPO.into();
        view.app.bug_report.summary = "Oversized crash report".into();
        view.app.bug_report.description = format!(
            "{} /home/bob/secret api_key=topsecret UNIQUE_TAIL_MARKER_98765",
            "long-step-line ".repeat(700)
        );
        view
    });
    window.simulate_resize(size(px(900.0), px(620.0)));
    window.run_until_parked();

    assert!(
        window.debug_bounds("bug-consent-clipboard-notice").is_some(),
        "Consent screen should warn when report exceeds URL budget"
    );

    click_debug_target(window, "bug-confirm");

    let opened = window.opened_url().expect("GPUI open_url should open bounded draft URL");
    assert!(opened.len() <= MAX_DRAFT_URL_BYTES);
    assert!(opened.starts_with("https://github.com/Tuim-editor/Gtuim/issues/new?body="));
    assert!(opened.contains("Paste%20the%20complete%20report"));

    let copied = window
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("Oversized report must copy complete redacted Markdown to GPUI clipboard");
    assert!(copied.contains("Oversized crash report"));
    assert!(copied.contains("UNIQUE_TAIL_MARKER_98765"));
    assert!(copied.contains("~/secret"));
    assert!(copied.contains("api_key=[REDACTED]"));
    assert!(!copied.contains("topsecret"));
    assert!(!copied.contains("/home/bob"));

    view.read_with(window, |this, _| {
        assert_eq!(this.app.bug_report.stage, BugStage::Success);
        assert!(this.app.bug_report.needs_clipboard);
        assert_eq!(this.app.bug_report.issue_url, opened);
    });
    assert!(
        window.debug_bounds("bug-clipboard-callout").is_some(),
        "Success screen should show copy/paste instructions for oversized reports"
    );
}

#[gpui::test]
fn bug_report_gui_keyboard_consent_uses_gpui_open_url_and_clipboard_consistently(
    cx: &mut TestAppContext,
) {
    use crate::tui::widgets::bug_report::{BugStage, MAX_DRAFT_URL_BYTES};

    let (view, window) = cx.add_window_view(|win, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.bug_report.is_open = true;
        view.app.bug_report.stage = BugStage::Consent;
        view.app.bug_report.report_repo = "custom-org/gtuim-tracker".into();
        view.app.bug_report.summary = "Keyboard confirm check".into();
        view.app.bug_report.description = format!("{} KEYBOARD_TAIL", "x".repeat(9000));
        win.focus(&view.root_focus);
        view
    });
    window.simulate_resize(size(px(900.0), px(620.0)));
    window.run_until_parked();

    // Pressing 'n' goes back to Form, matching the Back button.
    window.simulate_keystrokes("n");
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert_eq!(this.app.bug_report.stage, BugStage::Form);
    });
    assert_eq!(window.opened_url(), None);

    // Return to Consent and confirm with Enter via real KeyDownEvent.
    view.update(window, |this, cx| {
        this.app.bug_report.stage = BugStage::Consent;
        cx.notify();
    });
    window.run_until_parked();
    window.simulate_keystrokes("enter");
    window.run_until_parked();

    let opened = window
        .opened_url()
        .expect("Keyboard Enter on Consent must use GPUI open_url");
    assert!(opened.starts_with("https://github.com/custom-org/gtuim-tracker/issues/new?"));
    assert!(opened.len() <= MAX_DRAFT_URL_BYTES);
    let copied = window
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("Keyboard Enter on oversized Consent must copy full Markdown to GPUI clipboard");
    assert!(copied.contains("KEYBOARD_TAIL"));
    view.read_with(window, |this, _| {
        assert_eq!(this.app.bug_report.stage, BugStage::Success);
        assert!(this.app.bug_report.needs_clipboard);
    });

    // Also verify send_tuim_key("<CR>") on Consent uses the same native GPUI path.
    view.update(window, |this, cx| {
        this.app.bug_report.stage = BugStage::Consent;
        this.app.bug_report.description = "Short keyboard body".into();
        this.send_tuim_key("<CR>", cx);
    });
    window.run_until_parked();
    let opened2 = window.opened_url().expect("send_tuim_key <CR> should open draft");
    assert!(opened2.contains("Short%20keyboard%20body"));
    view.read_with(window, |this, _| {
        assert_eq!(this.app.bug_report.stage, BugStage::Success);
        assert!(!this.app.bug_report.needs_clipboard);
    });
}

#[gpui::test]
fn bug_report_gui_invalid_repo_fails_safely_preserves_form_and_allows_copy_and_retry(
    cx: &mut TestAppContext,
) {
    use crate::tui::widgets::bug_report::BugStage;

    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.bug_report.is_open = true;
        view.app.bug_report.stage = BugStage::Consent;
        view.app.bug_report.report_repo = "https://github.com/bad/url".into();
        view.app.bug_report.summary = "Keep this summary".into();
        view.app.bug_report.description = "Keep this description".into();
        view
    });
    window.simulate_resize(size(px(900.0), px(620.0)));
    window.run_until_parked();

    click_debug_target(window, "bug-confirm");

    assert_eq!(window.opened_url(), None);
    view.read_with(window, |this, _| {
        assert_eq!(this.app.bug_report.stage, BugStage::Failure);
        assert!(this.app.bug_report.error_message.contains("GTUIM_BUG_REPORT_REPO"));
        assert_eq!(this.app.bug_report.summary, "Keep this summary");
        assert_eq!(this.app.bug_report.description, "Keep this description");
    });

    // Copy Report still works on Failure so the user never loses their report.
    click_debug_target(window, "bug-copy");
    let copied = window
        .read_from_clipboard()
        .and_then(|item| item.text())
        .expect("Copy Report on Failure should copy full Markdown");
    assert!(copied.contains("Keep this summary"));
    assert!(copied.contains("Keep this description"));

    // Edit & Retry returns to Form with inputs intact.
    click_debug_target(window, "bug-retry");
    view.read_with(window, |this, _| {
        assert_eq!(this.app.bug_report.stage, BugStage::Form);
        assert_eq!(this.app.bug_report.summary, "Keep this summary");
        assert_eq!(this.app.bug_report.description, "Keep this description");
    });
}

#[gpui::test]
fn bug_report_gui_success_recovery_shortcuts_use_native_actions(cx: &mut TestAppContext) {
    use crate::tui::widgets::bug_report::BugStage;
    let url = "https://github.com/Tuim-editor/Gtuim/issues/new?body=keyboard-recovery";
    let (view, window) = cx.add_window_view(|win, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.bug_report.is_open = true;
        view.app.bug_report.stage = BugStage::Success;
        view.app.bug_report.issue_url = url.into();
        view.app.bug_report.home_dir = "/home/alice".into();
        view.app.bug_report.summary = "Recover /home/alice/work ghp_secret".into();
        view.app.bug_report.description = "Keep complete report tail".into();
        win.focus(&view.root_focus);
        view
    });
    window.simulate_resize(size(px(900.0), px(620.0)));
    window.run_until_parked();

    window.simulate_keystrokes("shift-o");
    window.run_until_parked();
    assert_eq!(window.opened_url().as_deref(), Some(url));

    window.simulate_keystrokes("shift-u");
    window.run_until_parked();
    assert_eq!(window.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some(url));

    window.simulate_keystrokes("shift-c");
    window.run_until_parked();
    let copied = window.read_from_clipboard().and_then(|item| item.text()).unwrap();
    assert!(copied.contains("Recover ~/work ghp_[REDACTED]"));
    assert!(copied.contains("Keep complete report tail"));
    assert!(copied.contains("## Diagnostics"));
    assert!(!copied.contains("ghp_secret"));
    view.read_with(window, |this, _| {
        assert_eq!(this.app.bug_report.stage, BugStage::Success);
        assert!(this.app.bug_report.is_open);
    });

    window.simulate_keystrokes("shift-r");
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert_eq!(this.app.bug_report.stage, BugStage::Form);
        assert_eq!(this.app.bug_report.description, "Keep complete report tail");
    });
}

#[gpui::test]
fn bug_report_gui_failure_shortcuts_copy_retry_and_ignore_stale_url(cx: &mut TestAppContext) {
    use crate::tui::widgets::bug_report::BugStage;
    let (view, window) = cx.add_window_view(|win, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.bug_report.is_open = true;
        view.app.bug_report.stage = BugStage::Failure;
        // A previous successful draft must not be used after a new preparation failure.
        view.app.bug_report.issue_url = "https://github.com/old/repo/issues/new".into();
        view.app.bug_report.summary = "Retained failure summary".into();
        view.app.bug_report.description = "Retained failure description".into();
        win.focus(&view.root_focus);
        view
    });
    window.simulate_resize(size(px(900.0), px(620.0)));
    window.run_until_parked();
    window.write_to_clipboard(gpui::ClipboardItem::new_string("unchanged".into()));

    window.simulate_keystrokes("o u shift-o shift-u");
    window.run_until_parked();
    assert_eq!(window.opened_url(), None);
    assert_eq!(window.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("unchanged"));

    window.simulate_keystrokes("c");
    window.run_until_parked();
    let copied = window.read_from_clipboard().and_then(|item| item.text()).unwrap();
    assert!(copied.contains("Retained failure summary"));
    assert!(copied.contains("Retained failure description"));
    assert!(copied.contains("## Diagnostics"));
    view.read_with(window, |this, _| assert_eq!(this.app.bug_report.stage, BugStage::Failure));

    window.simulate_keystrokes("r");
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert_eq!(this.app.bug_report.stage, BugStage::Form);
        assert_eq!(this.app.bug_report.summary, "Retained failure summary");
        assert!(this.app.bug_report.is_open);
    });
}

#[gpui::test]
fn bug_report_gui_result_shortcuts_preserve_close_and_ignore_modifiers(cx: &mut TestAppContext) {
    use crate::tui::widgets::bug_report::BugStage;
    let (view, window) = cx.add_window_view(|win, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.bug_report.is_open = true;
        view.app.bug_report.issue_url = "https://github.com/Tuim-editor/Gtuim/issues/new".into();
        win.focus(&view.root_focus);
        view
    });
    window.simulate_resize(size(px(900.0), px(620.0)));
    for stage in [BugStage::Success, BugStage::Failure] {
        for close_key in ["escape", "enter"] {
            view.update(window, |this, cx| {
                this.app.bug_report.is_open = true;
                this.app.bug_report.stage = stage;
                cx.notify();
            });
            window.run_until_parked();
            window.write_to_clipboard(gpui::ClipboardItem::new_string("sentinel".into()));
            window.simulate_keystrokes("ctrl-c ctrl-r ctrl-o ctrl-u");
            window.run_until_parked();
            assert_eq!(window.opened_url(), None);
            assert_eq!(window.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("sentinel"));
            view.read_with(window, |this, _| {
                assert_eq!(this.app.bug_report.stage, stage);
                assert!(this.app.bug_report.is_open);
            });
            window.simulate_keystrokes(close_key);
            window.run_until_parked();
            view.read_with(window, |this, _| assert!(!this.app.bug_report.is_open));
        }
    }
}
