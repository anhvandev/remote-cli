# Design: rcli GUI

The locked system for the Agent GUI window (`src/gui.rs`, built with `--features gui`). It is the Dashboard's system
(`dashboard/design.md`), mapped onto egui: one product, one look. Change it on purpose; the file is the rule.

## System
- Genre: modern-minimal
- Shape: app screen, one window (560 x 760, min 420 x 360), a header plate over one scrolling column
- Theme: Wayfind, colourway platform (shared with the Dashboard)
- Axes: light/mono/cool · posture: committed
- Light only: `look::install` sets `Theme::Light`, so the OS dark mode never swaps the style

## Tokens
`mod look` in `src/gui.rs` is the source of truth: the Dashboard's OKLCH tokens (`dashboard/app/tokens.css`) converted
to sRGB, since egui takes `Color32`. If a Dashboard token changes, convert it again and change both.
```rust
PAPER   = (237, 240, 244)  // oklch(95.5% 0.006 250)  window ground
PAPER_2 = (221, 226, 231)  // oklch(91% 0.009 250)    the note bar
PAPER_3 = (251, 254, 255)  // oklch(99.5% 0.005 250)  fields; text on ink
INK     = (10, 13, 19)     // oklch(16% 0.014 262)    text, borders, the header plate
INK_3   = (86, 91, 100)    // oklch(47% 0.016 260)    labels, hints
RULE    = (185, 190, 197)  // oklch(80% 0.011 250)    hairlines between rows
SIGNAL  = (32, 85, 225)    // oklch(51% 0.22 264)     live: connected, granted, the primary action, focus
AMBER   = (244, 181, 34)   // oklch(81% 0.16 82)      a request waiting on the person; text selection
ERROR   = (190, 35, 35)    // oklch(52% 0.19 27)      errors
```
- Font: Chivo Mono, embedded from `assets/fonts` (OFL, `assets/fonts/OFL.txt`): Regular for text, ExtraBold as the
  `bold` family. Static instances of the variable font, subset to Latin, Latin Extended and Vietnamese (about 37 KB
  each). egui's own fonts stay behind Chivo as the fallback.
- Sizes: body 15, button 14 bold, small 12, section heads 17 bold, header wordmark 20 bold, the Device ID 30 bold.
- Borders: `BAR` 2px on controls, fields and under section heads; `HAIR` 1px between rows. Corner radius 0 everywhere.

## Rules
- The header is an ink plate: the wordmark on the left, the connection on the right as a square signal and a word.
- The Device ID is the window's figure: it is what a Controller types, so it is set large, like a platform number.
- A section is a bold head over a 2px ink rule (`look::section`); facts are a two-column `Grid`, label in `INK_3`.
- Cobalt only for what is live or the one action of a group; never as decoration.
- Amber only as a fill under ink text: the "Waiting for you" plate. Never amber text on paper.
- Status is a square (`look::signal`): filled cobalt when on, hollow when off. Never a round dot.
- A name above its buttons, never beside them: the window is narrow and names run long.
- Plain text, no emoji; labels without trailing colons.

## Controls
- Primary (`look::primary`): cobalt fill, 2px cobalt border, white bold text. One per group (Install, Apply, Change, Accept, Allow).
- Plain button: transparent with a 2px ink border; hover and press invert to ink with paper text.
- A choice set (`look::choices` + `look::choice`): cells joined edge to edge; the chosen one fills ink with paper text.
- Field: `PAPER_3` with a 2px ink border; focused, a 2px cobalt border.
- Buttons and fields 32px tall; padding 12 x 6.

## Motion
- Character: severe. egui's defaults only; no animation is added. The window repaints every 2 s to follow the Agent.

## Exports
- `src/gui.rs` (`mod look`) holds the egui mapping; `dashboard/design.md` holds the web system it comes from.
