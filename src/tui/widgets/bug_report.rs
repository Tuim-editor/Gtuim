use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use crate::tui::input::InputEvent;
use crate::tui::layout::Rect;
use crate::tui::renderer::Renderer;
use crate::tui::theme::{chrome, Theme};
use crate::tui::widgets::primitives;

pub const BUG_CATEGORIES: &[&str] = &[
    "Editor",
    "Terminal",
    "Git",
    "Extensions",
    "Performance",
    "Other",
];
pub const DEFAULT_BUG_REPORT_REPO: &str = "Tuim-editor/Gtuim";
/// Conservative browser/desktop-handler limit, measured after UTF-8 percent encoding.
pub const MAX_DRAFT_URL_BYTES: usize = 8000;
static NEXT_EXPORT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BugStage {
    Form,
    Consent,
    Submitting,
    /// The desktop opener accepted a draft URL, not evidence of issue submission.
    Success,
    Failure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueDraft {
    pub url: String,
    /// Complete redacted report, even when it cannot fit in the URL.
    pub report_markdown: String,
    /// Copy report_markdown before opening url, then ask the user to paste it.
    pub needs_clipboard: bool,
}

pub struct BugReportWidget {
    pub is_open: bool,
    pub stage: BugStage,
    pub focus: usize,
    pub category_idx: usize,
    pub summary: String,
    pub description: String,
    pub data_dir: String,
    pub home_dir: String,
    pub report_repo: String,
    /// Issue composer URL; never an assertion that an issue was submitted.
    pub issue_url: String,
    pub error_message: String,
    pub needs_clipboard: bool,
    submission: Option<Receiver<Result<(), String>>>,
    /// Recovery feedback is independent of draft-opening success.
    pub status_message: String,
    retained_report: Option<String>,
    recovery_stage: Option<BugStage>,
    recovery_success: String,
}

impl BugReportWidget {
    pub fn new(data_dir: &str, home_dir: &str) -> Self {
        Self {
            is_open: false,
            stage: BugStage::Form,
            focus: 1,
            category_idx: 0,
            summary: String::new(),
            description: String::new(),
            data_dir: data_dir.into(),
            home_dir: home_dir.into(),
            report_repo: std::env::var("GTUIM_BUG_REPORT_REPO")
                .unwrap_or_else(|_| DEFAULT_BUG_REPORT_REPO.into()),
            issue_url: String::new(),
            error_message: String::new(),
            needs_clipboard: false,
            submission: None,
            status_message: String::new(),
            retained_report: None,
            recovery_stage: None,
            recovery_success: String::new(),
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
        self.stage = if self.submission.is_some() {
            BugStage::Submitting
        } else {
            BugStage::Form
        };
    }

    pub fn modal_rect(screen: Rect) -> Rect {
        let w = 76u16.min(screen.w.saturating_sub(4)).max(42.min(screen.w));
        let h = 23u16.min(screen.h.saturating_sub(2)).max(16.min(screen.h));
        Rect::new(
            screen.x + screen.w.saturating_sub(w) / 2,
            screen.y + screen.h.saturating_sub(h) / 2,
            w,
            h,
        )
    }

    /// Best-effort redaction, not a guarantee that arbitrary secrets are detected.
    /// Always let the user review the public draft before submitting on GitHub.
    pub fn redact_text(&self, input: &str) -> String {
        let mut s = if self.home_dir.is_empty() {
            input.into()
        } else {
            input.replace(&self.home_dir, "~")
        };
        for prefix in [
            "ghp_",
            "github_pat_",
            "gho_",
            "ghu_",
            "ghs_",
            "ghr_",
            "sk-",
            "Bearer ",
            "token=",
            "api_key=",
            "apikey=",
            "secret=",
            "password=",
        ] {
            let mut cursor = 0;
            while let Some(offset) = s[cursor..].find(prefix) {
                let start = cursor + offset + prefix.len();
                let end = s[start..]
                    .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '&')
                    .map(|off| start + off)
                    .unwrap_or(s.len());
                if end > start {
                    s.replace_range(start..end, "[REDACTED]");
                    cursor = start + "[REDACTED]".len();
                } else {
                    cursor = start;
                }
            }
        }
        s
    }

    pub fn report_markdown(&self, diagnostics_json: &str) -> String {
        let category = BUG_CATEGORIES
            .get(self.category_idx)
            .copied()
            .unwrap_or("Other");
        // Indent diagnostics instead of fencing: arbitrary backticks cannot break out.
        let diagnostics = diagnostics_json
            .lines()
            .map(|line| format!("    {line}\n"))
            .collect::<String>();
        self.redact_text(&format!(
            "## Summary\n\n{}\n\n## Category\n\n{}\n\n## Description / Steps to reproduce\n\n{}\n\n## Diagnostics (review before posting publicly)\n\n{}",
            self.summary, category, self.description, diagnostics))
    }

    pub fn prepare_draft(&self, diagnostics_json: &str) -> Result<IssueDraft, String> {
        if self.summary.trim().is_empty() {
            return Err("Please enter a summary before opening a draft.".into());
        }
        let repo = self.report_repo.trim();
        let parts: Vec<_> = repo.split('/').collect();
        if parts.len() != 2
            || parts.iter().any(|part| {
                part.is_empty()
                    || part.len() > 100
                    || *part == "."
                    || *part == ".."
                    || !part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
        {
            return Err(
                "GTUIM_BUG_REPORT_REPO must be a GitHub owner/repository, not a URL.".into(),
            );
        }
        let report_markdown = self.report_markdown(diagnostics_json);
        let base = format!("https://github.com/{repo}/issues/new");
        let full_url = format!(
            "{base}?title={}&body={}",
            percent_encode(&self.redact_text(&self.summary)),
            percent_encode(&report_markdown)
        );
        let needs_clipboard = full_url.len() > MAX_DRAFT_URL_BYTES;
        let url = if needs_clipboard {
            // Omit title AND body rather than silently truncating either field. Both
            // remain intact in the copied report, including an unusually large title.
            format!("{base}?body={}", percent_encode(
                "Paste the complete report copied by Gtuim here, and set a title from its Summary. Review for private information before submitting."))
        } else {
            full_url
        };
        debug_assert!(url.len() <= MAX_DRAFT_URL_BYTES);
        Ok(IssueDraft {
            url,
            report_markdown,
            needs_clipboard,
        })
    }

    /// GUI callers copy draft.report_markdown using the native clipboard first
    /// when needs_clipboard is true, then call this. Does not upload anything.
    pub fn launch_draft(&mut self, draft: &IssueDraft) {
        self.start_draft(draft, false);
    }

    /// TUI path: perform clipboard fallback (if needed) and desktop opening off
    /// the UI thread. A clipboard failure leaves the full report in the form.
    pub fn submit(&mut self, diagnostics_json: &str) {
        if self.submission.is_some() {
            return;
        }
        self.retained_report = Some(self.report_markdown(diagnostics_json));
        match self.prepare_draft(diagnostics_json) {
            Ok(draft) => self.start_draft(&draft, true),
            Err(error) => {
                self.error_message = error;
                self.stage = BugStage::Failure;
            }
        }
    }

    fn start_draft(&mut self, draft: &IssueDraft, copy: bool) {
        if self.submission.is_some() {
            return;
        }
        self.retained_report = Some(draft.report_markdown.clone());
        self.recovery_stage = None;
        self.status_message.clear();
        self.issue_url = draft.url.clone();
        self.needs_clipboard = draft.needs_clipboard;
        self.error_message.clear();
        self.stage = BugStage::Submitting;
        let draft = draft.clone();
        let (tx, rx) = mpsc::channel();
        self.submission = Some(rx);
        std::thread::spawn(move || {
            let result = (|| {
                if copy && draft.needs_clipboard {
                    copy_report(&draft.report_markdown)?;
                }
                open_browser(&draft.url)
            })();
            let _ = tx.send(result);
        });
    }

    /// User-triggered recovery. Never launches a browser or claims an issue draft
    /// was opened. Keep the same redacted diagnostic snapshot after a failure.
    fn recover(&mut self, diagnostics_json: &str, export: bool) {
        self.recover_with_copier(diagnostics_json, export, copy_report);
    }

    fn recover_with_copier<F>(&mut self, diagnostics_json: &str, export: bool, copier: F)
    where
        F: FnOnce(&str) -> Result<(), String> + Send + 'static,
    {
        if self.submission.is_some() {
            return;
        }
        let report = if self.stage == BugStage::Consent || self.retained_report.is_none() {
            self.report_markdown(diagnostics_json)
        } else {
            self.retained_report.clone().unwrap()
        };
        self.retained_report = Some(report.clone());
        if export {
            let path = Path::new(&self.data_dir).join(format!(
                "bug-report-{}-{}.md",
                std::process::id(),
                NEXT_EXPORT.fetch_add(1, Ordering::Relaxed)
            ));
            self.start_recovery(
                "Exporting complete report locally...".into(),
                format!(
                    "Complete redacted report exported (not submitted): {}",
                    path.display()
                ),
                move || write_private_report(&path, &report),
            );
        } else {
            self.start_recovery(
                "Copying complete redacted report...".into(),
                "Complete redacted report copied. No draft opened or issue submitted.".into(),
                move || copier(&report),
            );
        }
    }

    fn start_recovery<F>(&mut self, pending: String, success: String, operation: F)
    where
        F: FnOnce() -> Result<(), String> + Send + 'static,
    {
        if self.submission.is_some() {
            return;
        }
        self.recovery_stage = Some(self.stage);
        self.recovery_success = success;
        self.status_message = pending;
        self.stage = BugStage::Submitting;
        let (tx, rx) = mpsc::channel();
        self.submission = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(operation());
        });
    }

    pub fn poll_submission(&mut self) -> bool {
        let Some(rx) = self.submission.as_ref() else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("Draft launcher stopped unexpectedly.".into())
            }
        };
        self.submission = None;
        match result {
            Ok(()) => {
                if let Some(stage) = self.recovery_stage.take() {
                    self.stage = stage;
                    self.status_message = std::mem::take(&mut self.recovery_success);
                } else {
                    self.stage = BugStage::Success;
                }
            }
            Err(error) => {
                self.recovery_stage = None;
                self.status_message.clear();
                self.error_message = error;
                self.stage = BugStage::Failure;
            }
        }
        true
    }

    pub fn handle_key(&mut self, ev: &InputEvent, diagnostics_json: &str) -> bool {
        if !self.is_open {
            return false;
        }
        let key = match ev {
            InputEvent::Key(k) => k.as_str(),
            InputEvent::Paste(text) => {
                if self.stage == BugStage::Form {
                    if self.focus == 1 {
                        self.summary
                            .extend(text.chars().filter(|c| *c != '\n' && *c != '\r'));
                    } else if self.focus == 2 {
                        self.description.push_str(text);
                    }
                }
                return true;
            }
            _ => return false,
        };
        if matches!(
            self.stage,
            BugStage::Consent | BugStage::Success | BugStage::Failure
        ) {
            match key {
                "c" | "C" => {
                    self.recover(diagnostics_json, false);
                    return true;
                }
                "e" | "E" => {
                    self.recover(diagnostics_json, true);
                    return true;
                }
                _ => {}
            }
        }
        match self.stage {
            BugStage::Form => match key {
                "<Esc>" => self.is_open = false,
                "<Tab>" => self.focus = (self.focus + 1) % 4,
                "<C-s>" if !self.summary.trim().is_empty() => self.stage = BugStage::Consent,
                _ => match self.focus {
                    0 => match key {
                        "h" | "<Left>" => {
                            self.category_idx = (self.category_idx + BUG_CATEGORIES.len() - 1)
                                % BUG_CATEGORIES.len()
                        }
                        "l" | "<Right>" | " " | "<CR>" => {
                            self.category_idx = (self.category_idx + 1) % BUG_CATEGORIES.len()
                        }
                        _ => {}
                    },
                    1 => match key {
                        "<BS>" => {
                            self.summary.pop();
                        }
                        "<CR>" | "<Down>" => self.focus = 2,
                        k if k.chars().count() == 1 => self.summary.push_str(k),
                        _ => {}
                    },
                    2 => match key {
                        "<BS>" => {
                            self.description.pop();
                        }
                        "<CR>" => self.description.push('\n'),
                        k if k.chars().count() == 1 => self.description.push_str(k),
                        _ => {}
                    },
                    3 if key == "<CR>" && !self.summary.trim().is_empty() => {
                        self.stage = BugStage::Consent
                    }
                    _ => {}
                },
            },
            BugStage::Consent => match key {
                "y" | "Y" | "<CR>" => self.submit(diagnostics_json),
                "n" | "N" | "<Esc>" => self.stage = BugStage::Form,
                _ => {}
            },
            BugStage::Submitting => {
                if key == "<Esc>" {
                    self.is_open = false;
                }
            }
            BugStage::Success | BugStage::Failure => {
                if matches!(key, "<Esc>" | "<CR>" | "q") {
                    self.is_open = false;
                } else if key == "r" && self.stage == BugStage::Failure {
                    self.stage = BugStage::Form;
                }
            }
        }
        true
    }

    pub fn draw(&self, renderer: &mut Renderer, screen: Rect, theme: &Theme) {
        if !self.is_open {
            return;
        }
        let mr = Self::modal_rect(screen);
        let c = chrome(theme);
        primitives::draw_modal_frame(renderer, mr, " Report a Bug — GitHub Draft ", theme);
        let lines: Vec<String> = match self.stage {
            BugStage::Form => {
                let category = BUG_CATEGORIES
                    .get(self.category_idx)
                    .copied()
                    .unwrap_or("Other");
                let mut lines = vec![
                    format!("Category: < {category} >"),
                    "".into(),
                    "Summary:".into(),
                ];
                lines.push(self.summary.clone());
                lines.extend(["".into(), "Description / Steps to reproduce:".into()]);
                lines.extend(self.description.lines().map(str::to_owned));
                lines
            }
            BugStage::Consent => vec![
                "Open a PUBLIC GitHub issue draft in your browser?".into(),
                "".into(),
                "Summary, description and diagnostics are redacted best-effort.".into(),
                "Opening sends URL report content to GitHub; your browser".into(),
                "may retain that content in its history, even without posting.".into(),
                "Review for private information before opening or posting.".into(),
                "Long reports are copied in full for you to paste into GitHub.".into(),
                "Sign-in is required to post. No issue is submitted automatically.".into(),
                "".into(),
                "[Y / Enter] Open draft    [N / Esc] Back".into(),
            ],
            BugStage::Submitting => {
                if self.recovery_stage.is_some() {
                    vec![] // Recovery's pending message is shown independently below.
                } else {
                    vec!["Opening GitHub draft (not submitting)...".into()]
                }
            }
            BugStage::Success => {
                let mut lines = vec![
                    "Draft opening requested — NOT submitted.".into(),
                    "Review it and submit on GitHub in your browser.".into(),
                ];
                if self.needs_clipboard {
                    lines.push(
                        "Full report copied: paste it into the issue and set a title.".into(),
                    );
                }
                lines
            }
            BugStage::Failure => vec![
                self.error_message.clone(),
                "".into(),
                "Your form is retained. [r] Edit/retry    [Esc] Close".into(),
            ],
        };
        let mut display_lines = if self.status_message.is_empty() {
            vec![]
        } else {
            let mut status = wrap_status(&self.status_message, mr.w.saturating_sub(4) as usize);
            status.push(String::new());
            status
        };
        display_lines.extend(lines);
        for (index, line) in display_lines
            .iter()
            .take(mr.h.saturating_sub(5) as usize)
            .enumerate()
        {
            renderer.draw_text(
                mr.x + 2,
                mr.y + 2 + index as u16,
                &primitives::truncate_to_width(line, mr.w.saturating_sub(4) as usize),
                c.text_strong,
                c.panel_bg,
            );
        }
        if self.stage == BugStage::Form {
            renderer.draw_text(
                mr.x + 2,
                mr.y + mr.h.saturating_sub(2),
                &format!("[Tab] Field {} / 4   [Ctrl+S] Review draft", self.focus + 1),
                c.accent,
                c.panel_bg,
            );
        } else if matches!(
            self.stage,
            BugStage::Consent | BugStage::Success | BugStage::Failure
        ) {
            renderer.draw_text(
                mr.x + 2,
                mr.y + mr.h.saturating_sub(2),
                "[C] Copy full report  [E] Export private local .md",
                c.accent,
                c.panel_bg,
            );
        }
    }
}

/// Hard-wrap recovery feedback without dropping long path components or spaces.
fn wrap_status(text: &str, width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    if width == 0 {
        return vec![];
    }
    let mut lines = vec![];
    let mut line = String::new();
    let mut cells = 0;
    for ch in text.chars() {
        let size = ch.width().unwrap_or(0);
        if cells + size > width && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            cells = 0;
        }
        line.push(ch);
        cells += size;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Explicit local export only. create_new rejects files and symlinks already at
/// this path; mode 0600 is applied at creation, before any report bytes are sent.
fn write_private_report(path: &Path, report: &str) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("Cannot export report to {}: {e}", path.display()))?;
    if let Err(error) = file
        .write_all(report.as_bytes())
        .and_then(|_| file.sync_all())
    {
        let _ = std::fs::remove_file(path);
        return Err(format!(
            "Cannot finish export to {}: {error}",
            path.display()
        ));
    }
    Ok(())
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            const HEX: &[u8] = b"0123456789ABCDEF";
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 15) as usize] as char);
        }
    }
    out
}

/// No shell or inherited terminal input/output; this helper is not a direct
/// HTTP client. Opening a draft does cause the browser to send its URL content
/// to GitHub and may retain that content in browser history. All external
/// helpers are bounded so a broken desktop service cannot strand UI state.
fn run_helper(program: &str, args: &[&str], input: Option<&str>) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Cannot start {program}: {e}"))?;
    let writer = input.map(|text| {
        let mut stdin = child.stdin.take().unwrap();
        let bytes = text.as_bytes().to_vec();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(stdin.write_all(&bytes));
        });
        rx
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return Err(format!("{program} failed ({status})."));
                }
                if let Some(writer) = writer {
                    writer
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        .map_err(|_| "Clipboard writer stopped or timed out.".to_string())?
                        .map_err(|e| format!("Cannot copy report: {e}"))?;
                }
                return Ok(());
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match result {
                    Err(e) => format!("Cannot monitor {program}: {e}"),
                    _ => format!("{program} timed out. Check your desktop handler and retry."),
                });
            }
        }
    }
}

fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(target_os = "macos"))]
    let program = "xdg-open";
    run_helper(program, &[url], None)
}

fn copy_report(report: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        run_helper("pbcopy", &[], Some(report))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let helpers: &[(&str, &[&str])] = &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ];
        for (program, args) in helpers {
            if run_helper(program, args, Some(report)).is_ok() {
                return Ok(());
            }
        }
        Err("Cannot copy the full report. Install wl-copy, xclip or xsel; your form is retained. No draft was opened.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn widget() -> BugReportWidget {
        let mut widget = BugReportWidget::new("", "/home/test");
        widget.report_repo = DEFAULT_BUG_REPORT_REPO.into();
        widget.summary = "Editor bug".into();
        widget
    }

    #[test]
    fn redaction_terminates_for_repeated_empty_and_unicode_secrets() {
        let widget = widget();
        for (input, expected) in [
            ("ghp_abc ghp_def", "ghp_[REDACTED] ghp_[REDACTED]"),
            (
                "token= token=abc&password=秘密",
                "token= token=[REDACTED]&password=[REDACTED]",
            ),
            ("token=", "token="),
            ("sk-[REDACTED]", "sk-[REDACTED]"),
            (
                "/home/test/日本語 api_key=鍵",
                "~/日本語 api_key=[REDACTED]",
            ),
        ] {
            assert_eq!(widget.redact_text(input), expected);
            assert_eq!(widget.redact_text(expected), expected);
        }
    }

    #[test]
    fn percent_encoding_encodes_utf8_and_query_delimiters() {
        assert_eq!(
            percent_encode("a b&c=#?+/'\n日本"),
            "a%20b%26c%3D%23%3F%2B%2F%27%0A%E6%97%A5%E6%9C%AC"
        );
        assert_eq!(percent_encode("AZaz09-._~"), "AZaz09-._~");
    }

    #[test]
    fn draft_redacts_every_user_field_and_diagnostics() {
        let mut w = widget();
        w.summary = "/home/test/a ghp_summary".into();
        w.description = "secret=description 日本".into();
        let draft = w.prepare_draft("token=diagnostic").unwrap();
        for secret in [
            "/home/test",
            "ghp_summary",
            "secret=description",
            "token=diagnostic",
        ] {
            assert!(!draft.report_markdown.contains(secret));
            assert!(!draft.url.contains(&percent_encode(secret)));
        }
        assert!(draft.report_markdown.contains("Editor"));
        assert!(draft.report_markdown.contains("日本"));
        assert!(draft
            .url
            .starts_with("https://github.com/Tuim-editor/Gtuim/issues/new?title="));
        assert!(!draft.needs_clipboard);
    }

    #[test]
    fn validates_repository_and_summary_without_launching() {
        let mut w = widget();
        for repo in [
            "",
            "a",
            "a/b/c",
            "https://github.com/a/b",
            "a/b?x=y",
            "../b",
            "a/..",
            "a/日本",
            "a/b\n",
        ] {
            w.report_repo = repo.into();
            // Trailing whitespace is intentionally trimmed from environment settings.
            if repo == "a/b\n" {
                assert!(w.prepare_draft("").is_ok());
            } else {
                assert!(w.prepare_draft("").is_err(), "{repo}");
            }
        }
        w.report_repo = "my-org/my.repo_1".into();
        assert!(w.prepare_draft("").is_ok());
        w.summary = " \n".into();
        assert!(w.prepare_draft("").is_err());
        w.submit("");
        assert_eq!(w.stage, BugStage::Failure);
        assert!(w.submission.is_none());
    }

    #[test]
    fn oversized_reports_are_complete_in_clipboard_fallback_not_truncated() {
        let mut w = widget();
        w.summary = "日本".repeat(3000);
        w.description = "complete-tail".repeat(4000);
        let draft = w.prepare_draft("last-diagnostic").unwrap();
        assert!(draft.needs_clipboard);
        assert!(draft.url.len() <= MAX_DRAFT_URL_BYTES);
        assert!(draft.report_markdown.contains(&w.summary));
        assert!(draft.report_markdown.contains(&w.description));
        assert!(draft.report_markdown.contains("last-diagnostic"));
        assert!(draft.url.contains("Paste%20the%20complete%20report"));
    }

    #[test]
    fn url_limit_is_applied_after_encoding() {
        let mut w = widget();
        w.description = "日".repeat(1000);
        assert!(w.report_markdown("").len() < MAX_DRAFT_URL_BYTES);
        assert!(w.prepare_draft("").unwrap().needs_clipboard);
        w.description.clear();
        let overhead = w.prepare_draft("").unwrap().url.len();
        w.description = "a".repeat(MAX_DRAFT_URL_BYTES - overhead);
        let exact = w.prepare_draft("").unwrap();
        assert_eq!(exact.url.len(), MAX_DRAFT_URL_BYTES);
        assert!(!exact.needs_clipboard);
        w.description.push('a');
        assert!(w.prepare_draft("").unwrap().needs_clipboard);
    }

    #[test]
    fn full_paste_is_retained_for_fallback() {
        let mut w = widget();
        w.is_open = true;
        w.focus = 2;
        let text = "日".repeat(10000);
        w.handle_key(&InputEvent::Paste(text.clone()), "");
        assert_eq!(w.description, text);
    }

    #[test]
    fn poll_tracks_async_results_and_reopening_without_real_browser() {
        let mut w = widget();
        let (tx, rx) = mpsc::channel();
        w.submission = Some(rx);
        w.open();
        assert_eq!(w.stage, BugStage::Submitting);
        assert!(!w.poll_submission());
        tx.send(Ok(())).unwrap();
        assert!(w.poll_submission());
        assert_eq!(w.stage, BugStage::Success);
        assert!(w.submission.is_none());
        let (tx, rx) = mpsc::channel();
        w.submission = Some(rx);
        tx.send(Err("No browser handler".into())).unwrap();
        assert!(w.poll_submission());
        assert_eq!(w.stage, BugStage::Failure);
        assert_eq!(w.error_message, "No browser handler");
        assert_eq!(w.summary, "Editor bug");
    }

    fn await_operation(w: &mut BugReportWidget) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while !w.poll_submission() {
            assert!(Instant::now() < deadline, "recovery timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn export_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gtuim-private-export-{}-{}",
            std::process::id(),
            NEXT_EXPORT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        dir
    }

    #[test]
    fn mocked_copy_is_async_and_never_claims_a_draft_was_opened() {
        for stage in [BugStage::Consent, BugStage::Success, BugStage::Failure] {
            let mut w = widget();
            w.stage = stage;
            w.description = "long complete 日本 tail".repeat(1000);
            let report = w.report_markdown("secret=hidden diagnostics-tail");
            w.retained_report = Some(report.clone());
            let (tx, rx) = mpsc::channel();
            w.recover_with_copier("secret=hidden diagnostics-tail", false, move |text| {
                tx.send(text.to_owned()).unwrap();
                Ok(())
            });
            assert_eq!(w.stage, BugStage::Submitting);
            await_operation(&mut w);
            assert_eq!(rx.recv().unwrap(), report);
            assert_eq!(w.stage, stage);
            assert!(w.status_message.contains("No draft opened"));
            assert!(w.issue_url.is_empty());
        }
    }

    #[test]
    fn failed_copy_can_export_original_complete_private_report_without_browser() {
        use std::os::unix::fs::PermissionsExt;
        let dir = export_dir();
        let mut w = widget();
        w.data_dir = dir.to_str().unwrap().into();
        w.is_open = true;
        w.stage = BugStage::Consent;
        w.description = "日本 complete report tail".repeat(1000);
        let report = w.report_markdown("api_key=hidden original-diagnostic");
        w.recover_with_copier("api_key=hidden original-diagnostic", false, |_| {
            Err("No clipboard helper".into())
        });
        await_operation(&mut w);
        assert_eq!(w.stage, BugStage::Failure);
        // Diagnostics supplied after failure must not replace the original report.
        w.handle_key(&InputEvent::Key("E".into()), "changed-diagnostic");
        await_operation(&mut w);
        assert_eq!(w.stage, BugStage::Failure); // Export is NOT draft success.
        let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1);
        let path = entries[0].as_ref().unwrap().path();
        assert_eq!(std::fs::read(&path).unwrap(), report.as_bytes());
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(w.status_message.contains(path.to_str().unwrap()));
        assert!(w.status_message.contains("not submitted"));
        assert!(w.issue_url.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recovery_path_wrapping_does_not_truncate_long_components() {
        let status = format!("Exported: /tmp/{}/日本 report.md", "a".repeat(150));
        let lines = wrap_status(&status, 40);
        assert_eq!(lines.concat(), status);
        assert!(lines.len() > 4);
    }

    #[test]
    fn private_export_never_overwrites_existing_files_or_symlinks() {
        let dir = export_dir();
        let path = dir.join("report.md");
        std::fs::write(&path, "existing bytes").unwrap();
        assert!(write_private_report(&path, "new report").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "existing bytes");
        let link = dir.join("linked.md");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(write_private_report(&link, "secret report").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "existing bytes");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn helper_uses_literal_argv_and_reports_spawn_and_exit_errors() {
        // Explicit shell is a TEST fixture only. The URL is a positional argument,
        // never shell code. No network, clipboard, or browser is touched.
        assert!(run_helper(
            "/bin/sh",
            &[
                "-c",
                "test \"$1\" = 'https://example.invalid/?x=$(false)&y=日本'",
                "test",
                "https://example.invalid/?x=$(false)&y=日本"
            ],
            None
        )
        .is_ok());
        assert!(run_helper("/bin/sh", &["-c", "exit 7"], None).is_err());
        assert!(run_helper("/nonexistent/gtuim-opener", &[], None)
            .unwrap_err()
            .contains("Cannot start"));
        assert!(run_helper(
            "/bin/sh",
            &["-c", "test \"$(cat)\" = 'complete 日本 report'"],
            Some("complete 日本 report")
        )
        .is_ok());
    }
}
