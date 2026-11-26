# Local profiles

## Using profiles

Open the right panel with the avatar button in the top-right header. It contains:

- Local profile creation and switching. Switching preserves open editor buffers, including unsaved changes, and loads the selected profile's saved settings. It does not implicitly commit an unconfirmed settings draft or overwrite another window's saved settings.
- A clickable display name (1–48 printable characters; Enter saves, Escape cancels).
- A native local picture picker accepting PNG, JPEG and WebP. Images must be at most 10 MiB and 8192×8192 pixels; decoding and resizing happen on a background executor. A thumbnail up to 256×256 is copied into profile storage. No remote avatar service is contacted. Use **Use / cycle built-in avatar** to return to a glyph.
- A clickable green 26-week activity calendar and language shares.
- Explicit practice opt-in, independently for every profile. The initial default profile does not automatically initialize practice. Disabling practice retains its progress for later re-enabling.

The right panel scrolls independently of the editor, and widens to up to 640 px when practice is enabled. Escape cancels name editing first, then closes the panel; modal overlays take keyboard precedence. Closing the panel or disabling practice restores editor focus and flushes pending practice drafts. The header and profile surface are hidden in Zen mode; leave Zen mode to access them.

## Storage and migration

The root is `$XDG_DATA_HOME/tuim`, falling back to `$HOME/.local/share/tuim`:

```text
tuim/
  profiles.json                  # identities, startup selection, activity, practice opt-in
  profiles.lock                  # advisory process lock for registry updates
  profiles/
    default/
      settings.json
      avatar-<unique>.png         # optional owned picture copy
      practice/                  # created only after explicit opt-in
    p-<opaque-id>/
      settings.json
      avatar-<unique>.png
      practice/
```

Stable opaque IDs determine paths. Display names can be identical, renamed, or contain punctuation without affecting storage. Practice receives the selected profile's stable, absolute `practice/` path, never a display-name path or a shared global directory.

On first use, the default profile copies the existing root `settings.json` if one exists. The original is left untouched. New profiles start from factory settings, not the previously active profile's settings. Runtime files, installed plugins, plugin configuration and project files remain shared; profiles are not separate plugin installations or filesystem sandboxes. Plugin manager changes still require an explicitly requested editor restart.

Malformed or unsupported registries report an error rather than silently overwriting user data. There is no profile deletion UI or remote synchronization. Back up `profiles.json` and the `profiles/` tree together. Replacing a profile picture or returning to a built-in avatar removes the previously referenced owned thumbnail only after the registry update commits. Unreferenced files left by an interrupted import can be removed manually without affecting activity or practice.

## Activity semantics and concurrency

Only text changes and saves in named, modifiable normal-file Neovim buffers count. Filetype identifies the language; unknown types appear as plain text. Cursor movement, rendering, terminal interaction, and duplicate file-save UI notifications do not count. Formatter changes may count as edit batches. These are local usage statistics, not a Git contribution graph or a measure of source lines.

Activity dates follow the machine's **local calendar date**, matching the practice daily boundary. Calendar arithmetic operates on civil-day ordinals rather than adding 24-hour durations, so daylight-saving changes do not skip days.

Events accumulate in memory. Registry writes are batched at approximately ten-second intervals, plus explicit profile mutations and clean shutdown. No disk write occurs on each typed character. An abrupt process or machine crash can lose the most recent unflushed interval.

Registry updates acquire a process-level file lock, reload the current registry, merge pending per-profile/per-day deltas, and atomically rename the new JSON into place. Periodic flush uses a nonblocking lock and retries if another window is writing. This prevents a stale window snapshot from overwriting another window's activity, profile names, avatars, or opt-in changes. Unchanged idle registries are checked by file metadata without locking/reparsing or repainting. If an active profile is removed externally, the window retains its identity and buffered counts and reports an error until that profile is restored, rather than silently switching settings or panicking. Each existing window retains its own selected identity; the last explicit switch determines the startup profile for a new window.

## Practice integration contract

The profile surface owns the opt-in gate and renders the practice widget only for an enabled profile, below the language statistics. Integration uses `practice::PracticePanel::new(profile_dir: PathBuf, cx: &mut Context<PracticePanel>)`, cached as an entity per profile by `profile_panel.rs`. Cached entities preserve in-progress UI state across profile switches without sharing a directory or challenge state. Theme/font changes use `sync_appearance`; the host checks `refresh_daily` while the panel is open. A close event dismisses the host panel rather than opting the profile out. The widget receives a unique stable per-profile data directory. The practice implementation must use local calendar dates (`super::profiles::local_day_key()` is available inside `src/gui`) and must not put progress in shared root storage. Disabling practice must not delete its data.

## Verification

```sh
cd /home/roubs/Project/Gtuim
cargo check --offline
cargo test --offline --lib
nvim --clean --headless -n '+lua assert(loadfile("/home/roubs/Project/Gtuim/src/nvim/tuim_init.lua"))' +qa
```

Profile tests cover settings migration and isolation, stable practice paths, default opt-out, name/avatar persistence, local date roundtrips, corrupt-registry preservation, batched writes, stale and simultaneous concurrent-window merges, busy-lock retry safety, owned picture copies and validation, and native panel interaction. A real clean-Neovim test executes the production settings loader/activity hooks and verifies that switching keeps unsaved buffer contents, switches the Lua settings path, applies profile settings, and attributes queued events to the outgoing profile.
