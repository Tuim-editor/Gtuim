# Scrolling and input isolation

The GUI preserves wheel magnitude and accumulates fractional trackpad movement using
actual grid cell height. Previously every nonzero pixel event became a full Neovim
wheel tick (normally three lines), so even tiny touchpad motions caused large jumps.
The editor now uses one-line wheel ticks; the native terminals accept precise row
counts without changing the TUI's three-line wheel behavior. Platform-provided
trackpad momentum is retained rather than applying a second synthetic inertia curve.

This is **row-granular scrolling**, not pixel-interpolated editor animation.
Neovim's `smoothscroll` option is enabled when available to scroll through wrapped
lines; that option itself is not an animation engine. Native GPUI lists retain their
normal pixel scrolling.

Modal backdrops and context-menu layers occlude underlying mouse hitboxes and consume
wheel propagation. Editor and terminal wheel handlers also reject input while a GUI
overlay is open. The settings dropdown similarly isolates its scroll from the settings
list below it.

## PDF continuous scrolling

The PDF viewer stacks pages vertically in a single native scroll surface. Mouse-wheel
and trackpad movement crosses page boundaries without requiring page-turn buttons.
The page counter follows the viewport; previous/next and first/last-page navigation
jump within the same document. The viewer only creates a small window of page elements
and lazily renders nearby pages rather than rasterizing the entire PDF.

Use **View → Open PDF in Split…** (also available through **F1**) to choose a PDF
beside your current editor without replacing the source buffer. Scroll directly over
the PDF pane; editor/terminal scrolling stays independent. The viewer toolbar can
switch between full and split presentation.

Additional manual checks:

1. Open a multipage PDF, scroll slowly through a page boundary, then scroll rapidly
   across several pages. Adjacent pages should appear in one continuous document,
   and the page indicator should track the viewport without snapping back.
2. Jump to a page using navigation, zoom in/out, change fit mode, and resize the
   window. Scrolling and page geometry should remain coherent.
3. Open a PDF in a split beside an unrelated source file. Verify the source remains
   selected and editable and wheel input over the PDF does not scroll the editor.
4. Repeat with a long PDF, a mixed-orientation PDF, and the vertical terminal visible.
   Open a modal and verify background PDF/editor scrolling remains isolated.
5. Replace the PDF on disk, including a version with fewer pages. Verify auto-reload
   keeps a valid position and that closing or switching documents cannot restore stale pages.

## Checks

```sh
cargo test --lib gui::scroll
cargo test --lib tui::shell_terminal
cargo test --lib -- --test-threads=1
cargo build --release
```

Manual GUI checks (use the newly built executable, not an already running instance):

1. Open a file longer than the viewport. Slowly move a trackpad, then swipe quickly:
   slow movements should accumulate into single-row motion; faster input should
   cover proportionally more rows. Reverse direction and check for immediate response.
2. Check a physical wheel, font zoom, wrapped lines, split editor windows, the bottom
   terminal and the vertical terminal. Each uses its actual row height. In a terminal
   application with mouse reporting, wheel input must still reach the application;
   Shift+wheel should use host scrollback instead.
3. Record the editor's top line. Open Settings, the Extension Shop, the command
   palette, and each context/menu-bar menu in turn. Wheel inside the overlay, at its
   scroll limits, on a non-scrollable label and on the backdrop. The editor top line
   and background terminal scrollback must stay unchanged.
4. Open the Settings theme dropdown and wheel to both limits. Only the dropdown
   should scroll, not the settings list beneath it.
5. Dismiss each overlay and check that ordinary editor/terminal scrolling resumes.

The automated tests cover delta accumulation, magnitude, pixel normalization,
direction changes, stale gesture reset, invalid input, terminal row counts, Shift
bypass and preservation of the TUI wheel behavior. Overlay hit testing and subjective
trackpad feel still require the manual GUI checks above.
