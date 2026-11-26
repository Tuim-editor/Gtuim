use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::git_snapshot::GitSnapshot;
use crate::tui::input::InputEvent;
use crate::tui::layout::Rect;
use crate::tui::renderer::Renderer;
use crate::tui::theme::{chrome, Theme};
use crate::tui::widgets::primitives::{self, ListState};

#[derive(Clone, Debug)]
pub struct ExplorerEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
    pub expanded: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputMode {
    None,
    NewFile,
    NewFolder,
    Rename,
    ConfirmDelete,
}

pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut ia = a.chars().peekable();
    let mut ib = b.chars().peekable();
    loop {
        match (ia.peek(), ib.peek()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(&ca), Some(&cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let mut na = 0u64;
                    while let Some(&d) = ia.peek() {
                        if d.is_ascii_digit() {
                            na = na
                                .saturating_mul(10)
                                .saturating_add((d as u8 - b'0') as u64);
                            ia.next();
                        } else {
                            break;
                        }
                    }
                    let mut nb = 0u64;
                    while let Some(&d) = ib.peek() {
                        if d.is_ascii_digit() {
                            nb = nb
                                .saturating_mul(10)
                                .saturating_add((d as u8 - b'0') as u64);
                            ib.next();
                        } else {
                            break;
                        }
                    }
                    if na != nb {
                        return na.cmp(&nb);
                    }
                } else {
                    let la = ca.to_ascii_lowercase();
                    let lb = cb.to_ascii_lowercase();
                    if la != lb {
                        return la.cmp(&lb);
                    }
                    ia.next();
                    ib.next();
                }
            }
        }
    }
}

pub struct FileExplorer {
    pub root_dir: String,
    pub follow_cwd: bool,
    pub entries: Vec<ExplorerEntry>,
    pub expanded: HashSet<String>,
    pub list_state: ListState,
    pub git_badges: HashMap<String, char>,
    pub input_mode: InputMode,
    pub input_buffer: String,
    pub context_menu_open: bool,
    pub context_target: Option<String>,
    /// The initial highlighted row is not a user-selected creation destination.
    pub selection_explicit: bool,
    pub input_parent: Option<String>,
    pub action_target: Option<ExplorerEntry>,
    pub input_error: Option<String>,
    pub context_menu_x: u16,
    pub context_menu_y: u16,
    pub context_menu_idx: usize,
    pub open_file_requested: Option<String>,
    pub preview_file_requested: Option<String>,
    pub delete_buffer_requested: Option<String>,
    pub rename_buffer_requested: Option<(String, String)>,
}

impl FileExplorer {
    pub fn new() -> Self {
        let root_dir = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| ".".to_string());
        Self::new_with_mode(root_dir, true)
    }

    pub fn new_at(root_dir: String) -> Self {
        Self::new_with_mode(root_dir, false)
    }

    fn new_with_mode(root_dir: String, follow_cwd: bool) -> Self {
        let mut exp = Self {
            root_dir,
            follow_cwd,
            entries: Vec::new(),
            expanded: HashSet::new(),
            list_state: ListState::new(),
            git_badges: HashMap::new(),
            input_mode: InputMode::None,
            input_buffer: String::new(),
            context_menu_open: false,
            context_target: None,
            selection_explicit: false,
            input_parent: None,
            action_target: None,
            input_error: None,
            context_menu_x: 0,
            context_menu_y: 0,
            context_menu_idx: 0,
            open_file_requested: None,
            preview_file_requested: None,
            delete_buffer_requested: None,
            rename_buffer_requested: None,
        };
        exp.refresh();
        exp
    }

    pub fn refresh(&mut self) {
        let selected_path = self
            .selection_explicit
            .then(|| self.selected_entry().map(|e| e.path.clone()))
            .flatten();
        if self.follow_cwd {
            if let Ok(cwd) = std::env::current_dir() {
                if let Some(s) = cwd.to_str() {
                    self.root_dir = s.to_string();
                }
            }
        }
        self.entries.clear();
        let root = self.root_dir.clone();
        self.scan_dir(&root, 0);
        self.list_state.clamp(self.entries.len());
        if let Some(path) = selected_path {
            if let Some(idx) = self.entries.iter().position(|e| e.path == path) {
                self.list_state.selected = idx;
            } else {
                self.selection_explicit = false;
            }
        } else {
            self.selection_explicit = false;
        }
    }

    fn scan_dir(&mut self, dir: &str, depth: usize) {
        let Ok(rd) = fs::read_dir(dir) else {
            return;
        };
        let mut list: Vec<(String, String, bool)> = Vec::new();
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".git" || name == "target" || name == "zig-out" || name == ".zig-cache" {
                continue;
            }
            let path = entry.path().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            list.push((path, name, is_dir));
        }
        list.sort_by(|a, b| match (a.2, b.2) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => natural_cmp(&a.1, &b.1),
        });

        for (path, name, is_dir) in list {
            let expanded = is_dir && self.expanded.contains(&path);
            self.entries.push(ExplorerEntry {
                path: path.clone(),
                name,
                is_dir,
                depth,
                expanded,
            });
            if expanded {
                self.scan_dir(&path, depth + 1);
            }
        }
    }

    pub fn update_git_status(&mut self, snap: &GitSnapshot) {
        self.git_badges.clear();
        for f in &snap.files {
            let ch = f.status_badge(f.is_staged());
            self.git_badges.insert(f.path.clone(), ch);
        }
    }

    pub fn selected_entry(&self) -> Option<&ExplorerEntry> {
        self.entries.get(self.list_state.selected)
    }

    /// Returns true if moving `src_path` into `dest_dir` is structurally valid
    /// (changes directory and does not move a folder into itself or a descendant).
    pub fn can_move_path_to_dir(src_path: &str, dest_dir: &str) -> bool {
        let src = Path::new(src_path);
        let dest = Path::new(dest_dir);
        if src.file_name().is_none() || dest_dir.is_empty() {
            return false;
        }
        if src.parent() == Some(dest) {
            return false;
        }
        if dest.starts_with(src) {
            return false;
        }
        true
    }

    /// Resolves the destination directory when dropping onto an optional explorer entry path.
    /// Dropping onto a folder targets that folder; dropping onto a file targets its parent directory;
    /// dropping onto empty space (`None`) targets the explorer root directory.
    pub fn resolve_drop_dir(&self, target_entry_path: Option<&str>) -> String {
        let Some(target) = target_entry_path else {
            return self.root_dir.clone();
        };
        if let Some(entry) = self.entries.iter().find(|e| e.path == target) {
            if entry.is_dir {
                return entry.path.clone();
            }
            return Path::new(&entry.path)
                .parent()
                .and_then(|p| p.to_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| self.root_dir.clone());
        }
        let p = Path::new(target);
        if p.is_dir() {
            target.to_string()
        } else {
            p.parent()
                .and_then(|parent| parent.to_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_else(|| self.root_dir.clone())
        }
    }

    /// Moves a file or directory `src_path` into `dest_dir`.
    /// Returns `Ok(None)` for a same-directory no-op, `Ok(Some(new_path))` when moved,
    /// or `Err(message)` when the move is invalid or fails on disk.
    pub fn move_entry(&mut self, src_path: &str, dest_dir: &str) -> Result<Option<String>, String> {
        let src = Path::new(src_path);
        let dest_parent = Path::new(dest_dir);
        let Some(file_name) = src.file_name() else {
            return Err("Invalid source path".to_string());
        };
        if src.parent() == Some(dest_parent) {
            return Ok(None);
        }
        if dest_parent.starts_with(src) {
            return Err("Cannot move a folder into itself".to_string());
        }
        if !dest_parent.is_dir() {
            return Err("Destination directory does not exist".to_string());
        }

        let dest = dest_parent.join(file_name);
        if dest.exists() {
            return Err(format!(
                "'{}' already exists in destination",
                file_name.to_string_lossy()
            ));
        }

        fs::rename(src, &dest)
            .map_err(|e| format!("Failed to move '{}': {}", file_name.to_string_lossy(), e))?;

        let dest_str = dest.to_string_lossy().to_string();
        let remapped: HashSet<String> = self
            .expanded
            .iter()
            .map(|p| {
                let ep = Path::new(p);
                if ep == src {
                    dest_str.clone()
                } else if let Ok(rel) = ep.strip_prefix(src) {
                    dest.join(rel).to_string_lossy().to_string()
                } else {
                    p.clone()
                }
            })
            .collect();
        self.expanded = remapped;
        if dest_parent != Path::new(&self.root_dir) {
            self.expanded.insert(dest_dir.to_string());
        }

        self.rename_buffer_requested = Some((src_path.to_string(), dest_str.clone()));
        self.refresh();
        if let Some(pos) = self.entries.iter().position(|e| e.path == dest_str) {
            self.list_state.selected = pos;
        }
        Ok(Some(dest_str))
    }

    pub fn begin_creation(&mut self, mode: InputMode, parent: Option<String>) {
        self.input_mode = mode;
        self.input_parent = Some(parent.unwrap_or_else(|| {
            let target = self
                .selection_explicit
                .then(|| self.selected_entry())
                .flatten();
            self.resolve_drop_dir(target.map(|entry| entry.path.as_str()))
        }));
        self.input_buffer.clear();
        self.input_error = None;
        self.action_target = None;
    }

    pub fn begin_delete(&mut self) {
        self.action_target = self.selected_entry().cloned();
        if self.action_target.is_some() {
            self.input_mode = InputMode::ConfirmDelete;
            self.input_error = None;
        }
    }

    pub fn confirm_delete(&mut self) {
        let Some(target) = self.action_target.clone() else {
            return;
        };
        let result = if target.is_dir {
            fs::remove_dir_all(&target.path)
        } else {
            fs::remove_file(&target.path)
        };
        match result {
            Ok(()) => {
                self.delete_buffer_requested = Some(target.path.clone());
                self.selection_explicit = false;
                self.expanded
                    .retain(|p| !Path::new(p).starts_with(&target.path));
                self.input_mode = InputMode::None;
                self.action_target = None;
                self.input_error = None;
                self.refresh();
            }
            Err(e) => self.input_error = Some(format!("Could not delete '{}': {e}", target.name)),
        }
    }

    fn target_parent_dir(&self) -> PathBuf {
        PathBuf::from(self.input_parent.as_deref().unwrap_or(&self.root_dir))
    }

    fn reveal_path(&mut self, target: &Path) {
        for parent in target.ancestors().skip(1) {
            if parent == Path::new(&self.root_dir) || !parent.starts_with(&self.root_dir) {
                break;
            }
            self.expanded.insert(parent.to_string_lossy().into());
        }
        self.refresh();
        if let Some(idx) = self
            .entries
            .iter()
            .position(|entry| Path::new(&entry.path) == target)
        {
            self.list_state.selected = idx;
            self.selection_explicit = true;
        }
    }

    fn execute_input_action(&mut self) {
        let name = self.input_buffer.trim().to_string();
        let mode = self.input_mode;
        self.input_error = None;
        if name.is_empty() {
            self.input_error = Some("Enter a name".into());
            return;
        }
        if Path::new(&name)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            self.input_error = Some("Use a relative name without '.' or '..' components".into());
            return;
        }
        match mode {
            InputMode::NewFile => {
                let target = self.target_parent_dir().join(&name);
                let result = (|| -> std::io::Result<()> {
                    if let Some(p) = target.parent() {
                        fs::create_dir_all(p)?;
                    }
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&target)?;
                    Ok(())
                })();
                match result {
                    Ok(()) => {
                        self.open_file_requested = Some(target.to_string_lossy().to_string());
                        self.reveal_path(&target);
                    }
                    Err(e) => {
                        self.input_error = Some(format!("Could not create file: {e}"));
                        return;
                    }
                }
            }
            InputMode::NewFolder => {
                let target = self.target_parent_dir().join(&name);
                if target.symlink_metadata().is_ok() {
                    self.input_error = Some("That name already exists".into());
                    return;
                }
                if let Err(e) = fs::create_dir_all(&target) {
                    self.input_error = Some(format!("Could not create folder: {e}"));
                    return;
                }
                self.reveal_path(&target);
            }
            InputMode::Rename => {
                if name.contains('/') {
                    self.input_error =
                        Some("Rename accepts a single name; use drag-and-drop to move".into());
                    return;
                }
                if let Some(sel) = self
                    .action_target
                    .clone()
                    .or_else(|| self.selected_entry().cloned())
                {
                    let old_p = Path::new(&sel.path);
                    if let Some(parent) = old_p.parent() {
                        let new_p = parent.join(&name);
                        if new_p != old_p && new_p.symlink_metadata().is_ok() {
                            self.input_error = Some("That name already exists".into());
                            return;
                        }
                        if let Err(e) = fs::rename(old_p, &new_p) {
                            self.input_error = Some(format!("Could not rename: {e}"));
                            return;
                        } else {
                            self.expanded = self
                                .expanded
                                .iter()
                                .map(|path| {
                                    Path::new(path)
                                        .strip_prefix(old_p)
                                        .map(|relative| {
                                            new_p.join(relative).to_string_lossy().into_owned()
                                        })
                                        .unwrap_or_else(|_| path.clone())
                                })
                                .collect();
                            self.rename_buffer_requested =
                                Some((sel.path, new_p.to_string_lossy().to_string()));
                            self.reveal_path(&new_p);
                        }
                    }
                }
            }
            _ => {}
        }
        self.input_mode = InputMode::None;
        self.input_buffer.clear();
        self.input_parent = None;
        self.action_target = None;
    }

    pub fn handle_key(&mut self, ev: &InputEvent) -> bool {
        if self.context_menu_open {
            if let InputEvent::Key(k) = ev {
                match k.as_str() {
                    "<Esc>" => self.context_menu_open = false,
                    "<Up>" | "k" => {
                        self.context_menu_idx = self.context_menu_idx.saturating_sub(1);
                    }
                    "<Down>" | "j" => {
                        self.context_menu_idx =
                            (self.context_menu_idx + 1).min(self.context_items().len() - 1);
                    }
                    "<CR>" => {
                        self.context_menu_open = false;
                        self.trigger_context_action(self.context_menu_idx);
                    }
                    _ => {}
                }
                return true;
            }
        }

        if self.input_mode == InputMode::ConfirmDelete {
            if let InputEvent::Key(k) = ev {
                match k.as_str() {
                    "y" | "Y" => self.confirm_delete(),
                    "n" | "N" | "<Esc>" | "<CR>" => {
                        self.input_mode = InputMode::None;
                        self.action_target = None;
                        self.input_error = None;
                    }
                    _ => {}
                }
                return true;
            }
        }

        if self.input_mode != InputMode::None {
            match ev {
                InputEvent::Paste(text) => {
                    for ch in text.chars() {
                        if ch != '\n' && ch != '\r' {
                            self.input_buffer.push(ch);
                        }
                    }
                    return true;
                }
                InputEvent::Key(k) => {
                    match k.as_str() {
                        "<Esc>" => {
                            self.input_mode = InputMode::None;
                            self.input_buffer.clear();
                            self.input_parent = None;
                            self.action_target = None;
                            self.input_error = None;
                        }
                        "<CR>" => self.execute_input_action(),
                        "<BS>" => {
                            self.input_buffer.pop();
                        }
                        s if s.chars().count() == 1 && !s.starts_with('<') => {
                            self.input_buffer.push_str(s);
                        }
                        _ => {}
                    }
                    return true;
                }
                _ => return false,
            }
        }

        let InputEvent::Key(k) = ev else {
            return false;
        };

        if matches!(
            k.as_str(),
            "j" | "<Down>"
                | "k"
                | "<Up>"
                | "<Home>"
                | "g"
                | "<End>"
                | "G"
                | "<CR>"
                | "o"
                | "l"
                | "<Right>"
                | "h"
                | "<Left>"
        ) {
            self.selection_explicit = true;
        }
        match k.as_str() {
            "j" | "<Down>" => {
                self.list_state.move_down(self.entries.len(), 1);
                if let Some(sel) = self.selected_entry() {
                    if !sel.is_dir {
                        self.preview_file_requested = Some(sel.path.clone());
                    }
                }
                true
            }
            "k" | "<Up>" => {
                self.list_state.move_up(1);
                if let Some(sel) = self.selected_entry() {
                    if !sel.is_dir {
                        self.preview_file_requested = Some(sel.path.clone());
                    }
                }
                true
            }
            "<Home>" | "g" => {
                self.list_state.move_home();
                true
            }
            "<End>" | "G" => {
                self.list_state.move_end(self.entries.len());
                true
            }
            "<CR>" | "o" | "l" | "<Right>" => {
                if let Some(sel) = self.selected_entry().cloned() {
                    if sel.is_dir {
                        if self.expanded.contains(&sel.path) {
                            self.expanded.remove(&sel.path);
                        } else {
                            self.expanded.insert(sel.path);
                        }
                        self.refresh();
                    } else {
                        self.open_file_requested = Some(sel.path);
                    }
                }
                true
            }
            "h" | "<Left>" => {
                if let Some(sel) = self.selected_entry().cloned() {
                    if sel.is_dir && self.expanded.contains(&sel.path) {
                        self.expanded.remove(&sel.path);
                        self.refresh();
                    }
                }
                true
            }
            "a" | "n" | "c" => {
                self.begin_creation(InputMode::NewFile, None);
                true
            }
            "A" | "N" => {
                self.begin_creation(InputMode::NewFolder, None);
                true
            }
            "r" => {
                if let Some(sel) = self.selected_entry().cloned() {
                    self.action_target = Some(sel.clone());
                    self.input_error = None;
                    self.input_mode = InputMode::Rename;
                    self.input_buffer = sel.name;
                }
                true
            }
            "d" | "x" | "<Del>" => {
                self.begin_delete();
                true
            }
            "R" => {
                self.refresh();
                true
            }
            _ => false,
        }
    }

    pub fn context_items(&self) -> &'static [&'static str] {
        if self.context_target.is_some() {
            &["New File", "New Folder", "Rename", "Delete"]
        } else {
            &["New File", "New Folder"]
        }
    }

    pub fn trigger_context_action(&mut self, idx: usize) {
        // Resolve destructive actions from the right-clicked path, not a selection
        // index which may have shifted after a refresh while the menu was open.
        if idx >= 2 {
            let Some(pos) = self
                .context_target
                .as_ref()
                .and_then(|target| self.entries.iter().position(|entry| &entry.path == target))
            else {
                return;
            };
            self.list_state.selected = pos;
        }
        match idx {
            0 | 1 => {
                let parent = self.resolve_drop_dir(self.context_target.as_deref());
                self.begin_creation(
                    if idx == 0 {
                        InputMode::NewFile
                    } else {
                        InputMode::NewFolder
                    },
                    Some(parent),
                );
            }
            2 => {
                if self.context_target.is_none() {
                    return;
                }
                if let Some(sel) = self.selected_entry().cloned() {
                    self.action_target = Some(sel.clone());
                    self.input_error = None;
                    self.input_mode = InputMode::Rename;
                    self.input_buffer = sel.name;
                }
            }
            3 => {
                if self.context_target.is_some() {
                    self.begin_delete();
                }
            }
            _ => {}
        }
    }

    pub fn handle_mouse(
        &mut self,
        rect: Rect,
        mx: u16,
        my: u16,
        button: u8,
        pressed: bool,
    ) -> bool {
        if !pressed {
            return false;
        }
        if button == 64 {
            self.list_state.move_up(1);
            return true;
        }
        if button == 65 {
            self.list_state.move_down(self.entries.len(), 1);
            return true;
        }

        if self.context_menu_open {
            let count = self.context_items().len() as u16;
            let menu_r = Rect::new(self.context_menu_x, self.context_menu_y, 16, count + 2);
            if menu_r.contains(mx, my) && my > menu_r.y && my < menu_r.y + count + 1 {
                let act = (my - menu_r.y - 1) as usize;
                self.context_menu_open = false;
                self.trigger_context_action(act);
                return true;
            }
            self.context_menu_open = false;
            return true;
        }

        if !rect.contains(mx, my) {
            return false;
        }

        let rel_y = my - rect.y;
        let rel_x = mx - rect.x;
        if rel_y == 0 {
            if rel_x >= rect.w.saturating_sub(9) && rel_x < rect.w.saturating_sub(6) {
                self.begin_creation(InputMode::NewFile, None);
            } else if rel_x >= rect.w.saturating_sub(6) && rel_x < rect.w.saturating_sub(3) {
                self.begin_creation(InputMode::NewFolder, None);
            } else if rel_x >= rect.w.saturating_sub(3) {
                self.refresh();
            }
            return true;
        }

        let list_start_y = if self.input_mode != InputMode::None {
            rect.y + 2 + u16::from(self.input_error.is_some())
        } else {
            rect.y + 1
        };

        if my >= list_start_y {
            let idx = self.list_state.scroll_offset + (my - list_start_y) as usize;
            if idx < self.entries.len() {
                self.list_state.selected = idx;
                self.selection_explicit = true;
                if button == 2 {
                    self.context_menu_open = true;
                    self.context_target = Some(self.entries[idx].path.clone());
                    self.context_menu_x = mx;
                    self.context_menu_y = my;
                    self.context_menu_idx = 0;
                } else if button == 0 {
                    let sel = self.entries[idx].clone();
                    if sel.is_dir {
                        if self.expanded.contains(&sel.path) {
                            self.expanded.remove(&sel.path);
                        } else {
                            self.expanded.insert(sel.path);
                        }
                        self.refresh();
                    } else {
                        self.open_file_requested = Some(sel.path);
                    }
                }
            } else if button == 2 {
                self.selection_explicit = false;
                self.context_target = None;
                self.context_menu_open = true;
                self.context_menu_x = mx;
                self.context_menu_y = my;
                self.context_menu_idx = 0;
            } else if button == 0 {
                self.selection_explicit = false;
            }
        }
        true
    }

    pub fn draw(&mut self, renderer: &mut Renderer, rect: Rect, theme: &Theme, focused: bool) {
        if rect.w == 0 || rect.h == 0 {
            return;
        }
        let c = chrome(theme);
        renderer.fill_rect(rect, c.panel_bg);

        // Header: EXPLORER  +F +D ↻
        let hdr_max = if rect.w >= 16 {
            rect.w.saturating_sub(10) as usize
        } else {
            rect.w.saturating_sub(2) as usize
        };
        let hdr = primitives::truncate_to_width("EXPLORER", hdr_max);
        renderer.draw_text_styled(
            rect.x + 1,
            rect.y,
            &hdr,
            c.text_muted,
            c.panel_bg,
            true,
            false,
        );
        if rect.w >= 16 {
            renderer.draw_text(
                rect.x + rect.w.saturating_sub(9),
                rect.y,
                "+F +D ↻",
                c.text_muted,
                c.panel_bg,
            );
        }

        let mut list_y = rect.y + 1;
        if self.input_mode != InputMode::None && list_y < rect.y + rect.h {
            let prompt = match self.input_mode {
                InputMode::NewFile => format!("New file: {}▏", self.input_buffer),
                InputMode::NewFolder => format!("New dir: {}▏", self.input_buffer),
                InputMode::Rename => format!("Rename: {}▏", self.input_buffer),
                InputMode::ConfirmDelete => format!(
                    "Permanently delete '{}'? [y/N]",
                    self.action_target
                        .as_ref()
                        .map(|e| e.name.as_str())
                        .unwrap_or("item")
                ),
                InputMode::None => String::new(),
            };
            renderer.fill_rect(Rect::new(rect.x, list_y, rect.w, 1), c.elevated_bg);
            let trunc = primitives::truncate_to_width(&prompt, rect.w.saturating_sub(2) as usize);
            renderer.draw_text_styled(
                rect.x + 1,
                list_y,
                &trunc,
                c.accent,
                c.elevated_bg,
                true,
                false,
            );
            list_y += 1;
        }

        if self.input_mode != InputMode::None && list_y < rect.y + rect.h {
            if let Some(error) = &self.input_error {
                let text = primitives::truncate_to_width(error, rect.w.saturating_sub(2) as usize);
                renderer.draw_text(rect.x + 1, list_y, &text, c.accent, c.elevated_bg);
                list_y += 1;
            }
        }
        let view_h = (rect.y + rect.h).saturating_sub(list_y) as usize;
        self.list_state.adjust_scroll(view_h);

        for i in 0..view_h {
            let idx = self.list_state.scroll_offset + i;
            if idx >= self.entries.len() {
                break;
            }
            let item = &self.entries[idx];
            let ry = list_y + i as u16;
            let selected = self.selection_explicit && idx == self.list_state.selected;
            let bg = if selected && focused {
                c.row_selected_bg
            } else {
                c.panel_bg
            };
            renderer.fill_rect(Rect::new(rect.x, ry, rect.w, 1), bg);

            let indent = "  ".repeat(item.depth.min(6));
            let prefix = if item.is_dir {
                if item.expanded {
                    "▾ "
                } else {
                    "▸ "
                }
            } else {
                "  "
            };
            let line = format!(" {}{}{}", indent, prefix, item.name);
            let trunc = primitives::truncate_to_width(&line, rect.w.saturating_sub(3) as usize);
            let fg = if item.is_dir {
                c.accent
            } else if selected && focused {
                c.text_strong
            } else {
                c.text
            };
            renderer.draw_text_styled(rect.x, ry, &trunc, fg, bg, item.is_dir || selected, false);

            // Git badge on right edge
            let rel_path = item
                .path
                .strip_prefix(&self.root_dir)
                .map(|s| s.trim_start_matches('/'))
                .unwrap_or(&item.path);
            if let Some(&badge) = self.git_badges.get(rel_path) {
                let b_fg = match badge {
                    'M' => c.warning,
                    'A' => c.success,
                    'D' => c.error,
                    _ => c.text_muted,
                };
                renderer.draw_text_styled(
                    rect.x + rect.w.saturating_sub(2),
                    ry,
                    &badge.to_string(),
                    b_fg,
                    bg,
                    true,
                    false,
                );
            }
        }
    }

    pub fn draw_overlay(&self, renderer: &mut Renderer, _sidebar_rect: Rect, theme: &Theme) {
        if !self.context_menu_open {
            return;
        }
        let c = chrome(theme);
        let items = self.context_items();
        let r = Rect::new(
            self.context_menu_x,
            self.context_menu_y,
            16,
            items.len() as u16 + 2,
        );
        primitives::draw_modal_frame(renderer, r, "", theme);
        for (i, label) in items.iter().enumerate() {
            let ry = r.y + 1 + i as u16;
            let sel = i == self.context_menu_idx;
            let bg = if sel { c.row_selected_bg } else { c.panel_bg };
            renderer.fill_rect(Rect::new(r.x + 1, ry, r.w - 2, 1), bg);
            renderer.draw_text_styled(r.x + 2, ry, label, c.text_strong, bg, sel, false);
        }
    }
}

impl Default for FileExplorer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "gtuim-explorer-test-{}-{}-{}",
            tag,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn context_actions_pin_clicked_path_and_rename_preserves_folder_expansion() {
        let root = make_temp_dir("context-target");
        let folder = root.join("folder");
        let child = folder.join("child");
        fs::create_dir_all(&child).unwrap();
        fs::write(root.join("unrelated.txt"), "keep").unwrap();
        let mut exp = FileExplorer::new_at(root.to_string_lossy().into());
        exp.expanded.insert(folder.to_string_lossy().into());
        exp.expanded.insert(child.to_string_lossy().into());
        exp.refresh();
        exp.context_target = Some(folder.to_string_lossy().into());
        exp.list_state.selected = exp.entries.len() - 1;
        exp.trigger_context_action(2);
        assert_eq!(exp.input_buffer, "folder");
        exp.input_buffer = "child/invalid".into();
        exp.execute_input_action();
        assert!(exp.input_error.is_some());
        exp.input_buffer = "renamed".into();
        exp.execute_input_action();
        assert!(root.join("renamed/child").is_dir());
        assert!(exp
            .expanded
            .contains(root.join("renamed/child").to_str().unwrap()));
        assert!(!exp.expanded.contains(child.to_str().unwrap()));
        assert_eq!(exp.selected_entry().unwrap().name, "renamed");
        exp.context_target = Some(root.join("unrelated.txt").to_string_lossy().into());
        exp.list_state.selected = 0;
        exp.trigger_context_action(3);
        assert_eq!(exp.action_target.as_ref().unwrap().name, "unrelated.txt");
        exp.context_target = Some(root.join("missing.txt").to_string_lossy().into());
        exp.input_mode = InputMode::None;
        exp.trigger_context_action(3);
        assert_eq!(exp.input_mode, InputMode::None);
        exp.context_target = None;
        exp.context_menu_open = true;
        assert_eq!(exp.context_items().len(), 2);
        for _ in 0..8 {
            exp.handle_key(&InputEvent::Key("<Down>".into()));
        }
        assert_eq!(exp.context_menu_idx, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_selection_creates_in_folder_or_beside_file() {
        let root = make_temp_dir("selected-creation");
        let folder = root.join("folder");
        fs::create_dir(&folder).unwrap();
        let mut exp = FileExplorer::new_at(root.to_string_lossy().into());
        // An initial automatic highlight must not redirect creation into folder.
        exp.begin_creation(InputMode::NewFile, None);
        assert_eq!(exp.input_parent.as_deref(), root.to_str());
        exp.input_mode = InputMode::None;
        exp.handle_key(&InputEvent::Key("<Right>".into()));
        exp.handle_key(&InputEvent::Key("a".into()));
        exp.input_buffer = "inside.txt".into();
        exp.execute_input_action();
        assert!(folder.join("inside.txt").exists());
        assert_eq!(exp.selected_entry().unwrap().name, "inside.txt");
        exp.handle_key(&InputEvent::Key("A".into()));
        exp.input_buffer = "beside-file".into();
        exp.execute_input_action();
        assert!(folder.join("beside-file").is_dir());
        fs::create_dir(root.join("aaa-new-first-row")).unwrap();
        exp.refresh();
        assert_eq!(exp.selected_entry().unwrap().name, "beside-file");
        exp.begin_delete();
        exp.confirm_delete();
        assert!(!exp.selection_explicit);
        exp.begin_creation(InputMode::NewFile, None);
        assert_eq!(exp.input_parent.as_deref(), root.to_str());
        exp.selection_explicit = false;
        exp.begin_creation(InputMode::NewFile, None);
        assert_eq!(exp.input_parent.as_deref(), root.to_str());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn creation_defaults_to_root_and_context_creation_uses_explicit_parent() {
        let root = make_temp_dir("create");
        let folder = root.join("first-folder");
        fs::create_dir(&folder).unwrap();
        let mut exp = FileExplorer::new_at(root.to_string_lossy().into());
        exp.handle_key(&InputEvent::Key("a".into()));
        exp.input_buffer = "root.txt".into();
        exp.execute_input_action();
        assert!(root.join("root.txt").exists());
        assert!(!folder.join("root.txt").exists());
        exp.context_target = Some(folder.to_string_lossy().into());
        exp.trigger_context_action(0);
        exp.input_buffer = "nested.txt".into();
        exp.execute_input_action();
        assert!(folder.join("nested.txt").exists());
        assert_eq!(exp.selected_entry().unwrap().name, "nested.txt");
        assert!(exp.expanded.contains(folder.to_str().unwrap()));
        exp.context_target = None;
        exp.trigger_context_action(1);
        exp.input_buffer = "root-folder".into();
        exp.execute_input_action();
        assert!(root.join("root-folder").is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn creation_and_rename_never_overwrite_existing_files() {
        let root = make_temp_dir("collisions");
        fs::write(root.join("keep.txt"), "precious").unwrap();
        let mut exp = FileExplorer::new_at(root.to_string_lossy().into());
        exp.begin_creation(InputMode::NewFile, None);
        exp.input_buffer = "keep.txt".into();
        exp.execute_input_action();
        assert_eq!(exp.input_mode, InputMode::NewFile);
        assert!(exp.input_error.is_some());
        assert_eq!(
            fs::read_to_string(root.join("keep.txt")).unwrap(),
            "precious"
        );
        exp.input_buffer = "../outside.txt".into();
        exp.execute_input_action();
        assert!(exp.input_error.is_some());
        fs::write(root.join("other.txt"), "other").unwrap();
        exp.refresh();
        exp.list_state.selected = exp
            .entries
            .iter()
            .position(|e| e.name == "other.txt")
            .unwrap();
        exp.handle_key(&InputEvent::Key("r".into()));
        exp.input_buffer = "keep.txt".into();
        exp.execute_input_action();
        assert!(exp.input_error.is_some());
        assert_eq!(
            fs::read_to_string(root.join("keep.txt")).unwrap(),
            "precious"
        );
        assert!(root.join("other.txt").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deletion_defaults_to_no_and_pins_target_and_reports_failures() {
        let root = make_temp_dir("delete");
        fs::write(root.join("a.txt"), "a").unwrap();
        fs::write(root.join("b.txt"), "b").unwrap();
        let mut exp = FileExplorer::new_at(root.to_string_lossy().into());
        exp.begin_delete();
        exp.handle_key(&InputEvent::Key("<CR>".into()));
        assert!(root.join("a.txt").exists());
        exp.begin_delete();
        exp.list_state.selected = 1;
        exp.handle_key(&InputEvent::Key("y".into()));
        assert!(!root.join("a.txt").exists());
        assert!(root.join("b.txt").exists());
        exp.delete_buffer_requested = None;
        exp.begin_delete();
        fs::remove_file(root.join("b.txt")).unwrap();
        exp.confirm_delete();
        assert!(exp.input_error.is_some());
        assert!(exp.delete_buffer_requested.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn moves_file_out_of_parent_and_between_folders_safely() {
        let root = make_temp_dir("move-file");
        let dir_a = root.join("alpha");
        let dir_b = root.join("beta");
        fs::create_dir_all(&dir_a).unwrap();
        fs::create_dir_all(&dir_b).unwrap();
        let file_a = dir_a.join("notes.txt");
        let file_b = dir_b.join("existing.txt");
        let root_file = root.join("README.md");
        fs::write(&file_a, "hello").unwrap();
        fs::write(&file_b, "keep").unwrap();
        fs::write(&root_file, "readme").unwrap();

        let mut exp = FileExplorer::new_at(root.to_string_lossy().to_string());
        exp.expanded.insert(dir_a.to_string_lossy().to_string());
        exp.refresh();

        // Same-directory drop is a clean no-op.
        assert!(!FileExplorer::can_move_path_to_dir(
            file_a.to_str().unwrap(),
            dir_a.to_str().unwrap()
        ));
        assert_eq!(
            exp.move_entry(file_a.to_str().unwrap(), dir_a.to_str().unwrap()),
            Ok(None)
        );

        // Dropping onto a root file resolves to root_dir and moves the file out of alpha/.
        let resolved_root = exp.resolve_drop_dir(Some(root_file.to_str().unwrap()));
        assert_eq!(Path::new(&resolved_root), root.as_path());
        let moved_to_root = exp
            .move_entry(file_a.to_str().unwrap(), &resolved_root)
            .unwrap()
            .expect("should move to root");
        assert_eq!(Path::new(&moved_to_root), root.join("notes.txt"));
        assert!(!file_a.exists());
        assert!(root.join("notes.txt").exists());
        assert_eq!(
            exp.rename_buffer_requested.take(),
            Some((
                file_a.to_string_lossy().to_string(),
                root.join("notes.txt").to_string_lossy().to_string()
            ))
        );

        // Dropping onto beta/ moves notes.txt into beta/ and expands beta/.
        let resolved_b = exp.resolve_drop_dir(Some(dir_b.to_str().unwrap()));
        assert_eq!(Path::new(&resolved_b), dir_b.as_path());
        let moved_to_b = exp
            .move_entry(&moved_to_root, &resolved_b)
            .unwrap()
            .expect("should move to beta");
        assert_eq!(Path::new(&moved_to_b), dir_b.join("notes.txt"));
        assert!(dir_b.join("notes.txt").exists());
        assert!(exp.expanded.contains(dir_b.to_str().unwrap()));
        assert_eq!(
            exp.selected_entry().map(|e| e.path.as_str()),
            Some(moved_to_b.as_str())
        );

        // Name collision refuses to overwrite existing file.
        let collide = root.join("existing.txt");
        fs::write(&collide, "new").unwrap();
        let err = exp
            .move_entry(collide.to_str().unwrap(), dir_b.to_str().unwrap())
            .unwrap_err();
        assert!(err.contains("already exists"), "unexpected err: {err}");
        assert_eq!(fs::read_to_string(&file_b).unwrap(), "keep");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn moves_folder_remaps_expanded_and_rejects_descendant_drop() {
        let root = make_temp_dir("move-dir");
        let parent = root.join("parent");
        let child = parent.join("child");
        let dest = root.join("dest");
        fs::create_dir_all(&child).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(child.join("leaf.rs"), "fn main() {}").unwrap();

        let mut exp = FileExplorer::new_at(root.to_string_lossy().to_string());
        exp.expanded.insert(parent.to_string_lossy().to_string());
        exp.expanded.insert(child.to_string_lossy().to_string());
        exp.refresh();

        // Moving folder into itself or descendant is rejected.
        assert!(!FileExplorer::can_move_path_to_dir(
            parent.to_str().unwrap(),
            parent.to_str().unwrap()
        ));
        assert!(!FileExplorer::can_move_path_to_dir(
            parent.to_str().unwrap(),
            child.to_str().unwrap()
        ));
        assert!(exp
            .move_entry(parent.to_str().unwrap(), child.to_str().unwrap())
            .is_err());

        // Moving parent/ into dest/ remaps expanded paths for both parent and child.
        let moved = exp
            .move_entry(parent.to_str().unwrap(), dest.to_str().unwrap())
            .unwrap()
            .expect("should move folder");
        let new_parent = dest.join("parent");
        let new_child = new_parent.join("child");
        assert_eq!(Path::new(&moved), new_parent.as_path());
        assert!(new_child.join("leaf.rs").exists());
        assert!(exp.expanded.contains(new_parent.to_str().unwrap()));
        assert!(exp.expanded.contains(new_child.to_str().unwrap()));

        let _ = fs::remove_dir_all(&root);
    }
}
