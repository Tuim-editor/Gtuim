use crate::gui::modal_tests::fixture;
use crate::tui::widgets::explorer::{FileExplorer, InputMode};
use gpui::{px, size, Modifiers, MouseButton, TestAppContext};

#[gpui::test]
fn toolbar_creation_respects_explicit_folder_selection(cx: &mut TestAppContext) {
    let root =
        std::env::temp_dir().join(format!("gtuim-selected-create-gui-{}", std::process::id()));
    let folder = root.join("selected-folder");
    std::fs::create_dir_all(&folder).unwrap();
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.workspace.overview = false;
        view.app.workspace.sidebar_visible = true;
        view.app.activity_bar.active_tab = 0;
        view.app.explorer = FileExplorer::new_at(root.to_string_lossy().into());
        view
    });
    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();
    let row = window.debug_bounds("exp-row-0").unwrap();
    window.simulate_mouse_down(row.center(), MouseButton::Left, Modifiers::default());
    window.simulate_mouse_up(row.center(), MouseButton::Left, Modifiers::default());
    window.run_until_parked();
    let new_file = window.debug_bounds("exp-new-file").unwrap();
    window.simulate_mouse_down(new_file.center(), MouseButton::Left, Modifiers::default());
    window.simulate_mouse_up(new_file.center(), MouseButton::Left, Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert_eq!(this.app.explorer.input_parent.as_deref(), folder.to_str());
    });
    view.update(window, |this, cx| {
        this.app.explorer.input_buffer = "inside.txt".into();
        this.app
            .explorer
            .handle_key(&crate::tui::input::InputEvent::Key("<CR>".into()));
        cx.notify();
    });
    window.run_until_parked();
    assert!(folder.join("inside.txt").exists());
    assert!(!root.join("inside.txt").exists());
    let tree = window.debug_bounds("explorer-tree").unwrap();
    let blank = gpui::point(tree.left() + px(20.0), tree.bottom() - px(20.0));
    window.simulate_mouse_down(blank, MouseButton::Left, Modifiers::default());
    window.simulate_mouse_up(blank, MouseButton::Left, Modifiers::default());
    window.run_until_parked();
    let new_dir = window.debug_bounds("exp-new-dir").unwrap();
    window.simulate_mouse_down(new_dir.center(), MouseButton::Left, Modifiers::default());
    window.simulate_mouse_up(new_dir.center(), MouseButton::Left, Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert_eq!(this.app.explorer.input_parent.as_deref(), root.to_str())
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[gpui::test]
fn deletion_has_working_cancel_and_explicit_permanent_delete_buttons(cx: &mut TestAppContext) {
    let root =
        std::env::temp_dir().join(format!("gtuim-explorer-delete-gui-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("notes.txt");
    std::fs::write(&file, "keep until confirmed").unwrap();
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.workspace.overview = false;
        view.app.workspace.sidebar_visible = true;
        view.app.activity_bar.active_tab = 0;
        view.app.explorer = FileExplorer::new_at(root.to_string_lossy().into());
        view.app.explorer.begin_delete();
        view
    });
    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();
    let cancel = window
        .debug_bounds("exp-delete-cancel")
        .expect("cancel button");
    window.simulate_mouse_down(cancel.center(), MouseButton::Left, Modifiers::default());
    window.simulate_mouse_up(cancel.center(), MouseButton::Left, Modifiers::default());
    window.run_until_parked();
    assert!(file.exists());
    view.read_with(window, |this, _| {
        assert_eq!(this.app.explorer.input_mode, InputMode::None)
    });
    window.update(|_, cx| {
        view.update(cx, |this, cx| {
            this.app.explorer.begin_delete();
            cx.notify();
        })
    });
    window.run_until_parked();
    let confirm = window
        .debug_bounds("exp-delete-confirm")
        .expect("explicit delete button");
    window.simulate_mouse_down(confirm.center(), MouseButton::Left, Modifiers::default());
    window.simulate_mouse_up(confirm.center(), MouseButton::Left, Modifiers::default());
    window.run_until_parked();
    assert!(!file.exists());
    view.read_with(window, |this, _| {
        assert_eq!(this.app.explorer.input_mode, InputMode::None)
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[gpui::test]
fn right_click_folder_and_whitespace_create_in_expected_directory(cx: &mut TestAppContext) {
    let root = std::env::temp_dir().join(format!("gtuim-explorer-gui-{}", std::process::id()));
    let folder = root.join("first");
    std::fs::create_dir_all(&folder).unwrap();
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.workspace.overview = false;
        view.app.workspace.sidebar_visible = true;
        view.app.activity_bar.active_tab = 0;
        view.app.explorer = FileExplorer::new_at(root.to_string_lossy().into());
        view
    });
    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();
    let row = window.debug_bounds("exp-row-0").expect("folder row");
    window.simulate_mouse_down(row.center(), MouseButton::Right, Modifiers::default());
    window.simulate_mouse_up(row.center(), MouseButton::Right, Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert!(this.app.explorer.context_menu_open);
        assert_eq!(this.app.explorer.context_target.as_deref(), folder.to_str());
    });
    let new_file = window
        .debug_bounds("exp-ctx-item-0")
        .expect("new file action");
    window.simulate_mouse_down(new_file.center(), MouseButton::Left, Modifiers::default());
    window.simulate_mouse_up(new_file.center(), MouseButton::Left, Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert_eq!(this.app.explorer.input_mode, InputMode::NewFile);
        assert_eq!(this.app.explorer.input_parent.as_deref(), folder.to_str());
    });
    let tree = window.debug_bounds("explorer-tree").unwrap();
    let blank = gpui::point(tree.left() + px(20.0), tree.bottom() - px(20.0));
    window.simulate_mouse_down(blank, MouseButton::Right, Modifiers::default());
    window.simulate_mouse_up(blank, MouseButton::Right, Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert!(this.app.explorer.context_menu_open);
        assert!(this.app.explorer.context_target.is_none());
    });
    let new_folder = window
        .debug_bounds("exp-ctx-item-1")
        .expect("new folder action");
    window.simulate_mouse_down(new_folder.center(), MouseButton::Left, Modifiers::default());
    window.simulate_mouse_up(new_folder.center(), MouseButton::Left, Modifiers::default());
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert_eq!(this.app.explorer.input_mode, InputMode::NewFolder);
        assert_eq!(this.app.explorer.input_parent.as_deref(), root.to_str());
    });
    std::fs::remove_dir_all(root).unwrap();
}
