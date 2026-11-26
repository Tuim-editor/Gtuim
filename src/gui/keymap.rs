use gpui::Keystroke;

/// Translate a GPUI `Keystroke` into Rtuim's canonical Vim/Neovim key notation
/// (e.g. `<C-e>`, `<M-Left>`, `<S-F6>`, `<CR>`, `<Esc>`, `"a"`, `" "`...).
pub fn keystroke_to_tuim_key(ks: &Keystroke) -> Option<String> {
    let ctrl = ks.modifiers.control;
    let alt = ks.modifiers.alt;
    let shift = ks.modifiers.shift;
    let platform = ks.modifiers.platform;

    // On Linux, treat Super/Platform only when explicitly paired with common IDE combos,
    // otherwise ignore lone modifier keystrokes.
    let key = ks.key.as_str();
    if matches!(
        key,
        "shift" | "control" | "alt" | "platform" | "super" | "meta" | "capslock" | ""
    ) {
        return None;
    }

    let special = match key {
        "escape" => Some("Esc"),
        "enter" => Some("CR"),
        "backspace" => Some("BS"),
        "tab" => Some("Tab"),
        "up" => Some("Up"),
        "down" => Some("Down"),
        "left" => Some("Left"),
        "right" => Some("Right"),
        "home" => Some("Home"),
        "end" => Some("End"),
        "pageup" => Some("PageUp"),
        "pagedown" => Some("PageDown"),
        "insert" => Some("Insert"),
        "delete" => Some("Del"),
        "f1" => Some("F1"),
        "f2" => Some("F2"),
        "f3" => Some("F3"),
        "f4" => Some("F4"),
        "f5" => Some("F5"),
        "f6" => Some("F6"),
        "f7" => Some("F7"),
        "f8" => Some("F8"),
        "f9" => Some("F9"),
        "f10" => Some("F10"),
        "f11" => Some("F11"),
        "f12" => Some("F12"),
        _ => None,
    };

    if let Some(name) = special {
        let mut mods = String::new();
        if ctrl {
            mods.push_str("C-");
        }
        if alt {
            mods.push_str("M-");
        }
        if shift {
            mods.push_str("S-");
        }
        if mods.is_empty() {
            return Some(format!("<{}>", name));
        }
        return Some(format!("<{}{}>", mods, name));
    }

    if key == "space" {
        if ctrl || alt || shift {
            let mut mods = String::new();
            if ctrl {
                mods.push_str("C-");
            }
            if alt {
                mods.push_str("M-");
            }
            if shift {
                mods.push_str("S-");
            }
            return Some(format!("<{}Space>", mods));
        }
        return Some(" ".to_string());
    }

    // Ctrl / Alt / Super modified character key
    if ctrl || alt || platform {
        let effective_ctrl = ctrl || platform;
        let shifted_symbol = ks.key_char.as_ref().filter(|text| {
            text.chars().count() == 1 && text.chars().all(|ch| ch.is_ascii_punctuation())
        });
        let base = if alt && shift && !effective_ctrl && shifted_symbol.is_some() {
            shifted_symbol.unwrap().clone()
        } else if key.chars().count() == 1 {
            key.to_lowercase()
        } else if let Some(kc) = &ks.key_char {
            if kc.chars().count() == 1 {
                kc.to_lowercase()
            } else {
                return None;
            }
        } else {
            return None;
        };

        let mut mods = String::new();
        if effective_ctrl {
            mods.push_str("C-");
        }
        if alt {
            mods.push_str("M-");
        }
        if shift && base.chars().all(|c| c.is_ascii_alphabetic()) {
            mods.push_str("S-");
        }
        return Some(format!("<{}{}>", mods, base));
    }

    // Plain printable character (prefer key_char so Shift+1 -> "!", Shift+a -> "A", Unicode works)
    if let Some(kc) = &ks.key_char {
        if !kc.is_empty() && !kc.chars().any(|c| c.is_control()) {
            return Some(kc.clone());
        }
    }

    if key.chars().count() == 1 {
        if shift {
            return Some(key.to_uppercase());
        }
        return Some(key.to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Modifiers;

    fn ks(key: &str, key_char: Option<&str>, ctrl: bool, alt: bool, shift: bool) -> Keystroke {
        Keystroke {
            modifiers: Modifiers {
                control: ctrl,
                alt,
                shift,
                platform: false,
                function: false,
            },
            key: key.to_string(),
            key_char: key_char.map(|s| s.to_string()),
        }
    }

    #[test]
    fn translates_tuim_shortcuts_and_printable_keys() {
        assert_eq!(keystroke_to_tuim_key(&ks("f1", None, false, false, false)).as_deref(), Some("<F1>"));
        assert_eq!(keystroke_to_tuim_key(&ks("f6", None, false, false, true)).as_deref(), Some("<S-F6>"));
        assert_eq!(keystroke_to_tuim_key(&ks("e", None, true, false, false)).as_deref(), Some("<C-e>"));
        assert_eq!(keystroke_to_tuim_key(&ks("c", None, true, false, true)).as_deref(), Some("<C-S-c>"));
        assert_eq!(keystroke_to_tuim_key(&ks("s", None, true, true, false)).as_deref(), Some("<C-M-s>"));
        assert_eq!(keystroke_to_tuim_key(&ks("left", None, false, true, false)).as_deref(), Some("<M-Left>"));
        assert_eq!(keystroke_to_tuim_key(&ks("tab", None, false, false, true)).as_deref(), Some("<S-Tab>"));
        assert_eq!(keystroke_to_tuim_key(&ks("space", Some(" "), false, false, false)).as_deref(), Some(" "));
        assert_eq!(keystroke_to_tuim_key(&ks("a", Some("A"), false, false, true)).as_deref(), Some("A"));
        assert_eq!(keystroke_to_tuim_key(&ks("1", Some("!"), false, false, true)).as_deref(), Some("!"));
        assert_eq!(keystroke_to_tuim_key(&ks(",", Some("<"), false, true, true)).as_deref(), Some("<M-<>"));
        assert_eq!(keystroke_to_tuim_key(&ks(".", Some(">"), false, true, true)).as_deref(), Some("<M->>"));
    }
}
