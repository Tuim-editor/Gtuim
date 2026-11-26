//! Local identities with stable IDs, buffered activity and process-safe merged writes.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DayActivity {
    pub edits: u64,
    pub saves: u64,
    pub languages: BTreeMap<String, u64>,
}
impl DayActivity {
    fn merge(&mut self, other: &Self) {
        self.edits += other.edits;
        self.saves += other.saves;
        for (language, count) in &other.languages {
            *self.languages.entry(language.clone()).or_default() += count;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub display_name: String,
    pub avatar: usize,
    #[serde(default)]
    pub avatar_file: Option<String>,
    #[serde(default)]
    pub practice_enabled: bool,
    #[serde(default)]
    pub activity: BTreeMap<String, DayActivity>,
}
impl Profile {
    fn new(id: String, display_name: String, avatar: usize) -> Self {
        Self {
            id,
            display_name,
            avatar,
            avatar_file: None,
            practice_enabled: false,
            activity: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Registry {
    version: u32,
    active: String,
    profiles: Vec<Profile>,
}
impl Registry {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.profiles.is_empty()
            || !self.profiles.iter().any(|p| p.id == self.active)
            || self.profiles.iter().any(|p| {
                p.id.is_empty() || !p.id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            })
            || self
                .profiles
                .iter()
                .enumerate()
                .any(|(i, p)| self.profiles[..i].iter().any(|q| q.id == p.id))
        {
            return Err("Invalid profile registry (version, IDs or active profile)".into());
        }
        Ok(())
    }
}

pub struct Profiles {
    root: PathBuf,
    registry: Registry,
    // A window keeps its identity even if another window switches the startup default.
    active_id: String,
    pending: BTreeMap<String, BTreeMap<String, DayActivity>>,
    last_flush: Instant,
    registry_stamp: Option<(u64, u64, i64, i64)>,
    pub open: bool,
    pub editing_name: bool,
    pub name_draft: String,
    pub selected_day: Option<String>,
    pub error: Option<String>,
}

pub const AVATARS: [&str; 8] = ["◈", "✦", "●", "◆", "▲", "⬡", "★", "☀"];

struct RegistryLock(fs::File);
impl RegistryLock {
    fn acquire(root: &Path, nonblocking: bool) -> Result<Option<Self>, String> {
        fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("profiles.lock"))
            .map_err(|e| e.to_string())?;
        let flags = libc::LOCK_EX | if nonblocking { libc::LOCK_NB } else { 0 };
        if unsafe { libc::flock(file.as_raw_fd(), flags) } != 0 {
            let error = std::io::Error::last_os_error();
            if nonblocking && error.kind() == std::io::ErrorKind::WouldBlock {
                return Ok(None);
            }
            return Err(error.to_string());
        }
        Ok(Some(Self(file)))
    }
}
impl Drop for RegistryLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or("Missing storage directory")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&temp, path).map_err(|e| e.to_string())
}
fn registry_stamp(root: &Path) -> Option<(u64, u64, i64, i64)> {
    fs::metadata(root.join("profiles.json"))
        .ok()
        .map(|m| (m.ino(), m.len(), m.mtime(), m.mtime_nsec()))
}
fn owned_avatar_path(root: &Path, id: &str, name: &str) -> Option<PathBuf> {
    let stamp = name.strip_prefix("avatar-")?.strip_suffix(".png")?;
    if stamp.is_empty() || !stamp.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(root.join("profiles").join(id).join(name))
}
fn read_registry(root: &Path) -> Result<Registry, String> {
    let path = root.join("profiles.json");
    let bytes = fs::read(&path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let registry: Registry =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid {}: {e}", path.display()))?;
    registry.validate()?;
    Ok(registry)
}

impl Profiles {
    pub fn load(root: impl Into<PathBuf>) -> Result<Self, String> {
        let root = root.into();
        let root = if root.is_absolute() {
            root
        } else {
            std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(root)
        };
        let _lock = RegistryLock::acquire(&root, false)?;
        let registry = if root.join("profiles.json").exists() {
            read_registry(&root)?
        } else {
            Registry {
                version: 1,
                active: "default".into(),
                profiles: vec![Profile::new("default".into(), "Default".into(), 0)],
            }
        };
        let mut this = Self {
            active_id: registry.active.clone(),
            root,
            registry,
            pending: BTreeMap::new(),
            last_flush: Instant::now(),
            registry_stamp: None,
            open: false,
            editing_name: false,
            name_draft: String::new(),
            selected_day: None,
            error: None,
        };
        // One-time migration copies legacy settings; the legacy file remains untouched.
        let settings = this.settings_path();
        if !settings.exists() {
            let legacy = this.root.join("settings.json");
            fs::create_dir_all(settings.parent().unwrap()).map_err(|e| e.to_string())?;
            if legacy.exists() && this.active().id == "default" {
                fs::copy(legacy, &settings).map_err(|e| e.to_string())?;
            } else {
                crate::tui::widgets::settings::SettingsConfig::default()
                    .save(&settings.to_string_lossy())?;
            }
        }
        atomic_json(&this.root.join("profiles.json"), &this.registry)?;
        this.registry_stamp = registry_stamp(&this.root);
        // Practice is opt-in; even its directory is not created for a fresh guest.
        Ok(this)
    }

    pub fn active(&self) -> &Profile {
        self.registry
            .profiles
            .iter()
            .find(|p| p.id == self.active_id)
            .unwrap()
    }
    pub fn list(&self) -> &[Profile] {
        &self.registry.profiles
    }
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("profiles").join(&self.active_id)
    }
    pub fn practice_dir(&self) -> PathBuf {
        self.data_dir().join("practice")
    }
    pub fn settings_path(&self) -> PathBuf {
        self.data_dir().join("settings.json")
    }
    pub fn avatar_path(&self) -> Option<PathBuf> {
        self.active()
            .avatar_file
            .as_ref()
            .filter(|name| {
                name.starts_with("avatar-")
                    && name.ends_with(".png")
                    && !name.contains('/')
                    && !name.contains('\\')
            })
            .map(|name| self.data_dir().join(name))
            .filter(|path| path.is_file())
    }

    fn validate_window_identity(&self, registry: &Registry) -> Result<(), String> {
        if !registry.profiles.iter().any(|p| p.id == self.active_id)
            || self
                .pending
                .keys()
                .any(|id| !registry.profiles.iter().any(|p| &p.id == id))
        {
            return Err("A profile active in this window was removed externally; restore it before retrying".into());
        }
        Ok(())
    }
    fn clear_activity_error(&mut self) {
        if self
            .error
            .as_deref()
            .is_some_and(|e| e.starts_with("Activity not saved:"))
        {
            self.error = None;
        }
    }
    fn merge_pending(&self, registry: &mut Registry) {
        for (id, days) in &self.pending {
            if let Some(profile) = registry.profiles.iter_mut().find(|p| &p.id == id) {
                for (date, stats) in days {
                    profile
                        .activity
                        .entry(date.clone())
                        .or_default()
                        .merge(stats);
                }
            }
        }
    }
    fn transaction<T>(
        &mut self,
        update: impl FnOnce(&mut Registry, &str) -> Result<T, String>,
    ) -> Result<T, String> {
        let _lock = RegistryLock::acquire(&self.root, false)?;
        let mut fresh = read_registry(&self.root)?;
        self.validate_window_identity(&fresh)?;
        self.merge_pending(&mut fresh);
        let result = update(&mut fresh, &self.active_id)?;
        atomic_json(&self.root.join("profiles.json"), &fresh)?;
        self.registry = fresh;
        self.pending.clear();
        self.registry_stamp = registry_stamp(&self.root);
        self.clear_activity_error();
        self.last_flush = Instant::now();
        Ok(result)
    }
    pub fn create(&mut self) -> Result<String, String> {
        let id = format!(
            "p-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        );
        let dir = self.root.join("profiles").join(&id);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        crate::tui::widgets::settings::SettingsConfig::default()
            .save(&dir.join("settings.json").to_string_lossy())?;
        self.transaction(|registry, _| {
            registry.profiles.push(Profile::new(
                id.clone(),
                format!("Profile {}", registry.profiles.len() + 1),
                registry.profiles.len() % AVATARS.len(),
            ));
            Ok(())
        })?;
        Ok(id)
    }
    pub fn select_for_window(&mut self, id: &str) -> Result<(), String> {
        if !self.list().iter().any(|p| p.id == id) {
            return Err("Unknown profile".into());
        }
        self.active_id = id.into();
        Ok(())
    }
    pub fn switch(&mut self, id: &str) -> Result<(), String> {
        self.transaction(|registry, _| {
            if !registry.profiles.iter().any(|p| p.id == id) {
                return Err("Unknown profile".into());
            }
            registry.active = id.into();
            Ok(())
        })?;
        self.active_id = id.into();
        self.editing_name = false;
        self.selected_day = None;
        Ok(())
    }
    pub fn rename(&mut self, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 48 || name.chars().any(char::is_control) {
            return Err("Use a display name of 1–48 printable characters".into());
        }
        self.update_profile(|profile| {
            profile.display_name = name.into();
        })
    }
    fn update_profile(&mut self, update: impl FnOnce(&mut Profile)) -> Result<(), String> {
        self.transaction(|registry, id| {
            update(
                registry
                    .profiles
                    .iter_mut()
                    .find(|p| p.id == id)
                    .ok_or("Profile missing")?,
            );
            Ok(())
        })
    }
    pub fn next_avatar(&mut self) -> Result<(), String> {
        let id = self.active_id.clone();
        let old = self.transaction(|registry, id| {
            let profile = registry
                .profiles
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or("Profile missing")?;
            profile.avatar = (profile.avatar % AVATARS.len() + 1) % AVATARS.len();
            Ok(profile.avatar_file.take())
        })?;
        if let Some(path) = old.and_then(|name| owned_avatar_path(&self.root, &id, &name)) {
            let _ = fs::remove_file(path);
        }
        Ok(())
    }
    pub fn set_avatar_for(&mut self, id: &str, filename: String) -> Result<(), String> {
        if owned_avatar_path(&self.root, id, &filename).is_none() {
            return Err("Invalid owned avatar filename".into());
        }
        let old = self.transaction(|registry, _| {
            let profile = registry
                .profiles
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or("Profile missing")?;
            Ok(profile.avatar_file.replace(filename.clone()))
        })?;
        if let Some(path) = old
            .filter(|name| name != &filename)
            .and_then(|name| owned_avatar_path(&self.root, id, &name))
        {
            let _ = fs::remove_file(path);
        }
        Ok(())
    }
    pub fn enable_practice(&mut self) -> Result<(), String> {
        fs::create_dir_all(self.practice_dir()).map_err(|e| e.to_string())?;
        self.update_profile(|profile| profile.practice_enabled = true)
    }
    pub fn disable_practice(&mut self) -> Result<(), String> {
        self.update_profile(|profile| profile.practice_enabled = false)
    }

    pub fn record(&mut self, language: &str, saved: bool) {
        self.record_on(&local_day_key(), language, saved);
    }
    fn record_on(&mut self, date: &str, language: &str, saved: bool) {
        let mut delta = DayActivity::default();
        if saved {
            delta.saves = 1;
        } else {
            delta.edits = 1;
            let language = if language.is_empty() {
                "plain text"
            } else {
                language
            };
            delta
                .languages
                .insert(language.chars().take(64).collect(), 1);
        }
        let id = self.active_id.clone();
        self.pending
            .entry(id.clone())
            .or_default()
            .entry(date.into())
            .or_default()
            .merge(&delta);
        self.registry
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .unwrap()
            .activity
            .entry(date.into())
            .or_default()
            .merge(&delta);
    }
    pub fn flush_if_due(&mut self) -> bool {
        if self.last_flush.elapsed().as_secs() < 10 {
            return false;
        }
        // Bound retries as well as writes when storage is unavailable or another window is busy.
        self.last_flush = Instant::now();
        if self.pending.is_empty() && registry_stamp(&self.root) == self.registry_stamp {
            return false;
        }
        match self.flush(true) {
            Ok(changed) => changed,
            Err(e) => {
                let error = format!("Activity not saved: {e}");
                let changed = self.error.as_ref() != Some(&error);
                self.error = Some(error);
                changed
            }
        }
    }
    fn flush(&mut self, nonblocking: bool) -> Result<bool, String> {
        let Some(_lock) = RegistryLock::acquire(&self.root, nonblocking)? else {
            return Ok(false);
        };
        let mut fresh = read_registry(&self.root)?;
        self.validate_window_identity(&fresh)?;
        self.merge_pending(&mut fresh);
        if !self.pending.is_empty() {
            atomic_json(&self.root.join("profiles.json"), &fresh)?;
        }
        let old_error = self.error.clone();
        self.clear_activity_error();
        let changed = self.registry != fresh || old_error != self.error;
        self.registry = fresh;
        self.pending.clear();
        self.registry_stamp = registry_stamp(&self.root);
        self.last_flush = Instant::now();
        Ok(changed)
    }
}
impl Drop for Profiles {
    fn drop(&mut self) {
        if !self.pending.is_empty() {
            if let Err(e) = self.flush(false) {
                eprintln!("Could not flush profile activity: {e}");
            }
        }
    }
}

/// Same local civil day as practice's daily challenge (not UTC elapsed days).
pub fn local_day_key() -> String {
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    let mut local = std::mem::MaybeUninit::<libc::tm>::uninit();
    if unsafe { libc::localtime_r(&now, local.as_mut_ptr()) }.is_null() {
        return day_key(now / 86400);
    }
    let local = unsafe { local.assume_init() };
    format!(
        "{:04}-{:02}-{:02}",
        local.tm_year + 1900,
        local.tm_mon + 1,
        local.tm_mday
    )
}
/// Calendar arithmetic uses civil-day ordinals, so DST days are never skipped or repeated.
pub fn today() -> i64 {
    let date = local_day_key();
    let mut parts = date.split('-').filter_map(|v| v.parse::<i64>().ok());
    let (mut y, m, d) = (
        parts.next().unwrap(),
        parts.next().unwrap(),
        parts.next().unwrap(),
    );
    y -= i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = m + if m > 2 { -3 } else { 9 };
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + (153 * mp + 2) / 5 + d - 1 - 719468
}
pub fn day_key(days: i64) -> String {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    y += i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp() -> PathBuf {
        std::env::temp_dir().join(format!(
            "gtuim-profiles-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
    #[test]
    fn identities_settings_activity_and_practice_are_isolated_and_persistent() {
        let root = temp();
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("settings.json"), "{\"font_size\":19}").unwrap();
        let mut store = Profiles::load(&root).unwrap();
        assert_eq!(
            fs::read_to_string(store.settings_path()).unwrap(),
            "{\"font_size\":19}"
        );
        assert!(!store.active().practice_enabled);
        assert!(!store.practice_dir().exists());
        store.rename("Alice").unwrap();
        store.next_avatar().unwrap();
        store.record("rust", false);
        store.record("rust", true);
        store.enable_practice().unwrap();
        let first = store.practice_dir();
        let second = store.create().unwrap();
        store.switch(&second).unwrap();
        assert_ne!(first, store.practice_dir());
        assert!(store.active().activity.is_empty());
        assert!(!store.active().practice_enabled);
        assert!(!store.practice_dir().exists());
        assert_ne!(
            fs::read_to_string(store.settings_path()).unwrap(),
            "{\"font_size\":19}"
        );
        store.rename("../same name").unwrap();
        assert!(store.practice_dir().starts_with(root.join("profiles")));
        assert!(store.switch("../../bad").is_err());
        drop(store);
        let mut store = Profiles::load(&root).unwrap();
        assert_eq!(store.active().id, second);
        store.switch("default").unwrap();
        assert_eq!(store.active().display_name, "Alice");
        assert_eq!(store.active().avatar, 1);
        assert_eq!(
            store.active().activity[&local_day_key()].languages["rust"],
            1
        );
        assert_eq!(store.active().activity[&local_day_key()].saves, 1);
        assert_eq!(store.practice_dir(), first);
        assert!(store.active().practice_enabled);
        drop(store);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn concurrent_windows_merge_buffered_counts_without_changing_identity() {
        let root = temp();
        let mut a = Profiles::load(&root).unwrap();
        let mut b = Profiles::load(&root).unwrap();
        let original = fs::read(root.join("profiles.json")).unwrap();
        for _ in 0..100 {
            a.record_on("2026-01-01", "rust", false);
            b.record_on("2026-01-01", "lua", false);
        }
        assert_eq!(
            fs::read(root.join("profiles.json")).unwrap(),
            original,
            "edits must not write to disk"
        );
        a.flush(false).unwrap();
        b.flush(false).unwrap();
        let other = a.create().unwrap();
        a.switch(&other).unwrap();
        b.record_on("2026-01-01", "lua", true);
        b.rename("Still Default").unwrap();
        assert_eq!(b.active().id, "default");
        let mut c = Profiles::load(&root).unwrap();
        assert_eq!(c.active().id, other);
        c.switch("default").unwrap();
        let day = &c.active().activity["2026-01-01"];
        assert_eq!(day.edits, 200);
        assert_eq!(day.saves, 1);
        assert_eq!(day.languages["rust"], 100);
        assert_eq!(day.languages["lua"], 100);
        drop(a);
        drop(b);
        drop(c);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn simultaneous_flushes_and_busy_lock_preserve_all_deltas() {
        let root = temp();
        let mut main = Profiles::load(&root).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
        let mut handles = Vec::new();
        for worker in 0..4 {
            let root = root.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                let mut window = Profiles::load(root).unwrap();
                for _ in 0..100 {
                    window.record_on("2026-01-02", &format!("lang-{worker}"), false);
                }
                barrier.wait();
                window.flush(false).unwrap();
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        main.record_on("2026-01-02", "rust", true);
        let lock = RegistryLock::acquire(&root, false).unwrap().unwrap();
        main.flush(true).unwrap();
        assert!(
            !main.pending.is_empty(),
            "busy nonblocking flush must retain its buffer"
        );
        drop(lock);
        main.flush(false).unwrap();
        let day = &main.active().activity["2026-01-02"];
        assert_eq!(day.edits, 400);
        assert_eq!(day.saves, 1);
        assert_eq!(day.languages.len(), 4);
        drop(main);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn external_removal_retains_window_identity_and_pending_activity_until_recovery() {
        let root = temp();
        let mut window = Profiles::load(&root).unwrap();
        let second = window.create().unwrap();
        let original = window.registry.clone();
        window.record_on("2026-01-03", "rust", false);
        let mut external = original.clone();
        external.profiles.retain(|p| p.id != "default");
        external.active = second;
        atomic_json(&root.join("profiles.json"), &external).unwrap();
        assert!(window.flush(false).is_err());
        assert_eq!(window.active().id, "default");
        window.record_on("2026-01-03", "rust", false);
        assert!(!window.pending.is_empty());
        window.error = Some("Activity not saved: temporary problem".into());
        atomic_json(&root.join("profiles.json"), &original).unwrap();
        window.flush(false).unwrap();
        assert!(window.error.is_none());
        assert_eq!(window.active().activity["2026-01-03"].edits, 2);
        drop(window);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn idle_refresh_does_not_repaint_but_detects_external_changes() {
        let root = temp();
        let mut window = Profiles::load(&root).unwrap();
        let stamp = registry_stamp(&root);
        window.last_flush -= std::time::Duration::from_secs(11);
        let lock = RegistryLock::acquire(&root, false).unwrap();
        assert!(!window.flush_if_due());
        assert_eq!(registry_stamp(&root), stamp);
        drop(lock);
        let mut other = Profiles::load(&root).unwrap();
        other.rename("Other window name").unwrap();
        window.last_flush -= std::time::Duration::from_secs(11);
        assert!(window.flush_if_due());
        assert_eq!(window.active().display_name, "Other window name");
        window.last_flush -= std::time::Duration::from_secs(11);
        assert!(!window.flush_if_due());
        drop(other);
        drop(window);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replacing_and_clearing_owned_avatars_removes_old_files_only_after_commit() {
        let root = temp();
        let mut window = Profiles::load(&root).unwrap();
        let directory = window.data_dir();
        fs::write(directory.join("avatar-1.png"), "first").unwrap();
        window
            .set_avatar_for("default", "avatar-1.png".into())
            .unwrap();
        fs::write(directory.join("avatar-2.png"), "second").unwrap();
        window
            .set_avatar_for("default", "avatar-2.png".into())
            .unwrap();
        assert!(!directory.join("avatar-1.png").exists());
        assert!(directory.join("avatar-2.png").exists());
        assert!(window
            .set_avatar_for("default", "../unrelated.png".into())
            .is_err());
        assert!(directory.join("avatar-2.png").exists());
        window.next_avatar().unwrap();
        assert!(!directory.join("avatar-2.png").exists());
        drop(window);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dates_and_corruption_are_handled_without_overwriting() {
        assert_eq!(day_key(0), "1970-01-01");
        assert_eq!(day_key(19782), "2024-02-29");
        assert_eq!(day_key(today()), local_day_key());
        let root = temp();
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("profiles.json"), "broken").unwrap();
        assert!(Profiles::load(&root).is_err());
        assert_eq!(
            fs::read_to_string(root.join("profiles.json")).unwrap(),
            "broken"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
