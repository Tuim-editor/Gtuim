use rmpv::Value;
use serde::Deserialize;

use crate::nvim::rpc::RpcClient;
use crate::tui::input::InputEvent;
use crate::tui::layout::Rect;
use crate::tui::renderer::Renderer;
use crate::tui::theme::{chrome, Theme};
use crate::tui::widgets::extension_shop::{
    ensure_plugin_config_file, ensure_user_config_file, resolve_user_config_dir,
    user_plugins_template, PluginInfo,
};
use crate::tui::widgets::primitives::{self, ListState};

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LazyPlugin {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub loaded: bool,
    #[serde(default)]
    pub has_updates: bool,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub config_path: String,
    #[serde(default)]
    pub auto_init: String,
}

pub const LAZY_TABS: &[&str] = &[" All ", " Loaded ", " Lazy ", " Updates "];

pub struct LazyWidget {
    pub is_open: bool,
    pub active_tab: usize,
    pub search_query: String,
    pub is_searching: bool,
    pub plugins: Vec<LazyPlugin>,
    pub filtered: Vec<usize>,
    pub list_state: ListState,
    pub status_msg: String,
    pub edit_config_path: Option<String>,
}

impl LazyWidget {
    pub fn new() -> Self {
        Self {
            is_open: false,
            active_tab: 0,
            search_query: String::new(),
            is_searching: false,
            plugins: Vec::new(),
            filtered: Vec::new(),
            list_state: ListState::new(),
            status_msg: String::new(),
            edit_config_path: None,
        }
    }

    pub fn refresh(&mut self, rpc: &mut RpcClient) {
        let lua_code = r#"
            local ok, lazy = pcall(require, 'lazy')
            if not ok then return '[]' end
            local data_dir = vim.fn.stdpath('data')
            local inv_by_repo = {}
            local inv_f = io.open(data_dir .. '/plugin_inventory.json', 'r')
            if inv_f then
                local content = inv_f:read('*a')
                inv_f:close()
                local ok_j, parsed = pcall(vim.json.decode, content)
                if ok_j and type(parsed) == 'table' then
                    for _, item in ipairs(parsed) do
                        if type(item) == 'table' and item.full_name then
                            inv_by_repo[item.full_name] = item
                        end
                    end
                end
            end
            local res = {}
            for _, p in ipairs(lazy.plugins()) do
                local repo = type(p[1]) == 'string' and p[1] or ''
                local inv = inv_by_repo[repo] or {}
                table.insert(res, {
                    name = p.name or repo or 'unknown',
                    repo = repo,
                    loaded = (p._ and p._.loaded ~= nil) or false,
                    has_updates = (p._ and p._.updates ~= nil) or false,
                    url = p.url or '',
                    config_path = inv.config_path or '',
                    auto_init = inv.auto_init or '',
                })
            end
            return vim.fn.json_encode(res)
        "#;

        if let Ok(Value::String(s)) = rpc.call(
            "nvim_exec_lua",
            vec![Value::from(lua_code), Value::Array(vec![])],
        ) {
            if let Some(json_str) = s.as_str() {
                if let Ok(parsed) = serde_json::from_str::<Vec<LazyPlugin>>(json_str) {
                    self.plugins = parsed;
                    self.plugins.sort_by(|a, b| a.name.cmp(&b.name));
                }
            }
        }
        self.apply_filter();
    }

    pub fn apply_filter(&mut self) {
        let q = self.search_query.to_lowercase();
        self.filtered.clear();
        for (idx, p) in self.plugins.iter().enumerate() {
            let tab_match = match self.active_tab {
                0 => true,
                1 => p.loaded,
                2 => !p.loaded,
                3 => p.has_updates,
                _ => true,
            };
            if !tab_match {
                continue;
            }
            if !q.is_empty() && !p.name.to_lowercase().contains(&q) {
                continue;
            }
            self.filtered.push(idx);
        }
        self.list_state.clamp(self.filtered.len());
    }

    pub fn handle_key(&mut self, ev: &InputEvent, rpc: &mut RpcClient) -> bool {
        if !self.is_open {
            return false;
        }
        let key = match ev {
            InputEvent::Key(k) => k.as_str(),
            _ => return false,
        };

        if self.is_searching {
            match key {
                "<Esc>" | "<CR>" => {
                    self.is_searching = false;
                    return true;
                }
                "<BS>" => {
                    self.search_query.pop();
                    self.apply_filter();
                    return true;
                }
                k if k.chars().count() == 1 && !k.starts_with('<') => {
                    self.search_query.push_str(k);
                    self.apply_filter();
                    return true;
                }
                _ => return true,
            }
        }

        match key {
            "<Esc>" | "q" => {
                self.is_open = false;
                true
            }
            "/" => {
                self.is_searching = true;
                true
            }
            "1" | "2" | "3" | "4" => {
                self.active_tab = (key.as_bytes()[0] - b'1') as usize;
                self.apply_filter();
                true
            }
            "<Tab>" | "l" | "<Right>" => {
                self.active_tab = (self.active_tab + 1) % LAZY_TABS.len();
                self.apply_filter();
                true
            }
            "h" | "<Left>" => {
                self.active_tab = (self.active_tab + LAZY_TABS.len() - 1) % LAZY_TABS.len();
                self.apply_filter();
                true
            }
            "j" | "<Down>" => {
                self.list_state.move_down(self.filtered.len(), 1);
                true
            }
            "k" | "<Up>" => {
                self.list_state.move_up(1);
                true
            }
            "e" | "E" => {
                if let Some(&idx) = self.filtered.get(self.list_state.selected) {
                    if let Some(p) = self.plugins.get(idx) {
                        let repo = if !p.repo.is_empty() {
                            p.repo.clone()
                        } else {
                            p.url
                                .trim_end_matches(".git")
                                .strip_prefix("https://github.com/")
                                .unwrap_or("")
                                .to_string()
                        };
                        let data_dir = std::env::var("XDG_DATA_HOME")
                            .ok()
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| {
                                format!(
                                    "{}/.local/share",
                                    std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())
                                )
                            })
                            + "/tuim";
                        let info = PluginInfo {
                            name: p.name.clone(),
                            full_name: repo,
                            config_path: p.config_path.clone(),
                            installed: true,
                            enabled: true,
                            ..Default::default()
                        };
                        match ensure_plugin_config_file(&data_dir, &resolve_user_config_dir(), &info) {
                            Ok(path) => {
                                self.edit_config_path = Some(path);
                                self.is_open = false;
                            }
                            Err(err) => {
                                self.status_msg = err;
                            }
                        }
                    }
                }
                true
            }
            "p" | "P" => {
                match ensure_user_config_file(
                    &resolve_user_config_dir(),
                    "plugins.lua",
                    user_plugins_template(),
                ) {
                    Ok(path) => {
                        self.edit_config_path = Some(path);
                        self.is_open = false;
                    }
                    Err(err) => self.status_msg = err,
                }
                true
            }
            "u" | "U" => {
                let _ = rpc.notify("nvim_command", vec![Value::from("Lazy update")]);
                self.status_msg = "Triggered Lazy update...".to_string();
                true
            }
            "s" | "S" => {
                let _ = rpc.notify(
                    "nvim_exec_lua",
                    vec![
                        Value::from(
                            "if _G.tuim_retry_plugins then _G.tuim_retry_plugins() else vim.cmd('Lazy sync') end",
                        ),
                        Value::Array(vec![]),
                    ],
                );
                self.status_msg = "Triggered Lazy sync...".to_string();
                true
            }
            "x" | "X" => {
                let _ = rpc.notify("nvim_command", vec![Value::from("Lazy clean")]);
                self.status_msg = "Triggered Lazy clean...".to_string();
                true
            }
            _ => true,
        }
    }

    pub fn handle_mouse(
        &mut self, screen: Rect, mx: u16, my: u16, button: u8, pressed: bool, rpc: &mut RpcClient,
    ) -> bool {
        if !self.is_open || !pressed { return false; }
        let layout = primitives::ManagerLayout::new(screen);
        if !layout.outer.contains(mx, my) {
            if button == 0 { self.is_open = false; }
            return true;
        }
        if button == 64 { self.list_state.move_up(1); return true; }
        if button == 65 { self.list_state.move_down(self.filtered.len(), 1); return true; }
        if button != 0 { return true; }
        if layout.search.contains(mx, my) { self.is_searching = true; return true; }
        if layout.nav.contains(mx, my) {
            let index = (my - layout.nav.y) as usize;
            if index < LAZY_TABS.len() { self.active_tab = index; self.apply_filter(); }
            return true;
        }
        let actions = ["[ Sync ]", "[ Update ]", "[ Close ]"];
        for (i, key) in ["s", "u", "<Esc>"].iter().enumerate() {
            if layout.action_rect(&actions, i).contains(mx, my) {
                self.handle_key(&InputEvent::Key((*key).into()), rpc);
                return true;
            }
        }
        if layout.content.contains(mx, my) && my >= layout.content.y + 3 {
            let index = self.list_state.scroll_offset + (my - layout.content.y - 3) as usize;
            if index < self.filtered.len() { self.list_state.selected = index; }
        }
        true
    }

    pub fn draw(&mut self, renderer: &mut Renderer, screen: Rect, theme: &Theme) {
        if !self.is_open { return; }
        let layout = primitives::ManagerLayout::new(screen);
        let c = chrome(theme);
        layout.draw(renderer, "Plugin Manager", theme);
        if layout.outer.w < 16 || layout.outer.h < 8 { return; }
        primitives::draw_text_input(renderer, layout.search, &self.search_query,
            "/ Search loaded plugins...", self.is_searching, theme);
        for (i, label) in LAZY_TABS.iter().enumerate().take(layout.nav.h as usize) {
            if layout.nav.w == 0 { break; }
            let active = i == self.active_tab;
            let bg = if active { c.row_selected_bg } else { c.panel_bg };
            let row = Rect::new(layout.nav.x, layout.nav.y + i as u16, layout.nav.w.saturating_sub(1), 1);
            renderer.fill_rect(row, bg);
            primitives::text_in(renderer, row, &format!("{}{}", i + 1, label), c.text_strong, bg, active);
        }
        let r = layout.content;
        primitives::text_in(renderer, Rect::new(r.x, r.y, r.w, 1), "Plugin runtime", c.text_strong, c.panel_bg, true);
        primitives::text_in(renderer, Rect::new(r.x, r.y + 1, r.w, 1),
            &format!("{} plugins · {}", self.filtered.len(), LAZY_TABS[self.active_tab].trim()), c.text_muted, c.panel_bg, false);
        let list_h = r.h.saturating_sub(3) as usize;
        self.list_state.adjust_scroll(list_h);
        if self.filtered.is_empty() && list_h > 0 {
            primitives::text_in(renderer, Rect::new(r.x, r.y + 3, r.w, 1),
                "No plugins match filter", c.text_muted, c.panel_bg, false);
        }
        for (i, &index) in self.filtered.iter().skip(self.list_state.scroll_offset).take(list_h).enumerate() {
            let p = &self.plugins[index];
            let selected = self.list_state.scroll_offset + i == self.list_state.selected;
            let bg = if selected { c.row_selected_bg } else { c.panel_bg };
            let row = Rect::new(r.x, r.y + 3 + i as u16, r.w, 1);
            renderer.fill_rect(row, bg);
            let status = if p.loaded { "loaded" } else { "lazy" };
            primitives::text_in(renderer, row, &format!("[{}] {}  {}", status, p.name, p.url), c.text_strong, bg, selected);
        }
        let hint = if self.status_msg.is_empty() { "Tab Category   ↑↓ Navigate   X Clean" } else { &self.status_msg };
        layout.draw_footer(renderer, hint, &["[ Sync ]", "[ Update ]", "[ Close ]"], theme);
    }

}

impl Default for LazyWidget {
    fn default() -> Self {
        Self::new()
    }
}
