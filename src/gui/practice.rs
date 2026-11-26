//! Native GPUI Daily Coding Practice engine and interactive panel widget.
//!
//! Self-contained GPUI view (`PracticePanel`) backed by an isolated per-profile
//! persistence directory. Hosts mount it as an `Entity<PracticePanel>`, optionally
//! sync Gtuim theme/fonts via `sync_appearance`, and subscribe to `PracticeEvent`.

pub mod catalog;
pub mod editor;
pub mod runner;
pub mod storage;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gpui::{prelude::*, *};

const DRAFT_SAVE_DEBOUNCE: Duration = Duration::from_millis(750);

use crate::gui::icons::{gui_palette, icon, GuiPalette};
use crate::tui::theme::{blend_u32, Theme};

pub use catalog::{Difficulty, Exercise, ExerciseSignature, PracticeLanguage, TestCase};
pub use runner::{
    CaseOutcome, RunLimits, SubmissionReport, SubmissionVerdict, ToolchainRegistry, ToolchainStatus,
};
pub use storage::{LocalDate, PracticeProfileState, StreakSummary};

use editor::CodeEditor;

/// Events emitted by `PracticePanel` to its host container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PracticeEvent {
    /// User clicked the panel's close button or pressed Escape while idle.
    CloseRequested,
    /// Streak or daily-completion status changed after an accepted submission or day rollover.
    StreakUpdated(StreakSummary),
}

/// Interactive daily coding practice widget.
pub struct PracticePanel {
    profile_dir: PathBuf,
    profile: PracticeProfileState,
    today: LocalDate,
    language: PracticeLanguage,
    difficulty: Difficulty,
    pool: Vec<Exercise>,
    exercise_index: usize,
    daily_index: usize,
    editor: CodeEditor,
    toolchains: ToolchainRegistry,
    probing_toolchains: bool,
    limits: RunLimits,
    submitting: bool,
    submission_generation: u64,
    active_cancel: Option<Arc<AtomicBool>>,
    last_report: Option<SubmissionReport>,
    storage_notice: Option<String>,
    corrupt_state_blocked: bool,
    streak_celebration: Option<String>,
    pending_draft_deltas: HashMap<String, Option<String>>,
    prefs_dirty: bool,
    draft_save_seq: u64,
    #[cfg(test)]
    test_today_override: Option<LocalDate>,
    problem_collapsed: bool,
    show_close_button: bool,
    theme: Theme,
    mono_font: SharedString,
    ui_font: SharedString,
    font_size: f32,
    focus_handle: FocusHandle,
    main_scroll: ScrollHandle,
    editor_scroll: ScrollHandle,
}

impl PracticePanel {
    /// Create a new Daily Coding Practice widget backed by a unique persistent
    /// per-profile directory (`profile_dir`).
    pub fn new(profile_dir: PathBuf, cx: &mut Context<Self>) -> Self {
        let today = LocalDate::today();
        let loaded = PracticeProfileState::load_checked(&profile_dir);
        let profile = loaded.state;
        let storage_notice = loaded.warning;
        let corrupt_state_blocked = loaded.corrupt_blocked;
        let language = profile.selected_language();
        let difficulty = profile.selected_difficulty();
        let pool = catalog::exercises_for_difficulty(difficulty);
        let daily_index = catalog::daily_exercise_index(today, difficulty, pool.len());
        let exercise_index = daily_index;
        let initial_code = load_code_for(&profile, &pool[exercise_index], language);
        let editor = CodeEditor::new(initial_code, language.indent_spaces());
        let toolchains = ToolchainRegistry::detect_fast();
        let focus_handle = cx.focus_handle();

        let panel = Self {
            profile_dir,
            profile,
            today,
            language,
            difficulty,
            pool,
            exercise_index,
            daily_index,
            editor,
            toolchains,
            probing_toolchains: true,
            limits: RunLimits::default(),
            submitting: false,
            submission_generation: 0,
            active_cancel: None,
            last_report: None,
            storage_notice,
            corrupt_state_blocked,
            streak_celebration: None,
            pending_draft_deltas: HashMap::new(),
            prefs_dirty: false,
            draft_save_seq: 0,
            #[cfg(test)]
            test_today_override: None,
            problem_collapsed: false,
            show_close_button: true,
            theme: Theme::default(),
            mono_font: SharedString::from("monospace"),
            ui_font: SharedString::from("sans-serif"),
            font_size: 13.0,
            focus_handle,
            main_scroll: ScrollHandle::new(),
            editor_scroll: ScrollHandle::new(),
        };

        panel.spawn_toolchain_probe(cx);
        panel
    }

    /// Synchronize active Gtuim theme and typography with the practice widget.
    /// No-ops without notifying when appearance values are unchanged.
    pub fn sync_appearance(
        &mut self,
        theme: Theme,
        mono_font: SharedString,
        ui_font: SharedString,
        font_size: f32,
        cx: &mut Context<Self>,
    ) {
        let clamped_size = font_size.clamp(10.0, 24.0);
        if self.theme == theme
            && self.mono_font == mono_font
            && self.ui_font == ui_font
            && (self.font_size - clamped_size).abs() < 0.01
        {
            return;
        }
        self.theme = theme;
        self.mono_font = mono_font;
        self.ui_font = ui_font;
        self.font_size = clamped_size;
        cx.notify();
    }

    /// Show or hide the panel header close button (useful if the host renders its own chrome).
    pub fn set_show_close_button(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_close_button != show {
            self.show_close_button = show;
            cx.notify();
        }
    }

    /// Re-check the local calendar date and rotate to the new day's exercise if midnight passed.
    /// Cancels any stale in-flight submission from the previous day.
    pub fn refresh_daily(&mut self, cx: &mut Context<Self>) {
        let now_today = self.current_local_date();
        self.rollover_to_date(now_today, cx);
    }

    /// Explicitly quarantine a corrupt/unreadable `practice_state.json` to a timestamped
    /// backup inside `profile_dir` and resume normal saving with a clean state.
    pub fn recover_corrupt_state(&mut self, cx: &mut Context<Self>) -> Result<Option<PathBuf>, String> {
        let (fresh, backup) = PracticeProfileState::quarantine_and_reset(&self.profile_dir)?;
        self.profile = fresh;
        self.corrupt_state_blocked = false;
        self.pending_draft_deltas.clear();
        self.prefs_dirty = false;
        self.storage_notice = backup.as_ref().map(|p| {
            format!(
                "Quarantined original file to {} and reset practice state.",
                p.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("backup")
            )
        });
        self.reload_editor_for_active();
        cx.emit(PracticeEvent::StreakUpdated(self.streak_summary()));
        cx.notify();
        Ok(backup)
    }

    /// Flush any debounced draft or preference changes to disk immediately via locked
    /// read-merge-write. Safe to call on panel close, profile switch, or shutdown.
    pub fn flush_pending_save(&mut self) {
        if self.corrupt_state_blocked {
            return;
        }
        if self.pending_draft_deltas.is_empty() && !self.prefs_dirty {
            return;
        }
        self.draft_save_seq = self.draft_save_seq.wrapping_add(1);
        self.persist_profile();
    }

    /// Focus this widget so keyboard typing edits the solution buffer.
    pub fn focus(&self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    /// Explicitly handle a GPUI keystroke (for hosts forwarding keys from a root focus handle).
    /// Returns `true` if the keystroke was consumed.
    pub fn handle_keystroke(&mut self, ks: &Keystroke, cx: &mut Context<Self>) -> bool {
        let ctrl = ks.modifiers.control || ks.modifiers.platform;
        let shift = ks.modifiers.shift;
        let alt = ks.modifiers.alt;
        let key = ks.key.as_str();

        if ctrl && !alt && key == "enter" {
            self.submit(cx);
            return true;
        }

        if key == "escape" {
            if self.editor.clear_selection() {
                cx.notify();
            } else if self.submitting {
                self.cancel_submission(cx);
            } else {
                self.flush_pending_save();
                cx.emit(PracticeEvent::CloseRequested);
            }
            return true;
        }

        if ctrl && !alt {
            match key.to_ascii_lowercase().as_str() {
                "a" => {
                    self.editor.select_all();
                    cx.notify();
                    return true;
                }
                "c" => {
                    let copied = self
                        .editor
                        .selected_text()
                        .unwrap_or_else(|| self.editor.text())
                        .to_string();
                    cx.write_to_clipboard(ClipboardItem::new_string(copied));
                    return true;
                }
                "x" => {
                    if let Some(cut) = self.editor.cut_selection() {
                        cx.write_to_clipboard(ClipboardItem::new_string(cut));
                        self.on_editor_mutated(cx);
                        cx.notify();
                    }
                    return true;
                }
                "v" => {
                    if let Some(item) = cx.read_from_clipboard() {
                        if let Some(text) = item.text() {
                            if self.editor.insert_str(&text) {
                                self.on_editor_mutated(cx);
                                cx.notify();
                            }
                        }
                    }
                    return true;
                }
                "z" if shift => {
                    if self.editor.redo() {
                        self.on_editor_mutated(cx);
                        cx.notify();
                    }
                    return true;
                }
                "z" => {
                    if self.editor.undo() {
                        self.on_editor_mutated(cx);
                        cx.notify();
                    }
                    return true;
                }
                "y" => {
                    if self.editor.redo() {
                        self.on_editor_mutated(cx);
                        cx.notify();
                    }
                    return true;
                }
                "left" => {
                    self.editor.move_word_left(shift);
                    self.scroll_caret_into_view();
                    cx.notify();
                    return true;
                }
                "right" => {
                    self.editor.move_word_right(shift);
                    self.scroll_caret_into_view();
                    cx.notify();
                    return true;
                }
                _ => return false,
            }
        }

        if alt {
            return false;
        }

        let changed = match key {
            "enter" => self.editor.insert_newline_auto_indent(),
            "tab" if shift => self.editor.unindent_line(),
            "tab" => self.editor.insert_tab(),
            "backspace" => self.editor.backspace(),
            "delete" => self.editor.delete_forward(),
            "left" => {
                self.editor.move_left(shift);
                self.scroll_caret_into_view();
                cx.notify();
                return true;
            }
            "right" => {
                self.editor.move_right(shift);
                self.scroll_caret_into_view();
                cx.notify();
                return true;
            }
            "up" => {
                self.editor.move_up(shift);
                self.scroll_caret_into_view();
                cx.notify();
                return true;
            }
            "down" => {
                self.editor.move_down(shift);
                self.scroll_caret_into_view();
                cx.notify();
                return true;
            }
            "home" => {
                self.editor.move_home(shift);
                self.scroll_caret_into_view();
                cx.notify();
                return true;
            }
            "end" => {
                self.editor.move_end(shift);
                self.scroll_caret_into_view();
                cx.notify();
                return true;
            }
            "space" => self.editor.insert_str(" "),
            _ => {
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
                    self.editor.insert_str(&ch)
                } else {
                    return false;
                }
            }
        };

        if changed {
            self.on_editor_mutated(cx);
            cx.notify();
        }
        true
    }

    /// Launch a non-blocking local evaluation of the current solution buffer.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if self.submitting {
            return;
        }
        // If midnight passed before submitting, rotate cleanly first so we never
        // evaluate a stale day's exercise under a new day's slot unexpectedly.
        let originating_date = self.today;
        self.save_current_draft();
        self.flush_pending_save();

        let exercise = self.active_exercise().clone();
        let language = self.language;
        let source = self.editor.text().to_string();
        let toolchain = self.toolchains.get(language);
        let limits = self.limits;

        self.submission_generation = self.submission_generation.wrapping_add(1);
        let generation = self.submission_generation;
        let cancel = Arc::new(AtomicBool::new(false));
        self.active_cancel = Some(cancel.clone());
        self.submitting = true;
        self.streak_celebration = None;
        cx.notify();

        cx.spawn(async move |view, cx| {
            let report = cx
                .background_executor()
                .spawn(async move {
                    runner::evaluate_submission(&exercise, language, &source, &toolchain, limits, &cancel)
                })
                .await;

            let _ = view.update(cx, |this, cx| {
                this.apply_submission_report(generation, originating_date, report, cx);
            });
        })
        .detach();
    }

    /// Cancel an active in-flight submission (terminating its isolated process group).
    pub fn cancel_submission(&mut self, cx: &mut Context<Self>) {
        if let Some(flag) = self.active_cancel.take() {
            flag.store(true, Ordering::Relaxed);
        }
        if self.submitting {
            self.submission_generation = self.submission_generation.wrapping_add(1);
            self.submitting = false;
            self.last_report = Some(SubmissionReport {
                language: self.language,
                exercise_id: self.active_exercise().id.to_string(),
                verdict: SubmissionVerdict::Cancelled,
                summary: "Submission cancelled.".into(),
                elapsed_ms: 0,
                passed_count: 0,
                total_count: self.active_exercise().tests.len(),
                cases: Vec::new(),
                stdout: String::new(),
                stderr: String::new(),
                output_truncated: false,
            });
            cx.notify();
        }
    }

    pub fn profile_dir(&self) -> &Path {
        &self.profile_dir
    }

    pub fn streak_summary(&self) -> StreakSummary {
        self.profile.streak_summary(self.today)
    }

    pub fn is_submitting(&self) -> bool {
        self.submitting
    }

    pub fn active_language(&self) -> PracticeLanguage {
        self.language
    }

    pub fn active_difficulty(&self) -> Difficulty {
        self.difficulty
    }

    pub fn active_exercise_id(&self) -> &'static str {
        self.active_exercise().id
    }

    pub fn last_report(&self) -> Option<&SubmissionReport> {
        self.last_report.as_ref()
    }

    pub fn solution_text(&self) -> &str {
        self.editor.text()
    }

    pub fn set_solution_text(&mut self, code: &str, cx: &mut Context<Self>) {
        self.editor.set_text(code, self.language.indent_spaces());
        self.on_editor_mutated(cx);
        cx.notify();
    }

    // ── Internal helpers ────────────────────────────────────────────────────

    fn active_exercise(&self) -> &Exercise {
        &self.pool[self.exercise_index.min(self.pool.len().saturating_sub(1))]
    }

    fn is_today_daily(&self) -> bool {
        self.exercise_index == self.daily_index
    }

    fn spawn_toolchain_probe(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |view, cx| {
            let detected = cx
                .background_executor()
                .spawn(async move { ToolchainRegistry::detect_full() })
                .await;
            let _ = view.update(cx, |this, cx| {
                this.toolchains = detected;
                this.probing_toolchains = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn current_local_date(&self) -> LocalDate {
        #[cfg(test)]
        if let Some(d) = self.test_today_override {
            return d;
        }
        LocalDate::today()
    }

    fn rollover_to_date(&mut self, new_today: LocalDate, cx: &mut Context<Self>) -> bool {
        if new_today == self.today {
            return false;
        }
        self.cancel_in_flight_silently();
        self.save_current_draft();
        self.flush_pending_save();
        self.today = new_today;
        self.daily_index = catalog::daily_exercise_index(self.today, self.difficulty, self.pool.len());
        self.exercise_index = self.daily_index;
        self.reload_editor_for_active();
        self.last_report = None;
        self.streak_celebration = None;
        cx.emit(PracticeEvent::StreakUpdated(self.streak_summary()));
        cx.notify();
        true
    }

    fn apply_submission_report(
        &mut self,
        generation: u64,
        originating_date: LocalDate,
        report: SubmissionReport,
        cx: &mut Context<Self>,
    ) {
        if generation != self.submission_generation {
            return;
        }
        self.submitting = false;
        self.active_cancel = None;

        let now_today = self.current_local_date();
        let day_changed = now_today != originating_date;

        if report.verdict == SubmissionVerdict::Accepted {
            // Always credit the originating local calendar day, never the rolled-over day,
            // so a submission started before midnight cannot falsely mark the new day's daily solved.
            let (summary, advanced_origin_day) =
                self.profile
                    .record_accepted(originating_date, &report.exercise_id, report.language);
            self.persist_profile();
            if !day_changed {
                self.streak_celebration = Some(if advanced_origin_day {
                    format!(
                        "Daily streak updated: {} day{}! (Best: {})",
                        summary.current_streak,
                        if summary.current_streak == 1 { "" } else { "s" },
                        summary.best_streak
                    )
                } else {
                    format!(
                        "Accepted! Daily streak stays at {} day{} today.",
                        summary.current_streak,
                        if summary.current_streak == 1 { "" } else { "s" }
                    )
                });
                cx.emit(PracticeEvent::StreakUpdated(summary));
            }
        }

        if day_changed {
            // Date rolled over while the submission was running: rotate cleanly to the new
            // day's daily exercise so `refresh_daily` is never blocked and the new day's slot
            // stays accurate.
            self.rollover_to_date(now_today, cx);
            return;
        }

        self.last_report = Some(report);
        cx.notify();
    }

    fn select_language(&mut self, lang: PracticeLanguage, cx: &mut Context<Self>) {
        if self.language == lang {
            return;
        }
        self.cancel_in_flight_silently();
        self.save_current_draft();
        self.language = lang;
        self.profile.set_preferences(self.language, self.difficulty);
        self.prefs_dirty = true;
        self.reload_editor_for_active();
        self.last_report = None;
        self.streak_celebration = None;
        self.flush_pending_save();
        cx.notify();
    }

    fn select_difficulty(&mut self, diff: Difficulty, cx: &mut Context<Self>) {
        if self.difficulty == diff {
            return;
        }
        self.cancel_in_flight_silently();
        self.save_current_draft();
        self.difficulty = diff;
        self.pool = catalog::exercises_for_difficulty(diff);
        self.daily_index = catalog::daily_exercise_index(self.today, self.difficulty, self.pool.len());
        self.exercise_index = self.daily_index;
        self.profile.set_preferences(self.language, self.difficulty);
        self.prefs_dirty = true;
        self.reload_editor_for_active();
        self.last_report = None;
        self.streak_celebration = None;
        self.flush_pending_save();
        cx.notify();
    }

    fn step_exercise(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.pool.is_empty() {
            return;
        }
        self.cancel_in_flight_silently();
        self.save_current_draft();
        self.flush_pending_save();
        let len = self.pool.len() as isize;
        self.exercise_index = (self.exercise_index as isize + delta).rem_euclid(len) as usize;
        self.reload_editor_for_active();
        self.last_report = None;
        self.streak_celebration = None;
        cx.notify();
    }

    fn jump_to_today_daily(&mut self, cx: &mut Context<Self>) {
        self.cancel_in_flight_silently();
        self.save_current_draft();
        self.flush_pending_save();
        self.today = self.current_local_date();
        self.daily_index = catalog::daily_exercise_index(self.today, self.difficulty, self.pool.len());
        self.exercise_index = self.daily_index;
        self.reload_editor_for_active();
        self.last_report = None;
        self.streak_celebration = None;
        cx.notify();
    }

    fn reset_to_starter(&mut self, cx: &mut Context<Self>) {
        let ex_id = self.active_exercise().id;
        let starter = catalog::starter_code(self.active_exercise(), self.language);
        let key = PracticeProfileState::draft_key(ex_id, self.language);
        self.profile.clear_draft(ex_id, self.language);
        self.pending_draft_deltas.insert(key, None);
        self.editor.set_text(&starter, self.language.indent_spaces());
        self.flush_pending_save();
        self.last_report = None;
        self.streak_celebration = None;
        cx.notify();
    }

    fn cancel_in_flight_silently(&mut self) {
        if let Some(flag) = self.active_cancel.take() {
            flag.store(true, Ordering::Relaxed);
        }
        if self.submitting {
            self.submission_generation = self.submission_generation.wrapping_add(1);
            self.submitting = false;
        }
    }

    fn reload_editor_for_active(&mut self) {
        let code = load_code_for(&self.profile, self.active_exercise(), self.language);
        self.editor.set_text(&code, self.language.indent_spaces());
    }

    fn save_current_draft(&mut self) {
        let ex = self.active_exercise();
        let starter = catalog::starter_code(ex, self.language);
        let cur = self.editor.text();
        let key = PracticeProfileState::draft_key(ex.id, self.language);
        if cur == starter {
            self.profile.clear_draft(ex.id, self.language);
            self.pending_draft_deltas.insert(key, None);
        } else {
            let owned = cur.to_string();
            self.profile.set_draft(ex.id, self.language, owned.clone());
            self.pending_draft_deltas.insert(key, Some(owned));
        }
    }

    fn on_editor_mutated(&mut self, cx: &mut Context<Self>) {
        self.save_current_draft();
        self.scroll_caret_into_view();
        if !self.corrupt_state_blocked {
            self.schedule_debounced_save(cx);
        }
    }

    fn scroll_caret_into_view(&self) {
        let (line, _) = self.editor.cursor_line_col();
        self.editor_scroll.scroll_to_item(line);
    }

    fn column_for_click(&self, line_str: &str, click_x: Pixels, window: &Window) -> usize {
        if line_str.is_empty() {
            return 0;
        }
        // `practice-editor-lines` has p_2 (8px) + row px_1 (4px) + gutter w(28px) + gap_2 (8px) = 48px.
        let scroll_origin_x = self.editor_scroll.bounds().origin.x;
        let text_origin_x = scroll_origin_x + px(48.0);
        let rel_x = (click_x - text_origin_x).max(px(0.0));
        let run = TextRun {
            len: line_str.len(),
            font: Font {
                family: self.mono_font.clone(),
                features: FontFeatures::default(),
                fallbacks: None,
                weight: FontWeight::NORMAL,
                style: FontStyle::Normal,
            },
            color: Hsla::white(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_line(
            line_str.to_string().into(),
            px(self.font_size),
            &[run],
            None,
        );
        let byte_idx = shaped.closest_index_for_x(rel_x).min(line_str.len());
        let mut clamped = byte_idx;
        while clamped > 0 && !line_str.is_char_boundary(clamped) {
            clamped -= 1;
        }
        line_str[..clamped].chars().count()
    }

    fn schedule_debounced_save(&mut self, cx: &mut Context<Self>) {
        self.draft_save_seq = self.draft_save_seq.wrapping_add(1);
        let seq = self.draft_save_seq;
        cx.spawn(async move |view, cx| {
            cx.background_executor().timer(DRAFT_SAVE_DEBOUNCE).await;
            let _ = view.update(cx, |this, cx| {
                if this.draft_save_seq == seq
                    && !this.corrupt_state_blocked
                    && (!this.pending_draft_deltas.is_empty() || this.prefs_dirty)
                {
                    this.persist_profile();
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn persist_profile(&mut self) {
        if self.corrupt_state_blocked {
            return;
        }
        let deltas = std::mem::take(&mut self.pending_draft_deltas);
        let prefs_dirty = self.prefs_dirty;
        match self
            .profile
            .save_merged(&self.profile_dir, &deltas, prefs_dirty)
        {
            Ok(()) => {
                self.prefs_dirty = false;
                if self.storage_notice.as_deref().is_some_and(|m| m.starts_with("Failed")) {
                    self.storage_notice = None;
                }
            }
            Err(err) => {
                if err.contains("Refusing to overwrite") {
                    self.corrupt_state_blocked = true;
                }
                // Restore pending deltas so a later retry/flush does not drop edits.
                for (k, v) in deltas {
                    self.pending_draft_deltas.entry(k).or_insert(v);
                }
                self.storage_notice = Some(err);
            }
        }
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.handle_keystroke(&ev.keystroke, cx) {
            cx.stop_propagation();
        }
    }

    // ── Rendering helpers ───────────────────────────────────────────────────

    fn render_header(&self, p: GuiPalette, cx: &mut Context<Self>) -> impl IntoElement {
        let streak = self.streak_summary();
        let streak_color = if streak.solved_today {
            p.success
        } else if streak.current_streak > 0 {
            p.warning
        } else {
            p.text_muted
        };
        let streak_bg = if streak.solved_today {
            blend_u32(p.bg_panel, p.success, 18)
        } else if streak.current_streak > 0 {
            blend_u32(p.bg_panel, p.warning, 16)
        } else {
            p.bg_surface
        };
        let streak_text = if streak.solved_today {
            format!(
                "{}d streak · Best {} · Today done",
                streak.current_streak, streak.best_streak
            )
        } else if streak.current_streak > 0 {
            format!(
                "{}d streak · Best {} · Solve today",
                streak.current_streak, streak.best_streak
            )
        } else {
            format!("0d streak · Best {}", streak.best_streak)
        };

        div()
            .debug_selector(|| "practice-header".into())
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .px_3()
            .py_2()
            .bg(rgb(p.bg_panel))
            .border_b_1()
            .border_color(rgb(p.border_subtle))
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .child(icon("file-code", 15.0, p.accent))
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(p.text_strong))
                            .whitespace_nowrap()
                            .child("Daily Practice"),
                    )
                    .child(
                        div()
                            .px_2()
                            .py(px(1.5))
                            .rounded(px(4.0))
                            .bg(rgb(p.bg_surface))
                            .text_size(px(10.0))
                            .text_color(rgb(p.text_muted))
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .child(self.today.to_string()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_shrink_0()
                    .child(
                        div()
                            .debug_selector(|| "practice-streak".into())
                            .px_2()
                            .py(px(2.5))
                            .rounded(px(5.0))
                            .border_1()
                            .border_color(rgb(blend_u32(p.border_subtle, streak_color, 40)))
                            .bg(rgb(streak_bg))
                            .text_size(px(10.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(streak_color))
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .child(streak_text),
                    )
                    .when(self.show_close_button, |d| {
                        d.child(
                            div()
                                .id("practice-close")
                                .debug_selector(|| "practice-close".into())
                                .size(px(22.0))
                                .rounded(px(4.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(rgb(blend_u32(p.bg_panel, p.error, 22))))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.flush_pending_save();
                                    cx.emit(PracticeEvent::CloseRequested);
                                }))
                                .child(icon("x", 13.0, p.text_muted)),
                        )
                    }),
            )
    }

    fn render_controls(&self, p: GuiPalette, cx: &mut Context<Self>) -> impl IntoElement {
        let mut lang_row = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.0));

        for &lang in PracticeLanguage::ALL {
            let active = lang == self.language;
            let avail = self.toolchains.is_available(lang);
            let dot_color = if avail { p.success } else { p.text_muted };
            lang_row = lang_row.child(
                div()
                    .id(SharedString::from(format!("practice-lang-{}", lang.id())))
                    .debug_selector(move || format!("practice-lang-{}", lang.id()))
                    .px(px(7.0))
                    .py(px(3.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(rgb(if active { p.accent } else { p.border_subtle }))
                    .bg(rgb(if active { p.bg_selected } else { p.bg_surface }))
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_language(lang, cx);
                    }))
                    .child(
                        div()
                            .size(px(5.5))
                            .rounded_full()
                            .bg(rgb(dot_color))
                            .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .font_weight(if active {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(rgb(if active {
                                p.text_strong
                            } else if avail {
                                p.text
                            } else {
                                p.text_muted
                            }))
                            .whitespace_nowrap()
                            .child(lang.short_label()),
                    ),
            );
        }

        let mut diff_row = div()
            .w_full()
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_2();

        let mut diffs = div().flex().items_center().gap(px(4.0));
        for &diff in Difficulty::ALL {
            let active = diff == self.difficulty;
            let accent = match diff {
                Difficulty::Easy => p.success,
                Difficulty::Medium => p.warning,
                Difficulty::Hard => p.error,
            };
            diffs = diffs.child(
                div()
                    .id(SharedString::from(format!("practice-diff-{}", diff.id())))
                    .debug_selector(move || format!("practice-diff-{}", diff.id()))
                    .px(px(8.0))
                    .py(px(3.0))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(rgb(if active { accent } else { p.border_subtle }))
                    .bg(rgb(if active {
                        blend_u32(p.bg_panel, accent, 18)
                    } else {
                        p.bg_surface
                    }))
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .flex_shrink_0()
                    .text_size(px(11.0))
                    .font_weight(if active {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(rgb(if active { accent } else { p.text_muted }))
                    .whitespace_nowrap()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_difficulty(diff, cx);
                    }))
                    .child(diff.label()),
            );
        }

        let nav = div()
            .flex()
            .items_center()
            .gap(px(4.0))
            .flex_shrink_0()
            .child(
                div()
                    .id("practice-prev-ex")
                    .px(px(6.0))
                    .py(px(2.5))
                    .rounded(px(4.0))
                    .bg(rgb(p.bg_surface))
                    .border_1()
                    .border_color(rgb(p.border_subtle))
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .text_size(px(11.0))
                    .text_color(rgb(p.text))
                    .on_click(cx.listener(|this, _, _, cx| this.step_exercise(-1, cx)))
                    .child("‹"),
            )
            .child(
                div()
                    .id("practice-today-ex")
                    .px(px(8.0))
                    .py(px(2.5))
                    .rounded(px(4.0))
                    .border_1()
                    .border_color(rgb(if self.is_today_daily() {
                        p.accent
                    } else {
                        p.border_subtle
                    }))
                    .bg(rgb(if self.is_today_daily() {
                        p.bg_selected
                    } else {
                        p.bg_surface
                    }))
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .text_size(px(10.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(if self.is_today_daily() {
                        p.text_strong
                    } else {
                        p.text_muted
                    }))
                    .whitespace_nowrap()
                    .on_click(cx.listener(|this, _, _, cx| this.jump_to_today_daily(cx)))
                    .child(if self.is_today_daily() {
                        format!("Daily ({}/{})", self.exercise_index + 1, self.pool.len())
                    } else {
                        format!(
                            "Drill {}/{} · Jump to Daily",
                            self.exercise_index + 1,
                            self.pool.len()
                        )
                    }),
            )
            .child(
                div()
                    .id("practice-next-ex")
                    .px(px(6.0))
                    .py(px(2.5))
                    .rounded(px(4.0))
                    .bg(rgb(p.bg_surface))
                    .border_1()
                    .border_color(rgb(p.border_subtle))
                    .hover(|s| s.bg(rgb(p.bg_hover)))
                    .cursor_pointer()
                    .text_size(px(11.0))
                    .text_color(rgb(p.text))
                    .on_click(cx.listener(|this, _, _, cx| this.step_exercise(1, cx)))
                    .child("›"),
            );

        diff_row = diff_row.child(diffs).child(nav);

        div()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .px_3()
            .py_2()
            .bg(rgb(p.bg_panel))
            .border_b_1()
            .border_color(rgb(p.border_subtle))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(lang_row)
            .child(diff_row)
    }

    fn render_problem_card(&self, p: GuiPalette, cx: &mut Context<Self>) -> impl IntoElement {
        let ex = self.active_exercise();
        let solved_today = self.profile.is_solved_on(self.today, ex.id, self.language);
        let solved_ever = self.profile.is_exercise_solved(ex.id, self.language);
        let tc_status = self.toolchains.get(self.language);

        let mut card = div()
            .debug_selector(|| "practice-problem".into())
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .p_3()
            .rounded(px(6.0))
            .border_1()
            .border_color(rgb(p.border_subtle))
            .bg(rgb(p.bg_surface))
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(p.text_strong))
                                    .child(ex.title),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(rgb(p.text_muted))
                                    .whitespace_nowrap()
                                    .child(ex.difficulty.estimated_minutes()),
                            )
                            .when(solved_today || solved_ever, |d| {
                                d.child(
                                    div()
                                        .px(px(6.0))
                                        .py(px(1.5))
                                        .rounded(px(4.0))
                                        .bg(rgb(blend_u32(p.bg_surface, p.success, 20)))
                                        .text_size(px(10.0))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(p.success))
                                        .whitespace_nowrap()
                                        .child(if solved_today { "✓ Solved today" } else { "✓ Solved" }),
                                )
                            }),
                    )
                    .child(
                        div()
                            .id("practice-toggle-problem")
                            .px_2()
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .hover(|s| s.bg(rgb(p.bg_hover)))
                            .cursor_pointer()
                            .text_size(px(10.5))
                            .text_color(rgb(p.text_muted))
                            .whitespace_nowrap()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.problem_collapsed = !this.problem_collapsed;
                                cx.notify();
                            }))
                            .child(if self.problem_collapsed {
                                "Show prompt"
                            } else {
                                "Hide prompt"
                            }),
                    ),
            );

        if !tc_status.available {
            card = card.child(
                div()
                    .debug_selector(|| "practice-toolchain-warning".into())
                    .w_full()
                    .min_w_0()
                    .p_2()
                    .rounded(px(5.0))
                    .border_1()
                    .border_color(rgb(blend_u32(p.border_subtle, p.warning, 50)))
                    .bg(rgb(blend_u32(p.bg_surface, p.warning, 14)))
                    .whitespace_normal()
                    .text_size(px(11.5))
                    .text_color(rgb(p.warning))
                    .child(
                        tc_status
                            .unavailable_reason
                            .unwrap_or_else(|| self.language.install_hint().to_string()),
                    ),
            );
        }

        if let Some(notice) = &self.storage_notice {
            let mut notice_row = div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_col()
                .items_start()
                .gap_1()
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(11.0))
                        .text_color(rgb(p.warning))
                        .child(notice.clone()),
                );
            if self.corrupt_state_blocked {
                notice_row = notice_row.child(
                    div()
                        .id("practice-recover-corrupt")
                        .debug_selector(|| "practice-recover-corrupt".into())
                        .px_2()
                        .py(px(3.0))
                        .rounded(px(4.0))
                        .border_1()
                        .border_color(rgb(p.warning))
                        .bg(rgb(blend_u32(p.bg_surface, p.warning, 18)))
                        .hover(|s| s.bg(rgb(blend_u32(p.bg_surface, p.warning, 28))))
                        .cursor_pointer()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(p.warning))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Err(err) = this.recover_corrupt_state(cx) {
                                this.storage_notice = Some(err);
                                cx.notify();
                            }
                        }))
                        .child("Backup Corrupt File & Reset"),
                );
            }
            card = card.child(notice_row);
        }

        if !self.problem_collapsed {
            card = card
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(12.0))
                        .text_color(rgb(p.text))
                        .child(ex.description),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(11.0))
                        .text_color(rgb(p.text_muted))
                        .child(format!(
                            "Constraints: {} · {} test cases ({} visible, {} hidden)",
                            ex.constraints,
                            ex.tests.len(),
                            ex.tests.iter().filter(|t| !t.hidden).count(),
                            ex.tests.iter().filter(|t| t.hidden).count()
                        )),
                );

            let mut examples = div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .pt_1();
            for (idx, (inp, exp)) in ex.visible_examples().into_iter().enumerate() {
                examples = examples.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .px_2()
                        .py(px(4.0))
                        .rounded(px(4.0))
                        .bg(rgb(p.bg_input))
                        .font_family(self.mono_font.clone())
                        .text_size(px((self.font_size - 1.5).max(10.5)))
                        .text_color(rgb(p.text))
                        .whitespace_normal()
                        .child(format!("Ex {}: {}  ->  {}", idx + 1, inp, exp)),
                );
            }
            card = card.child(examples);
        }

        card
    }

    fn render_editor_section(
        &self,
        p: GuiPalette,
        is_focused: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (cur_line, cur_col) = self.editor.cursor_line_col();
        let selection = self.editor.selection_range();
        let ranges = self.editor.line_ranges();
        let text = self.editor.text();
        let tc_status = self.toolchains.get(self.language);
        let can_submit = tc_status.available && !self.submitting;

        let toolbar = div()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .px_3()
            .py(px(6.0))
            .bg(rgb(p.bg_panel))
            .border_b_1()
            .border_color(rgb(p.border_subtle))
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .child(
                        div()
                            .font_family(self.mono_font.clone())
                            .text_size(px(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(p.text_strong))
                            .whitespace_nowrap()
                            .child(self.language.file_name()),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(rgb(p.text_muted))
                            .whitespace_nowrap()
                            .child(format!("Ln {}, Col {}", cur_line + 1, cur_col + 1)),
                    )
                    .when_some(tc_status.version.clone(), |d, ver| {
                        d.child(
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(p.text_muted))
                                .truncate()
                                .child(ver),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .flex_shrink_0()
                    .child(
                        div()
                            .id("practice-reset")
                            .debug_selector(|| "practice-reset".into())
                            .px_2()
                            .py(px(3.0))
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(rgb(p.border_subtle))
                            .bg(rgb(p.bg_surface))
                            .hover(|s| s.bg(rgb(p.bg_hover)))
                            .cursor_pointer()
                            .text_size(px(11.0))
                            .text_color(rgb(p.text))
                            .whitespace_nowrap()
                            .on_click(cx.listener(|this, _, window, cx| {
                                window.focus(&this.focus_handle);
                                this.reset_to_starter(cx);
                            }))
                            .child("Reset"),
                    )
                    .child(
                        div()
                            .id("practice-paste")
                            .px_2()
                            .py(px(3.0))
                            .rounded(px(4.0))
                            .border_1()
                            .border_color(rgb(p.border_subtle))
                            .bg(rgb(p.bg_surface))
                            .hover(|s| s.bg(rgb(p.bg_hover)))
                            .cursor_pointer()
                            .text_size(px(11.0))
                            .text_color(rgb(p.text))
                            .whitespace_nowrap()
                            .on_click(cx.listener(|this, _, window, cx| {
                                window.focus(&this.focus_handle);
                                if let Some(item) = cx.read_from_clipboard() {
                                    if let Some(txt) = item.text() {
                                        if this.editor.insert_str(&txt) {
                                            this.on_editor_mutated(cx);
                                            cx.notify();
                                        }
                                    }
                                }
                            }))
                            .child("Paste"),
                    )
                    .child(if self.submitting {
                        div()
                            .id("practice-cancel")
                            .debug_selector(|| "practice-cancel".into())
                            .px_3()
                            .py(px(3.5))
                            .rounded(px(5.0))
                            .bg(rgb(blend_u32(p.bg_panel, p.warning, 26)))
                            .border_1()
                            .border_color(rgb(p.warning))
                            .hover(|s| s.bg(rgb(blend_u32(p.bg_panel, p.warning, 36))))
                            .cursor_pointer()
                            .text_size(px(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(p.warning))
                            .whitespace_nowrap()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cancel_submission(cx);
                            }))
                            .child("Cancel Run")
                    } else {
                        div()
                            .id("practice-submit")
                            .debug_selector(|| "practice-submit".into())
                            .px_3()
                            .py(px(3.5))
                            .rounded(px(5.0))
                            .bg(rgb(if can_submit { p.accent } else { p.bg_elevated }))
                            .when(can_submit, |d| {
                                d.hover(|s| s.bg(rgb(blend_u32(p.accent, 0xffffff, 12))))
                                    .cursor_pointer()
                            })
                            .text_size(px(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(if can_submit {
                                p.accent_fg
                            } else {
                                p.text_muted
                            }))
                            .whitespace_nowrap()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                window.focus(&this.focus_handle);
                                this.submit(cx);
                            }))
                            .child("Submit (Ctrl+Enter)")
                    }),
            );

        let mut lines_col = div()
            .id("practice-editor-lines")
            .track_scroll(&self.editor_scroll)
            .w_full()
            .min_w_0()
            .min_h(px(140.0))
            .max_h(px(320.0))
            .overflow_y_scroll()
            .p_2()
            .bg(rgb(p.bg_input))
            .font_family(self.mono_font.clone())
            .text_size(px(self.font_size))
            .cursor_text()
            .flex()
            .flex_col()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.focus_handle);
                    cx.notify();
                }),
            );

        for (line_idx, &(start, end)) in ranges.iter().enumerate() {
            let line_str = &text[start..end];
            let is_cur = line_idx == cur_line;
            let is_selected_line = selection
                .is_some_and(|(s, e)| start < e && end >= s && (s < e));

            let rendered_line = if is_cur && is_focused {
                let rel = self.editor.cursor_byte().saturating_sub(start).min(line_str.len());
                let left = &line_str[..rel];
                let right = &line_str[rel..];
                format!("{left}▏{right}")
            } else if line_str.is_empty() {
                " ".to_string()
            } else {
                line_str.to_string()
            };

            let row_bg = if is_selected_line {
                blend_u32(p.bg_input, p.accent, 24)
            } else if is_cur && is_focused {
                blend_u32(p.bg_input, p.text_strong, 6)
            } else {
                p.bg_input
            };

            let line_owned = line_str.to_string();
            lines_col = lines_col.child(
                div()
                    .id(("practice-line", line_idx))
                    .debug_selector(move || format!("practice-line-{line_idx}"))
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .px_1()
                    .py(px(1.0))
                    .rounded(px(3.0))
                    .bg(rgb(row_bg))
                    .flex()
                    .items_start()
                    .gap_2()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.focus(&this.focus_handle);
                            let col = this.column_for_click(&line_owned, ev.position.x, window);
                            this.editor.set_cursor_line_col(line_idx, col, false);
                            this.scroll_caret_into_view();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .w(px(28.0))
                            .flex_shrink_0()
                            .text_size(px((self.font_size - 1.5).max(10.0)))
                            .text_color(rgb(if is_cur && is_focused {
                                p.accent
                            } else {
                                p.text_muted
                            }))
                            .child(format!("{:>2}", line_idx + 1)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_color(rgb(p.text_strong))
                            .child(rendered_line),
                    ),
            );
        }

        let trust_banner = div()
            .debug_selector(|| "practice-trust-warning".into())
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .px_3()
            .py(px(5.0))
            .bg(rgb(blend_u32(p.bg_panel, p.warning, 10)))
            .border_b_1()
            .border_color(rgb(p.border_subtle))
            .whitespace_normal()
            .text_size(px(10.5))
            .text_color(rgb(p.warning))
            .child("Local execution runs under your user account with basic time/memory/output limits (not a security sandbox). Only submit code you trust.");

        div()
            .debug_selector(|| "practice-editor".into())
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .rounded(px(6.0))
            .overflow_hidden()
            .border_1()
            .border_color(rgb(if is_focused { p.accent } else { p.border_subtle }))
            .flex()
            .flex_col()
            .child(toolbar)
            .child(trust_banner)
            .child(lines_col)
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .px_3()
                    .py(px(4.0))
                    .bg(rgb(p.bg_panel))
                    .border_t_1()
                    .border_color(rgb(p.border_subtle))
                    .text_size(px(10.0))
                    .text_color(rgb(p.text_muted))
                    .whitespace_normal()
                    .child("Click code to edit · Tab/Shift+Tab indent · Ctrl+Z/Y undo/redo · Ctrl+V paste · Ctrl+Enter submit"),
            )
    }

    fn render_results_section(&self, p: GuiPalette) -> impl IntoElement {
        let mut box_el = div()
            .debug_selector(|| "practice-results".into())
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .p_3()
            .rounded(px(6.0))
            .border_1()
            .border_color(rgb(p.border_subtle))
            .bg(rgb(p.bg_surface))
            .flex()
            .flex_col()
            .gap_2();

        if self.submitting {
            return box_el
                .child(
                    div()
                        .text_size(px(12.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(p.accent))
                        .child(format!(
                            "Running {} solution against {} test cases…",
                            self.language.label(),
                            self.active_exercise().tests.len()
                        )),
                )
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .whitespace_normal()
                        .text_size(px(11.0))
                        .text_color(rgb(p.text_muted))
                        .child(format!(
                            "Isolated temp dir · sanitized env · limits: {:.0}s wall, {}s CPU, {} MiB mem. UI remains interactive.",
                            self.limits.exec_timeout.as_secs_f32(),
                            self.limits.exec_cpu_secs,
                            self.limits.exec_memory_bytes / (1024 * 1024)
                        )),
                );
        }

        let Some(report) = &self.last_report else {
            let probe_note = if self.probing_toolchains {
                "Checking installed local compilers and runtimes…"
            } else {
                "Ready. Press Ctrl+Enter or click Submit to run all visible and hidden test cases locally."
            };
            return box_el.child(
                div()
                    .w_full()
                    .min_w_0()
                    .whitespace_normal()
                    .text_size(px(11.5))
                    .text_color(rgb(p.text_muted))
                    .child(format!(
                        "{probe_note} ({}/{} languages available locally)",
                        self.toolchains.available_count(),
                        PracticeLanguage::ALL.len()
                    )),
            );
        };

        let verdict_color = match report.verdict {
            SubmissionVerdict::Accepted => p.success,
            SubmissionVerdict::WrongAnswer
            | SubmissionVerdict::CompileError
            | SubmissionVerdict::RuntimeError
            | SubmissionVerdict::InternalError => p.error,
            SubmissionVerdict::TimeLimitExceeded | SubmissionVerdict::ToolchainUnavailable => {
                p.warning
            }
            SubmissionVerdict::Cancelled => p.text_muted,
        };

        box_el = box_el
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .debug_selector(|| "practice-verdict".into())
                            .text_size(px(13.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(verdict_color))
                            .child(format!(
                                "{} · {}/{} passed",
                                report.verdict.label(),
                                report.passed_count,
                                report.total_count
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(rgb(p.text_muted))
                            .whitespace_nowrap()
                            .child(format!("{} ms", report.elapsed_ms)),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .whitespace_normal()
                    .text_size(px(11.5))
                    .text_color(rgb(p.text))
                    .child(report.summary.clone()),
            );

        if let Some(celebration) = &self.streak_celebration {
            box_el = box_el.child(
                div()
                    .w_full()
                    .min_w_0()
                    .px_2()
                    .py(px(4.0))
                    .rounded(px(4.0))
                    .bg(rgb(blend_u32(p.bg_surface, p.success, 18)))
                    .whitespace_normal()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(p.success))
                    .child(celebration.clone()),
            );
        }

        if !report.cases.is_empty() {
            let mut cases_el = div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(4.0));
            for c in &report.cases {
                let badge_col = if c.passed { p.success } else { p.error };
                let label = if c.hidden {
                    format!("Hidden Case {}", c.index + 1)
                } else {
                    format!("Case {}", c.index + 1)
                };
                let mut row = div()
                    .w_full()
                    .min_w_0()
                    .px_2()
                    .py(px(4.0))
                    .rounded(px(4.0))
                    .bg(rgb(p.bg_input))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(badge_col))
                                    .child(if c.passed { "✓" } else { "✗" }),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(p.text_strong))
                                    .child(label),
                            ),
                    );
                if !c.passed {
                    if let Some(err) = &c.error {
                        row = row.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .font_family(self.mono_font.clone())
                                .text_size(px(10.5))
                                .text_color(rgb(p.error))
                                .child(err.clone()),
                        );
                    } else {
                        row = row.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .whitespace_normal()
                                .font_family(self.mono_font.clone())
                                .text_size(px(10.5))
                                .text_color(rgb(p.text_muted))
                                .child(format!(
                                    "Input: {} | Expected: {} | Actual: {}",
                                    c.input_display, c.expected_display, c.actual_display
                                )),
                        );
                    }
                }
                cases_el = cases_el.child(row);
            }
            box_el = box_el.child(cases_el);
        }

        if !report.stderr.trim().is_empty() {
            box_el = box_el.child(
                div()
                    .w_full()
                    .min_w_0()
                    .p_2()
                    .rounded(px(4.0))
                    .bg(rgb(p.bg_input))
                    .font_family(self.mono_font.clone())
                    .text_size(px(10.5))
                    .text_color(rgb(p.error))
                    .whitespace_normal()
                    .child(truncate_preview(&report.stderr, 1600)),
            );
        }

        if !report.stdout.trim().is_empty() {
            let note = if report.output_truncated {
                " (truncated)"
            } else {
                ""
            };
            box_el = box_el.child(
                div()
                    .w_full()
                    .min_w_0()
                    .p_2()
                    .rounded(px(4.0))
                    .bg(rgb(p.bg_input))
                    .font_family(self.mono_font.clone())
                    .text_size(px(10.5))
                    .text_color(rgb(p.text_muted))
                    .whitespace_normal()
                    .child(format!(
                        "stdout{note}:\n{}",
                        truncate_preview(&report.stdout, 1000)
                    )),
            );
        }

        box_el
    }
}

fn load_code_for(
    profile: &PracticeProfileState,
    exercise: &Exercise,
    language: PracticeLanguage,
) -> String {
    profile
        .get_draft(exercise.id, language)
        .map(|s| s.to_string())
        .unwrap_or_else(|| catalog::starter_code(exercise, language))
}

fn truncate_preview(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push_str("\n… [output truncated]");
        out
    }
}

impl Drop for PracticePanel {
    fn drop(&mut self) {
        self.cancel_in_flight_silently();
        self.flush_pending_save();
    }
}

impl Focusable for PracticePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<PracticeEvent> for PracticePanel {}

impl Render for PracticePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = gui_palette(&self.theme);
        let is_focused = self.focus_handle.is_focused(window);

        let body = div()
            .id("practice-body-scroll")
            .track_scroll(&self.main_scroll)
            .flex_1()
            .min_h_0()
            .w_full()
            .min_w_0()
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(self.render_problem_card(p, cx))
            .child(self.render_editor_section(p, is_focused, cx))
            .child(self.render_results_section(p));

        div()
            .id("practice-panel")
            .debug_selector(|| "practice-panel".into())
            .track_focus(&self.focus_handle)
            .font_family(self.ui_font.clone())
            .size_full()
            .min_w_0()
            .min_h_0()
            .bg(rgb(p.bg_editor))
            .text_color(rgb(p.text))
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.render_header(p, cx))
            .child(self.render_controls(p, cx))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Difficulty, LocalDate, PracticeLanguage, PracticePanel, PracticeProfileState,
        SubmissionReport, SubmissionVerdict,
    };
    use gpui::{point, px, size, Keystroke, Modifiers, MouseButton, TestAppContext};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static UI_TEST_SEQ: AtomicU64 = AtomicU64::new(1);

    fn unique_profile_dir(tag: &str) -> PathBuf {
        let n = UI_TEST_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "gtuim-practice-ui-test-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[gpui::test]
    fn practice_panel_renders_cleanly_at_small_and_large_sizes(cx: &mut TestAppContext) {
        let profile = unique_profile_dir("layout");
        for (width, height) in [(1100.0, 760.0), (420.0, 500.0)] {
            let p_dir = profile.clone();
            let (_, window) = cx.add_window_view(move |_, cx| PracticePanel::new(p_dir, cx));
            window.simulate_resize(size(px(width), px(height)));
            window.run_until_parked();

            let panel = window.debug_bounds("practice-panel").expect("panel rendered");
            let header = window.debug_bounds("practice-header").expect("header rendered");
            let streak = window.debug_bounds("practice-streak").expect("streak badge");
            let problem = window.debug_bounds("practice-problem").expect("problem card");
            let editor = window.debug_bounds("practice-editor").expect("editor area");
            let submit = window.debug_bounds("practice-submit").expect("submit button");
            let trust = window
                .debug_bounds("practice-trust-warning")
                .expect("trusted local code warning adjacent to submit");
            assert!(window.debug_bounds("practice-lang-c").is_some(), "C picker button");
            assert!(window.debug_bounds("practice-lang-zig").is_some(), "Zig picker button");

            assert!(header.size.width <= panel.size.width + px(1.0));
            assert!(streak.size.width >= px(60.0));
            assert!(problem.size.width <= panel.size.width + px(1.0));
            assert!(editor.size.height >= px(120.0));
            assert!(submit.size.width >= px(60.0));
            assert!(trust.size.width <= panel.size.width + px(1.0));
            assert!(trust.top() >= submit.top(), "trust warning sits adjacent to submit toolbar");
        }
        let _ = fs::remove_dir_all(&profile);
    }

    #[gpui::test]
    fn widget_supports_editing_nonblocking_submit_and_persisted_streak(cx: &mut TestAppContext) {
        let profile_a = unique_profile_dir("submit-a");
        let profile_b = unique_profile_dir("submit-b");
        let p_dir = profile_a.clone();
        let (view, window) = cx.add_window_view(move |_, cx| {
            let mut panel = PracticePanel::new(p_dir, cx);
            // Pin to deterministic known exercise (`digital-root`, Python) so test is date-independent.
            panel.select_language(PracticeLanguage::Python, cx);
            panel.select_difficulty(Difficulty::Easy, cx);
            if let Some(pos) = panel.pool.iter().position(|e| e.id == "digital-root") {
                panel.exercise_index = pos;
                panel.reload_editor_for_active();
            }
            panel
        });
        window.simulate_resize(size(px(900.0), px(720.0)));
        window.run_until_parked();

        // Set a correct Python solution via widget API and trigger non-blocking submit.
        view.update(window, |panel, cx| {
            panel.set_solution_text(
                "def solve(n: int) -> int:\n    return 0 if n == 0 else 1 + (n - 1) % 9\n",
                cx,
            );
            panel.submit(cx);
            assert!(panel.is_submitting(), "submit should launch asynchronously");
        });

        window.run_until_parked();

        view.read_with(window, |panel, _| {
            assert!(!panel.is_submitting());
            let rep = panel.last_report().expect("submission report present");
            assert_eq!(rep.verdict, SubmissionVerdict::Accepted);
            assert_eq!(rep.passed_count, rep.total_count);
            let streak = panel.streak_summary();
            assert_eq!(streak.current_streak, 1);
            assert_eq!(streak.best_streak, 1);
            assert!(streak.solved_today);
        });

        // Re-opening a new PracticePanel on the same profile directory restores the streak.
        let p_dir2 = profile_a.clone();
        let (view2, window2) = cx.add_window_view(move |_, cx| PracticePanel::new(p_dir2, cx));
        window2.run_until_parked();
        view2.read_with(window2, |panel, _| {
            let streak = panel.streak_summary();
            assert_eq!(streak.current_streak, 1);
            assert!(streak.solved_today);
        });

        // A distinct profile directory remains completely isolated with streak = 0.
        let p_dir_b = profile_b.clone();
        let (view_b, window_b) = cx.add_window_view(move |_, cx| PracticePanel::new(p_dir_b, cx));
        window_b.run_until_parked();
        view_b.read_with(window_b, |panel, _| {
            let streak = panel.streak_summary();
            assert_eq!(streak.current_streak, 0);
            assert!(!streak.solved_today);
        });

        let _ = fs::remove_dir_all(&profile_a);
        let _ = fs::remove_dir_all(&profile_b);
    }

    #[gpui::test]
    fn keyboard_editing_and_language_switch_preserve_drafts(cx: &mut TestAppContext) {
        let profile = unique_profile_dir("keyboard");
        let p_dir = profile.clone();
        let (view, window) = cx.add_window_view(move |_, cx| PracticePanel::new(p_dir, cx));
        window.simulate_resize(size(px(900.0), px(700.0)));
        window.run_until_parked();

        view.update(window, |panel, cx| {
            panel.select_language(PracticeLanguage::Python, cx);
            let daily_id = panel.active_exercise_id();
            let state_file = PracticeProfileState::state_file_path(&profile);
            // Typing keystrokes must be debounced (no synchronous disk write per keystroke).
            let _ = fs::remove_file(&state_file);
            panel.set_solution_text("def solve(n):", cx);
            let enter = Keystroke::parse("enter").unwrap();
            assert!(panel.handle_keystroke(&enter, cx));
            let r = Keystroke::parse("r").unwrap();
            assert!(panel.handle_keystroke(&r, cx));
            assert_eq!(panel.solution_text(), "def solve(n):\n    r");
            assert!(
                !state_file.exists(),
                "typing keystrokes must not synchronously write practice_state.json"
            );

            // Switching to C flushes the pending Python draft immediately, keeps the exact
            // same daily exercise, and loads C's starter template.
            panel.select_language(PracticeLanguage::C, cx);
            assert!(
                state_file.exists(),
                "switching language must reliably flush pending draft to disk"
            );
            assert_eq!(panel.active_exercise_id(), daily_id);
            assert!(panel.solution_text().contains("solve("));

            panel.select_language(PracticeLanguage::Zig, cx);
            assert_eq!(panel.active_exercise_id(), daily_id);
            assert!(panel.solution_text().contains("pub fn solve("));

            panel.select_language(PracticeLanguage::Python, cx);
            assert_eq!(panel.active_exercise_id(), daily_id);
            assert_eq!(panel.solution_text(), "def solve(n):\n    r");
        });

        let _ = fs::remove_dir_all(&profile);
    }

    #[gpui::test]
    fn widget_runs_real_c_and_zig_accepted_and_rejected_submissions(cx: &mut TestAppContext) {
        let profile = unique_profile_dir("c-and-zig");
        let p_dir = profile.clone();
        let (view, window) = cx.add_window_view(move |_, cx| {
            let mut panel = PracticePanel::new(p_dir, cx);
            panel.select_difficulty(Difficulty::Easy, cx);
            if let Some(pos) = panel.pool.iter().position(|e| e.id == "digital-root") {
                panel.exercise_index = pos;
                panel.reload_editor_for_active();
            }
            panel
        });
        window.simulate_resize(size(px(900.0), px(720.0)));
        window.run_until_parked();

        // 1. Rejected C submission (WrongAnswer) must not advance streak.
        view.update(window, |panel, cx| {
            panel.select_language(PracticeLanguage::C, cx);
            panel.set_solution_text("long long solve(long long n) { (void)n; return 0; }\n", cx);
            panel.submit(cx);
        });
        window.run_until_parked();
        view.read_with(window, |panel, _| {
            assert_eq!(panel.last_report().unwrap().verdict, SubmissionVerdict::WrongAnswer);
            assert_eq!(panel.streak_summary().current_streak, 0);
        });

        // 2. Accepted C submission passes and updates streak to 1.
        view.update(window, |panel, cx| {
            panel.set_solution_text(
                "long long solve(long long n) { return n == 0 ? 0 : 1 + (n - 1) % 9; }\n",
                cx,
            );
            panel.submit(cx);
        });
        window.run_until_parked();
        view.read_with(window, |panel, _| {
            assert_eq!(panel.last_report().unwrap().verdict, SubmissionVerdict::Accepted);
            assert_eq!(panel.streak_summary().current_streak, 1);
            assert!(panel.streak_summary().solved_today);
        });

        // 3. Rejected Zig submission (WrongAnswer).
        view.update(window, |panel, cx| {
            panel.select_language(PracticeLanguage::Zig, cx);
            panel.set_solution_text("pub fn solve(n: i64) i64 { _ = n; return 0; }\n", cx);
            panel.submit(cx);
        });
        window.run_until_parked();
        view.read_with(window, |panel, _| {
            assert_eq!(panel.last_report().unwrap().verdict, SubmissionVerdict::WrongAnswer);
        });

        // 4. Accepted Zig submission passes cleanly on the same exercise.
        view.update(window, |panel, cx| {
            panel.set_solution_text(
                "pub fn solve(n: i64) i64 { return if (n == 0) 0 else 1 + @rem(n - 1, 9); }\n",
                cx,
            );
            panel.submit(cx);
        });
        window.run_until_parked();
        view.read_with(window, |panel, _| {
            assert_eq!(panel.last_report().unwrap().verdict, SubmissionVerdict::Accepted);
            assert_eq!(panel.streak_summary().current_streak, 1);
        });

        let _ = fs::remove_dir_all(&profile);
    }

    #[gpui::test]
    fn midnight_rollover_invalidates_stale_runs_and_credits_originating_day(
        cx: &mut TestAppContext,
    ) {
        let profile = unique_profile_dir("rollover");
        let day1 = LocalDate::from_ymd(2026, 11, 25).unwrap();
        let day2 = LocalDate::from_ymd(2026, 11, 26).unwrap();
        let day1_ex = super::catalog::daily_exercise(day1, Difficulty::Easy).id;
        let day2_ex = super::catalog::daily_exercise(day2, Difficulty::Easy).id;
        assert_ne!(day1_ex, day2_ex, "choose two consecutive days with distinct dailies");

        let p_dir = profile.clone();
        let (view, window) = cx.add_window_view(move |_, cx| {
            let mut panel = PracticePanel::new(p_dir, cx);
            panel.test_today_override = Some(day1);
            panel.rollover_to_date(day1, cx);
            panel.select_language(PracticeLanguage::Python, cx);
            panel
        });
        window.run_until_parked();

        // Case A: refresh_daily runs after midnight while Day 1 submission is in flight ->
        // invalidates old generation and rotates to Day 2's exercise without marking Day 2 solved.
        view.update(window, |panel, cx| {
            assert_eq!(panel.active_exercise_id(), day1_ex);
            panel.submission_generation = 10;
            panel.submitting = true;
            panel.test_today_override = Some(day2);
            panel.refresh_daily(cx);
            assert!(!panel.is_submitting());
            assert_eq!(panel.active_exercise_id(), day2_ex);
            // Late completion from generation 10 must be ignored.
            let stale_report = SubmissionReport {
                language: PracticeLanguage::Python,
                exercise_id: day1_ex.to_string(),
                verdict: SubmissionVerdict::Accepted,
                summary: "ok".into(),
                elapsed_ms: 10,
                passed_count: 1,
                total_count: 1,
                cases: Vec::new(),
                stdout: String::new(),
                stderr: String::new(),
                output_truncated: false,
            };
            panel.apply_submission_report(10, day1, stale_report, cx);
            assert!(!panel.streak_summary().solved_today);
            assert_eq!(panel.streak_summary().current_streak, 0);
        });

        // Case B: Submission started on Day 1 completes AFTER midnight before refresh_daily ran ->
        // credits Day 1 (preserving streak = 1 pending Day 2), rotates to Day 2's daily exercise,
        // and does NOT mark Day 2 solved!
        view.update(window, |panel, cx| {
            panel.test_today_override = Some(day1);
            panel.today = day1;
            panel.daily_index =
                super::catalog::daily_exercise_index(day1, Difficulty::Easy, panel.pool.len());
            panel.exercise_index = panel.daily_index;
            panel.submission_generation = 20;
            panel.submitting = true;

            // Midnight happens while submission 20 is in flight.
            panel.test_today_override = Some(day2);
            let late_report = SubmissionReport {
                language: PracticeLanguage::Python,
                exercise_id: day1_ex.to_string(),
                verdict: SubmissionVerdict::Accepted,
                summary: "ok".into(),
                elapsed_ms: 10,
                passed_count: 1,
                total_count: 1,
                cases: Vec::new(),
                stdout: String::new(),
                stderr: String::new(),
                output_truncated: false,
            };
            panel.apply_submission_report(20, day1, late_report, cx);

            // Must have rotated cleanly to Day 2's exercise!
            assert_eq!(panel.today, day2);
            assert_eq!(panel.active_exercise_id(), day2_ex);
            // Day 1 was credited (streak=1 alive), Day 2 itself is NOT marked solved!
            let streak = panel.streak_summary();
            assert_eq!(streak.current_streak, 1);
            assert!(!streak.solved_today);
            assert!(panel.profile.is_solved_on(day1, day1_ex, PracticeLanguage::Python));
            assert!(!panel.profile.is_solved_on(day2, day1_ex, PracticeLanguage::Python));
        });

        let _ = fs::remove_dir_all(&profile);
    }

    #[gpui::test]
    fn corrupt_state_is_preserved_across_edits_until_explicit_recovery(cx: &mut TestAppContext) {
        let profile = unique_profile_dir("corrupt-widget");
        fs::create_dir_all(&profile).unwrap();
        let state_path = PracticeProfileState::state_file_path(&profile);
        let original_corrupt = "{corrupt-user-history-do-not-destroy";
        fs::write(&state_path, original_corrupt).unwrap();

        let p_dir = profile.clone();
        let (view, window) = cx.add_window_view(move |_, cx| PracticePanel::new(p_dir, cx));
        window.simulate_resize(size(px(900.0), px(720.0)));
        window.run_until_parked();

        assert!(
            window.debug_bounds("practice-recover-corrupt").is_some(),
            "recovery button should be visible when corrupt state is detected"
        );

        // Editing, switching languages, and flushing must NOT overwrite the corrupt file.
        view.update(window, |panel, cx| {
            assert!(panel.corrupt_state_blocked);
            panel.set_solution_text("# draft edit while corrupt", cx);
            panel.select_language(PracticeLanguage::Rust, cx);
            panel.flush_pending_save();
        });
        assert_eq!(fs::read_to_string(&state_path).unwrap(), original_corrupt);

        // Explicit recovery quarantines the original bytes and restores normal saving.
        view.update(window, |panel, cx| {
            let backup = panel
                .recover_corrupt_state(cx)
                .expect("recovery succeeds")
                .expect("backup path returned");
            assert_eq!(fs::read_to_string(&backup).unwrap(), original_corrupt);
            assert!(!panel.corrupt_state_blocked);
            panel.set_solution_text("pub fn solve(n: i64) -> i64 { n }", cx);
            panel.flush_pending_save();
        });
        let reloaded = PracticeProfileState::load_checked(&profile);
        assert!(!reloaded.corrupt_blocked);
        assert!(reloaded.warning.is_none());

        let _ = fs::remove_dir_all(&profile);
    }

    #[gpui::test]
    fn mouse_click_places_caret_at_clicked_column_and_scroll_follows_caret(
        cx: &mut TestAppContext,
    ) {
        let profile = unique_profile_dir("caret-click");
        let p_dir = profile.clone();
        let (view, window) = cx.add_window_view(move |_, cx| {
            let mut panel = PracticePanel::new(p_dir, cx);
            panel.set_solution_text("0123456789abcdefghij\nsecond line", cx);
            panel
        });
        window.simulate_resize(size(px(900.0), px(760.0)));
        window.run_until_parked();

        let line0 = window
            .debug_bounds("practice-line-0")
            .expect("line 0 rendered");

        // Click at left edge of line 0 -> column 0 (NOT end of line!).
        let left_pt = point(line0.origin.x + px(4.0), line0.center().y);
        window.simulate_mouse_down(left_pt, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_up(left_pt, MouseButton::Left, Modifiers::default());
        window.run_until_parked();
        view.read_with(window, |panel, _| {
            assert_eq!(panel.editor.cursor_line_col(), (0, 0));
        });

        // Click in the interior of line 0 -> strictly between column 0 and EOL (20).
        let mid_pt = point(line0.origin.x + px(95.0), line0.center().y);
        window.simulate_mouse_down(mid_pt, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_up(mid_pt, MouseButton::Left, Modifiers::default());
        window.run_until_parked();
        view.read_with(window, |panel, _| {
            let (ln, col) = panel.editor.cursor_line_col();
            assert_eq!(ln, 0);
            assert!(col > 0 && col < 20, "expected interior column, got {col}");
        });

        // Click well past the end of line 0 -> EOL (column 20).
        let right_pt = point(line0.right() - px(4.0), line0.center().y);
        window.simulate_mouse_down(right_pt, MouseButton::Left, Modifiers::default());
        window.simulate_mouse_up(right_pt, MouseButton::Left, Modifiers::default());
        window.run_until_parked();
        view.read_with(window, |panel, _| {
            assert_eq!(panel.editor.cursor_line_col(), (0, 20));
        });

        // Inserting many lines updates editor_scroll to keep the caret in view.
        view.update(window, |panel, cx| {
            let enter = Keystroke::parse("enter").unwrap();
            for _ in 0..35 {
                panel.handle_keystroke(&enter, cx);
            }
            assert!(panel.editor.cursor_line_col().0 >= 35);
        });
        window.run_until_parked();
        view.read_with(window, |panel, _| {
            assert!(
                f32::from(panel.editor_scroll.offset().y) < 0.0,
                "editor_scroll should scroll down to follow caret into view, offset={:?}",
                panel.editor_scroll.offset()
            );
        });

        let _ = fs::remove_dir_all(&profile);
    }

    #[cfg(unix)]
    #[gpui::test]
    fn dropping_panel_cancels_running_submission_and_kills_live_child(cx: &mut TestAppContext) {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        use std::time::{Duration, Instant};

        let profile = unique_profile_dir("drop-cancel");
        let pid_file = std::env::temp_dir().join(format!(
            "gtuim-panel-drop-pid-{}.txt",
            std::process::id()
        ));
        let _ = fs::remove_file(&pid_file);

        let reg = super::ToolchainRegistry::detect_full();
        let py = reg.get(PracticeLanguage::Python);
        assert!(py.available);
        let ex = super::catalog::all_exercises()
            .into_iter()
            .find(|e| e.id == "digital-root")
            .unwrap();
        let src = format!(
            r#"import os, time
def solve(n):
    with open({:?}, "w") as f:
        f.write(str(os.getpid()))
    time.sleep(10)
    return 0
"#,
            pid_file.to_string_lossy()
        );

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_worker = cancel.clone();
        let worker = std::thread::spawn(move || {
            super::runner::evaluate_submission(
                &ex,
                PracticeLanguage::Python,
                &src,
                &py,
                super::RunLimits::default(),
                &cancel_worker,
            )
        });

        // Wait until the real child process is live and has recorded its PID.
        let wait_start = Instant::now();
        while !pid_file.exists() && wait_start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let child_pid: i32 = fs::read_to_string(&pid_file)
            .expect("live child pid written")
            .trim()
            .parse()
            .expect("valid pid");
        assert_eq!(unsafe { libc::kill(child_pid, 0) }, 0, "child should be alive before drop");

        // Attach the running cancel token to a PracticePanel, then close the window to drop the panel.
        let p_dir = profile.clone();
        let (_, window) = cx.add_window_view(move |_, cx| {
            let mut panel = PracticePanel::new(p_dir, cx);
            panel.submitting = true;
            panel.active_cancel = Some(cancel);
            panel
        });
        window.update(|window, _| window.remove_window());
        cx.run_until_parked();

        let report = worker.join().expect("worker joined");
        assert_eq!(report.verdict, SubmissionVerdict::Cancelled);
        std::thread::sleep(Duration::from_millis(50));
        assert_ne!(
            unsafe { libc::kill(child_pid, 0) },
            0,
            "child {child_pid} must be dead after panel disposal"
        );

        let _ = fs::remove_file(&pid_file);
        let _ = fs::remove_dir_all(&profile);
    }
}
