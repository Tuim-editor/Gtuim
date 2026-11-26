use std::fs;
use std::path::Path;

use crate::tui::input::InputEvent;
use crate::tui::widgets::settings::{Keybindings, SettingsConfig};

/// App-wide, not profile-specific: Rtuim shares settings and the data directory
/// but must not count as having completed Gtuim's native welcome/setup flow.
pub const COMPLETION_MARKER: &str = "gtuim-onboarding-complete";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingStep {
    Theme = 0,
    Controls = 1,
    Guide = 2,
    Setup = 3,
}

impl OnboardingStep {
    pub const ALL: [OnboardingStep; 4] = [
        OnboardingStep::Theme,
        OnboardingStep::Controls,
        OnboardingStep::Guide,
        OnboardingStep::Setup,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(idx: usize) -> Self {
        match idx {
            0 => Self::Theme,
            1 => Self::Controls,
            2 => Self::Guide,
            _ => Self::Setup,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Theme => "Theme & Look",
            Self::Controls => "Controls",
            Self::Guide => "How to Use",
            Self::Setup => "Editor & Tools",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Theme => "palette",
            Self::Controls => "keyboard",
            Self::Guide => "book-open",
            Self::Setup => "sliders",
        }
    }

    pub fn next(self) -> Option<Self> {
        match self {
            Self::Theme => Some(Self::Controls),
            Self::Controls => Some(Self::Guide),
            Self::Guide => Some(Self::Setup),
            Self::Setup => None,
        }
    }

    pub fn prev(self) -> Option<Self> {
        match self {
            Self::Theme => None,
            Self::Controls => Some(Self::Theme),
            Self::Guide => Some(Self::Controls),
            Self::Setup => Some(Self::Guide),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlPreset {
    Default,
    Familiar,
    VimSafe,
    Custom,
}

impl ControlPreset {
    pub const SELECTABLE: [ControlPreset; 3] = [
        ControlPreset::Default,
        ControlPreset::Familiar,
        ControlPreset::VimSafe,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Familiar => "Familiar (VS Code / Zed)",
            Self::VimSafe => "Vim-Safe",
            Self::Custom => "Custom",
        }
    }

    pub fn badge(self) -> &'static str {
        match self {
            Self::Default => "Recommended",
            Self::Familiar => "GUI Classic",
            Self::VimSafe => "Pure Vim Ctrl",
            Self::Custom => "Customized",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Self::Default => "Balanced Gtuim shortcuts: Ctrl+E Explorer, Ctrl+T Terminal, Ctrl+P Find File, Ctrl+S Save, F1 Commands.",
            Self::Familiar => "Familiar desktop IDE controls: Ctrl+B toggles sidebar, Ctrl+P Quick Open, Ctrl+T Terminal, Ctrl+S Save.",
            Self::VimSafe => "Moves workspace shortcuts to Alt (Alt+E, Alt+T, Alt+P, Alt+N, F2 Save) so Ctrl stays free for Vim.",
            Self::Custom => "Preserves your custom keybindings from Settings.",
        }
    }

    pub fn detect(kb: &Keybindings) -> Self {
        if *kb == Keybindings::default() {
            Self::Default
        } else if *kb == Keybindings::familiar_preset() {
            Self::Familiar
        } else if *kb == Keybindings::vim_safe_preset() {
            Self::VimSafe
        } else {
            Self::Custom
        }
    }

    pub fn to_keybindings(self) -> Option<Keybindings> {
        match self {
            Self::Default => Some(Keybindings::default()),
            Self::Familiar => Some(Keybindings::familiar_preset()),
            Self::VimSafe => Some(Keybindings::vim_safe_preset()),
            Self::Custom => None,
        }
    }
}

/// Visual preview swatch palette for each built-in theme so the setup modal
/// can render Zed-style mini editor cards before or while switching themes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeSwatch {
    pub bg_editor: u32,
    pub bg_sidebar: u32,
    pub accent: u32,
    pub keyword: u32,
    pub string: u32,
    pub text: u32,
    pub subtitle: &'static str,
}

pub fn theme_swatch(theme_id: &str) -> ThemeSwatch {
    match theme_id {
        "system" => ThemeSwatch {
            bg_editor: 0x1d2026,
            bg_sidebar: 0x16181d,
            accent: 0x4ea8de,
            keyword: 0xa78bfa,
            string: 0x7dd3fc,
            text: 0xe5e7eb,
            subtitle: "Follows desktop palette",
        },
        "vscode" => ThemeSwatch {
            bg_editor: 0x1f1f1f,
            bg_sidebar: 0x181818,
            accent: 0x0078d4,
            keyword: 0x569cd6,
            string: 0xce9178,
            text: 0xcccccc,
            subtitle: "Default · Clean neutral dark",
        },
        "matteblack" => ThemeSwatch {
            bg_editor: 0x121212,
            bg_sidebar: 0x0c0c0c,
            accent: 0xe68e0d,
            keyword: 0xd35f5f,
            string: 0xb9b9b9,
            text: 0xbebebe,
            subtitle: "OLED matte charcoal & amber",
        },
        "tokyonight" => ThemeSwatch {
            bg_editor: 0x1a1b26,
            bg_sidebar: 0x16161e,
            accent: 0x7aa2f7,
            keyword: 0xbb9af7,
            string: 0x9ece6a,
            text: 0xc0caf5,
            subtitle: "Deep indigo night & neon blue",
        },
        "tokyonight-storm" => ThemeSwatch {
            bg_editor: 0x24283b,
            bg_sidebar: 0x1f2335,
            accent: 0x7aa2f7,
            keyword: 0xbb9af7,
            string: 0x9ece6a,
            text: 0xc0caf5,
            subtitle: "Softer storm-blue contrast",
        },
        "catppuccin" => ThemeSwatch {
            bg_editor: 0x1e1e2e,
            bg_sidebar: 0x181825,
            accent: 0xcba6f7,
            keyword: 0xf38ba8,
            string: 0xa6e3a1,
            text: 0xcdd6f4,
            subtitle: "Warm pastel mocha tones",
        },
        "gruvbox" => ThemeSwatch {
            bg_editor: 0x282828,
            bg_sidebar: 0x1d2021,
            accent: 0xfe8019,
            keyword: 0xfb4934,
            string: 0xb8bb26,
            text: 0xebdbb2,
            subtitle: "Retro warm earthy contrast",
        },
        "nord" => ThemeSwatch {
            bg_editor: 0x2e3440,
            bg_sidebar: 0x272c36,
            accent: 0x88c0d0,
            keyword: 0x81a1c1,
            string: 0xa3be8c,
            text: 0xeceff4,
            subtitle: "Arctic slate & frost blue",
        },
        "cyberdream" => ThemeSwatch {
            bg_editor: 0x16181a,
            bg_sidebar: 0x111214,
            accent: 0x5ef1ff,
            keyword: 0xff5ef1,
            string: 0x5eff6c,
            text: 0xffffff,
            subtitle: "High-contrast cyber neon",
        },
        "rose-pine" => ThemeSwatch {
            bg_editor: 0x191724,
            bg_sidebar: 0x1f1d2e,
            accent: 0xebbcba,
            keyword: 0xc4a7e7,
            string: 0xf6c177,
            text: 0xe0def4,
            subtitle: "Soho dusk, pine & rose gold",
        },
        "kanagawa" => ThemeSwatch {
            bg_editor: 0x1f1f28,
            bg_sidebar: 0x16161d,
            accent: 0x7e9cd8,
            keyword: 0x957fb8,
            string: 0x98bb6c,
            text: 0xdcd7ba,
            subtitle: "Ink wash wave & parchment",
        },
        "nightfox" => ThemeSwatch {
            bg_editor: 0x192330,
            bg_sidebar: 0x131a24,
            accent: 0x719cd6,
            keyword: 0x9d79d6,
            string: 0x81b29a,
            text: 0xcdcecf,
            subtitle: "Midnight slate & soft teal",
        },
        _ => ThemeSwatch {
            bg_editor: 0x20232a,
            bg_sidebar: 0x181a20,
            accent: 0x61afef,
            keyword: 0xc678dd,
            string: 0x98c379,
            text: 0xdcdcdc,
            subtitle: "Installed Neovim colorscheme",
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingKeyAction {
    None,
    Cancel,
    Finish,
}

#[derive(Debug, Clone)]
pub struct OnboardingWidget {
    pub is_open: bool,
    /// Read-only native help; never applies the setup draft.
    pub help_only: bool,
    pub is_first_run: bool,
    pub step: OnboardingStep,
    pub draft: SettingsConfig,
    pub initial: SettingsConfig,
    pub preset: ControlPreset,
    pub status_msg: String,
    pub open_mason_after: bool,
    pub open_shop_after: bool,
    pub open_settings_after: bool,
}

impl Default for OnboardingWidget {
    fn default() -> Self {
        Self::new()
    }
}

impl OnboardingWidget {
    pub fn new() -> Self {
        let cfg = SettingsConfig::default();
        Self {
            is_open: false,
            help_only: false,
            is_first_run: false,
            step: OnboardingStep::Theme,
            preset: ControlPreset::detect(&cfg.keybindings),
            initial: cfg.clone(),
            draft: cfg,
            status_msg: String::new(),
            open_mason_after: false,
            open_shop_after: false,
            open_settings_after: false,
        }
    }

    pub fn open(&mut self, config: &SettingsConfig, is_first_run: bool) {
        self.is_open = true;
        self.help_only = false;
        self.is_first_run = is_first_run;
        self.step = OnboardingStep::Theme;
        self.initial = config.clone();
        self.draft = config.clone();
        self.preset = ControlPreset::detect(&config.keybindings);
        self.status_msg.clear();
        self.open_mason_after = false;
        self.open_shop_after = false;
        self.open_settings_after = false;
    }

    pub fn has_changes(&self) -> bool {
        self.draft != self.initial
    }

    pub fn select_preset(&mut self, preset: ControlPreset) {
        self.preset = preset;
        if let Some(kb) = preset.to_keybindings() {
            self.draft.keybindings = kb;
        }
    }

    pub fn select_mode(&mut self, mode: &str) {
        self.draft.mode = match mode {
            "normal" => "normal".to_string(),
            "zen" => "zen".to_string(),
            _ => "ide".to_string(),
        };
    }

    pub fn select_theme(&mut self, theme_id: impl Into<String>) {
        self.draft.theme = theme_id.into();
    }

    pub fn adjust_font_size(&mut self, delta: i8) {
        let next = (self.draft.font_size as i16 + delta as i16).clamp(9, 26) as u8;
        self.draft.font_size = next;
    }

    pub fn reset_to_defaults(&mut self) {
        let mut defaults = SettingsConfig::default();
        defaults.onboarding_completed = self.initial.onboarding_completed;
        self.draft = defaults;
        self.preset = ControlPreset::Default;
        self.status_msg = "Restored recommended defaults".to_string();
    }

    pub fn next_step(&mut self) -> bool {
        if let Some(next) = self.step.next() {
            self.step = next;
            true
        } else {
            false
        }
    }

    pub fn prev_step(&mut self) -> bool {
        if let Some(prev) = self.step.prev() {
            self.step = prev;
            true
        } else {
            false
        }
    }

    pub fn handle_key(&mut self, ev: &InputEvent) -> OnboardingKeyAction {
        if !self.is_open {
            return OnboardingKeyAction::None;
        }
        let InputEvent::Key(key) = ev else {
            return OnboardingKeyAction::None;
        };
        if self.help_only {
            return if matches!(key.as_str(), "<Esc>" | "q" | "<C-c>") {
                OnboardingKeyAction::Cancel
            } else {
                OnboardingKeyAction::None
            };
        }
        match key.as_str() {
            "<Esc>" => OnboardingKeyAction::Cancel,
            "<C-s>" => OnboardingKeyAction::Finish,
            "1" => {
                self.step = OnboardingStep::Theme;
                OnboardingKeyAction::None
            }
            "2" => {
                self.step = OnboardingStep::Controls;
                OnboardingKeyAction::None
            }
            "3" => {
                self.step = OnboardingStep::Guide;
                OnboardingKeyAction::None
            }
            "4" => {
                self.step = OnboardingStep::Setup;
                OnboardingKeyAction::None
            }
            "<Tab>" | "<Right>" => {
                let next = (self.step.index() + 1) % OnboardingStep::ALL.len();
                self.step = OnboardingStep::from_index(next);
                OnboardingKeyAction::None
            }
            "<S-Tab>" | "<Left>" => {
                let prev = (self.step.index() + OnboardingStep::ALL.len() - 1)
                    % OnboardingStep::ALL.len();
                self.step = OnboardingStep::from_index(prev);
                OnboardingKeyAction::None
            }
            "<CR>" => {
                if self.step == OnboardingStep::Setup {
                    OnboardingKeyAction::Finish
                } else {
                    self.next_step();
                    OnboardingKeyAction::None
                }
            }
            _ => OnboardingKeyAction::None,
        }
    }

    pub fn should_auto_open(data_dir: &str, _config: &SettingsConfig) -> bool {
        if ["GTUIM_SKIP_ONBOARDING", "TUIM_SKIP_ONBOARDING"].iter()
            .any(|key| std::env::var(key).ok().as_deref() == Some("1")) {
            return false;
        }
        // Neither the legacy onboarding-complete marker nor the shared/profile
        // config.onboarding_completed flag proves Gtuim setup was completed.
        // A failed/interrupted marker write must not suppress the next launch.
        !matches!(fs::read_to_string(Path::new(data_dir).join(COMPLETION_MARKER)),
            Ok(content) if content == "completed\n")
    }

    pub fn write_completion_marker(data_dir: &str) -> Result<(), String> {
        if data_dir.is_empty() {
            return Err("Gtuim data directory is unavailable".into());
        }
        fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
        fs::write(Path::new(data_dir).join(COMPLETION_MARKER), "completed\n")
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_detection_and_switching_work() {
        let mut w = OnboardingWidget::new();
        assert_eq!(w.preset, ControlPreset::Default);

        w.select_preset(ControlPreset::Familiar);
        assert_eq!(w.draft.keybindings.toggle_explorer, "<C-b>");
        assert_eq!(ControlPreset::detect(&w.draft.keybindings), ControlPreset::Familiar);

        w.select_preset(ControlPreset::VimSafe);
        assert_eq!(w.draft.keybindings.toggle_explorer, "<M-e>");
        assert_eq!(w.draft.keybindings.save_file, "<F2>");
        assert_eq!(ControlPreset::detect(&w.draft.keybindings), ControlPreset::VimSafe);

        w.draft.keybindings.toggle_explorer = "<F9>".to_string();
        assert_eq!(ControlPreset::detect(&w.draft.keybindings), ControlPreset::Custom);
    }

    #[test]
    fn keyboard_navigation_steps_and_finishes() {
        let mut w = OnboardingWidget::new();
        w.open(&SettingsConfig::default(), true);
        assert_eq!(w.step, OnboardingStep::Theme);

        assert_eq!(w.handle_key(&InputEvent::Key("<CR>".into())), OnboardingKeyAction::None);
        assert_eq!(w.step, OnboardingStep::Controls);

        assert_eq!(w.handle_key(&InputEvent::Key("4".into())), OnboardingKeyAction::None);
        assert_eq!(w.step, OnboardingStep::Setup);

        assert_eq!(w.handle_key(&InputEvent::Key("<CR>".into())), OnboardingKeyAction::Finish);
        assert_eq!(w.handle_key(&InputEvent::Key("<Esc>".into())), OnboardingKeyAction::Cancel);
        assert_eq!(w.handle_key(&InputEvent::Key("<C-s>".into())), OnboardingKeyAction::Finish);
    }

    #[test]
    fn font_size_clamps_to_safe_bounds() {
        let mut w = OnboardingWidget::new();
        w.draft.font_size = 14;
        w.adjust_font_size(-20);
        assert_eq!(w.draft.font_size, 9);
        w.adjust_font_size(50);
        assert_eq!(w.draft.font_size, 26);
    }

    #[test]
    fn completion_marker_is_app_specific_and_write_errors_are_reported() {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!("gtuim-onboarding-marker-{}-{}",
            std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("onboarding-complete"), "legacy marker").unwrap();
        fs::write(root.join("settings.json"), "existing settings").unwrap();
        OnboardingWidget::write_completion_marker(root.to_str().unwrap()).unwrap();
        assert_eq!(fs::read_to_string(root.join(COMPLETION_MARKER)).unwrap(), "completed\n");
        assert_eq!(fs::read_to_string(root.join("onboarding-complete")).unwrap(), "legacy marker");
        assert_eq!(fs::read_to_string(root.join("settings.json")).unwrap(), "existing settings");
        // Existing directory at the marker path must report failure, not claim
        // setup completion persisted. Empty data roots must never write to cwd.
        fs::remove_file(root.join(COMPLETION_MARKER)).unwrap();
        fs::create_dir(root.join(COMPLETION_MARKER)).unwrap();
        assert!(OnboardingWidget::write_completion_marker(root.to_str().unwrap()).is_err());
        assert!(OnboardingWidget::write_completion_marker("").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn built_in_themes_have_distinct_swatches() {
        let vscode = theme_swatch("vscode");
        let cat = theme_swatch("catppuccin");
        let gruv = theme_swatch("gruvbox");
        assert_ne!(vscode.accent, cat.accent);
        assert_ne!(cat.bg_editor, gruv.bg_editor);
    }
}
