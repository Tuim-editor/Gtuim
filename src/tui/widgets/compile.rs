use std::collections::HashMap;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use crate::tui::input::InputEvent;

const MAX_OUTPUT_LINES: usize = 2_500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileStage {
    Ready,
    Running,
    Succeeded,
    Failed(i32),
    Stopped,
    StartError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiColor {
    Default,
    Muted,
    Error,
    Success,
    Warning,
    Accent,
    Cyan,
    Magenta,
    Rgb(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnsiSpan {
    pub text: String,
    pub color: AnsiColor,
    pub bold: bool,
    pub dim: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileLine {
    pub spans: Vec<AnsiSpan>,
    pub plain: String,
}

struct RunningJob {
    child: Child,
    pgid: i32,
    rx: Receiver<Vec<u8>>,
    stream_eof: bool,
}

impl RunningJob {
    fn terminate(&mut self) {
        if self.pgid > 1 {
            unsafe {
                let _ = libc::kill(-self.pgid, libc::SIGTERM);
            }
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            if self.pgid > 1 {
                unsafe {
                    let _ = libc::kill(-self.pgid, libc::SIGKILL);
                }
            }
        }
        let _ = self.child.wait();
    }
}

impl Drop for RunningJob {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub struct CompileWidget {
    pub is_open: bool,
    pub stage: CompileStage,
    pub cwd: String,
    pub project_key: String,
    pub command: String,
    pub default_command: String,
    pub cursor: usize,
    pub lines: Vec<CompileLine>,
    pub output_version: u64,
    pub selected_diagnostic: usize,
    pub visited_diagnostic: Option<usize>,
    pub pending_g: bool,
    pub dock_requested: bool,
    pub error_message: String,
    pub home_dir: String,
    custom_commands: HashMap<String, String>,
    started_at: Option<Instant>,
    finished_elapsed: Duration,
    partial_line: String,
    job: Option<RunningJob>,
}

impl CompileWidget {
    pub fn new(home_dir: &str) -> Self {
        Self {
            is_open: false,
            stage: CompileStage::Ready,
            cwd: String::new(),
            project_key: String::new(),
            command: String::new(),
            default_command: String::new(),
            cursor: 0,
            lines: Vec::new(),
            output_version: 0,
            selected_diagnostic: 0,
            visited_diagnostic: None,
            pending_g: false,
            dock_requested: false,
            error_message: String::new(),
            home_dir: home_dir.to_string(),
            custom_commands: HashMap::new(),
            started_at: None,
            finished_elapsed: Duration::ZERO,
            partial_line: String::new(),
            job: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.stage == CompileStage::Running && self.job.is_some()
    }

    pub fn elapsed(&self) -> Duration {
        if self.is_running() {
            self.started_at
                .map(|t| t.elapsed())
                .unwrap_or(self.finished_elapsed)
        } else {
            self.finished_elapsed
        }
    }

    pub fn display_cwd(&self) -> String {
        if !self.home_dir.is_empty() && self.cwd.starts_with(&self.home_dir) {
            let rest = &self.cwd[self.home_dir.len()..];
            if rest.is_empty() || rest.starts_with('/') {
                return format!("~{}", rest);
            }
        }
        self.cwd.clone()
    }

    pub fn open_with(
        &mut self,
        cwd: String,
        command: String,
        default_command: String,
        key: String,
    ) {
        let same_project = !key.is_empty() && key == self.project_key && cwd == self.cwd;
        self.is_open = true;
        if self.is_running() {
            return;
        }
        self.cwd = cwd;
        self.project_key = key.clone();
        self.default_command = default_command;
        self.command = self
            .custom_commands
            .get(&key)
            .cloned()
            .unwrap_or(command);
        self.cursor = self.command.chars().count();
        if !same_project {
            self.dock_requested = false;
            self.selected_diagnostic = 0;
            self.visited_diagnostic = None;
            self.pending_g = false;
            self.lines.clear();
            self.partial_line.clear();
            self.error_message.clear();
            self.stage = CompileStage::Ready;
            self.finished_elapsed = Duration::ZERO;
            self.output_version = self.output_version.wrapping_add(1);
        }
    }

    pub fn close(&mut self) {
        // Dismissing a view must not terminate its build. Stop/Ctrl+C is explicit.
        self.is_open = false;
    }

    pub fn reset_to_default(&mut self) {
        if self.default_command.is_empty() || self.is_running() {
            return;
        }
        self.command = self.default_command.clone();
        self.cursor = self.command.chars().count();
        self.custom_commands.remove(&self.project_key);
    }

    pub fn clear_output(&mut self) {
        if self.is_running() {
            return;
        }
        self.selected_diagnostic = 0;
        self.visited_diagnostic = None;
        self.pending_g = false;
        self.lines.clear();
        self.partial_line.clear();
        self.error_message.clear();
        self.stage = CompileStage::Ready;
        self.finished_elapsed = Duration::ZERO;
        self.output_version = self.output_version.wrapping_add(1);
    }

    pub fn plain_output(&self) -> String {
        let mut out = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&line.plain);
        }
        out
    }

    pub fn start(&mut self) {
        self.dock_requested = false;
        self.selected_diagnostic = 0;
        self.visited_diagnostic = None;
        self.pending_g = false;
        let trimmed = self.command.trim().to_string();
        if trimmed.is_empty() {
            self.error_message = "Enter a compile command before running.".to_string();
            self.stage = CompileStage::StartError;
            return;
        }

        if self.job.is_some() {
            self.stop();
        }

        if !self.project_key.is_empty() {
            if trimmed != self.default_command.trim() {
                self.custom_commands
                    .insert(self.project_key.clone(), trimmed.clone());
            } else {
                self.custom_commands.remove(&self.project_key);
            }
        }

        self.lines.clear();
        self.partial_line.clear();
        self.error_message.clear();
        self.finished_elapsed = Duration::ZERO;
        self.started_at = Some(Instant::now());
        self.output_version = self.output_version.wrapping_add(1);

        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg(&trimmed)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("CLICOLOR_FORCE", "1")
            .env("CARGO_TERM_COLOR", "always")
            .env("FORCE_COLOR", "1")
            .env("TERM", "xterm-256color");

        if !self.cwd.is_empty() && std::path::Path::new(&self.cwd).is_dir() {
            cmd.current_dir(&self.cwd);
        }

        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }

        match cmd.spawn() {
            Ok(mut child) => {
                let pgid = child.id() as i32;
                let (tx, rx) = mpsc::channel::<Vec<u8>>();
                if let Some(mut stdout) = child.stdout.take() {
                    let tx_out = tx.clone();
                    thread::spawn(move || {
                        let mut buf = [0u8; 4096];
                        while let Ok(n) = stdout.read(&mut buf) {
                            if n == 0 {
                                break;
                            }
                            if tx_out.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                    });
                }
                if let Some(mut stderr) = child.stderr.take() {
                    thread::spawn(move || {
                        let mut buf = [0u8; 4096];
                        while let Ok(n) = stderr.read(&mut buf) {
                            if n == 0 {
                                break;
                            }
                            if tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                    });
                }
                self.job = Some(RunningJob {
                    child,
                    pgid,
                    rx,
                    stream_eof: false,
                });
                self.stage = CompileStage::Running;
                self.dock_requested = true;
            }
            Err(err) => {
                self.error_message = format!("Could not start command: {err}");
                self.lines.push(parse_ansi_line(&self.error_message));
                self.stage = CompileStage::StartError;
                self.started_at = None;
                self.output_version = self.output_version.wrapping_add(1);
            }
        }
    }

    pub fn stop(&mut self) {
        self.dock_requested = false;
        if let Some(mut job) = self.job.take() {
            job.terminate();
            if let Some(started) = self.started_at.take() {
                self.finished_elapsed = started.elapsed();
            }
            self.flush_partial_line();
            self.stage = CompileStage::Stopped;
            self.output_version = self.output_version.wrapping_add(1);
        }
    }

    pub fn poll(&mut self) -> bool {
        let mut chunks = Vec::new();
        let mut disconnected = false;
        let mut exit_status = None;

        if let Some(job) = self.job.as_mut() {
            loop {
                match job.rx.try_recv() {
                    Ok(bytes) => chunks.push(bytes),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        job.stream_eof = true;
                        disconnected = true;
                        break;
                    }
                }
            }
            if job.stream_eof {
                match job.child.try_wait() {
                    Ok(Some(status)) => exit_status = Some(status.code().unwrap_or(-1)),
                    Ok(None) => {}
                    Err(_) => exit_status = Some(-1),
                }
            }
        } else {
            return false;
        }

        let mut changed = !chunks.is_empty();
        for bytes in chunks {
            self.ingest_bytes(&bytes);
        }

        if let Some(code) = exit_status {
            self.job = None;
            self.flush_partial_line();
            if let Some(started) = self.started_at.take() {
                self.finished_elapsed = started.elapsed();
            }
            self.stage = if code == 0 {
                CompileStage::Succeeded
            } else {
                CompileStage::Failed(code)
            };
            self.output_version = self.output_version.wrapping_add(1);
            changed = true;
        } else if disconnected {
            changed = true;
        }

        if changed {
            self.output_version = self.output_version.wrapping_add(1);
        }
        // Keep elapsed timer ticking smoothly while running.
        changed || self.is_running()
    }

    fn ingest_bytes(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        for ch in text.chars() {
            match ch {
                '\n' => {
                    let raw = std::mem::take(&mut self.partial_line);
                    self.push_parsed_line(&raw);
                }
                '\r' => {
                    // Carriage return resets the in-progress line (e.g. cargo progress bars).
                    self.partial_line.clear();
                }
                _ => {
                    self.partial_line.push(ch);
                }
            }
        }
    }

    fn flush_partial_line(&mut self) {
        if !self.partial_line.is_empty() {
            let raw = std::mem::take(&mut self.partial_line);
            self.push_parsed_line(&raw);
        }
    }

    fn push_parsed_line(&mut self, raw: &str) {
        if self.lines.len() >= MAX_OUTPUT_LINES {
            let drop_count = self.lines.len() + 1 - MAX_OUTPUT_LINES;
            self.lines.drain(0..drop_count);
        }
        self.lines.push(parse_ansi_line(raw));
    }

    pub fn active_partial_line(&self) -> Option<CompileLine> {
        if self.partial_line.is_empty() {
            None
        } else {
            Some(parse_ansi_line(&self.partial_line))
        }
    }

    fn byte_index_at_char(&self, char_idx: usize) -> usize {
        self.command
            .char_indices()
            .nth(char_idx)
            .map(|(idx, _)| idx)
            .unwrap_or(self.command.len())
    }

    pub fn handle_key(&mut self, ev: &InputEvent) {
        match ev {
            InputEvent::Paste(text) => {
                if self.is_running() {
                    return;
                }
                let clean: String = text
                    .chars()
                    .map(|c| if c == '\n' || c == '\r' || c == '\t' { ' ' } else { c })
                    .filter(|c| !c.is_control())
                    .collect();
                if !clean.is_empty() {
                    let pos = self.byte_index_at_char(self.cursor);
                    self.command.insert_str(pos, &clean);
                    self.cursor += clean.chars().count();
                }
            }
            InputEvent::Key(k) => match k.as_str() {
                "<Esc>" => {
                    self.close();
                }
                "<C-c>" => {
                    if self.is_running() {
                        self.stop();
                    } else {
                        self.close();
                    }
                }
                "<CR>" => {
                    if !self.is_running() {
                        self.start();
                    }
                }
                "<C-r>" => {
                    self.start();
                }
                _ if self.is_running() => {}
                "<BS>" => {
                    if self.cursor > 0 {
                        let start = self.byte_index_at_char(self.cursor - 1);
                        let end = self.byte_index_at_char(self.cursor);
                        self.command.replace_range(start..end, "");
                        self.cursor -= 1;
                    }
                }
                "<Del>" => {
                    let len = self.command.chars().count();
                    if self.cursor < len {
                        let start = self.byte_index_at_char(self.cursor);
                        let end = self.byte_index_at_char(self.cursor + 1);
                        self.command.replace_range(start..end, "");
                    }
                }
                "<Left>" => {
                    self.cursor = self.cursor.saturating_sub(1);
                }
                "<Right>" => {
                    let len = self.command.chars().count();
                    if self.cursor < len {
                        self.cursor += 1;
                    }
                }
                "<Home>" | "<C-a>" => {
                    self.cursor = 0;
                }
                "<End>" | "<C-e>" => {
                    self.cursor = self.command.chars().count();
                }
                "<C-u>" => {
                    let end = self.byte_index_at_char(self.cursor);
                    self.command.replace_range(0..end, "");
                    self.cursor = 0;
                }
                "<C-w>" => {
                    let before: Vec<char> = self.command.chars().take(self.cursor).collect();
                    let mut new_cur = before.len();
                    while new_cur > 0 && before[new_cur - 1].is_whitespace() {
                        new_cur -= 1;
                    }
                    while new_cur > 0 && !before[new_cur - 1].is_whitespace() {
                        new_cur -= 1;
                    }
                    let start = self.byte_index_at_char(new_cur);
                    let end = self.byte_index_at_char(self.cursor);
                    self.command.replace_range(start..end, "");
                    self.cursor = new_cur;
                }
                s if s.chars().count() == 1 => {
                    let ch = s.chars().next().unwrap();
                    if !ch.is_control() {
                        let pos = self.byte_index_at_char(self.cursor);
                        self.command.insert(pos, ch);
                        self.cursor += 1;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
}

/// Parses ANSI SGR escape sequences into semantic color/weight spans while
/// stripping non-SGR CSI/OSC terminal control sequences.
pub fn parse_ansi_line(raw: &str) -> CompileLine {
    let bytes = raw.as_bytes();
    let mut i = 0usize;
    let mut color = AnsiColor::Default;
    let mut bold = false;
    let mut dim = false;
    let mut spans: Vec<AnsiSpan> = Vec::new();
    let mut plain = String::with_capacity(raw.len());
    let mut current = String::new();

    let flush = |current: &mut String,
                 spans: &mut Vec<AnsiSpan>,
                 color: AnsiColor,
                 bold: bool,
                 dim: bool| {
        if current.is_empty() {
            return;
        }
        let text = std::mem::take(current);
        if let Some(last) = spans.last_mut() {
            if last.color == color && last.bold == bold && last.dim == dim {
                last.text.push_str(&text);
                return;
            }
        }
        spans.push(AnsiSpan {
            text,
            color,
            bold,
            dim,
        });
    };

    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                // CSI sequence: ESC [ params final_byte
                let start = i + 2;
                let mut end = start;
                while end < bytes.len() && !(0x40..=0x7e).contains(&bytes[end]) {
                    end += 1;
                }
                if end < bytes.len() {
                    if bytes[end] == b'm' {
                        flush(&mut current, &mut spans, color, bold, dim);
                        let params = std::str::from_utf8(&bytes[start..end]).unwrap_or("");
                        apply_sgr(params, &mut color, &mut bold, &mut dim);
                    }
                    i = end + 1;
                    continue;
                }
                break;
            } else if i + 1 < bytes.len() && bytes[i + 1] == b']' {
                // OSC sequence terminated by BEL or ST (ESC \)
                i += 2;
                while i < bytes.len() {
                    if bytes[i] == 0x07 {
                        i += 1;
                        break;
                    }
                    if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                continue;
            } else {
                i += 2.min(bytes.len() - i);
                continue;
            }
        }

        let ch = raw[i..].chars().next().unwrap();
        i += ch.len_utf8();
        if ch == '\t' {
            current.push_str("    ");
            plain.push_str("    ");
        } else if !ch.is_control() {
            current.push(ch);
            plain.push(ch);
        }
    }

    flush(&mut current, &mut spans, color, bold, dim);
    CompileLine { spans, plain }
}

fn apply_sgr(params: &str, color: &mut AnsiColor, bold: &mut bool, dim: &mut bool) {
    if params.is_empty() {
        *color = AnsiColor::Default;
        *bold = false;
        *dim = false;
        return;
    }
    let nums: Vec<u16> = params
        .split(';')
        .map(|part| part.parse::<u16>().unwrap_or(0))
        .collect();
    let mut idx = 0usize;
    while idx < nums.len() {
        match nums[idx] {
            0 => {
                *color = AnsiColor::Default;
                *bold = false;
                *dim = false;
            }
            1 => *bold = true,
            2 => *dim = true,
            22 => {
                *bold = false;
                *dim = false;
            }
            30 | 90 => *color = AnsiColor::Muted,
            31 | 91 => *color = AnsiColor::Error,
            32 | 92 => *color = AnsiColor::Success,
            33 | 93 => *color = AnsiColor::Warning,
            34 | 94 => *color = AnsiColor::Accent,
            35 | 95 => *color = AnsiColor::Magenta,
            36 | 96 => *color = AnsiColor::Cyan,
            37 | 97 | 39 => *color = AnsiColor::Default,
            38 if idx + 2 < nums.len() && nums[idx + 1] == 5 => {
                *color = ansi_256_color(nums[idx + 2] as u8);
                idx += 2;
            }
            38 if idx + 4 < nums.len() && nums[idx + 1] == 2 => {
                let r = (nums[idx + 2] as u32) & 0xff;
                let g = (nums[idx + 3] as u32) & 0xff;
                let b = (nums[idx + 4] as u32) & 0xff;
                *color = AnsiColor::Rgb((r << 16) | (g << 8) | b);
                idx += 4;
            }
            _ => {}
        }
        idx += 1;
    }
}

fn ansi_256_color(code: u8) -> AnsiColor {
    match code {
        0 | 8 => AnsiColor::Muted,
        1 | 9 => AnsiColor::Error,
        2 | 10 => AnsiColor::Success,
        3 | 11 => AnsiColor::Warning,
        4 | 12 => AnsiColor::Accent,
        5 | 13 => AnsiColor::Magenta,
        6 | 14 => AnsiColor::Cyan,
        7 | 15 => AnsiColor::Default,
        16..=231 => {
            let n = code - 16;
            let r = (n / 36) as u32 * 51;
            let g = ((n / 6) % 6) as u32 * 51;
            let b = (n % 6) as u32 * 51;
            AnsiColor::Rgb((r << 16) | (g << 8) | b)
        }
        232..=255 => {
            let v = (8 + (code - 232) as u32 * 10) & 0xff;
            AnsiColor::Rgb((v << 16) | (v << 8) | v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_parser_preserves_colors_and_strips_escape_garbage() {
        let line = parse_ansi_line("\x1b[1;31merror[E0499]\x1b[0m: cannot borrow\x1b[K\x1b]0;title\x07");
        assert_eq!(line.plain, "error[E0499]: cannot borrow");
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].text, "error[E0499]");
        assert_eq!(line.spans[0].color, AnsiColor::Error);
        assert!(line.spans[0].bold);
        assert_eq!(line.spans[1].text, ": cannot borrow");
        assert_eq!(line.spans[1].color, AnsiColor::Default);
        assert!(!line.spans[1].bold);
    }

    #[test]
    fn compile_widget_runs_command_and_captures_exit_status() {
        let mut w = CompileWidget::new("/home/test");
        w.open_with("/tmp".into(), "printf '\\033[32mok\\033[0m\\n'".into(), "make".into(), "k".into());
        w.start();
        let deadline = Instant::now() + Duration::from_secs(3);
        while w.is_running() && Instant::now() < deadline {
            w.poll();
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(w.stage, CompileStage::Succeeded);
        assert_eq!(w.plain_output(), "ok");
        assert_eq!(w.lines[0].spans[0].color, AnsiColor::Success);
    }

    #[test]
    fn compile_widget_command_editing_and_custom_memory() {
        let mut w = CompileWidget::new("/home/test");
        w.open_with("/home/test/proj".into(), "cargo build".into(), "cargo build".into(), "p".into());
        assert_eq!(w.display_cwd(), "~/proj");
        w.handle_key(&InputEvent::Paste(" --release\n".into()));
        assert_eq!(w.command, "cargo build --release ");
        w.handle_key(&InputEvent::Key("<BS>".into()));
        assert_eq!(w.command, "cargo build --release");
        w.reset_to_default();
        assert_eq!(w.command, "cargo build");
    }
}
