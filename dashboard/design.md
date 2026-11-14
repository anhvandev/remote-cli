# Design: Remote CLI Dashboard

The locked system. Every Hallmark run reads this first and screens share it. Change it on purpose; the file is the rule.

## System
- Genre: modern-minimal
- Shape: app screen (sidebar shell, tables before cards)
- Theme: Wayfind, colourway platform (public signage set as a console: the fleet reads like a departure board)
- Axes: light/mono/cool · posture: committed
- Light only: `@custom-variant dark` stays pinned to `.dark`, which nothing sets

## Tokens
`app/tokens.css` is the source of truth (`@theme static`, so each is a Tailwind utility: `bg-paper`, `text-ink-3`, `border-ink`).
Wayfind's `accent`/`muted` are renamed `signal`/`ink-3` so they don't collide with shadcn's `accent`/`muted`.
```css
--color-paper: oklch(95.5% 0.006 250);  --color-paper-2: oklch(91% 0.009 250);  --color-paper-3: oklch(99.5% 0.005 250);
--color-ink: oklch(16% 0.014 262);  --color-ink-2: oklch(29% 0.014 262);  --color-ink-3: oklch(47% 0.016 260);
--color-rule: oklch(80% 0.011 250);  --color-ink-rule: oklch(34% 0.014 262);
--color-signal: oklch(51% 0.22 264);  --color-signal-ink: oklch(99.5% 0.003 250);  --color-focus: oklch(51% 0.22 264);
--color-amber: oklch(81% 0.16 82);  --color-error: oklch(52% 0.19 27);
--font-display: var(--font-chivo), "Chivo Mono", ui-monospace, monospace;  --font-body: var(--font-display);
--radius-card: 0;  --rule-hair: 1px;  --rule-bar: 2px;  --shadow-card: 0 24px 40px -24px oklch(8% 0.01 260 / 0.6);
```
The spacing scale, the app type scale (1.2 ratio, 16px body), the easings and the durations live in `tokens.css`.

## Rules
- Chivo Mono everywhere (400, 500, 800, loaded by `next/font` in `app/layout.tsx`); no proportional face, no second family.
- Heads 800, one colour; labels and table heads 500 in `ink-3`, a step smaller, sentence case.
- Cobalt (`signal`) only for what is live: Online, the primary action, the current tab or nav item.
- Amber only as a fill under ink text (waking, revoked, update needed) or as a figure on ink; never amber text on paper.
- `error` for destructive actions and errors; no other bright colour.
- Status is a square: cobalt fill Online, hollow Offline, amber Waking. Never a round or glowing dot.
- Surfaces: radius 0; 2px ink borders on cards, tables' head rule, inputs; hairline `rule` between rows. No shadow on cards, `shadow-card` only on popovers.
- Ink plates: the sidebar, the sign-in column, the live view and the terminal sit on `ink` with paper text.
- Page head: `h2` at `text-3xl font-extrabold` over a 2px ink rule; figures that matter (counts, CPU, RAM) in 800 at `text-2xl` or larger.

## Controls
- Primary: cobalt fill, square, 2px border, 800, 32 to 40px tall; hover goes to ink.
- Secondary: `paper-2` fill; outline: transparent with a 2px ink border, hover inverts to ink.
- A choice set (tabs, range picker) is square cells sharing 2px borders; the chosen cell fills ink with paper text.
- Focus: a 2px `focus` outline, never animated (inset on inputs).

## Motion
- Character: severe. Colour changes at `--dur-micro` only; no reveal, no loop; under reduced motion everything ends at once.
- `scrollbar-gutter: stable` on `html`: the width must not jump (the Live view maps clicks by its width).

## Exports
- `app/tokens.css` and `app/base.css` (imported in `app/globals.css`, base in `layer(base)`).
- shadcn: `app/globals.css` points shadcn's variables at the tokens (`--primary: var(--color-signal)`, `--muted-foreground: var(--color-ink-3)`, `--input: var(--color-ink)`, every radius 0). Re-running `shadcn add` on a component overwrites its Wayfind classes; re-apply them.
