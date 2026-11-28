//! Resolution-independent Lucide-style vector icons and Zed-inspired GUI theme palette.
use std::borrow::Cow;

use gpui::{prelude::*, *};

use crate::tui::theme::{blend_u32, contrast_ratio_u32, readable_u32, Theme};

pub const ICON_LSP: &str = "lsp";
pub const ICON_SETTINGS: &str = "settings";

const ICONS: &[(&str, &str)] = &[
    (
        "files",
        r#"<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/>"#,
    ),
    (
        "folder",
        r#"<path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>"#,
    ),
    (
        "folder-open",
        r#"<path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.54 6a2 2 0 0 1-1.95 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2"/>"#,
    ),
    (
        "file-code",
        r#"<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="m10 13-2 2 2 2"/><path d="m14 17 2-2-2-2"/>"#,
    ),
    (
        "file-plus",
        r#"<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M9 15h6"/><path d="M12 18v-6"/>"#,
    ),
    (
        "folder-plus",
        r#"<path d="M12 10v6"/><path d="M9 13h6"/><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>"#,
    ),
    (
        "git-branch",
        r#"<line x1="6" x2="6" y1="3" y2="15"/><circle cx="18" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M18 9a9 9 0 0 1-9 9"/>"#,
    ),
    (
        "git-fork",
        r#"<circle cx="12" cy="18" r="3"/><circle cx="6" cy="6" r="3"/><circle cx="18" cy="6" r="3"/><path d="M18 9v2c0 .6-.4 1-1 1H7c-.6 0-1-.4-1-1V9"/><path d="M12 12v3"/>"#,
    ),
    (
        "search",
        r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>"#,
    ),
    (
        "sparkles",
        r#"<path d="M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z"/><path d="M20 3v4"/><path d="M22 5h-4"/>"#,
    ),
    (
        "blocks",
        r#"<rect width="7" height="7" x="14" y="3" rx="1"/><path d="M10 21V8a1 1 0 0 0-1-1H4a1 1 0 0 0-1 1v12a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-5a1 1 0 0 0-1-1H3"/>"#,
    ),
    (
        "wrench",
        r#"<path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/>"#,
    ),
    (
        "lsp",
        r#"<path d="m18 16 4-4-4-4"/><path d="m6 8-4 4 4 4"/><path d="m14.5 4-5 16"/>"#,
    ),
    (
        "settings",
        r#"<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>"#,
    ),
    (
        "panel-left",
        r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M9 3v18"/>"#,
    ),
    (
        "panel-bottom",
        r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M3 15h18"/>"#,
    ),
    (
        "panel-right",
        r#"<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M15 3v18"/>"#,
    ),
    (
        "split-vertical",
        r#"<rect width="18" height="18" x="3" y="3" rx="2"/><line x1="12" x2="12" y1="3" y2="21"/>"#,
    ),
    (
        "terminal",
        r#"<polyline points="4 17 10 11 4 5"/><line x1="12" x2="20" y1="19" y2="19"/>"#,
    ),
    (
        "command",
        r#"<path d="M15 6v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3V6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3"/>"#,
    ),
    ("chevron-right", r#"<path d="m9 18 6-6-6-6"/>"#),
    ("chevron-down", r#"<path d="m6 9 6 6 6-6"/>"#),
    ("chevron-up", r#"<path d="m18 15-6-6-6 6"/>"#),
    ("chevron-left", r#"<path d="m15 18-6-6 6-6"/>"#),
    (
        "replace",
        r#"<path d="M14 4a2 2 0 0 1 2-2"/><path d="M16 10a2 2 0 0 1-2-2"/><path d="M20 2a2 2 0 0 1 2 2"/><path d="M22 8a2 2 0 0 1-2 2"/><path d="m3 7 3 3 3-3"/><path d="M6 10V5a3 3 0 0 1 3-3h1"/><rect x="2" y="14" width="8" height="8" rx="2"/>"#,
    ),
    (
        "refresh",
        r#"<path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16"/><path d="M16 16h5v5"/>"#,
    ),
    ("plus", r#"<path d="M5 12h14"/><path d="M12 5v14"/>"#),
    ("minus", r#"<path d="M5 12h14"/>"#),
    ("check", r#"<path d="M20 6 9 17l-5-5"/>"#),
    ("x", r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#),
    (
        "bug",
        r#"<path d="m8 2 1.88 1.88"/><path d="M14.12 3.88 16 2"/><path d="M9 7.13v-1a3.003 3.003 0 1 1 6 0v1"/><path d="M12 20c-3.3 0-6-2.7-6-6v-3a4 4 0 0 1 4-4h4a4 4 0 0 1 4 4v3c0 3.3-2.7 6-6 6"/><path d="M12 20v-9"/><path d="M6.53 9C4.6 8.8 3 7.1 3 5"/><path d="M6 13H2"/><path d="M3 21c0-2.1 1.7-3.9 3.8-4"/><path d="M20.97 5c0 2.1-1.6 3.8-3.5 4"/><path d="M22 13h-4"/><path d="M17.2 17c2.1.1 3.8 1.9 3.8 4"/>"#,
    ),
    (
        "maximize",
        r#"<path d="M8 3H5a2 2 0 0 0-2 2v3"/><path d="M21 8V5a2 2 0 0 0-2-2h-3"/><path d="M3 16v3a2 2 0 0 0 2 2h3"/><path d="M16 21h3a2 2 0 0 0 2-2v-3"/>"#,
    ),
    (
        "download",
        r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/>"#,
    ),
    (
        "upload",
        r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="17 8 12 3 7 8"/><line x1="12" x2="12" y1="3" y2="15"/>"#,
    ),
    (
        "history",
        r#"<path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M12 7v5l4 2"/>"#,
    ),
    (
        "palette",
        r#"<circle cx="13.5" cy="6.5" r=".5" fill="currentColor"/><circle cx="17.5" cy="10.5" r=".5" fill="currentColor"/><circle cx="8.5" cy="7.5" r=".5" fill="currentColor"/><circle cx="6.5" cy="12.5" r=".5" fill="currentColor"/><path d="M12 2C6.5 2 2 6.5 2 12s4.5 10 10 10c.926 0 1.648-.746 1.648-1.688 0-.437-.18-.835-.437-1.125-.29-.289-.438-.652-.438-1.125a1.64 1.64 0 0 1 1.668-1.668h1.996c3.051 0 5.555-2.503 5.555-5.554C21.965 6.012 17.461 2 12 2z"/>"#,
    ),
    (
        "keyboard",
        r#"<path d="M10 8h.01"/><path d="M12 12h.01"/><path d="M14 8h.01"/><path d="M16 12h.01"/><path d="M18 8h.01"/><path d="M6 8h.01"/><path d="M7 16h10"/><path d="M8 12h.01"/><rect width="20" height="16" x="2" y="4" rx="2"/>"#,
    ),
    (
        "book-open",
        r#"<path d="M12 7v14"/><path d="M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z"/>"#,
    ),
    (
        "image",
        r#"<rect width="18" height="18" x="3" y="3" rx="2" ry="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/>"#,
    ),
    (
        "sliders",
        r#"<line x1="21" x2="14" y1="4" y2="4"/><line x1="10" x2="3" y1="4" y2="4"/><line x1="21" x2="12" y1="12" y2="12"/><line x1="8" x2="3" y1="12" y2="12"/><line x1="21" x2="16" y1="20" y2="20"/><line x1="12" x2="3" y1="20" y2="20"/><line x1="14" x2="14" y1="2" y2="6"/><line x1="8" x2="8" y1="10" y2="14"/><line x1="16" x2="16" y1="18" y2="22"/>"#,
    ),
    (
        "arrow-right",
        r#"<path d="M5 12h14"/><path d="m12 5 7 7-7 7"/>"#,
    ),
    (
        "arrow-left",
        r#"<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>"#,
    ),
    (
        "grip-vertical",
        r#"<circle cx="9" cy="12" r="1"/><circle cx="9" cy="5" r="1"/><circle cx="9" cy="19" r="1"/><circle cx="15" cy="12" r="1"/><circle cx="15" cy="5" r="1"/><circle cx="15" cy="19" r="1"/>"#,
    ),
];

pub struct Assets;

const TUIM_LOGO: &str = "tuim.svg";

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if path == TUIM_LOGO {
            return Ok(Some(Cow::Borrowed(include_bytes!("../../assets/tuim.svg"))));
        }
        Ok(ICONS.iter().find(|(name, _)| *name == path).map(|(_, body)| {
            Cow::Owned(
                format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#
                )
                .into_bytes(),
            )
        }))
    }

    fn list(&self, _: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .map(|(name, _)| SharedString::from(*name))
            .chain(std::iter::once(SharedString::from(TUIM_LOGO)))
            .collect())
    }
}

pub fn tuim_logo(size: f32) -> Img {
    img(ImageSource::Resource(Resource::Embedded(TUIM_LOGO.into())))
        .size(px(size))
        .flex_shrink_0()
}

pub fn icon_svg_body(name: &str) -> Option<&'static str> {
    ICONS
        .iter()
        .find(|(icon_name, _)| *icon_name == name)
        .map(|(_, body)| *body)
}

pub fn icon(name: &'static str, size: f32, color: u32) -> Svg {
    svg()
        .path(name)
        .size(px(size))
        .flex_shrink_0()
        .text_color(rgb(color))
}

/// Zed-inspired refined GUI palette derived cleanly from the active editor theme.
/// Avoids TUI artifacts like Comment-green secondary text or overly bright 3:1 borders.
#[derive(Clone, Copy, Debug)]
pub struct GuiPalette {
    pub bg_editor: u32,
    pub bg_panel: u32,
    pub bg_rail: u32,
    pub bg_surface: u32,
    pub bg_elevated: u32,
    pub bg_input: u32,
    pub bg_hover: u32,
    pub bg_selected: u32,
    pub border: u32,
    pub border_subtle: u32,
    pub text_strong: u32,
    pub text: u32,
    pub text_muted: u32,
    pub accent: u32,
    pub accent_fg: u32,
    pub success: u32,
    pub warning: u32,
    pub error: u32,
}

pub fn gui_palette(theme: &Theme) -> GuiPalette {
    let bg_editor = theme.bg_editor;
    let is_light = contrast_ratio_u32(0x000000, bg_editor) > contrast_ratio_u32(0xffffff, bg_editor);

    let (bg_rail, bg_panel, bg_surface, bg_elevated, bg_input) = if is_light {
        (
            blend_u32(bg_editor, 0x000000, 7),
            blend_u32(bg_editor, 0x000000, 4),
            blend_u32(bg_editor, 0x000000, 6),
            blend_u32(bg_editor, 0xffffff, 40),
            blend_u32(bg_editor, 0x000000, 3),
        )
    } else {
        (
            blend_u32(bg_editor, 0x000000, 26),
            blend_u32(bg_editor, 0x000000, 14),
            blend_u32(bg_editor, 0x000000, 20),
            blend_u32(bg_editor, 0xffffff, 7),
            blend_u32(bg_editor, 0x000000, 30),
        )
    };

    let text_endpoint = if is_light { 0x16181c } else { 0xe6e9ef };
    let text_strong = blend_u32(theme.fg_primary, text_endpoint, 35);
    let text = blend_u32(bg_panel, text_strong, 82);
    let text_muted = blend_u32(bg_panel, text_strong, 48);

    let border = blend_u32(bg_panel, text_strong, 11);
    let border_subtle = blend_u32(bg_panel, text_strong, 7);

    let raw_accent = if theme.accent == 0 {
        if theme.bg_accent == 0 { 0x3b82f6 } else { theme.bg_accent }
    } else {
        theme.accent
    };
    let accent = readable_u32(raw_accent, bg_panel, 3.0);
    let accent_fg = if contrast_ratio_u32(0xffffff, accent) >= 3.5 {
        0xffffff
    } else {
        0x111318
    };

    let bg_hover = blend_u32(bg_panel, text_strong, 7);
    let bg_selected = blend_u32(bg_panel, accent, 22);

    GuiPalette {
        bg_editor,
        bg_panel,
        bg_rail,
        bg_surface,
        bg_elevated,
        bg_input,
        bg_hover,
        bg_selected,
        border,
        border_subtle,
        text_strong,
        text,
        text_muted,
        accent,
        accent_fg,
        success: theme.success,
        warning: theme.warning,
        error: theme.error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn lsp_and_settings_icons_are_present_and_distinct() {
        assert_ne!(ICON_LSP, ICON_SETTINGS);
        assert_ne!(ICON_LSP, "wrench");

        let lsp_body = icon_svg_body(ICON_LSP).expect("lsp icon must exist");
        let settings_body = icon_svg_body(ICON_SETTINGS).expect("settings icon must exist");
        let wrench_body = icon_svg_body("wrench").expect("wrench icon must exist");

        assert!(!lsp_body.is_empty());
        assert!(!settings_body.is_empty());
        assert_ne!(lsp_body, settings_body);
        assert_ne!(lsp_body, wrench_body);

        let assets = Assets;
        let lsp_svg = assets.load(ICON_LSP).unwrap().expect("lsp asset bytes");
        let settings_svg = assets.load(ICON_SETTINGS).unwrap().expect("settings asset bytes");
        assert_ne!(lsp_svg.as_ref(), settings_svg.as_ref());
        assert!(std::str::from_utf8(&lsp_svg).unwrap().contains("<svg"));
        assert!(icon_svg_body("chevron-up").is_some());
        assert!(icon_svg_body("replace").is_some());
    }
}
