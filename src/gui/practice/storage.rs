//! Per-profile persistence and local-date streak tracking for daily coding practice.
//!
//! All data lives strictly inside the caller-supplied `profile_dir` (never global
//! paths), written atomically via temp file + rename so crashes never corrupt state.

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::fs;
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::catalog::{Difficulty, PracticeLanguage};

const STATE_FILE_NAME: &str = "practice_state.json";
const LOCK_FILE_NAME: &str = "practice_state.lock";
pub const CURRENT_STATE_VERSION: u32 = 1;
const MAX_SAVED_DRAFTS: usize = 64;
const MAX_DRAFT_BYTES: usize = 64 * 1024;
static ATOMIC_SEQ: AtomicU64 = AtomicU64::new(1);

/// Result of loading `practice_state.json` from a profile directory.
#[derive(Clone, Debug)]
pub struct LoadedProfileState {
    pub state: PracticeProfileState,
    /// Non-`None` when the existing file could not be read, parsed, or had an unsupported version.
    pub warning: Option<String>,
    /// True when an existing unreadable/corrupt/unsupported state file was preserved on disk
    /// and automatic saving must stay paused until explicit recovery (`quarantine_and_reset`).
    pub corrupt_blocked: bool,
}

struct ProfileLock {
    #[cfg(unix)]
    file: fs::File,
}

impl ProfileLock {
    fn acquire(profile_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(profile_dir).map_err(|e| {
            format!(
                "Failed to create practice directory {}: {e}",
                profile_dir.display()
            )
        })?;
        #[cfg(unix)]
        {
            let lock_path = profile_dir.join(LOCK_FILE_NAME);
            let file = fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&lock_path)
                .map_err(|e| format!("Failed to open practice lock {}: {e}", lock_path.display()))?;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
                let err = std::io::Error::last_os_error();
                return Err(format!("Failed to lock practice state: {err}"));
            }
            Ok(Self { file })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }
}

#[cfg(unix)]
impl Drop for ProfileLock {
    fn drop(&mut self) {
        unsafe {
            let _ = libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// Calendar date in the user's local timezone (Gregorian YYYY-MM-DD).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl LocalDate {
    /// Current calendar date in the host's local timezone using POSIX `localtime_r`.
    pub fn today() -> Self {
        #[cfg(unix)]
        {
            unsafe {
                let mut now: libc::time_t = 0;
                libc::time(&mut now);
                let mut tm: libc::tm = std::mem::zeroed();
                if !libc::localtime_r(&now, &mut tm).is_null() {
                    let year = tm.tm_year + 1900;
                    let month = (tm.tm_mon + 1).clamp(1, 12) as u8;
                    let day = tm.tm_mday.clamp(1, 31) as u8;
                    if let Some(d) = Self::from_ymd(year, month, day) {
                        return d;
                    }
                }
            }
        }
        // Fallback: UTC epoch day calculation if localtime_r ever fails.
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Self::from_epoch_days(secs.div_euclid(86_400))
    }

    /// Construct a validated Gregorian calendar date.
    pub fn from_ymd(year: i32, month: u8, day: u8) -> Option<Self> {
        if !(1..=12).contains(&month) {
            return None;
        }
        let max_day = days_in_month(year, month);
        if day < 1 || day > max_day {
            return None;
        }
        Some(Self { year, month, day })
    }

    /// Parse `"YYYY-MM-DD"`.
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.trim().split('-');
        let year = parts.next()?.parse::<i32>().ok()?;
        let month = parts.next()?.parse::<u8>().ok()?;
        let day = parts.next()?.parse::<u8>().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Self::from_ymd(year, month, day)
    }

    /// Days since 1970-01-01 (Howard Hinnant's civil-to-days algorithm).
    pub fn to_epoch_days(self) -> i64 {
        let y = self.year as i64 - if self.month <= 2 { 1 } else { 0 };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = (y - era * 400) as u64;
        let m = self.month as u64;
        let d = self.day as u64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe as i64 - 719_468
    }

    /// Inverse of `to_epoch_days` (Howard Hinnant's days-to-civil algorithm).
    pub fn from_epoch_days(days: i64) -> Self {
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = (z - era * 146_097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u8;
        let year = (y + if m <= 2 { 1 } else { 0 }) as i32;
        Self {
            year,
            month: m,
            day: d,
        }
    }

    pub fn prev_day(self) -> Self {
        Self::from_epoch_days(self.to_epoch_days() - 1)
    }

    pub fn next_day(self) -> Self {
        Self::from_epoch_days(self.to_epoch_days() + 1)
    }
}

impl fmt::Display for LocalDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Public streak snapshot for host chrome (status bar, badges, events).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreakSummary {
    pub current_streak: u32,
    pub best_streak: u32,
    pub solved_today: bool,
    pub total_solved: u32,
    pub today_local_date: String,
}

/// Persisted per-profile practice progress and drafts.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PracticeProfileState {
    pub version: u32,
    pub language: String,
    pub difficulty: String,
    /// Sorted, deduplicated local calendar dates (`YYYY-MM-DD`) with >= 1 accepted solve.
    pub completed_dates: Vec<String>,
    pub best_streak: u32,
    pub total_solved: u32,
    /// Unique `"{exercise_id}:{language}"` pairs solved at least once.
    pub solved_exercises: Vec<String>,
    /// Daily completions recorded as `"{YYYY-MM-DD}:{exercise_id}:{language}"`.
    pub daily_completions: Vec<String>,
    /// Saved solution drafts keyed by `"{exercise_id}:{language}"`.
    pub drafts: HashMap<String, String>,
}

impl Default for PracticeProfileState {
    fn default() -> Self {
        Self {
            version: 1,
            language: PracticeLanguage::Python.id().to_string(),
            difficulty: Difficulty::Easy.id().to_string(),
            completed_dates: Vec::new(),
            best_streak: 0,
            total_solved: 0,
            solved_exercises: Vec::new(),
            daily_completions: Vec::new(),
            drafts: HashMap::new(),
        }
    }
}

impl PracticeProfileState {
    pub fn state_file_path(profile_dir: &Path) -> PathBuf {
        profile_dir.join(STATE_FILE_NAME)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.version != CURRENT_STATE_VERSION {
            return Err(format!(
                "Unsupported practice state version {} (expected {CURRENT_STATE_VERSION})",
                self.version
            ));
        }
        Ok(())
    }

    /// Load profile state from `profile_dir/practice_state.json`.
    /// Only `ErrorKind::NotFound` starts a fresh writable profile. Any read error,
    /// corrupt JSON, or unsupported version preserves the original file untouched
    /// and marks `corrupt_blocked = true` until explicit user recovery.
    pub fn load_checked(profile_dir: &Path) -> LoadedProfileState {
        let path = Self::state_file_path(profile_dir);
        let raw = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return LoadedProfileState {
                    state: Self::default(),
                    warning: None,
                    corrupt_blocked: false,
                };
            }
            Err(err) => {
                return LoadedProfileState {
                    state: Self::default(),
                    warning: Some(format!(
                        "Could not read practice state ({err}). Original file preserved; saving is paused until recovered."
                    )),
                    corrupt_blocked: true,
                };
            }
        };
        match serde_json::from_str::<Self>(&raw) {
            Ok(mut state) => match state.validate() {
                Ok(()) => {
                    state.normalize();
                    LoadedProfileState {
                        state,
                        warning: None,
                        corrupt_blocked: false,
                    }
                }
                Err(err) => LoadedProfileState {
                    state: Self::default(),
                    warning: Some(format!(
                        "{err}. Original file preserved; automatic saving is paused until you backup & reset."
                    )),
                    corrupt_blocked: true,
                },
            },
            Err(err) => LoadedProfileState {
                state: Self::default(),
                warning: Some(format!(
                    "Practice state file is corrupt ({err}). Original file preserved; automatic saving is paused until you backup & reset."
                )),
                corrupt_blocked: true,
            },
        }
    }

    /// Convenience wrapper returning `(state, optional_warning)`.
    pub fn load(profile_dir: &Path) -> (Self, Option<String>) {
        let loaded = Self::load_checked(profile_dir);
        (loaded.state, loaded.warning)
    }

    /// Explicitly quarantine a corrupt/unreadable `practice_state.json` to a timestamped
    /// backup file inside `profile_dir` (preserving every byte) and initialize a clean state.
    pub fn quarantine_and_reset(profile_dir: &Path) -> Result<(Self, Option<PathBuf>), String> {
        let _lock = ProfileLock::acquire(profile_dir)?;
        let target = Self::state_file_path(profile_dir);
        let backup_path = if target.exists() {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let seq = ATOMIC_SEQ.fetch_add(1, Ordering::Relaxed);
            let backup = profile_dir.join(format!("practice_state.corrupt.{nanos}-{seq}.json"));
            fs::rename(&target, &backup).map_err(|e| {
                format!(
                    "Failed to quarantine corrupt state file to {}: {e}",
                    backup.display()
                )
            })?;
            Some(backup)
        } else {
            None
        };

        let mut fresh = Self::default();
        fresh.normalize();
        let seq = ATOMIC_SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = profile_dir.join(format!(".{STATE_FILE_NAME}.tmp.{}.{seq}", std::process::id()));
        let json = serde_json::to_string_pretty(&fresh)
            .map_err(|e| format!("Failed to serialize practice state: {e}"))?;
        fs::write(&tmp, json)
            .map_err(|e| format!("Failed to write temporary practice state: {e}"))?;
        if let Err(err) = fs::rename(&tmp, &target) {
            let _ = fs::remove_file(&tmp);
            return Err(format!("Failed to persist reset practice state: {err}"));
        }
        Ok((fresh, backup_path))
    }

    /// Persist profile state atomically inside `profile_dir` using an exclusive
    /// per-profile file lock and read-merge-write so concurrent windows never lose updates.
    pub fn save(&mut self, profile_dir: &Path) -> Result<(), String> {
        let all_drafts: HashMap<String, Option<String>> = self
            .drafts
            .iter()
            .map(|(k, v)| (k.clone(), Some(v.clone())))
            .collect();
        self.save_merged(profile_dir, &all_drafts, true)
    }

    /// Process-safe locked read-merge-write persisting only `draft_deltas` (and
    /// preferences when `prefs_dirty` is true) on top of fresh on-disk state, while
    /// monotonically unioning completed dates, solved exercises, and streak highs.
    ///
    /// Refuses to overwrite an existing unreadable, corrupt, or unsupported-version file;
    /// only `ErrorKind::NotFound` initializes a new state file.
    pub fn save_merged(
        &mut self,
        profile_dir: &Path,
        draft_deltas: &HashMap<String, Option<String>>,
        prefs_dirty: bool,
    ) -> Result<(), String> {
        self.validate()?;
        let _lock = ProfileLock::acquire(profile_dir)?;
        let target = Self::state_file_path(profile_dir);

        let mut merged = match fs::read_to_string(&target) {
            Ok(raw) => {
                let parsed = serde_json::from_str::<Self>(&raw).map_err(|e| {
                    format!(
                        "Refusing to overwrite corrupt practice state ({e}); original file preserved."
                    )
                })?;
                parsed.validate().map_err(|e| {
                    format!("Refusing to overwrite practice state ({e}); original file preserved.")
                })?;
                parsed
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Self {
                language: self.language.clone(),
                difficulty: self.difficulty.clone(),
                ..Self::default()
            },
            Err(err) => {
                return Err(format!(
                    "Refusing to overwrite unreadable practice state ({err}); original file preserved."
                ));
            }
        };

        if prefs_dirty {
            merged.language = self.language.clone();
            merged.difficulty = self.difficulty.clone();
        }

        // Monotonic union of progress across concurrent windows.
        for d in &self.completed_dates {
            if !merged.completed_dates.iter().any(|x| x == d) {
                merged.completed_dates.push(d.clone());
            }
        }
        for k in &self.solved_exercises {
            if !merged.solved_exercises.iter().any(|x| x == k) {
                merged.solved_exercises.push(k.clone());
            }
        }
        for k in &self.daily_completions {
            if !merged.daily_completions.iter().any(|x| x == k) {
                merged.daily_completions.push(k.clone());
            }
        }
        merged.best_streak = merged.best_streak.max(self.best_streak);
        merged.total_solved = merged
            .total_solved
            .max(self.total_solved)
            .max(merged.daily_completions.len() as u32);

        // Apply only the draft keys modified by this window so sibling windows' drafts stay intact.
        for (key, value) in draft_deltas {
            match value {
                Some(code) => {
                    merged.drafts.insert(key.clone(), code.clone());
                }
                None => {
                    merged.drafts.remove(key);
                }
            }
        }

        merged.normalize();

        let seq = ATOMIC_SEQ.fetch_add(1, Ordering::Relaxed);
        let tmp = profile_dir.join(format!(".{STATE_FILE_NAME}.tmp.{}.{seq}", std::process::id()));
        let json = serde_json::to_string_pretty(&merged)
            .map_err(|e| format!("Failed to serialize practice state: {e}"))?;
        fs::write(&tmp, json)
            .map_err(|e| format!("Failed to write temporary practice state: {e}"))?;
        if let Err(err) = fs::rename(&tmp, &target) {
            let _ = fs::remove_file(&tmp);
            return Err(format!("Failed to persist practice state: {err}"));
        }

        *self = merged;
        Ok(())
    }

    fn normalize(&mut self) {
        let valid_dates: BTreeSet<LocalDate> = self
            .completed_dates
            .iter()
            .filter_map(|s| LocalDate::parse(s))
            .collect();
        self.completed_dates = valid_dates.iter().map(|d| d.to_string()).collect();

        // Keep best_streak at least as large as the longest historical consecutive run.
        let historical_best = longest_run(&valid_dates);
        if historical_best > self.best_streak {
            self.best_streak = historical_best;
        }

        // Bound saved drafts to prevent unbounded growth.
        for draft in self.drafts.values_mut() {
            if draft.len() > MAX_DRAFT_BYTES {
                let mut cut = MAX_DRAFT_BYTES;
                while cut > 0 && !draft.is_char_boundary(cut) {
                    cut -= 1;
                }
                draft.truncate(cut);
            }
        }
        if self.drafts.len() > MAX_SAVED_DRAFTS {
            let mut keys: Vec<String> = self.drafts.keys().cloned().collect();
            keys.sort();
            let remove_count = self.drafts.len() - MAX_SAVED_DRAFTS;
            for k in keys.into_iter().take(remove_count) {
                self.drafts.remove(&k);
            }
        }
    }

    pub fn selected_language(&self) -> PracticeLanguage {
        PracticeLanguage::from_id(&self.language).unwrap_or(PracticeLanguage::Python)
    }

    pub fn selected_difficulty(&self) -> Difficulty {
        Difficulty::from_id(&self.difficulty).unwrap_or(Difficulty::Easy)
    }

    pub fn set_preferences(&mut self, language: PracticeLanguage, difficulty: Difficulty) {
        self.language = language.id().to_string();
        self.difficulty = difficulty.id().to_string();
    }

    pub fn draft_key(exercise_id: &str, language: PracticeLanguage) -> String {
        format!("{exercise_id}:{}", language.id())
    }

    pub fn get_draft(&self, exercise_id: &str, language: PracticeLanguage) -> Option<&str> {
        self.drafts
            .get(&Self::draft_key(exercise_id, language))
            .map(String::as_str)
    }

    pub fn set_draft(&mut self, exercise_id: &str, language: PracticeLanguage, code: String) {
        let key = Self::draft_key(exercise_id, language);
        self.drafts.insert(key, code);
    }

    pub fn clear_draft(&mut self, exercise_id: &str, language: PracticeLanguage) {
        self.drafts.remove(&Self::draft_key(exercise_id, language));
    }

    pub fn is_solved_on(&self, date: LocalDate, exercise_id: &str, language: PracticeLanguage) -> bool {
        let key = format!("{date}:{exercise_id}:{}", language.id());
        self.daily_completions.iter().any(|k| k == &key)
    }

    pub fn is_exercise_solved(&self, exercise_id: &str, language: PracticeLanguage) -> bool {
        let key = Self::draft_key(exercise_id, language);
        self.solved_exercises.iter().any(|k| k == &key)
    }

    /// Compute live streak status relative to `today` (in local time).
    ///
    /// - If `today` is completed, counts consecutive completed days ending on `today`.
    /// - Otherwise, if `today - 1` is completed, the streak is still active (pending
    ///   today's solve) and counts consecutive days ending on `today - 1`.
    /// - Otherwise, the current streak is 0.
    pub fn streak_summary(&self, today: LocalDate) -> StreakSummary {
        let dates: BTreeSet<LocalDate> = self
            .completed_dates
            .iter()
            .filter_map(|s| LocalDate::parse(s))
            .collect();

        let solved_today = dates.contains(&today);
        let anchor = if solved_today {
            Some(today)
        } else if dates.contains(&today.prev_day()) {
            Some(today.prev_day())
        } else {
            None
        };

        let mut current_streak = 0u32;
        if let Some(mut cursor) = anchor {
            while dates.contains(&cursor) {
                current_streak = current_streak.saturating_add(1);
                cursor = cursor.prev_day();
            }
        }

        let best_streak = self.best_streak.max(current_streak).max(longest_run(&dates));
        StreakSummary {
            current_streak,
            best_streak,
            solved_today,
            total_solved: self.total_solved,
            today_local_date: today.to_string(),
        }
    }

    /// Record an accepted submission on `today` (local calendar date).
    /// Returns `(updated_summary, streak_advanced_today)`.
    pub fn record_accepted(
        &mut self,
        today: LocalDate,
        exercise_id: &str,
        language: PracticeLanguage,
    ) -> (StreakSummary, bool) {
        let today_str = today.to_string();
        let first_solve_today = !self.completed_dates.iter().any(|d| d == &today_str);
        if first_solve_today {
            self.completed_dates.push(today_str);
        }

        let ex_key = Self::draft_key(exercise_id, language);
        if !self.solved_exercises.iter().any(|k| k == &ex_key) {
            self.solved_exercises.push(ex_key);
        }

        let daily_key = format!("{today}:{exercise_id}:{}", language.id());
        if !self.daily_completions.iter().any(|k| k == &daily_key) {
            self.daily_completions.push(daily_key);
            self.total_solved = self.total_solved.saturating_add(1);
        }

        self.normalize();
        let summary = self.streak_summary(today);
        self.best_streak = self.best_streak.max(summary.best_streak);
        (summary, first_solve_today)
    }
}

fn longest_run(dates: &BTreeSet<LocalDate>) -> u32 {
    let mut best = 0u32;
    let mut cur = 0u32;
    let mut prev: Option<i64> = None;
    for d in dates {
        let day_num = d.to_epoch_days();
        if prev.is_some_and(|p| day_num == p + 1) {
            cur = cur.saturating_add(1);
        } else {
            cur = 1;
        }
        best = best.max(cur);
        prev = Some(day_num);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_profile_dir(tag: &str) -> PathBuf {
        let seq = ATOMIC_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "gtuim-practice-storage-test-{tag}-{}-{seq}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn local_date_roundtrip_and_leap_years() {
        for (y, m, d) in [
            (1970, 1, 1),
            (2000, 2, 29),
            (2024, 2, 29),
            (2025, 12, 31),
            (2026, 1, 1),
            (2026, 11, 26),
        ] {
            let date = LocalDate::from_ymd(y, m, d).expect("valid date");
            let round = LocalDate::from_epoch_days(date.to_epoch_days());
            assert_eq!(date, round);
            assert_eq!(LocalDate::parse(&date.to_string()), Some(date));
        }
        assert!(LocalDate::from_ymd(2025, 2, 29).is_none());
        assert!(LocalDate::from_ymd(1900, 2, 29).is_none());
        assert!(LocalDate::from_ymd(2026, 4, 31).is_none());

        let feb28 = LocalDate::from_ymd(2024, 2, 28).unwrap();
        assert_eq!(feb28.next_day(), LocalDate::from_ymd(2024, 2, 29).unwrap());
        assert_eq!(
            feb28.next_day().next_day(),
            LocalDate::from_ymd(2024, 3, 1).unwrap()
        );
    }

    #[test]
    fn streak_handles_consecutive_days_same_day_repeats_and_gaps() {
        let mut state = PracticeProfileState::default();
        let d1 = LocalDate::from_ymd(2026, 11, 24).unwrap();
        let d2 = LocalDate::from_ymd(2026, 11, 25).unwrap();
        let d3 = LocalDate::from_ymd(2026, 11, 26).unwrap();
        let d5 = LocalDate::from_ymd(2026, 11, 28).unwrap();

        let (s1, adv1) = state.record_accepted(d1, "valid-parentheses", PracticeLanguage::Python);
        assert!(adv1);
        assert_eq!(s1.current_streak, 1);
        assert_eq!(s1.best_streak, 1);
        assert!(s1.solved_today);

        // Second solve on the same day must not double-increment the streak.
        let (s1_again, adv1_again) =
            state.record_accepted(d1, "two-sum", PracticeLanguage::Rust);
        assert!(!adv1_again);
        assert_eq!(s1_again.current_streak, 1);
        assert_eq!(s1_again.total_solved, 2);

        // Next day before solving: streak stays alive (1) but solved_today is false.
        let before_d2 = state.streak_summary(d2);
        assert_eq!(before_d2.current_streak, 1);
        assert!(!before_d2.solved_today);

        // Solving on d2 advances streak to 2.
        let (s2, adv2) = state.record_accepted(d2, "two-sum", PracticeLanguage::Python);
        assert!(adv2);
        assert_eq!(s2.current_streak, 2);
        assert_eq!(s2.best_streak, 2);

        // Solving on d3 advances streak to 3.
        let (s3, adv3) = state.record_accepted(d3, "digital-root", PracticeLanguage::Rust);
        assert!(adv3);
        assert_eq!(s3.current_streak, 3);
        assert_eq!(s3.best_streak, 3);

        // Skipping d4 breaks current streak on d5, while preserving best_streak = 3.
        let on_d5 = state.streak_summary(d5);
        assert_eq!(on_d5.current_streak, 0);
        assert_eq!(on_d5.best_streak, 3);
        assert!(!on_d5.solved_today);

        // Solving on d5 starts a new streak of 1 while keeping best_streak = 3.
        let (s5, adv5) = state.record_accepted(d5, "move-zeroes", PracticeLanguage::C);
        assert!(adv5);
        assert_eq!(s5.current_streak, 1);
        assert_eq!(s5.best_streak, 3);
    }

    #[test]
    fn per_profile_persistence_is_isolated_and_resilient() {
        let dir_a = temp_profile_dir("profile-a");
        let dir_b = temp_profile_dir("profile-b");
        let day = LocalDate::from_ymd(2026, 11, 26).unwrap();

        let mut a = PracticeProfileState::default();
        a.set_preferences(PracticeLanguage::Rust, Difficulty::Medium);
        a.set_draft("two-sum", PracticeLanguage::Rust, "pub fn solve() {}".into());
        a.record_accepted(day, "two-sum", PracticeLanguage::Rust);
        a.save(&dir_a).expect("save profile A");

        let (loaded_a, warn_a) = PracticeProfileState::load(&dir_a);
        assert!(warn_a.is_none());
        assert_eq!(loaded_a.selected_language(), PracticeLanguage::Rust);
        assert_eq!(loaded_a.selected_difficulty(), Difficulty::Medium);
        assert_eq!(loaded_a.streak_summary(day).current_streak, 1);
        assert_eq!(
            loaded_a.get_draft("two-sum", PracticeLanguage::Rust),
            Some("pub fn solve() {}")
        );

        // Profile B must remain completely clean and unaffected.
        let (loaded_b, warn_b) = PracticeProfileState::load(&dir_b);
        assert!(warn_b.is_none());
        assert_eq!(loaded_b.streak_summary(day).current_streak, 0);
        assert_eq!(loaded_b.get_draft("two-sum", PracticeLanguage::Rust), None);

        // Corrupt JSON recovers cleanly in memory with a warning, preserves original bytes,
        // refuses automatic overwrite, and supports explicit quarantine recovery.
        fs::create_dir_all(&dir_b).unwrap();
        let corrupt_bytes = "{not-valid-json-preserve-me";
        let state_path_b = PracticeProfileState::state_file_path(&dir_b);
        fs::write(&state_path_b, corrupt_bytes).unwrap();
        let loaded_corrupt = PracticeProfileState::load_checked(&dir_b);
        assert!(loaded_corrupt.warning.is_some());
        assert!(loaded_corrupt.corrupt_blocked);
        assert_eq!(loaded_corrupt.state.streak_summary(day).current_streak, 0);

        // Attempting to save over corrupt bytes must FAIL and leave original bytes untouched!
        let mut attempted = loaded_corrupt.state;
        let mut draft_delta = HashMap::new();
        draft_delta.insert("two-sum:python".into(), Some("def solve(): pass".into()));
        assert!(attempted.save_merged(&dir_b, &draft_delta, false).is_err());
        assert_eq!(fs::read_to_string(&state_path_b).unwrap(), corrupt_bytes);

        // Unsupported version is also rejected without overwriting.
        let future_json = r#"{"version":99,"language":"python","difficulty":"easy"}"#;
        fs::write(&state_path_b, future_json).unwrap();
        let loaded_future = PracticeProfileState::load_checked(&dir_b);
        assert!(loaded_future.corrupt_blocked);
        assert!(attempted.save_merged(&dir_b, &draft_delta, false).is_err());
        assert_eq!(fs::read_to_string(&state_path_b).unwrap(), future_json);

        // Explicit recovery quarantines the exact original bytes and enables clean saving.
        let (mut reset_state, backup) = PracticeProfileState::quarantine_and_reset(&dir_b).unwrap();
        let backup = backup.expect("backup path created");
        assert_eq!(fs::read_to_string(&backup).unwrap(), future_json);
        reset_state.save_merged(&dir_b, &draft_delta, false).unwrap();
        let reloaded = PracticeProfileState::load_checked(&dir_b);
        assert!(!reloaded.corrupt_blocked);
        assert_eq!(
            reloaded.state.get_draft("two-sum", PracticeLanguage::Python),
            Some("def solve(): pass")
        );

        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);
    }

    #[test]
    fn multiwindow_concurrent_merges_preserve_streaks_solves_and_distinct_drafts() {
        let dir = temp_profile_dir("multiwindow");
        let day = LocalDate::from_ymd(2026, 11, 26).unwrap();

        let mut win_a = PracticeProfileState::default();
        let mut win_b = PracticeProfileState::default();

        // Window A solves digital-root in Python and persists.
        win_a.record_accepted(day, "digital-root", PracticeLanguage::Python);
        let mut deltas_a = HashMap::new();
        deltas_a.insert(
            PracticeProfileState::draft_key("digital-root", PracticeLanguage::Python),
            Some("def solve(n): return 0 if n == 0 else 1 + (n - 1) % 9".into()),
        );
        win_a.save_merged(&dir, &deltas_a, false).unwrap();

        // Window B (stale in-memory copy with streak 0) edits a Zig draft and flushes:
        // must NOT clobber Window A's streak, solved record, or Python draft!
        let mut deltas_b = HashMap::new();
        deltas_b.insert(
            PracticeProfileState::draft_key("two-sum", PracticeLanguage::Zig),
            Some("pub fn solve(nums: []const i64, k: i64) [2]i64 { return .{0, 1}; }".into()),
        );
        win_b.save_merged(&dir, &deltas_b, false).unwrap();

        assert_eq!(win_b.streak_summary(day).current_streak, 1);
        assert!(win_b.is_solved_on(day, "digital-root", PracticeLanguage::Python));
        assert!(win_b.get_draft("digital-root", PracticeLanguage::Python).is_some());
        assert!(win_b.get_draft("two-sum", PracticeLanguage::Zig).is_some());

        // Hammer with 4 concurrent worker threads recording distinct solves & drafts.
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let mut handles = Vec::new();
        for i in 0..4 {
            let dir = dir.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                let mut state = PracticeProfileState::default();
                let ex = format!("ex-{i}");
                state.record_accepted(day, &ex, PracticeLanguage::C);
                let mut deltas = HashMap::new();
                deltas.insert(
                    PracticeProfileState::draft_key(&ex, PracticeLanguage::C),
                    Some(format!("// draft {i}")),
                );
                barrier.wait();
                state.save_merged(&dir, &deltas, false).unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let (final_state, warn) = PracticeProfileState::load(&dir);
        assert!(warn.is_none());
        assert_eq!(final_state.streak_summary(day).current_streak, 1);
        assert_eq!(final_state.total_solved, 5); // digital-root + 4 worker solves
        for i in 0..4 {
            assert_eq!(
                final_state.get_draft(&format!("ex-{i}"), PracticeLanguage::C),
                Some(format!("// draft {i}").as_str())
            );
        }

        let _ = fs::remove_dir_all(&dir);
    }
}
