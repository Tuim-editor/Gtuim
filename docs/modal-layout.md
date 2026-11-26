# Modal sizing, wrapping, and readability

## Cause

Three GPUI/Taffy flex interactions caused modal regressions:

1. **Horizontal overflow and vertical displacement:** A horizontal flex child has an
   intrinsic automatic minimum width unless explicitly allowed to shrink. Long
   descriptions, repository paths, and status labels could make scroll panes wider
   than their modal, where the card's clipping mask hid the right edge (including
   badges). Missing vertical constraints also allowed status messages to push lists
   and footer actions outside the card.
2. **Intrinsic text collapse under percentage `max_w`:** Setting percentage
   `max_w(relative(...))` on an auto-sized text flex item (such as a Settings control,
   Plugin Center status badge, or Git metadata label) can resolve to padding-only
   width during Taffy's intrinsic measurement — even without `min_w_0()` — turning
   controls like `[x]`, `ide ▾`, or status badges into one-character-wide vertical
   ribbons.
3. **Zero-width labels in wrapping rows:** In GPUI, `.flex_1()` sets
   `flex_basis: 0%`. Inside a `.flex_wrap()` row alongside a wide non-wrapping badge,
   Taffy treats the label's hypothetical width as `0px`, keeps both items on line 1,
   and can squeeze the label to near-zero width instead of wrapping the badge onto
   the next line.

## Layout rules

- Modal content panes use `w_full().min_w_0()`; scroll panes also have
  `flex_1().min_h_0().overflow_y_scroll()`.
- Use `gui::modal_layout::scroll_column` and `list_card` for modal lists. Cards keep
  natural height with `flex_shrink_0()` so wrapped text can be read by scrolling.
- Prose, descriptions, and paths wrap; short fixed controls, badges, shortcuts, and
  stepper buttons use `flex_shrink_0().whitespace_nowrap()` with no percentage
  `max_w(relative(...))`. Do not mask layout bugs with ellipses or truncation.
- Auto-sized controls that may hold unusually long values (such as Settings theme
  values or recipe form values) use an explicit pixel cap on a default block `div()`
  — e.g. `flex_shrink_0().max_w(px(260.0)).whitespace_normal()` (or `max_w(px(180.0))`
  for recipe field values) — without `min_w_0()`, without `max_w(relative(...))`, and
  without `.flex()` (a flex container around a direct text child can fail to recompute
  wrapped height after shrinking). Fixed metadata columns use an explicit width such
  as `w(px(180.0)).max_w_full()`.
- Wrapping label+control rows (`.flex().flex_wrap()`) give the label column an
  explicit minimum wrapping threshold via `flex_basis(px(...)).flex_grow().min_w_0()`
  (e.g. `160px` for plugin names and recipe labels, `240px` for setting
  labels/descriptions) instead of `flex_1().min_w_0()`, so wide controls wrap cleanly
  onto the next line while preserving readable label width.
- Headers/footers stay outside the scrolling body. Dynamic status strips have a
  capped height and their own vertical scroll so diagnostics remain readable without
  displacing the content or actions.
- Auto-sized buttons must not combine `min_w_0()` with percentage max-width in
  wrapping flex rows: Taffy can resolve their intrinsic measurement to near-zero width.
- Context menus use GPUI `anchored()` window snapping. Settings dropdowns are height
  constrained relative to the dialog. Long notice toasts have a scrollable height cap.

## Tests

```sh
cargo test --lib gui::modal_layout -- --test-threads=1
cargo test --lib gui::modal_tests -- --test-threads=1
cargo test --lib -- --test-threads=1
```

These use GPUI's headless test platform and real Taffy/text layout. Production-modal
fixtures use an inert pipe backend rather than launching Neovim or writing user
configuration. Containment assertions (`right() <= card.right()`) alone accept
one-character-wide vertical ribbons, so tests enforce **both** containment and
readability bounds (minimum width, single-line height for ordinary controls/badges,
compact card heights, and bounded multi-line wrapping for unusually long values):

- Settings tabs 0, 1, 3, and 4 (toggles, Mode dropdown, Theme selector, plugin
  actions, keybindings): readability, normal horizontal alignment on desktop widths,
  clean wrapping on 720×460, compact row heights across 720×460, 1380×880, and
  1920×1200, plus intentional bounded wrapping (no truncation) for unusually long
  custom values.
- Plugin Center status badge readability, plugin name width/height, normal horizontal
  name/badge alignment, compact normal cards, long descriptions, footer bounds, and
  actual wheel scrolling at 720×460, 1380×880, and 1920×1200.
- Recipe forms (label/value readability and compact rows), health details, long
  errors, and full trust warnings at minimum size.
- Palette, Settings, Mason (including badge readability), Lazy, Git (including
  metadata column width), Orchestrator, and all five Bug Report stages at minimum
  and normal window sizes.
- Edge-positioned editor/explorer context menus, menu dropdowns, Settings dropdowns,
  and unusually long notice toasts (including minimum readable width).
- Additional shared-layout fixtures with larger fonts (including badge readability).

No window-manager interaction, screenshots, or network access are needed for unit
checks; automated tests verify GPUI/Taffy layout geometry directly and do not capture
desktop screenshots. For live visual confirmation, launch a fresh build cleanly (or
an isolated instance with temporary `HOME` and `XDG_{CONFIG,DATA,STATE,CACHE}_HOME`
directories without touching any existing Gtuim process). The `test-support` feature
is a dev-dependency only; release builds keep their normal GPUI feature set.
