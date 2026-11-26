# Bug reporting implementation verification

## Implementation and review

The three existing Herdr agents independently recommended prefilled GitHub issue
drafts over email or a credentialed webhook. Work was split between the shared
report widget, GPUI integration/tests, and release tooling/portability/docs. A
fresh fourth agent reviewed the result. Its two actionable findings (GUI
keyboard recovery and TUI manual recovery) were fixed and rechecked.

Default tracker: `Tuim-editor/Gtuim`; override with `GTUIM_BUG_REPORT_REPO`.
The repository was transferred from `Rouboufy/Gtuim` to the `Tuim-editor`
organization at the owner's request. A private tracker requires reporter access.
No GitHub issue is automatically posted. Opening a draft transmits URL-embedded
content to GitHub and may retain it in browser history. Redaction is best-effort.
Oversized reports retain their complete text for clipboard/manual recovery.

## Final local checks

- `cargo test --locked --offline`: **216 unit tests + 22 integration tests passed**.
- Independent focused report run: **21 tests passed** (8 GUI, 13 shared widget).
- `python3 tests/test_plugin_store.py`: **10 tests passed**.
- `cargo build --locked --offline --release --target x86_64-unknown-linux-gnu`: passed.
- Packaging script syntax and unsupported/missing-target rejection: passed.
- Packaged raw and archive-contained executables: `--version` passed (`0.4.0`).
- Release `--help`: passed; unknown option exits with status 2.
- Both release checksums verified; extracted executable matches raw executable.
- Linux shared-library resolution: no missing libraries on the build machine.

Artifacts: `dist/gtuim-0.4.0-linux-x86_64`, its `.tar.gz`, and `.sha256`.

## Limits

GUI tests use GPUI's headless test platform. No real issue was submitted, and
these checks do not establish live browser/clipboard integration or visual GUI
behavior on every OS. A very long TUI export path may exceed the visible modal
rows, although the full path and exported report are retained internally.

The local Linux build is dynamically linked and built on a rolling-release
system; compatibility with older Linux distributions is not guaranteed.

No macOS executable has been produced or tested. The native three-platform
workflow is prepared in `.github/workflows/build-release.yml` but has not run.
This source directory has no Git remote. Publishing the source to an agreed
repository and running native macOS CI (with Xcode/Metal) are still required.
Unsigned macOS artifacts will also require platform-specific smoke testing and
may prompt for Gatekeeper approval.
