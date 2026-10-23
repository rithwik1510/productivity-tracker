# Design reference: T3 Code + Grok, adapted

T3 Code and Grok both read as premium for the same reason: they hold back. Both use a near-black neutral canvas, separate layers with low-alpha white borders instead of gray fills, keep motion short, and give color a job before using it. We borrow that restraint and leave behind the chat and coding layouts it was built for. Source values were checked on 2026-10-21 (T3 Code `main` @ `680152e`; grok.com production CSS).

**Glass amendment (2026-10-21):** the owner chose a glass look. The window uses Windows 11's native **Acrylic** backdrop (Tauri `windowEffects`), and every surface is frosted glass on top of it. The restraint rules below still apply: translucency is for surfaces only; text and numbers stay solid and high-contrast. Known risk: Acrylic can lag while the window is dragged or resized. If that's bad in practice, switching to Mica is a one-line config change.

**Redesign amendment (2026-10-22):** the owner approved the redesign prototype. Navigation moved from top tabs to a left rail, the screen title moved into the titlebar, the picker became a tile grid (first run paints two passes; Apps & sites opens a category popover per tile), and Settings gained an Appearance choice. The rail, titlebar, tile and icon rules are in section 4, the new tokens in section 2, the motion additions in section 5 (items 7-9).

**Polish amendment (2026-10-22, issue 09):** the owner ran the real window and asked for three things.
- **The spacing scale is px.** `html` sets the 14px body size, so Tailwind's rem-based `--spacing` had shrunk every spacing utility to 14/16: the rail came out 63px instead of 72 and caption buttons 40px instead of 46. The rail thumb moved by a fixed 56px step while the items were really 49px apart, so it drifted further off with each item. `--spacing` is now `4px`, so every size in this document is what renders, and the rail thumb is laid out inside the selected item (Motion `layoutId`) instead of being moved by a constant.
- **The dark palette is a ladder.** The old solid steps (`#0a0a0a` → `#111` → `#181818`) were too close to tell apart, so cards read as outlines on one flat black. The steps are now bigger and each has one job: canvas < rail < card < raised < popover < selected. Text tiers are spread wider too (section 2).
- **Maximize.** The titlebar has a maximize/restore button, and views stop widening at `--content-max` (section 4; per-view `--view-max` since issue 14).

**Settings amendment (2026-10-22, issue 13):** Settings became one scrolling page of sections, each a 12/600 `--fg-2` heading over one card of rows, with quiet `--fg-3` lines under a card where needed. Delete all data confirms in a modal `<dialog>` styled as a popover (`--surface-3` with its blur, `--border-strong`, the popover shadow and motion), over a 45% black backdrop. Its "Delete everything" button is the one destructive action in the app and is filled with `--danger`, which is `--close-hover`: the Windows red stays the single non-palette color, now with two uses that both mean "this closes or destroys". White text on it is 5.3:1. Cancel takes focus first.

**Status line amendment (2026-10-22, issue 16):** the Day view's live line became a status line with one state at a time (see plan section 7). "Can't save, retrying" is the app's one warning, and it reuses `--danger` for its 8 px dot, with the words in `--fg-1`: `--danger` on `--surface-1` is about 3.4:1, enough for a dot but not for 12 px text. That is `--danger`'s third use, and it still means "something is wrong". The other states are `--fg-3` meta text like the old line, with no dot except Tracking's category dot.

**Adaptive layout amendment (2026-10-22, issue 14):** the owner said a full-screen window "looks weird" and asked for a layout that works from a laptop up to big monitors. Every view was captured at eight sizes (760×520 to 3440×1440). What was wrong: the Day ring card grew into a 400×960 tower with the ring floating in the middle; Day list rows ran 860 px wide, so a name sat far from its time; the Pie chart's legend spread across the whole card with the donut fixed at 232 px; Settings sat left-aligned in a 1280 px column with empty space to its right; first run's brand and footer lined up with the window instead of the content; and the 820×580 minimum was taller than a 1366×768 laptop's work area at 125% (about 1093×576). The responsive rules that replace `--content-max` are in section 4 under "Window sizes".

**Logo amendment (2026-10-23, issue 17):** the placeholder ring mark became the owner's mark, a P made of a white stem and a blue bowl. Its rules are in section 4 under "The mark".

**Watch switch amendment (2026-10-23, issue 18):** the app popover in Apps & sites gains a row under the category control for a Productive app that isn't a browser: "Counts while I watch" in 12/500 `--fg-1` over a `--fg-3` meta description, with the Settings `Switch` to the right. It adds no divider or new token, and picking it doesn't close the popover. The status line's Tracking state can end in " · watching", like " · waiting".

## 1. What we're borrowing

- A dark, neutral tint over the Acrylic backdrop, never a colored tint. T3's dark comment still guides us: "Borders and hover states provide separation without milky gray fills."
- Layers come from small white-alpha steps (4–8%) on frosted glass plus white-alpha borders. No drop shadows on in-window surfaces; glass cards get a 1px top highlight instead.
- Color carries meaning. Chrome stays grayscale, so the Productive accent only shows up where it encodes Productive time.
- The primary action is an inverted button (white fill, near-black text), as in Grok's `--button-filled`. That keeps the accent out of buttons.
- One set of radius tokens drives every control so sizes don't drift. T3 does this with `--control-radius`.
- Motion is 150 ms by default and respects reduced-motion everywhere. T3 ships panel animations at 0 ms (off) by default.
- Tabular numerals wherever numbers change. Both codebases apply `tabular-nums`.

## 2. Palette (dark default)

Glass surfaces (Acrylic active):

| Token | Value | Use |
|---|---|---|
| `--bg` | `rgb(10 10 10 / 66%)` (was 60%) | tint laid over the Acrylic backdrop. 66% keeps cards from going grey when a light window sits behind |
| `--rail-bg` | `rgb(255 255 255 / 2.5%)` (was `rgb(0 0 0 / 14%)`) | the navigation rail, one step above the canvas. It used to be darker than the canvas, which broke the ladder |
| `--surface-1` | `rgb(255 255 255 / 4.5%)` (was 4%) | cards, ring panel, list container (no `backdrop-filter`: inside WebView2 it only blurs page content; Acrylic already blurs the desktop behind the window) |
| `--surface-2` | `rgb(255 255 255 / 7.5%)` (was 7%) | row hover, segmented-control track |
| `--thumb` | `rgb(255 255 255 / 11%)` (was 9%) | selected: segment thumbs and the rail's current view |
| `--surface-3` | `rgb(24 24 24 / 82%)` + `backdrop-filter: blur(24px) saturate(140%)` | popovers, menus, tooltips: the only surfaces that blur, because they sit over page content |
| `--glass-highlight` | `inset 0 1px 0 rgb(255 255 255 / 6%)` | top edge of every glass card |

Glass follows the same ladder as solid, as white steps over the tint.

Solid fallback: Windows drops the Acrylic backdrop when the window is inactive, when "Transparency effects" is off, and in battery saver. The app must look finished then too, so these solid tokens apply whenever the backdrop is off. The switch is one root attribute, `data-surface="glass|solid"`, computed in one place from window focus, a backend check of the Windows settings, and the user's Appearance setting (Settings → `Glass · Solid`; Solid is opaque and lighter on battery). Glass shows only when Appearance is Glass, Transparency effects are on, and the window is focused:

A ladder of neutral near-blacks. Each step is 5–7 sRGB levels, enough to see without a border. The old steps were 3–7 levels with the same token doing two jobs, which is why cards read as outlines on a single black:

| Token | Value (was) | Use | Inspired by |
|---|---|---|---|
| `--bg` | `#090909` (`#0a0a0a`) | window canvas, titlebar | T3 dark `--background: neutral-950`; Grok `--neutral-1000` is 2% (`#050505`) |
| `--rail-bg` | `#0e0e0e` (`#0d0d0d`) | the navigation rail, one step above the canvas | T3's sidebar sits on `--card`, above its background |
| `--surface-1` | `#141414` (`#111111`) | cards, ring panel, list container | Grok `--surface-l1` 8% (`#141414`) |
| `--surface-2` | `#1b1b1b` (`#181818`) | raised: row hover, segmented-control track, monograms | between Grok l1 and l2 |
| `--surface-3` | `#202020` (`#212121`) | popovers, menus, tooltips | Grok `--surface-l2` = neutral-900 (13%) |
| `--thumb` | `#272727` (was `--surface-3`) | selected: segment thumbs and the rail's current view, always with a `--border-strong` hairline | one clear step above the track it sits in |

Shared tokens (both modes):

| Token | Value | Use | Inspired by |
|---|---|---|---|
| `--border` | `rgb(255 255 255 / 7%)` (was 6%) | default hairline | T3 dark `--border` 6%, Grok `--border-l1` 8% |
| `--border-strong` | `rgb(255 255 255 / 10%)` | inputs, selected row | T3 `--input` 8%, Grok `--border-l1` 8% |
| `--border-focus` | `rgb(255 255 255 / 50%)` | focus ring (2px; inset on controls, outer 2px offset on the white primary button). 50% clears 3:1 non-text contrast on every surface; Grok's 20% `--border-l3` measured under 2:1 | Grok `--border-l3`, raised for WCAG |
| `--fg-1` | `#f5f5f5` | primary text, big numbers | Grok `--neutral-100` (95%) |
| `--fg-2` | `#a8a8a8` (was `#9e9e9e`) | secondary labels (7.8:1 on surface-1, 6.9:1 on surface-3) | T3 `--muted-foreground` ≈ neutral-400 `#a3a3a3` |
| `--fg-3` | `#888888` (was `#858585`) | tertiary: chart axis labels, counts, hints, placeholders (5.2:1 on surface-1, 4.7:1 on surface-3) | Grok `--fg-tertiary` = neutral-400 (52%) |
| `--fg-4` | `#646464` (was `#636363`) | only what has no value yet: disabled controls, future days, the donut's "more" slice (3.1:1 on surface-1). Never for text you need to read: counts, hints and placeholders moved to `--fg-3` | Grok `--fg-quaternary` = neutral-500 (39%) |
| `--close-hover` | `#c42b1c` | the titlebar Close button on hover: the single non-palette color, because that is what every Windows user expects | Windows 11 caption buttons |
| `--productive` | `oklch(0.70 0.16 258)` ≈ `#5b9eff` | ring fill, Productive bars/slices, heatmap top step | T3 `--primary: oklch(0.571 0.21 264)`, Grok `--fg-electric-blue: #427eff`, both lightened for dark-on-data use |
| `--entertainment` | `oklch(0.60 0.04 75)` ≈ `#8f7d66` | Entertainment bars/slices/labels | our own: a desaturated warm sand |

Why these two hues: blue against warm sand sits on the blue–yellow axis, which stays distinguishable under the common red–green color-vision deficiencies. Entertainment is also lower in lightness and chroma, so it sits behind Productive without reading as an error color. Never use red or green for the categories, because they imply judgment.

Heatmap ramp (5 steps, relative to the current Daily Goal: empty · < 25% · < 50% · < 100% · goal met): `#1c1c1c` (empty, one step above the card; white 5% over glass) → `#112d55` → `#1e4b8d` → `#3370c7` → `#5b9eff`. Same hue, stepped OKLCH lightness 0.30 / 0.42 / 0.55 / 0.70, so the steps look evenly spaced.

Ring: the track is `--border-strong` and the fill is `--productive` with a soft glow (`drop-shadow(0 0 12px` Productive at 35%`)`) that suits the glass. Donut arcs and the top bar segment get the same glow. When the Daily Goal is met, the ring stays the same color and the center label changes. No new color, no extra glow.

## 3. Typography

- **Family: Inter Variable** (SIL OFL), bundled locally via `@fontsource-variable/inter` because Tauri runs offline. Grok's sans is proprietary (`universalSans`), but its declared fallback is `"Inter"`, so Inter is the closest free match. T3 uses the system stack (`Segoe UI` on Windows), which is fine for chrome but its numerals are weaker at display sizes. Inter 4 has an optical-size axis, so the hero number gets the tighter display cut automatically.
- **Mono: none.** We show no code. If one is ever needed, use Geist Mono (OFL), which Grok declares as `--font-geist-mono`.
- **Numerals:** `font-variant-numeric: tabular-nums` on every time value, so count-ups and live ticks don't jitter horizontally. Format as `5h 42m`, with units in `--fg-2` at 0.5× the number's size.
- **Scale** (Grok runs UI at 15 px with 13 px meta. We stay one step tighter because this is a desktop utility):

| Role | Size / line-height | Weight | Tracking |
|---|---|---|---|
| Hero number (ring center) | 19% of the ring's width / 1: about 38 in the narrow Day strip, 52 at the default size, capped at 72 from about 1440×900 up | 500 | -0.025em (Grok `--tracking-tight`) |
| Stat number (History totals) | 28 / 1.1 | 500 | -0.02em |
| First-run step heading | 26 / 1.15 | 500 | -0.02em |
| Title (screen name, in the titlebar) | 16 / 1.4 | 600 | -0.01em |
| Body / list row | 14 / 1.45 | 400; app name 500 | 0 |
| Meta (labels, axis, percent) | 12 / 1.35 | 400 | 0 |
| Micro (heatmap month labels only) | 11 / 1.3 | 400 | 0.01em |

Use 400/500/600 only. No bold, no all-caps headers.

## 4. Spacing, radii, borders

- 4 px base grid, in px: `--spacing: 4px`, so `p-6` is 24px whatever the root font size. Window padding 24, gap between cards 16, inside cards 16–20.
- **Window sizes.** The window opens at 1000×680, shrunk to fit the monitor's work area with 16 px to spare on each side, and centered. Its minimum is 760×520: that fits a 1366×768 laptop at 125%, and a window snapped to half of a 1536 px wide screen. It reopens where it was closed (size, position, maximized), unless that spot's monitor is gone; then it opens centered again. The layout follows one set of rules at every size, not per-size fixes:
  - **One column per view.** Each view has a maximum width and is centered right of the rail: Day 1160, History and Apps & sites 1280, Settings 680, first run 880 (`--view-max`, picked by `data-view` on the root). The titlebar's title (or first run's brand) sits on the same left edge. Charts and tile grids use the full width. The Day column stops where a list row is still easy to read across (under about 720 px). Settings and first run are reading columns. First run's footer is a card in its column, so Continue sits under the tiles it confirms.
  - **Dashboards stop at 760 tall.** Day and History fill the window's height up to `--view-max-h: 760px`, then stop. A taller window gets empty canvas below finished cards, not cards stretched into towers. Lists (Apps & sites, Settings) flow from the top.
  - **Sizes scale with the column, not the window.** The view column is the `@container` that sizes read from. On the Day view the ring column is `clamp(268px, 36cqw, 420px)` (316 at the default size), the ring fits its box up to 380, and the hero number is 19% of it, up to 72. The donut is `clamp(208px, 26cqw, 300px)` and sits with its legend (at most 480 wide) as one centered group. Week bars are a quarter of their day's column, 20 to 40 px.
  - **One breakpoint.** Below a 780 px column (a window under about 900 px), the Day view stacks: the ring card becomes a strip (ring and status line, then the two stats in a column beside them) above a full-width list, and the page scrolls, so rows keep their names. This is `--container-narrow`, used as `@max-narrow:`.
  - **Short windows scroll.** A view's own list or chart area scrolls when there isn't room. A view that can't shrink any further (Settings, the stacked Day) scrolls `<main>`.
  - **The scrollbar keeps its place.** `<main>` always reserves the 12 px scrollbar gutter inside its 24 px right margin (12 px padding plus the gutter), so content doesn't shift when scrolling starts, and the title stays on the column's edge. The picker's own scroller does the same inside the margin. Playwright draws real scrollbars for the same reason.
- App list row: 44 px tall, 24 px icon, 12 px gap, time right-aligned in tabular figures.
- Custom titlebar 40 px (T3's Windows Controls Overlay minimum is `max(40px, titlebar-area-height)`). It **carries the screen title** at Title size with 12 px `--fg-3` meta beside it (the date on the Day view), and the screen's own controls (the Day pager, "Back to today") right after, as Windows 11's own apps do. That frees a full row in a 680 px window. Everything that isn't a control drags the window, and a double press maximizes or restores, like a native caption. Caption buttons are 46×40 like Windows: Minimize; Maximize, whose glyph and name swap to "Restore down" while maximized (re-read on every resize, so Win+Up, Aero Snap and the double press keep it right); and Close, which turns `--close-hover` on hover.
- **Rail** (navigation): 72 px wide on `--rail-bg` with a 1 px `--border` on its right, the app mark at the top in a 40 px row level with the titlebar, then Today · History · Apps, and Settings alone at the bottom. Items are 56×52 at `--radius-segment`, a 20 px icon at 1.5 px stroke over an 11/500 label. Selected is the `--thumb` fill with a `--border-strong` hairline, like a segment thumb; the accent stays reserved for Productive data. The thumb is rendered inside the selected item, so it always covers that item's real box. The rail is not a drag region.
- **The mark**: a white stem and a blue bowl forming a P, on a 48-unit grid. `src-tauri/icons/source/logo.svg` is the master; `bun run icons` regenerates every icon from the SVGs there.
  - Colours: stem `#f2f4f8`; bowl a diagonal gradient `#7db3ff → #3d6fd6` (top left to bottom right); tile a vertical gradient `#1d2330 → #0b0c0f` with a 1 px white hairline at 9 %. The bowl's blue belongs to the mark, not to `--productive`, and appears nowhere else, so color in the UI still means Productive data.
  - Tile: on the app, exe and installer icons, the favicon and the README. In the app the tile is dropped and the bare glyph stands on the surface (rail top, first-run brand), because the surfaces are already near-black: the tile's tinted fill read as a muddy square on `--rail-bg` and added a fill outside the white-alpha ladder.
  - Mono: one flat color, no tile, for the tray. `#ffffff` on a dark taskbar, `#1a1a1a` on a light one, following the taskbar's own theme (`SystemUsesLightTheme`, not the apps' setting). Its strokes are heavier (6.5 on the 48 grid) and the glyph fills the tray box with 1 to 2 px to spare.
  - Small sizes: 16, 20, 24 and 32 px are redrawn on their own pixel grid (`logo-N.svg`, `tray-N.svg`) with stroke edges on whole pixels, never downscaled from the master.
  - Minimum size: 16 px, tiled or mono. In the app the bare glyph is 20 px tall, like the rail's icons.
  - Clear space: at least one stroke width (5.5 units) on every side; the tile already carries it. In the first-run brand, 8 px to the name.
  - Never: recolor the bowl to `--productive`, add a glow or outline, put the tiled mark on a colored background, or stretch or rotate it.
- **Tile** (the app and site picker): 40 px tall, `--radius-segment`, 24 px icon, 13/500 name, `--surface-1` fill, `--border` and the glass highlight. Selected takes the category tint (Productive: fill at 14 %, border at 45 %; Entertainment: fill at 20 %, border at 55 %) plus a 13 px check badge on the icon's corner, so color is never the only signal. Grid: `repeat(auto-fill, minmax(160px, 1fr))` with 8 px gaps. "Add" tiles are dashed and transparent.
- **App icons**: the extracted icon, or a neutral monogram (`--surface-2` fill, `--border-strong`, `--fg-2` letter) when there is none. Sites always get the monogram; nothing is ever fetched.
- Radii, as one scale like T3's `calc(var(--radius) ± n)`: `--radius-control: 8px` (buttons, inputs, toggle thumb; T3 `--control-radius` and Grok `--radius` are both 0.5rem), `--radius-card: 12px` (Grok `--radius-xl`), `--radius-panel: 16px` (popovers, picker sheet; Grok `--radius-2xl`). Use `9999px` only for the Productive/Entertainment switch track.
- Segmented control (Bars · Pie · Heatmap), modeled on T3's `toggle-group` segmented variant (`rounded-lg bg-input/40 p-0.5 gap-0.5`): `--surface-2` track, 2 px padding, 10 px outer radius, 8 px thumb in `--thumb` (selected) with a 1 px `--border-strong`. The selected label is `--fg-1`, the others `--fg-2`.
- Borders are always 1 px hairlines. The only shadow is on popovers: `0 12px 28px -18px rgb(0 0 0 / 75%)` (T3 `--shadow-composer-dark`).
- Buttons: an inverted primary (`#f5f5f5` fill, `#0a0a0a` text) used at most once per screen, a ghost secondary (hover `white/4%`, from Grok `--button-ghost-surface-hover`), and press feedback of `scale(0.97)` (T3 `button.tsx`).

## 5. Motion

Source values: T3 sidebar `{ duration: 150, easing: "ease-out" }`, switch thumb `translate .15s`, drawer ease `cubic-bezier(0.32, 0.72, 0, 1)`, panel durations capped at 400 ms and off by default. Grok's default transition is 150 ms `cubic-bezier(.4,0,.2,1)`, and it defines `--ease-out-quint: cubic-bezier(.23,1,.32,1)`.

Our tokens:
- `--dur-fast: 150ms` with `cubic-bezier(.4,0,.2,1)` for hover, color, and border changes.
- `--ease-out-quint: cubic-bezier(.23,1,.32,1)` for anything that fills or counts.
- Springs (Motion): `{ type: "spring", visualDuration: 0.25, bounce: 0 }`. No overshoot. Overshoot reads playful, not premium.

Motion is allowed only in these places:
1. **Ring fill**: animates from 0 to the current value over 900 ms `ease-out-quint` when the window opens or the shown Day changes (including the 4am rollover), never on a tab switch. Live updates while tracking animate 400 ms with the same curve. No pulse when the goal is met.
2. **Count-up**: the hero number and History stat numbers count over the same duration and curve as the ring, so they finish together. They count only on mount or date change. Live ticks just replace the value, which tabular figures keep steady.
3. **Segmented toggle and rail thumbs**: slide with the spring above. The rail's thumb slides between all four items, Settings included, through Motion's `layoutId`; it is never moved by a constant step. The chart underneath a toggle crossfades with opacity only, 150 ms. No slide or scale.
4. **Category control** (the tile popover in Apps & sites): a 3-segment `Off · Productive · Entertainment` control in the same style as the segmented toggle; the thumb slides with the same spring, and the selected segment takes its category color (none for Off).
5. **Data changes within a chart** (e.g. previous/next week): bars change height and donut arcs re-sweep over 250 ms with `cubic-bezier(0.32,0.72,0,1)`. Heatmap cells never animate.
6. **Popovers** (tooltips and the tile category popover): fade plus 4 px translate, 150 ms in and 100 ms out (T3 mobile PR rows use 140 ms in and 100 ms out).
7. **View switch**: an instant swap (issue 16). The 150 ms fade it replaced started the new view from transparent over an empty area, so every switch showed one or two blank frames, then a half-faded view whose title had already changed in the titlebar. Each view's data is loaded before it is opened, so its first frame is finished. The rail thumb's slide carries the motion.
8. **First-run step change**: fade plus a 6 px rise, 220 ms `ease-out-quint`. The one place a spatial cue is honest, because the steps are a sequence.
9. **Tile check and bucket icon**: the check badge scales 0.5 → 1 over 150 ms, and a picked icon drops into its first-run bucket at 0.5 → 1 over 220 ms, with no overshoot. Both mark data the user just created; icons already in a bucket don't replay.

Never: page or route slides, staggered list entrances, shimmer or skeleton pulses (local data is instant), hover lifts, glows on anything other than data fills, or confetti. Under `prefers-reduced-motion: reduce`, every duration is 0 and numbers render at their final value. Like T3's `.no-transitions`, disable transitions while switching themes.

## 6. What NOT to borrow

- **T3 Code:** the chat timeline, composer and its seam gradients, thread sidebar tree, terminal/diff/code tokens, the theme editor and VS Code theme import, and dense 10–11 px type (`text-2xs`/`3xs`), which suits a coding tool but would look cramped and cheap in a dashboard.
- **Grok:** the centered hero prompt bar, pill-shaped everything, decorative keyframes (`border-beam`, `shimmer-text-sweep`, `orb-boot-in`, `gradientText`), the orange viewport glow (`--solar: #ff640a`), incognito purple surfaces, and the proprietary Universal Sans.
- **Both:** multiple accent colors for status (info/warning/update). We have one status-like signal, the Daily Goal, and the ring alone carries it.

## 7. Sources

T3 Code (github.com/pingdotgg/t3code, `main` @ `680152e`):
- `apps/web/src/index.css`: `@theme` fonts, text-2xs..5xs, `--ease-drawer`, `--shadow-composer-dark`, radius scale, `:root` + `@variant dark` color tokens, `.no-transitions`, tabular-nums, reduced-motion rules
- `apps/web/src/components/Sidebar.motion.ts`: `motionTiming = { duration: 150, easing: "ease-out" }`
- `apps/web/src/components/ui/switch.tsx`, `toggle-group.tsx`, `button.tsx`: thumb timings, segmented variant, `scale-[0.97]` press
- `apps/web/src/panelAnimations.ts`, `packages/contracts/src/settings.ts`: panel animation default 0 ms, max 400 ms, reduced-motion gate
- `apps/mobile/src/features/layout/workspace-pane-animation.ts` (260 ms, cubic in-out), `apps/mobile/src/features/pull-requests/pullRequestAnimations.ts` (180/140/100 ms)
- `apps/marketing/src/styles/fonts.css`: marketing site uses DM Sans + JetBrains Mono (not the app)

Grok (closed source; values read from production CSS served to grok.com):
- `https://cdn.grok.com/_next/static/chunks/02v_wylfh_x96.css` (main bundle) and the 5 smaller chunks linked from `https://grok.com/` on 2026-10-21. Hashed filenames will rotate. Values used: the `.dark` surface/fg/border/button tokens, the `--neutral-*` HSL scale, `--font-sans`/`--font-universal-sans`/`--font-geist-mono`, `--font-size-ui/meta/chrome/title`, `--radius*`, `--ease-*`, `--default-transition-*`, `--tracking-*`, and the keyframe names.
- Third-party write-ups (lower confidence, used only for cross-checking): styles.refero.design (Grok style reference), kombai.com/gallery/design-systems/grok-kombai, assistant-ui.com/examples/grok.
