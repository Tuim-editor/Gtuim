use crate::gui::modal_tests::fixture;
use gpui::{px, size, TestAppContext};

#[gpui::test]
fn git_no_repository_exposes_initialize_action(cx: &mut TestAppContext) {
    let (_, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.workspace.sidebar_visible = true;
        view.app.activity_bar.active_tab = 1;
        view.app.git_panel.snapshot.is_repository = false;
        view
    });
    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();
    assert!(window.debug_bounds("git-init").is_some());
    assert!(window.debug_bounds("git-switch-branch").is_none());
}

#[gpui::test]
fn git_branch_switch_action_opens_branches_modal(cx: &mut TestAppContext) {
    let (view, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.workspace.sidebar_visible = true;
        view.app.activity_bar.active_tab = 1;
        view.app.git_panel.snapshot.is_repository = true;
        view.app.git_panel.snapshot.branch = "main".into();
        view.app.git_panel.snapshot.branches = vec!["main".into(), "feature".into()];
        view
    });
    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();
    let bounds = window
        .debug_bounds("git-switch-branch")
        .expect("switch branch action");
    window.simulate_mouse_down(
        bounds.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    window.simulate_mouse_up(
        bounds.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert!(this.app.git_detailed_widget.is_open);
        assert_eq!(this.app.git_detailed_widget.active_tab, 1);
    });
    assert!(window.debug_bounds("git-det-branch-1").is_some());
}

#[gpui::test]
fn git_diff_modal_renders_unified_diff(cx: &mut TestAppContext) {
    let (_, window) = cx.add_window_view(|_, cx| {
        let mut view = fixture(cx);
        view.app.extension_shop.is_open = false;
        view.app.git_detailed_widget.is_open = true;
        view.app.git_detailed_widget.active_tab = 2;
        view.app.git_detailed_widget.diff_preview = Some(crate::git_snapshot::GitDiffPreview {
            title: "Working tree diff · file.txt".into(),
            text: "--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-old\n+new".into(),
        });
        view
    });
    window.simulate_resize(size(px(1000.0), px(700.0)));
    window.run_until_parked();
    assert!(window.debug_bounds("git-det-list").is_some());
    assert!(window.debug_bounds("git-diff-line-4").is_some());
}
