//! Run the actual production bootstrap in child processes with isolated HOME/XDG
//! roots. No global environment mutation races and no real user runtime writes.
use super::*;
use crate::tui::widgets::onboarding::{OnboardingStep, COMPLETION_MARKER};
use std::path::PathBuf;

#[test]
fn isolated_first_run_production_startup_and_native_modal() {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    for scenario in [
        "clean",
        "inherited",
        "partial-marker",
        "skip-gtuim",
        "skip-tuim",
    ] {
        let root = std::env::temp_dir().join(format!(
            "gtuim-first-run-{}-{}-{scenario}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "gui::modal_tests::first_run_tests::production_startup_child",
            "--nocapture",
        ])
        .current_dir(&root)
        .env("GTUIM_FIRST_RUN_TEST_ROOT", &root)
        .env("GTUIM_FIRST_RUN_TEST_SCENARIO", scenario)
        .env("HOME", &root)
        .env("TUIM_DISABLE_PLUGINS", "1")
        .env_remove("GTUIM_SKIP_ONBOARDING")
        .env_remove("TUIM_SKIP_ONBOARDING");
        for kind in ["DATA", "CONFIG", "STATE", "CACHE"] {
            cmd.env(format!("XDG_{kind}_HOME"), root.join(kind.to_lowercase()));
        }
        match scenario {
            "skip-gtuim" => {
                cmd.env("GTUIM_SKIP_ONBOARDING", "1");
            }
            "skip-tuim" => {
                cmd.env("TUIM_SKIP_ONBOARDING", "1");
            }
            _ => {}
        }
        let output = cmd
            .output()
            .expect("run isolated first-launch test process");
        let _ = std::fs::remove_dir_all(&root);
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{scenario}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[gpui::test]
fn production_startup_child(cx: &mut TestAppContext) {
    let Some(root) = std::env::var_os("GTUIM_FIRST_RUN_TEST_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    assert!(root.starts_with(std::env::temp_dir()));
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("gtuim-first-run-"));
    assert_eq!(std::env::var_os("HOME").unwrap(), root.as_os_str());
    for kind in ["DATA", "CONFIG", "STATE", "CACHE"] {
        assert_eq!(
            PathBuf::from(std::env::var_os(format!("XDG_{kind}_HOME")).unwrap()),
            root.join(kind.to_lowercase())
        );
    }
    let scenario = std::env::var("GTUIM_FIRST_RUN_TEST_SCENARIO").unwrap();
    let data = root.join("data/tuim");
    std::fs::create_dir_all(&data).unwrap();
    if scenario == "inherited" {
        std::fs::write(data.join("onboarding-complete"), "completed by Rtuim\n").unwrap();
        let mut config = crate::tui::widgets::settings::SettingsConfig::default();
        config.onboarding_completed = true;
        config.theme = "habamax".into(); // Bundled Neovim theme: no plugin/network needed.
        config.numbers = "absolute".into();
        config.font_size = 17;
        config.keybindings = crate::tui::widgets::settings::Keybindings::familiar_preset();
        config
            .save(data.join("settings.json").to_str().unwrap())
            .unwrap();
    }
    if scenario == "partial-marker" {
        std::fs::write(data.join(COMPLETION_MARKER), "").unwrap();
    }
    let before_marker = std::fs::read(data.join(COMPLETION_MARKER)).ok();
    // This is the real startup path, not a manual call to the decision method.
    let (app, profiles) = super::super::boot_tuim_app(&[], 100, 30, None).unwrap();
    assert_eq!(PathBuf::from(&app.data_dir), data);
    let skipped = scenario.starts_with("skip-");
    assert_eq!(app.onboarding.is_open, !skipped);
    assert_eq!(
        std::fs::read(data.join(COMPLETION_MARKER)).ok(),
        before_marker
    );
    if scenario == "inherited" {
        assert_eq!(app.onboarding.draft.theme, "habamax");
        assert_eq!(app.onboarding.draft.numbers, "absolute");
        assert_eq!(app.onboarding.draft.font_size, 17);
        assert_eq!(app.onboarding.draft.keybindings.toggle_explorer, "<C-b>");
        assert!(app.onboarding.initial.onboarding_completed);
    }
    let (view, window) = cx.add_window_view(move |_, cx| {
        let mut view = fixture(cx);
        view.app = app;
        view.profiles = profiles;
        view
    });
    let window = &mut *window;
    window.simulate_resize(size(px(1000.), px(760.)));
    window.run_until_parked();
    if skipped {
        // Environment bypass is temporary and does not block manual setup.
        view.update(window, |this, cx| {
            crate::tui::events::execute_command(&mut this.app, "onboarding");
            cx.notify();
        });
        window.run_until_parked();
        assert!(window.debug_bounds("onboarding-header").is_some());
        view.update(window, |this, cx| this.cancel_onboarding(cx));
        assert_eq!(
            std::fs::read(data.join(COMPLETION_MARKER)).ok(),
            before_marker
        );
        return;
    }
    assert!(
        window.debug_bounds("onboarding-header").is_some(),
        "production first launch must render native setup"
    );
    assert!(window.debug_bounds("onboarding-primary-btn").is_some());
    // Skip-for-now/Escape must not write settings or a completion marker.
    let settings_path = view.read_with(window, |this, _| {
        this.app.settings_widget.settings_path.clone()
    });
    let before_skip = std::fs::read(&settings_path).ok();
    view.update(window, |this, cx| {
        this.app.onboarding.draft.numbers = "none".into();
        this.cancel_onboarding(cx);
    });
    assert_eq!(std::fs::read(&settings_path).ok(), before_skip);
    assert_eq!(
        std::fs::read(data.join(COMPLETION_MARKER)).ok(),
        before_marker
    );
    // Relaunch after skipping: the production decision must still open setup.
    let (reopened, reopened_profiles) = super::super::boot_tuim_app(&[], 100, 30, None).unwrap();
    assert!(reopened.onboarding.is_open);
    view.update(window, |this, cx| {
        this.app = reopened;
        this.profiles = reopened_profiles;
        this.app.onboarding.step = OnboardingStep::Setup;
        cx.notify();
    });
    window.run_until_parked();
    // A failed settings save must leave setup retryable and must not record
    // completion. This path is deliberately a directory inside our temp root.
    view.update(window, |this, cx| {
        let actual_path = this.app.settings_widget.settings_path.clone();
        let actual_config = this.app.settings_widget.config.clone();
        this.app.settings_widget.settings_path = root.to_string_lossy().into();
        this.app.finish_onboarding();
        assert!(this.app.onboarding.is_open);
        assert_eq!(this.app.settings_widget.config, actual_config);
        assert_eq!(
            std::fs::read(data.join(COMPLETION_MARKER)).ok(),
            before_marker
        );
        this.app.settings_widget.settings_path = actual_path;
        this.app.notice = None;
        cx.notify();
    });
    window.run_until_parked();
    let finish = window.debug_bounds("onboarding-primary-btn").unwrap();
    window.simulate_mouse_down(
        finish.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    window.simulate_mouse_up(
        finish.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    window.run_until_parked();
    view.read_with(window, |this, _| {
        assert!(!this.app.onboarding.is_open,
        "finish click did not close setup: step={:?}, status={}, notice={:?}, bounds={finish:?}",
        this.app.onboarding.step, this.app.settings_widget.status_msg, this.app.notice)
    });
    assert_eq!(
        std::fs::read_to_string(data.join(COMPLETION_MARKER)).unwrap(),
        "completed\n"
    );
    let saved = crate::tui::widgets::settings::SettingsConfig::load(&settings_path).unwrap();
    assert!(saved.onboarding_completed);
    if scenario == "inherited" {
        assert_eq!(
            saved.numbers, "absolute",
            "skip must discard draft changes, not overwrite inherited choices"
        );
        assert_eq!(saved.theme, "habamax");
        assert_eq!(
            std::fs::read_to_string(data.join("onboarding-complete")).unwrap(),
            "completed by Rtuim\n"
        );
    }
    let (next, next_profiles) = super::super::boot_tuim_app(&[], 100, 30, None).unwrap();
    assert!(
        !next.onboarding.is_open,
        "completed setup must stay closed across relaunch"
    );
    view.update(window, |this, cx| {
        this.app = next;
        this.profiles = next_profiles;
        // A fresh profile has onboarding_completed=false, but app completion is
        // shared across profiles and must not reset.
        let original = this.profiles.active().id.clone();
        let new_profile = this.profiles.create().unwrap();
        this.switch_profile(&new_profile, cx).unwrap();
        assert!(!this.app.settings_widget.config.onboarding_completed);
        this.app.maybe_open_first_run_onboarding();
        assert!(!this.app.onboarding.is_open);
        this.switch_profile(&original, cx).unwrap();
        this.app.maybe_open_first_run_onboarding();
        assert!(!this.app.onboarding.is_open);
        // Help -> Welcome & Setup / command palette route remains available.
        crate::tui::events::execute_command(&mut this.app, "onboarding");
        assert!(this.app.onboarding.is_open);
        assert!(!this.app.onboarding.is_first_run);
        cx.notify();
    });
    window.run_until_parked();
    assert!(window.debug_bounds("onboarding-header").is_some());
    view.update(window, |this, cx| {
        this.cancel_onboarding(cx);
        this.app.maybe_open_first_run_onboarding();
        assert!(!this.app.onboarding.is_open);
    });
    assert!(data.join(COMPLETION_MARKER).is_file());
}
