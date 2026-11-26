//! Native GPUI Find & Replace bar overlaid on the editor pane, with seamless
//! fallback/switching to classic Vim `/` and `:%s/` search & replace.

use gpui::{prelude::*, *};
use rmpv::Value;

use crate::gui::icons::{gui_palette, icon, GuiPalette};
use crate::gui::GtuimView;
use crate::nvim::rpc::RpcClient;
use crate::tui::app::FocusRegion;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FindReplaceField {
    #[default]
    Find,
    Replace,
}

#[derive(Clone, Debug, Default)]
pub struct FindReplaceState {
    pub is_open: bool,
    pub focused: bool,
    pub show_replace: bool,
    pub query: String,
    pub replacement: String,
    pub active_field: FindReplaceField,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub use_regex: bool,
    pub match_count: usize,
    pub current_match: usize,
    pub status_msg: String,
}

impl FindReplaceState {
    pub fn open(&mut self, with_replace: bool) {
        self.is_open = true;
        self.focused = true;
        self.show_replace = with_replace;
        self.active_field = if with_replace && !self.query.is_empty() {
            FindReplaceField::Replace
        } else {
            FindReplaceField::Find
        };
        self.status_msg.clear();
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.focused = false;
        self.status_msg.clear();
    }

    pub fn active_text_mut(&mut self) -> &mut String {
        match self.active_field {
            FindReplaceField::Find => &mut self.query,
            FindReplaceField::Replace => &mut self.replacement,
        }
    }

    pub fn update_from_rpc(&mut self, params: &[Value]) {
        if let Some(cur) = params.first().and_then(|v| v.as_u64()) {
            self.current_match = cur as usize;
        }
        if let Some(total) = params.get(1).and_then(|v| v.as_u64()) {
            self.match_count = total as usize;
        }
        if let Some(msg) = params.get(2).and_then(|v| v.as_str()) {
            self.status_msg = msg.to_string();
        }
    }

    pub fn vim_pattern(&self) -> String {
        build_vim_search_pattern(&self.query, self.case_sensitive, self.whole_word, self.use_regex)
    }

    pub fn vim_replacement(&self) -> String {
        build_vim_replacement_string(&self.replacement, self.use_regex)
    }
}

/// Build a Vim regex pattern from GUI Find & Replace options.
/// Uses `\V` (very nomagic) for literal queries and `\v` (very magic) for regex mode.
pub fn build_vim_search_pattern(
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    use_regex: bool,
) -> String {
    if query.is_empty() {
        return String::new();
    }
    let case_prefix = if case_sensitive { "\\C" } else { "\\c" };
    if use_regex {
        if whole_word {
            format!("{case_prefix}\\v<%({query})>")
        } else {
            format!("{case_prefix}\\v%({query})")
        }
    } else {
        let escaped = query.replace('\\', "\\\\");
        if whole_word {
            format!("{case_prefix}\\V\\<{escaped}\\>")
        } else {
            format!("{case_prefix}\\V{escaped}")
        }
    }
}

/// Prepare the replacement text for Vim's `substitute()`.
/// In literal mode, escapes `\`, `&`, and `~` so replacement is inserted verbatim.
pub fn build_vim_replacement_string(replacement: &str, use_regex: bool) -> String {
    if use_regex {
        replacement.to_string()
    } else {
        let mut out = String::with_capacity(replacement.len());
        for ch in replacement.chars() {
            match ch {
                '\\' | '&' | '~' => {
                    out.push('\\');
                    out.push(ch);
                }
                _ => out.push(ch),
            }
        }
        out
    }
}

/// Build a classic Vim command-line string (`/` or `:%s/.../.../gc`) when the user
/// clicks "Vim" inside the GUI bar to switch to native Vim search/replace.
/// Strips raw control chars (`\r`, `\n`, `\x1b`) so handed-off text never executes prematurely.
pub fn build_vim_handoff_keys(state: &FindReplaceState) -> String {
    let sanitize = |s: &str| -> String {
        s.chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .replace('/', "\\/")
    };
    if state.show_replace {
        if state.query.is_empty() {
            ":%s/".to_string()
        } else {
            let pat = sanitize(&state.vim_pattern());
            let rep = sanitize(&state.vim_replacement());
            format!(":%s/{pat}/{rep}/gc")
        }
    } else if state.query.is_empty() {
        "/".to_string()
    } else {
        let pat = sanitize(&state.vim_pattern());
        format!("/{pat}")
    }
}

pub fn send_sync_search(rpc: &mut RpcClient, state: &FindReplaceState, jump: bool) {
    let pat = state.vim_pattern();
    let lua = r#"
        local pat, jump = ...
        if pat == '' then
            pcall(vim.cmd, 'nohlsearch')
            pcall(vim.rpcnotify, 1, 'gtuim_find_replace_state', 0, 0, '')
            return
        end
        local ok, err = pcall(function()
            vim.fn.setreg('/', pat)
            vim.o.hlsearch = true
            if jump then
                local cur = vim.api.nvim_win_get_cursor(0)
                local hit = vim.fn.search(pat, 'cw')
                if hit == 0 then
                    vim.api.nvim_win_set_cursor(0, cur)
                else
                    pcall(vim.cmd, 'normal! zv')
                end
            end
            local sc = vim.fn.searchcount({ recompute = 1, maxcount = 9999 })
            pcall(vim.rpcnotify, 1, 'gtuim_find_replace_state', sc.current or 0, sc.total or 0, '')
        end)
        if not ok then
            local msg = tostring(err):gsub('^.-:%s*', '')
            pcall(vim.rpcnotify, 1, 'gtuim_find_replace_state', 0, 0, msg)
        end
    "#;
    let _ = rpc.notify(
        "nvim_exec_lua",
        vec![
            Value::from(lua),
            Value::Array(vec![Value::from(pat), Value::from(jump)]),
        ],
    );
}

pub fn send_find_step(rpc: &mut RpcClient, state: &FindReplaceState, forward: bool) {
    let pat = state.vim_pattern();
    if pat.is_empty() {
        return;
    }
    let lua = r#"
        local pat, forward = ...
        local ok, err = pcall(function()
            vim.fn.setreg('/', pat)
            vim.o.hlsearch = true
            local flags = forward and 'w' or 'bw'
            local hit = vim.fn.search(pat, flags)
            if hit > 0 then
                pcall(vim.cmd, 'normal! zv')
            end
            local sc = vim.fn.searchcount({ recompute = 1, maxcount = 9999 })
            pcall(vim.rpcnotify, 1, 'gtuim_find_replace_state', sc.current or 0, sc.total or 0, '')
        end)
        if not ok then
            local msg = tostring(err):gsub('^.-:%s*', '')
            pcall(vim.rpcnotify, 1, 'gtuim_find_replace_state', 0, 0, msg)
        end
    "#;
    let _ = rpc.notify(
        "nvim_exec_lua",
        vec![
            Value::from(lua),
            Value::Array(vec![Value::from(pat), Value::from(forward)]),
        ],
    );
}

pub fn send_replace_current(rpc: &mut RpcClient, state: &FindReplaceState) {
    let pat = state.vim_pattern();
    if pat.is_empty() {
        return;
    }
    let repl = state.vim_replacement();
    let lua = r#"
        local pat, repl = ...
        local ok, err = pcall(function()
            if not vim.bo.modifiable then
                error('Buffer is read-only')
            end
            vim.fn.setreg('/', pat)
            vim.o.hlsearch = true
            local hit = vim.fn.search(pat, 'cw')
            if hit == 0 then
                pcall(vim.rpcnotify, 1, 'gtuim_find_replace_state', 0, 0, 'No matches')
                return
            end
            local pos = vim.api.nvim_win_get_cursor(0)
            local row0 = pos[1] - 1
            local col0 = pos[2]
            local line = vim.api.nvim_buf_get_lines(0, row0, row0 + 1, false)[1] or ''
            -- Anchor at the exact 1-based cursor byte column and scan from index 0 so
            -- lookbehinds (\@<=), anchors (^/$), and word boundaries see full line context.
            local anchored_pat = string.format('\\%%%dc', col0 + 1) .. pat
            local m = vim.fn.matchstrpos(line, anchored_pat, 0)
            local s_col, e_col = m[2], m[3]
            if s_col < 0 or e_col < s_col then
                pcall(vim.rpcnotify, 1, 'gtuim_find_replace_state', 0, 0, 'No match at cursor')
                return
            end
            local new_line = vim.fn.substitute(line, anchored_pat, repl, '')
            local pieces = vim.split(new_line, '\n', { plain = true })
            vim.api.nvim_buf_set_lines(0, row0, row0 + 1, false, pieces)
            local end_row = row0 + #pieces
            local end_col
            if #pieces == 1 then
                local delta = #new_line - #line
                end_col = math.max(0, e_col + delta)
            else
                end_col = #pieces[#pieces]
            end
            local cur_line_len = #(vim.api.nvim_buf_get_lines(0, end_row - 1, end_row, false)[1] or '')
            vim.api.nvim_win_set_cursor(0, { end_row, math.min(end_col, math.max(0, cur_line_len - 1)) })
            local next_hit = vim.fn.search(pat, 'cw')
            if next_hit > 0 then
                pcall(vim.cmd, 'normal! zv')
            end
            local sc = vim.fn.searchcount({ recompute = 1, maxcount = 9999 })
            pcall(vim.rpcnotify, 1, 'gtuim_find_replace_state', sc.current or 0, sc.total or 0, 'Replaced 1 occurrence')
        end)
        if not ok then
            local msg = tostring(err):gsub('^.-:%s*', '')
            pcall(vim.rpcnotify, 1, 'gtuim_find_replace_state', 0, 0, msg)
        end
    "#;
    let _ = rpc.notify(
        "nvim_exec_lua",
        vec![
            Value::from(lua),
            Value::Array(vec![Value::from(pat), Value::from(repl)]),
        ],
    );
}

pub fn send_replace_all(rpc: &mut RpcClient, state: &FindReplaceState) {
    let pat = state.vim_pattern();
    if pat.is_empty() {
        return;
    }
    let repl = state.vim_replacement();
    let lua = r#"
        local pat, repl = ...
        local ok, err = pcall(function()
            if not vim.bo.modifiable then
                error('Buffer is read-only')
            end
            vim.fn.setreg('/', pat)
            local lines = vim.api.nvim_buf_get_lines(0, 0, -1, false)
            local total_replaced = 0
            local new_lines = {}
            local changed = false
            for _, line in ipairs(lines) do
                local count_in_line = 0
                local min_col = 0
                while min_col <= #line do
                    local scan_pat = (min_col == 0) and pat or (string.format('\\%%>%dc', min_col) .. pat)
                    local m = vim.fn.matchstrpos(line, scan_pat, 0)
                    if m[2] < 0 then break end
                    count_in_line = count_in_line + 1
                    min_col = (m[3] > m[2]) and m[3] or (m[2] + 1)
                end
                if count_in_line > 0 then
                    total_replaced = total_replaced + count_in_line
                    changed = true
                    local subbed = vim.fn.substitute(line, pat, repl, 'g')
                    for _, part in ipairs(vim.split(subbed, '\n', { plain = true })) do
                        table.insert(new_lines, part)
                    end
                else
                    table.insert(new_lines, line)
                end
            end
            if changed then
                vim.api.nvim_buf_set_lines(0, 0, -1, false, new_lines)
            end
            local sc = vim.fn.searchcount({ recompute = 1, maxcount = 9999 })
            local msg = (total_replaced == 1) and 'Replaced 1 occurrence'
                or ('Replaced ' .. tostring(total_replaced) .. ' occurrences')
            pcall(vim.rpcnotify, 1, 'gtuim_find_replace_state', sc.current or 0, sc.total or 0, msg)
        end)
        if not ok then
            local msg = tostring(err):gsub('^.-:%s*', '')
            pcall(vim.rpcnotify, 1, 'gtuim_find_replace_state', 0, 0, msg)
        end
    "#;
    let _ = rpc.notify(
        "nvim_exec_lua",
        vec![
            Value::from(lua),
            Value::Array(vec![Value::from(pat), Value::from(repl)]),
        ],
    );
}

pub fn send_clear_highlights(rpc: &mut RpcClient) {
    let _ = rpc.notify(
        "nvim_exec_lua",
        vec![
            Value::from("pcall(vim.cmd, 'nohlsearch')"),
            Value::Array(vec![]),
        ],
    );
}

pub fn send_vim_handoff(rpc: &mut RpcClient, state: &FindReplaceState) {
    let keys = build_vim_handoff_keys(state);
    // Expand ONLY the generated <Esc> prefix; feed user text literally with
    // escape_ks=true so literal <CR>/<Esc>/<Tab> in query/replacement stays text.
    let lua = r#"
        local cmd_keys = ...
        local mode = vim.api.nvim_get_mode().mode
        local visual = mode:match('[vV\22]') ~= nil
        if mode:sub(1, 1) == 'i' or visual then
            local esc = vim.api.nvim_replace_termcodes('<Esc>', true, false, true)
            vim.api.nvim_feedkeys(esc, 'n', false)
        end
        vim.api.nvim_feedkeys(cmd_keys, 'n', true)
    "#;
    let _ = rpc.notify(
        "nvim_exec_lua",
        vec![Value::from(lua), Value::Array(vec![Value::from(keys)])],
    );
}

/// Handle a GPUI keystroke when the GUI Find & Replace bar is open and focused.
/// Returns `true` if the keystroke was consumed.
pub fn handle_key_down(view: &mut GtuimView, ks: &Keystroke, cx: &mut Context<GtuimView>) -> bool {
    if !view.find_replace.is_open || !view.find_replace.focused {
        return false;
    }

    let ctrl = ks.modifiers.control || ks.modifiers.platform;
    let alt = ks.modifiers.alt;
    let shift = ks.modifiers.shift;
    let key = ks.key.as_str();

    if key == "escape" {
        view.close_find_replace(cx);
        return true;
    }

    if ctrl && !alt {
        match key.to_ascii_lowercase().as_str() {
            "f" => {
                view.find_replace.active_field = FindReplaceField::Find;
                view.find_replace.status_msg.clear();
                cx.notify();
                return true;
            }
            "h" => {
                view.find_replace.show_replace = true;
                view.find_replace.active_field = FindReplaceField::Replace;
                view.find_replace.status_msg.clear();
                cx.notify();
                return true;
            }
            "v" => {
                if let Some(item) = cx.read_from_clipboard() {
                    if let Some(text) = item.text() {
                        let clean: String = text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
                        if !clean.is_empty() {
                            view.find_replace.active_text_mut().push_str(&clean);
                            if view.find_replace.active_field == FindReplaceField::Find {
                                send_sync_search(&mut view.app.editor_rpc, &view.find_replace, true);
                                let _ = view.app.editor_rpc.flush_writes();
                            }
                            cx.notify();
                        }
                    }
                }
                return true;
            }
            "u" => {
                view.find_replace.active_text_mut().clear();
                if view.find_replace.active_field == FindReplaceField::Find {
                    view.find_replace.match_count = 0;
                    view.find_replace.current_match = 0;
                    send_sync_search(&mut view.app.editor_rpc, &view.find_replace, false);
                    let _ = view.app.editor_rpc.flush_writes();
                }
                cx.notify();
                return true;
            }
            "enter" => {
                if view.find_replace.show_replace {
                    send_replace_all(&mut view.app.editor_rpc, &view.find_replace);
                    let _ = view.app.editor_rpc.flush_writes();
                    cx.notify();
                }
                return true;
            }
            _ => {}
        }
    }

    if alt && !ctrl {
        match key.to_ascii_lowercase().as_str() {
            "c" => {
                view.find_replace.case_sensitive = !view.find_replace.case_sensitive;
                send_sync_search(&mut view.app.editor_rpc, &view.find_replace, true);
                let _ = view.app.editor_rpc.flush_writes();
                cx.notify();
                return true;
            }
            "w" => {
                view.find_replace.whole_word = !view.find_replace.whole_word;
                send_sync_search(&mut view.app.editor_rpc, &view.find_replace, true);
                let _ = view.app.editor_rpc.flush_writes();
                cx.notify();
                return true;
            }
            "r" => {
                view.find_replace.use_regex = !view.find_replace.use_regex;
                send_sync_search(&mut view.app.editor_rpc, &view.find_replace, true);
                let _ = view.app.editor_rpc.flush_writes();
                cx.notify();
                return true;
            }
            _ => {}
        }
    }

    match key {
        "tab" => {
            if !view.find_replace.show_replace {
                view.find_replace.show_replace = true;
                view.find_replace.active_field = FindReplaceField::Replace;
            } else {
                view.find_replace.active_field = match view.find_replace.active_field {
                    FindReplaceField::Find => FindReplaceField::Replace,
                    FindReplaceField::Replace => FindReplaceField::Find,
                };
            }
            cx.notify();
            true
        }
        "f3" => {
            send_find_step(&mut view.app.editor_rpc, &view.find_replace, !shift);
            let _ = view.app.editor_rpc.flush_writes();
            cx.notify();
            true
        }
        "enter" => {
            match view.find_replace.active_field {
                FindReplaceField::Find => {
                    send_find_step(&mut view.app.editor_rpc, &view.find_replace, !shift);
                }
                FindReplaceField::Replace => {
                    send_replace_current(&mut view.app.editor_rpc, &view.find_replace);
                }
            }
            let _ = view.app.editor_rpc.flush_writes();
            cx.notify();
            true
        }
        "backspace" => {
            view.find_replace.active_text_mut().pop();
            if view.find_replace.active_field == FindReplaceField::Find {
                if view.find_replace.query.is_empty() {
                    view.find_replace.match_count = 0;
                    view.find_replace.current_match = 0;
                    view.find_replace.status_msg.clear();
                }
                send_sync_search(&mut view.app.editor_rpc, &view.find_replace, true);
                let _ = view.app.editor_rpc.flush_writes();
            }
            cx.notify();
            true
        }
        "space" if !ctrl && !alt => {
            view.find_replace.active_text_mut().push(' ');
            if view.find_replace.active_field == FindReplaceField::Find {
                send_sync_search(&mut view.app.editor_rpc, &view.find_replace, true);
                let _ = view.app.editor_rpc.flush_writes();
            }
            cx.notify();
            true
        }
        _ if !ctrl && !alt => {
            let text = ks
                .key_char
                .as_deref()
                .filter(|s| !s.is_empty() && !s.chars().any(|c| c.is_control()))
                .map(|s| s.to_string())
                .or_else(|| {
                    if key.chars().count() == 1 {
                        Some(if shift {
                            key.to_uppercase()
                        } else {
                            key.to_string()
                        })
                    } else {
                        None
                    }
                });
            if let Some(ch) = text {
                view.find_replace.active_text_mut().push_str(&ch);
                if view.find_replace.active_field == FindReplaceField::Find {
                    send_sync_search(&mut view.app.editor_rpc, &view.find_replace, true);
                    let _ = view.app.editor_rpc.flush_writes();
                }
                cx.notify();
                return true;
            }
            true
        }
        _ => true,
    }
}

fn option_chip(
    id: &'static str,
    label: &'static str,
    active: bool,
    p: GuiPalette,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .debug_selector(move || id.to_string())
        .h(px(22.0))
        .px(px(5.0))
        .rounded(px(4.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .border_1()
        .border_color(rgb(if active { p.accent } else { p.border_subtle }))
        .bg(rgb(if active { p.bg_selected } else { p.bg_surface }))
        .text_size(px(10.5))
        .font_weight(if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .text_color(rgb(if active { p.text_strong } else { p.text_muted }))
        .hover(|s| s.bg(rgb(p.bg_hover)).text_color(rgb(p.text_strong)))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(handler)
        .child(label)
}

fn icon_btn(
    id: &'static str,
    icon_name: &'static str,
    p: GuiPalette,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .debug_selector(move || id.to_string())
        .size(px(22.0))
        .rounded(px(4.0))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .hover(|s| s.bg(rgb(p.bg_hover)))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(handler)
        .child(icon(icon_name, 13.0, p.text_muted))
}

pub fn render_find_replace_bar(
    view: &GtuimView,
    cx: &mut Context<GtuimView>,
) -> impl IntoElement {
    let fr = &view.find_replace;
    let p = gui_palette(&view.app.theme);
    let find_focused = fr.focused && fr.active_field == FindReplaceField::Find;
    let replace_focused = fr.focused && fr.active_field == FindReplaceField::Replace;

    let count_text = if fr.query.is_empty() {
        String::new()
    } else if fr.match_count == 0 {
        "No results".to_string()
    } else {
        format!("{} / {}", fr.current_match, fr.match_count)
    };

    let find_row = div()
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(4.0))
        .child(icon_btn(
            "find-replace-toggle",
            if fr.show_replace {
                "chevron-down"
            } else {
                "chevron-right"
            },
            p,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                window.focus(&this.root_focus);
                this.find_replace.focused = true;
                this.find_replace.show_replace = !this.find_replace.show_replace;
                if this.find_replace.show_replace {
                    this.find_replace.active_field = FindReplaceField::Replace;
                } else {
                    this.find_replace.active_field = FindReplaceField::Find;
                }
                cx.notify();
            }),
        ))
        .child(
            div()
                .id("find-input")
                .debug_selector(|| "find-input".into())
                .flex_1()
                .min_w(px(80.0))
                .h(px(26.0))
                .px(px(8.0))
                .rounded(px(5.0))
                .bg(rgb(p.bg_input))
                .border_1()
                .border_color(rgb(if find_focused { p.accent } else { p.border }))
                .flex()
                .items_center()
                .overflow_hidden()
                .cursor_text()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        window.focus(&this.root_focus);
                        this.app.workspace.focus = FocusRegion::Editor;
                        this.find_replace.focused = true;
                        this.find_replace.active_field = FindReplaceField::Find;
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_size(px(12.0))
                        .font_family(view.mono_font.clone())
                        .text_color(rgb(if fr.query.is_empty() {
                            p.text_muted
                        } else {
                            p.text_strong
                        }))
                        .child(if fr.query.is_empty() {
                            if find_focused {
                                "▏Find in file...".to_string()
                            } else {
                                "Find in file...".to_string()
                            }
                        } else if find_focused {
                            format!("{}▏", fr.query)
                        } else {
                            fr.query.clone()
                        }),
                ),
        )
        .child(option_chip(
            "find-case-btn",
            "Aa",
            fr.case_sensitive,
            p,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                window.focus(&this.root_focus);
                this.find_replace.focused = true;
                this.find_replace.case_sensitive = !this.find_replace.case_sensitive;
                send_sync_search(&mut this.app.editor_rpc, &this.find_replace, true);
                let _ = this.app.editor_rpc.flush_writes();
                cx.notify();
            }),
        ))
        .child(option_chip(
            "find-word-btn",
            "ab",
            fr.whole_word,
            p,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                window.focus(&this.root_focus);
                this.find_replace.focused = true;
                this.find_replace.whole_word = !this.find_replace.whole_word;
                send_sync_search(&mut this.app.editor_rpc, &this.find_replace, true);
                let _ = this.app.editor_rpc.flush_writes();
                cx.notify();
            }),
        ))
        .child(option_chip(
            "find-regex-btn",
            ".*",
            fr.use_regex,
            p,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                window.focus(&this.root_focus);
                this.find_replace.focused = true;
                this.find_replace.use_regex = !this.find_replace.use_regex;
                send_sync_search(&mut this.app.editor_rpc, &this.find_replace, true);
                let _ = this.app.editor_rpc.flush_writes();
                cx.notify();
            }),
        ))
        .when(!count_text.is_empty(), |d| {
            d.child(
                div()
                    .debug_selector(|| "find-count".into())
                    .flex_shrink_0()
                    .px(px(4.0))
                    .whitespace_nowrap()
                    .text_size(px(10.5))
                    .text_color(rgb(if fr.match_count == 0 {
                        p.warning
                    } else {
                        p.text_muted
                    }))
                    .child(count_text),
            )
        })
        .child(icon_btn(
            "find-prev-btn",
            "chevron-up",
            p,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                window.focus(&this.root_focus);
                this.find_replace.focused = true;
                send_find_step(&mut this.app.editor_rpc, &this.find_replace, false);
                let _ = this.app.editor_rpc.flush_writes();
                cx.notify();
            }),
        ))
        .child(icon_btn(
            "find-next-btn",
            "chevron-down",
            p,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                window.focus(&this.root_focus);
                this.find_replace.focused = true;
                send_find_step(&mut this.app.editor_rpc, &this.find_replace, true);
                let _ = this.app.editor_rpc.flush_writes();
                cx.notify();
            }),
        ))
        .child(option_chip(
            "find-vim-btn",
            "Vim",
            false,
            p,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                window.focus(&this.root_focus);
                this.switch_find_replace_to_vim(cx);
            }),
        ))
        .child(icon_btn(
            "find-close-btn",
            "x",
            p,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                window.focus(&this.root_focus);
                this.close_find_replace(cx);
            }),
        ));

    let replace_row = fr.show_replace.then(|| {
        div()
            .debug_selector(|| "replace-row".into())
            .w_full()
            .min_w_0()
            .pl(px(26.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(
                div()
                    .id("replace-input")
                    .debug_selector(|| "replace-input".into())
                    .flex_1()
                    .min_w(px(80.0))
                    .h(px(26.0))
                    .px(px(8.0))
                    .rounded(px(5.0))
                    .bg(rgb(p.bg_input))
                    .border_1()
                    .border_color(rgb(if replace_focused { p.accent } else { p.border }))
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .cursor_text()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.root_focus);
                            this.app.workspace.focus = FocusRegion::Editor;
                            this.find_replace.focused = true;
                            this.find_replace.active_field = FindReplaceField::Replace;
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_size(px(12.0))
                            .font_family(view.mono_font.clone())
                            .text_color(rgb(if fr.replacement.is_empty() {
                                p.text_muted
                            } else {
                                p.text_strong
                            }))
                            .child(if fr.replacement.is_empty() {
                                if replace_focused {
                                    "▏Replace with...".to_string()
                                } else {
                                    "Replace with...".to_string()
                                }
                            } else if replace_focused {
                                format!("{}▏", fr.replacement)
                            } else {
                                fr.replacement.clone()
                            }),
                    ),
            )
            .child(
                div()
                    .id("replace-one-btn")
                    .debug_selector(|| "replace-one-btn".into())
                    .flex_shrink_0()
                    .h(px(26.0))
                    .px(px(8.0))
                    .rounded(px(5.0))
                    .bg(rgb(p.bg_elevated))
                    .border_1()
                    .border_color(rgb(p.border_subtle))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .text_size(px(11.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(p.text_strong))
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        window.focus(&this.root_focus);
                        this.find_replace.focused = true;
                        send_replace_current(&mut this.app.editor_rpc, &this.find_replace);
                        let _ = this.app.editor_rpc.flush_writes();
                        cx.notify();
                    }))
                    .child(icon("replace", 12.0, p.accent))
                    .child("Replace"),
            )
            .child(
                div()
                    .id("replace-all-btn")
                    .debug_selector(|| "replace-all-btn".into())
                    .flex_shrink_0()
                    .h(px(26.0))
                    .px(px(8.0))
                    .rounded(px(5.0))
                    .bg(rgb(p.accent))
                    .flex()
                    .items_center()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(p.accent_fg))
                    .hover(|s| s.opacity(0.9))
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| {
                        cx.stop_propagation();
                        window.focus(&this.root_focus);
                        this.find_replace.focused = true;
                        send_replace_all(&mut this.app.editor_rpc, &this.find_replace);
                        let _ = this.app.editor_rpc.flush_writes();
                        cx.notify();
                    }))
                    .child("All"),
            )
    });

    let status_row = (!fr.status_msg.is_empty()).then(|| {
        div()
            .debug_selector(|| "find-replace-status".into())
            .w_full()
            .min_w_0()
            .pl(px(26.0))
            .text_size(px(10.5))
            .text_color(rgb(p.text_muted))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(fr.status_msg.clone())
    });

    div()
        .id("find-replace-bar")
        .debug_selector(|| "find-replace-bar".into())
        .occlude()
        .absolute()
        .top(px(8.0))
        .right(px(12.0))
        .w(px(456.0))
        .max_w(relative(0.95))
        .min_w_0()
        .rounded(px(8.0))
        .bg(rgb(p.bg_panel))
        .border_1()
        .border_color(rgb(if fr.focused { p.accent } else { p.border }))
        .shadow_lg()
        .p(px(6.0))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, window, cx| {
                cx.stop_propagation();
                window.focus(&this.root_focus);
                this.find_replace.focused = true;
                cx.notify();
            }),
        )
        .child(find_row)
        .children(replace_row)
        .children(status_row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn builds_literal_whole_word_and_regex_vim_patterns() {
        assert_eq!(build_vim_search_pattern("", false, false, false), "");
        assert_eq!(
            build_vim_search_pattern("a.b\\c", false, false, false),
            "\\c\\Va.b\\\\c"
        );
        assert_eq!(
            build_vim_search_pattern("foo", true, true, false),
            "\\C\\V\\<foo\\>"
        );
        assert_eq!(
            build_vim_search_pattern("foo|bar", false, false, true),
            "\\c\\v%(foo|bar)"
        );
        assert_eq!(
            build_vim_search_pattern("foo|bar", true, true, true),
            "\\C\\v<%(foo|bar)>"
        );
    }

    #[::core::prelude::v1::test]
    fn escapes_literal_replacements_and_preserves_regex_backrefs() {
        assert_eq!(
            build_vim_replacement_string(r"a & b ~ c\1", false),
            r"a \& b \~ c\\1"
        );
        assert_eq!(
            build_vim_replacement_string(r"\1_&", true),
            r"\1_&"
        );
    }

    #[::core::prelude::v1::test]
    fn builds_vim_handoff_keys_for_find_and_replace_modes() {
        let mut st = FindReplaceState::default();
        st.open(false);
        assert_eq!(build_vim_handoff_keys(&st), "/");
        st.query = "a/b".into();
        assert_eq!(build_vim_handoff_keys(&st), "/\\c\\Va\\/b");

        st.show_replace = true;
        st.replacement = "x/y&z".into();
        assert_eq!(
            build_vim_handoff_keys(&st),
            ":%s/\\c\\Va\\/b/x\\/y\\&z/gc"
        );
    }

    fn run_nvim_lua(tag: &str, script: &str, envs: &[(&str, String)]) -> Option<String> {
        use std::process::Command;

        let path = std::env::temp_dir().join(format!(
            "gtuim-fr-nvim-{}-{}.lua",
            tag,
            std::process::id()
        ));
        std::fs::write(&path, script).expect("write temp lua test script");
        let mut cmd = Command::new("nvim");
        cmd.args(["--headless", "-u", "NONE", "-l"]).arg(&path);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let res = cmd.output();
        let _ = std::fs::remove_file(&path);
        let Ok(out) = res else {
            return None;
        };
        assert!(
            out.status.success(),
            "nvim -l failed (status {:?}): stderr={}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    #[::core::prelude::v1::test]
    fn neovim_executes_literal_whole_word_and_regex_replace_all() {
        let script = r#"
            vim.api.nvim_buf_set_lines(0, 0, -1, false, {
                'foo foobar FOO',
                'a.b and axb and a.b',
                'item_12 item_34',
            })
            local function replace_all(pat, repl)
                local lines = vim.api.nvim_buf_get_lines(0, 0, -1, false)
                local out = {}
                for _, line in ipairs(lines) do
                    table.insert(out, vim.fn.substitute(line, pat, repl, 'g'))
                end
                vim.api.nvim_buf_set_lines(0, 0, -1, false, out)
            end
            replace_all(vim.env.PAT1, vim.env.REP1)
            replace_all(vim.env.PAT2, vim.env.REP2)
            replace_all(vim.env.PAT3, vim.env.REP3)
            for _, line in ipairs(vim.api.nvim_buf_get_lines(0, 0, -1, false)) do
                io.write(line .. '\n')
            end
            io.flush()
        "#;

        let envs = [
            ("PAT1", build_vim_search_pattern("foo", true, true, false)),
            ("REP1", build_vim_replacement_string("baz", false)),
            ("PAT2", build_vim_search_pattern("a.b", true, false, false)),
            ("REP2", build_vim_replacement_string("X&Y", false)),
            ("PAT3", build_vim_search_pattern(r"item_(\d+)", true, false, true)),
            ("REP3", build_vim_replacement_string(r"#\1", true)),
        ];
        let Some(stdout) = run_nvim_lua("replace-all", script, &envs) else {
            return;
        };
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(
            lines,
            vec![
                "baz foobar FOO",
                "X&Y and axb and X&Y",
                "#12 #34",
            ]
        );
    }

    #[::core::prelude::v1::test]
    fn neovim_replace_current_preserves_lookbehind_alternation_and_whole_word_context() {
        let script = r#"
            local function replace_at_cursor(pat, repl)
                local hit = vim.fn.search(pat, 'cw')
                assert(hit > 0, 'expected match for ' .. pat)
                local pos = vim.api.nvim_win_get_cursor(0)
                local row0, col0 = pos[1] - 1, pos[2]
                local line = vim.api.nvim_buf_get_lines(0, row0, row0 + 1, false)[1] or ''
                local anchored_pat = string.format('\\%%%dc', col0 + 1) .. pat
                local m = vim.fn.matchstrpos(line, anchored_pat, 0)
                assert(m[2] >= 0, 'matchstrpos failed for ' .. anchored_pat)
                local new_line = vim.fn.substitute(line, anchored_pat, repl, '')
                vim.api.nvim_buf_set_lines(0, row0, row0 + 1, false, vim.split(new_line, '\n', { plain = true }))
            end

            -- 1. Lookbehind context: foo@<=bar must replace bar only when preceded by foo.
            vim.api.nvim_buf_set_lines(0, 0, -1, false, {
                'bar foobar bar',
                'alpha beta alpha',
                'foobar foo bar',
            })
            vim.api.nvim_win_set_cursor(0, { 1, 0 })
            replace_at_cursor(vim.env.PAT_LB, vim.env.REP_LB)

            -- 2. Alternation at second match on line: must NOT touch first match at col 0.
            vim.api.nvim_win_set_cursor(0, { 2, 6 })
            replace_at_cursor(vim.env.PAT_ALT, vim.env.REP_ALT)

            -- 3. Whole-word match: must skip 'foobar' and replace standalone 'foo'.
            vim.api.nvim_win_set_cursor(0, { 3, 0 })
            replace_at_cursor(vim.env.PAT_WORD, vim.env.REP_WORD)

            for _, line in ipairs(vim.api.nvim_buf_get_lines(0, 0, -1, false)) do
                io.write(line .. '\n')
            end
            io.flush()
        "#;

        let envs = [
            ("PAT_LB", build_vim_search_pattern("(foo)@<=bar", true, false, true)),
            ("REP_LB", build_vim_replacement_string("BAZ", true)),
            ("PAT_ALT", build_vim_search_pattern("alpha|beta", true, false, true)),
            ("REP_ALT", build_vim_replacement_string("BETA", true)),
            ("PAT_WORD", build_vim_search_pattern("foo", true, true, false)),
            ("REP_WORD", build_vim_replacement_string("QUX", false)),
        ];
        let Some(stdout) = run_nvim_lua("replace-current", script, &envs) else {
            return;
        };
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(
            lines,
            vec![
                "bar fooBAZ bar",
                "alpha BETA alpha",
                "foobar QUX bar",
            ]
        );
    }

    #[::core::prelude::v1::test]
    fn neovim_vim_handoff_keeps_literal_cr_text_without_executing() {
        let mut st = FindReplaceState::default();
        st.open(true);
        st.query = "foo<CR>".into();
        st.replacement = "bar<Esc>".into();
        let handoff = build_vim_handoff_keys(&st);
        assert!(handoff.contains("<CR>"));
        assert!(handoff.contains("<Esc>"));

        let script = r#"
            vim.api.nvim_buf_set_lines(0, 0, -1, false, { 'foo<CR> stays intact' })
            -- Feed literal cmd_keys with escape_ks=true (same as send_vim_handoff).
            -- If <CR> were expanded, it would execute :%s/... and alter the buffer.
            vim.api.nvim_feedkeys(vim.env.CMD_KEYS, 'nx', true)
            io.write(vim.api.nvim_buf_get_lines(0, 0, 1, false)[1] .. '\n')
            io.flush()
        "#;
        let Some(stdout) = run_nvim_lua("vim-handoff", script, &[("CMD_KEYS", handoff)]) else {
            return;
        };
        assert_eq!(stdout.trim(), "foo<CR> stays intact");
    }
}
