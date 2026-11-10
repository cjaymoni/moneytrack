---
name: moneytrack
description: A ledger where the act of checking is visible — Ghana's cocoa grading line, rendered as a money app.
colors:
  ground: "#EFE7D7"
  surface: "#F7F2E7"
  sunk: "#E5DAC3"
  ink: "#17150F"
  ink-secondary: "#4A4438"
  ink-muted: "#6B6454"
  ink-future: "#655E4E"
  ink-hover: "#23201A"
  rule: "#D1C5AD"
  rule-strong: "#B6A789"
  grade: "#8C3A1E"
  grade-hover: "#79311A"
  income: "#2F5B42"
  await: "#854B0E"
  await-wash: "#EADBC2"
  await-edge: "#C2762B"
  await-hover: "#E3D2B5"
  backdrop: "#2A251D"
  ground-dark: "#141210"
  surface-dark: "#1D1A16"
  sunk-dark: "#0E0C0A"
  ink-dark: "#F2EADA"
  ink-secondary-dark: "#BAB1A0"
  ink-muted-dark: "#948B7A"
  ink-future-dark: "#948B7A"
  ink-hover-dark: "#E4DACA"
  rule-dark: "#332E26"
  rule-strong-dark: "#4A4337"
  grade-dark: "#D9704A"
  grade-hover-dark: "#E98764"
  income-dark: "#6FB98E"
  await-dark: "#E0A253"
  await-wash-dark: "#2A2219"
  await-edge-dark: "#B4791F"
  await-hover-dark: "#382D20"
  backdrop-dark: "#0A0908"
typography:
  amount-hero:
    fontFamily: "Archivo, Helvetica Neue, system-ui, sans-serif"
    fontSize: "54px"
    fontWeight: 600
    lineHeight: 0.96
    letterSpacing: "-0.035em"
    fontFeature: "tabular-nums"
  amount-lg:
    fontFamily: "Archivo, Helvetica Neue, system-ui, sans-serif"
    fontSize: "34px"
    fontWeight: 600
    letterSpacing: "-0.035em"
    fontFeature: "tabular-nums"
  amount:
    fontFamily: "Archivo, Helvetica Neue, system-ui, sans-serif"
    fontSize: "16px"
    fontWeight: 600
    letterSpacing: "-0.02em"
    fontFeature: "tabular-nums"
  title:
    fontFamily: "Archivo, Helvetica Neue, system-ui, sans-serif"
    fontSize: "19px"
    fontWeight: 600
    letterSpacing: "-0.012em"
  body:
    fontFamily: "Archivo, Helvetica Neue, system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 400
    lineHeight: 1.42
  body-sm:
    fontFamily: "Archivo, Helvetica Neue, system-ui, sans-serif"
    fontSize: "13.5px"
    fontWeight: 400
    lineHeight: 1.35
  label:
    fontFamily: "Archivo, Helvetica Neue, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 600
    letterSpacing: "0.07em"
  micro:
    fontFamily: "Archivo, Helvetica Neue, system-ui, sans-serif"
    fontSize: "11px"
    fontWeight: 600
    letterSpacing: "0.14em"
  stamp:
    fontFamily: "Stardos Stencil, Archivo, sans-serif"
    fontSize: "11px"
    fontWeight: 400
    lineHeight: 1.2
    letterSpacing: "0.08em"
  stamp-lg:
    fontFamily: "Stardos Stencil, Archivo, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.2
    letterSpacing: "0.08em"
rounded:
  sm: "2px"
  md: "3px"
  lg: "4px"
spacing:
  c1: "4px"
  c2: "8px"
  c3: "12px"
  c4: "16px"
  c5: "20px"
  c6: "24px"
  c8: "32px"
  c10: "40px"
  gutter: "20px"
  target-gap: "8px"
  target-ios: "44px"
  target-android: "48px"
  target: "48px"
components:
  button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.ground}"
    typography: "{typography.label}"
    rounded: "{rounded.md}"
    padding: "15px 0"
    width: "100%"
  button-primary-hover:
    backgroundColor: "{colors.ink-hover}"
    textColor: "{colors.ground}"
  button-grade:
    backgroundColor: "{colors.grade}"
    textColor: "{colors.ground}"
    typography: "{typography.label}"
    rounded: "{rounded.md}"
    padding: "15px 0"
  button-grade-hover:
    backgroundColor: "{colors.grade-hover}"
    textColor: "{colors.ground}"
  button-disabled:
    backgroundColor: "{colors.rule}"
    textColor: "{colors.ink-muted}"
    rounded: "{rounded.md}"
    padding: "15px 0"
  chip:
    backgroundColor: "transparent"
    textColor: "{colors.ink-secondary}"
    typography: "{typography.label}"
    rounded: "{rounded.sm}"
    padding: "6px 12px"
    height: "{spacing.target-ios}"
  category-chip:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    typography: "{typography.body-sm}"
    rounded: "{rounded.sm}"
    padding: "8px 14px"
    height: "{spacing.target}"
  category-chip-selected:
    backgroundColor: "{colors.grade}"
    textColor: "{colors.ground}"
    rounded: "{rounded.sm}"
    padding: "8px 14px"
    height: "{spacing.target}"
  segment:
    backgroundColor: "transparent"
    textColor: "{colors.ink-secondary}"
    typography: "{typography.label}"
    rounded: "{rounded.sm}"
    padding: "9px 0"
    height: "{spacing.target}"
  segment-selected:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.ground}"
    typography: "{typography.label}"
    rounded: "{rounded.sm}"
    height: "{spacing.target}"
  review-strip:
    backgroundColor: "{colors.await-wash}"
    textColor: "{colors.ink}"
    typography: "{typography.body-sm}"
    rounded: "{rounded.md}"
    padding: "{spacing.c3}"
  review-strip-hover:
    backgroundColor: "{colors.await-hover}"
  stamp:
    backgroundColor: "transparent"
    textColor: "{colors.grade}"
    typography: "{typography.stamp}"
    rounded: "{rounded.sm}"
    padding: "2px 6px 3px"
  stamp-lg:
    backgroundColor: "transparent"
    textColor: "{colors.grade}"
    typography: "{typography.stamp-lg}"
    rounded: "{rounded.sm}"
    padding: "6px 11px 7px"
  stamp-field:
    backgroundColor: "transparent"
    textColor: "{colors.await}"
    typography: "{typography.stamp}"
    rounded: "{rounded.sm}"
    width: "66px"
    height: "46px"
  by-hand:
    backgroundColor: "transparent"
    textColor: "{colors.ink-muted}"
    typography: "{typography.micro}"
  fab:
    backgroundColor: "{colors.grade}"
    textColor: "{colors.ground}"
    rounded: "{rounded.md}"
    size: "56px"
  nav-item:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink-muted}"
    typography: "{typography.micro}"
    padding: "9px 8px 24px"
  nav-item-active:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    typography: "{typography.micro}"
---

# Design System: moneytrack

## Overview

**Creative North Star: "Cocoa Grade Stencil"**

The world is taken from Ghana's cocoa grading and bagging line: oat jute under near-black stencil ink, grade marks pressed in oxblood, everything ruled on a hard cell. It lends this app four things and nothing else — the type register, the palette, the density, and one signature move. It does not lend navigation, control shape, sheet behaviour, gestures or system motion. Everything a cocoa shed does not need, this app does not have.

The thesis the build defends is that the act of checking should be visible. A money app's default arrangement — gradient hero balance, carousel of account cards, donut of categories — shows a total and hides which rows a human has actually verified. Here the screen is one continuous ledger running through today into future pending rows; the only coloured object above it is the strip saying machine guesses are still waiting for you; and every settled row declares how it got there, either with a pressed stamp or with a by-hand mark. Hierarchy comes from scale contrast alone: monumental tabular figures set against small uppercase labels tracked open, with no weight tricks, no boxes, no cards stacked for emphasis.

The light scheme is the default because the confirmed usage scene is direct Ghanaian sunlight. That one fact drove the whole palette: oat ground rather than white, near-black ink rather than grey, and no state carried by a transparency. Every text and mark pair is measured against its own background and held above WCAG AA 4.5:1 in both schemes — the quietest pair in the system, the date gutter on oat, sits at 4.78:1, and future rows get their own ink colour (5.23:1) rather than a faded one. Dark is the evening sit-down review: roasted, warm-black, not an inversion.

**Key Characteristics:**
- Oat ground (#EFE7D7), never white; near-black stencil ink, never grey body text.
- Monumental tabular figures against small uppercase labels tracked +0.07em — scale contrast is the only hierarchy device.
- One continuous ledger through a ruled oxblood TODAY line, not a dashboard of tiles.
- Every past row declares its verification state: a pressed stamp or a by-hand mark.
- A 4dp cell grid nothing sits off, and platform touch-target floors nothing falls under.
- One colour, one job: oxblood, leaf green, tally amber each mean exactly one thing.
- One authored motion moment in the whole app: the stamp pressing.

## Colors

A grading-shed palette: a warm oat ground carrying near-black ink, with three marks that each carry one meaning and are never borrowed for anything else.

### Primary
- **Grade Oxblood** (`{colors.grade}`): the stamp border and stamp text, the ruled TODAY line, the drawn budget cap boundary and the overspend cells past it, the primary-action control, the caret in the amount field, the outlook's low-point marker, the error rule, and every focus ring. It is the colour of a mark being made or a limit being crossed. Hover deepens to `{colors.grade-hover}`.

### Secondary
- **Leaf Green** (`{colors.income}`): money in, and budget still within its cap. Income amounts, the magnitude bar under an income row, the account-filter status dot. Nothing else is green.

### Tertiary
- **Tally Amber** (`{colors.await}` for text, `{colors.await-edge}` for the dashed edge, `{colors.await-wash}` for the ground, `{colors.await-hover}` for the pressed strip): "awaiting your check", and nothing else. The review strip, the empty stamp field, and the unstamped cards in the review inbox. When nothing is waiting, there is no amber on the screen.

### Neutral
- **Oat Ground** (`{colors.ground}`): the page, and the ring that separates the floating action control from whatever sits under it.
- **Raised Oat** (`{colors.surface}`): the one step up — the bottom navigation bar and sheet surfaces.
- **Sunk Oat** (`{colors.sunk}`): the one step down — recessed fields and pressed states.
- **Stencil Ink** (`{colors.ink}`): all primary text, all amounts, the selected segment fill, the budget tiers within cap, the outlook's balance line. Hover deepens to `{colors.ink-hover}`.
- **Counterparty Ink** (`{colors.ink-secondary}`): counterparty, meta lines, tracked labels, the small GHS beside the balance.
- **Gutter Ink** (`{colors.ink-muted}`): the date gutter, inactive navigation labels, the by-hand mark.
- **Future Ink** (`{colors.ink-future}`): pending and future-dated rows. Quieter than the present without being faint — a colour, never an opacity.
- **Hairline** (`{colors.rule}`) and **Strong Rule** (`{colors.rule-strong}`): row dividers and empty budget tiers, and section rules, field borders and chip strokes respectively.
- **Backdrop** (`{colors.backdrop}`): the space outside the device frame in the HTML references only; it has no role in the app.

The dark scheme re-grades every one of these roles (`{colors.ground-dark}` through `{colors.backdrop-dark}`); it is not an algorithmic inversion, and the marks are lifted rather than merely lightened.

### Named Rules
**The One Job Rule.** Oxblood is the stamp, today's rule and the cap boundary with everything past it. Green is money in and within-budget. Amber means awaiting your check and nothing else. A colour never does a second job, and no new accent is added to cover a new meaning — find the meaning a shape can carry instead.

**The Sunlight Floor Rule.** Light is the default scheme because the screen fights daylight. Every text or mark against its own background is measured and held above 4.5:1 before it ships. A value is never lowered to make a layout work.

**The Colour-Not-Opacity Rule.** A quieter state gets its own token, not a transparency on a louder one. Future rows use Future Ink because the faded version of ink computed to about 3.7:1 on oat and failed. Opacity is for decoration — the magnitude bar, the TODAY rule's hairline — never for text.

**The Monochrome Rule.** Meaning is never carried by colour alone. Income is green *and* signed; expense is ink *and* carries a minus. Over-budget is oxblood *and* sits past a drawn boundary. Checked is a stamp; hand-entered is a word. Read any screen in greyscale and nothing is lost.

## Typography

**Display / Body / Label Font:** Archivo (with Helvetica Neue, system-ui, sans-serif)
**Stamp Font:** Stardos Stencil (with Archivo fallback)

**Character:** One grotesque does everything — figures, labels, body — so the register comes from scale and tracking rather than from a second voice. Archivo's tabular figures are on globally (`font-variant-numeric: tabular-nums`), so columns of money align down the page and a changing balance does not shimmer. Stardos Stencil is the stencil voice of the grading shed and is confined to one object. Both faces ship as bundled app assets in the Flutter build; the CDN links in the HTML files exist only because those are HTML references.

### Hierarchy
- **Amount Hero** (600, 54px, 0.96, −0.035em): the balance on Ledger, the month's spend on Budgets, the lowest projected balance on Outlook, and the running figure in the entry sheet. One per screen.
- **Amount Large** (600, 34px, −0.035em): sheet totals.
- **Amount** (600, 16px, −0.02em, right-aligned): ledger row amounts.
- **Title** (600, 19px, −0.012em): the screen name in the top bar.
- **Body** (400, 15px, 1.42): explanatory prose; held to roughly 30–34ch at phone width.
- **Body Small** (400, 13.5px, 1.35): review-strip copy, helper text under controls, category chips.
- **Label** (600, 12px, +0.07em, uppercase): the tracked stencil register — the line under the balance, section labels, chips, segments, buttons.
- **Micro** (600, 11px, +0.14em, uppercase): the hardest-tracked register — the TODAY rule, budget foot lines, navigation labels, and the by-hand mark (at 500 with label tracking). 11px is the floor, not a style choice.
- **Stamp** (11px, +0.08em, Stardos Stencil) and **Stamp Large** (14px): only inside a stamp or an empty stamp field.

Row-level text sits slightly off the named ramp by design: the row label is 14.5px/500, the counterparty sub-line 11.5px and the date gutter 11px, tuned so three pieces of information fit a one-handed row on a 5-inch screen.

### Named Rules
**The Stencil-In-The-Stamp Rule.** Stardos Stencil appears nowhere except inside a stamp or a stamp field. Not in headings, not in the balance, not in the logo. Its rarity is the whole reason the stamp reads as a mark rather than as styling.

**The 11px Floor Rule.** Nothing that carries state is set below 11px — including the stamp, which is why the stamp face is its own token rather than whatever size happens to fit the row. Type follows the system size setting on both platforms; a mark that shrinks out of legibility is a mark that stopped working.

**The Scale-Contrast Rule.** Hierarchy comes from the gap between a monumental figure and a small tracked label. No decorative weight jumps, no coloured headings, no boxes drawn around text to promote it.

**The Tabular Rule.** Every figure, everywhere, is tabular. Amounts are right-aligned so decimal points stack.

## Layout

Phone-first at 390×844, built as a fixed column: a status-bar strip, a compact top bar, the screen's fixed head (balance, review strip), one scrolling region, and the bottom navigation. Only the middle region scrolls, and it is the only region that can be long.

Spacing runs on a 4dp cell — `{spacing.c1}` through `{spacing.c10}` — and nothing sits off it. The horizontal gutter is a constant 20px (`{spacing.gutter}`) on every screen; the scrolling ledger carries 96px of bottom padding so the last row clears both the navigation and the floating action control.

Touch is a second grid, and it is a floor rather than a rhythm: 44px on iOS (`{spacing.target-ios}`), 48px on Android (`{spacing.target-android}`), built to 48px (`{spacing.target}`) so both are satisfied, with at least 8px (`{spacing.target-gap}`) between adjacent targets. Segmented buttons, category chips and the back control are sized to the target and padded inward; the filter chip meets the iOS floor with its own padding. Adjacent-control gaps use the target-gap token, not an arbitrary cell step.

The ledger row is a three-column grid: a 46px date gutter, a flexible label/counterparty stack, and a right-aligned amount, aligned on the baseline with a 12px gap. A 2px magnitude bar spans the label and amount columns underneath, so the list is its own chart and no separate category graph is needed. Future rows carry the same geometry in Future Ink. A ruled oxblood TODAY line separates past from forecast; history and outlook are one list.

Budgets use the same grid logic: a name/value head, a row of equal tier cells, a drawn cap boundary, fixed-width overspend cells past it, and a micro foot line that adds information rather than restating the numbers above it.

Primary actions sit low and right — the add control floats 62px above the navigation at the gutter — because the confirmed scene is one-handed and thumb-reachable.

### Named Rules
**The 4dp Cell Rule.** Every margin, padding and gap resolves to a step on the cell scale. If a value needs to be 13px, the layout is wrong, not the scale.

**The Target Floor Rule.** No interactive control is smaller than 48×48, and no two adjacent ones sit closer than 8px. Visual compactness never buys itself out of a hit area; shrink the padding's appearance, not the target.

**The One Scroll Rule.** A screen has exactly one scrolling region. Chrome, balance and the review strip are fixed; nested scrollers and horizontal carousels do not exist here.

## Elevation & Depth

Depth is almost entirely tonal: three oat steps (sunk, ground, raised surface) plus hairlines do the work, and surfaces are flat at rest. Shadow is reserved for things that genuinely float above the page, and when it appears it is offset plus blur — an object casting onto jute — never a coloured halo or glow.

### Shadow Vocabulary
- **Lift 1** (`box-shadow: 0 1px 2px rgba(23,21,15,.07), 0 2px 8px rgba(23,21,15,.05)`): the gentlest separation, used for framed previews. Rarely needed in-app.
- **Lift 2** (`box-shadow: 0 2px 4px rgba(23,21,15,.10), 0 6px 18px rgba(23,21,15,.10)`): the floating primary action control, the only element in the app that is truly off the page.
- **Ground ring** (`box-shadow: 0 0 0 6px var(--mt-ground), var(--mt-lift-2)`): the floating control's full treatment. The 6px ring in the page's own colour cuts the control free of whatever row sits under it, so it reads as floating rather than as a hole punched through an amount.

Both lifts re-grade in dark to pure-black alphas at roughly four times the opacity, because the roasted ground cannot carry an ink-tinted shadow.

### Named Rules
**The No-Halo Rule.** A shadow has an offset. A symmetrical blur with no offset, a coloured glow, or a spread-only ring is not depth in this world. The one spread in the system is the ground ring, and it is a cut-out, not a glow: it is the page colour, never a tint.

**The Flat-At-Rest Rule.** Rows, cards, sheets and the navigation bar carry no shadow. Separation is a hairline or a tonal step. Only the floating action control lifts.

## Shapes

Stencilled, never pill. The radius scale stops at 4px: 2px (`{rounded.sm}`) on stamps, stamp fields and painted chips; 3px (`{rounded.md}`) on the review strip, buttons and framed blocks; 4px (`{rounded.lg}`) only on the largest frames.

An important limit the token file states itself: **these radii are for surfaces the world paints** — the review strip, the stamp, sheets. Native *controls* take the platform's own shape, Material 3 corner tokens on Android and HIG continuous corners on iOS. The world lends type, palette, density and the stamp. It does not lend control shape.

Borders carry most of the structure: a 1px hairline for dividers and controls, a 1.5px dashed edge for anything awaiting a check, and a 1px solid oxblood edge once it has been checked. Dashed means unresolved; solid means settled. That contrast is the only border vocabulary the system has, and it maps directly onto the product's core state.

Nothing is a capsule, and nothing is a circle except the 7px status dot in the account chip and the single marker point on the outlook line — small indicator geometry, not a form language. Icons are stroked line SVGs on a 24px grid at 1.7–2.1px, round caps and joins, drawn to match the hairline rules. In the Flutter build they are Material Symbols on Android and SF Symbols on iOS, never one set on both.

### Named Rules
**The Dashed-Means-Unverified Rule.** A dashed border is reserved for a thing the user has not yet checked. Never use dashes as decoration, as an empty-state flourish, or as a dividing line.

**The Painted-Surfaces-Only Rule.** The 2–4px radii apply to what the world paints. A control's corner belongs to the platform.

## Components

Read `design/ADAPTIVE.md` first. It is authoritative for the per-OS mapping — what each job becomes under Material 3 and under HIG, and exactly what the world contributes to each. This section describes the painted appearance; ADAPTIVE.md decides the container, and it is not duplicated here.

The classes below are **painted stand-ins**, not a component library. `.mt-nav`, `.mt-fab`, `.mt-chip`, `.mt-seg` and `.mt-btn` exist so the HTML references read as real screens. Do not build a custom bottom navigation, a custom sheet, a custom back button, a custom segmented control or a custom FAB in Flutter. Use the platform's and paint it with these tokens.

### Buttons
- **Shape (painted):** barely-softened rectangle (3px). On device, the platform's own corner.
- **Primary:** stencil ink fill, oat text, full width, 15px vertical padding, 12px uppercase at 700 tracked +0.07em. Hover deepens to Ink Hover.
- **Grade:** same geometry in oxblood, used for the act of stamping and nothing lighter. Hover deepens to Grade Hover.
- **Ghost:** transparent with a 1px strong-rule border and ink text, for the secondary half of a pair ("Not mine", "Send a redacted sample").
- **Disabled:** hairline fill with gutter ink.
- **Focus:** 2px oxblood outline, 3px offset — or ink outline where the control is already oxblood. Universal across every interactive element in the system.

### Chips
- **Filter chip:** transparent with a 1px strong-rule stroke, 2px corners, 12px uppercase tracked label in counterparty ink, 6×12px padding on a 44px minimum height. Used for the account filter and the month/currency filter. A 7px status dot in leaf green may lead.
- **Category chip:** 48px minimum height, 8×14px padding, body-small at 500 in ink; selected fills oxblood with oat text; disabled drops to 40% for recurring entries that cannot be re-categorised here. Laid out with an 8px target gap.

### Segments
- **Style:** a row of equal-width buttons at the 48px target with strong-rule strokes and uppercase label type, separated by the target gap. The pressed one inverts to full ink with oat text. Out / In / Transfer.

### Cards / Containers
There is deliberately no card. Content sits directly on the oat ground, separated by hairlines. The only enclosed blocks are the review strip and the unstamped review cards — both defined by a 1.5px dashed amber edge over the amber wash, with 3px corners and `{spacing.c3}`–`{spacing.c4}` padding — and the reference frames in the states screen, which use a solid hairline.

### Inputs / Fields
There is no text input in the shipped references. Amount entry is the figure itself: a 54px tabular number on the ground with a 3px oxblood caret blinking beside it. Nothing is boxed. Selection highlight is amber edge on oat.

### Navigation
Four destinations — Ledger, Outlook, Budgets, You — on a raised oat bar with a hairline top edge, each a stroked 22px icon over an 11px uppercase micro label tracked +0.04em. Inactive is gutter ink; current is full stencil ink. The floating add control is positioned against this bar in the references. On device this is the platform's navigation bar and the platform's primary action; what this system owns is the colour, the icon weight and the label register.

### The Verification Pair (signature)
The thesis made visible, and the reason this app is not a list of amounts. **Every settled row says how it got there.**

- **The stamp** — a parsed transaction that a person has checked. A small oxblood-bordered block of stencil type reading CHECKED, 11px, 1px border, rotated −6° off square at 80% opacity, sitting inline after the counterparty. It carries no date: the date gutter is two columns to the left and already says it.
- **The by-hand mark** — a hand-entered transaction. An 11px uppercase micro word, "By hand", in gutter ink, no border, no rotation. A hand-entered row is already a human act and needs no stamp, but it still has to declare its state or the ledger cannot answer "what has been checked?" at a glance.
- **The empty stamp field** — a 66×46px dashed amber rectangle reading AWAIT / STAMP in 11px stencil, for a parsed transaction nobody has looked at yet.

Stamp means parser-sourced and checked. By-hand means entered by a person. Dashed means nobody has looked yet. Those three states cover every row, and a row without one of them is a bug.

### The Stamp Press (signature motion)
The only authored motion in the app. At the moment of stamping — on the Review screen — the large variant presses in: 14px stencil, 2px oxblood border, rotated −7°, carrying both the verdict and the date because at that moment the date is the thing being asserted. The animation runs 320ms on `cubic-bezier(.16,1.1,.3,1)`: the mark arrives at −14° and 1.9× scale, blurred 1.5px and transparent, lands past its resting angle at 0.97×, then settles. It is wrapped in `prefers-reduced-motion: no-preference` and simply appears when motion is reduced. The press is transform, opacity and one short blur — cheap on purpose, because the confirmed device floor is a low-end Android. Once the row settles into the ledger it wears the small undated form.

### Graded Budget Tiers
Budget state reuses the grading grammar rather than inventing a second one, and the build is careful about the one thing a progress bar gets wrong. Equal 14px tier cells fill in ink up to the cap; **the cap itself is a drawn 2px oxblood boundary**, and overspend appears as fixed-width oxblood cells past that boundary (with a half-width cell for a part step). 123% of cap therefore cannot render identically to exactly 100% — the row visibly runs off the end. The foot line carries what the numbers above do not: days remaining, pace, how long it has been over. No progress pill, no ring, no donut.

### Loading and Error
Loading is 12px hairline-fill bars pulsing between 75% and 35% opacity over 1.5s — no shimmer sweep, no spinner. Errors are a block with a 1px oxblood border on the plain ground and an oxblood bolded lead line; no coloured fill, no icon.

## Do's and Don'ts

### Do:
- **Do** give every past row a verification mark: a stamp if a parser produced it and a person checked it, "By hand" if a person typed it.
- **Do** set every figure in tabular numerals and right-align amounts so decimals stack.
- **Do** build hierarchy from scale contrast — a 54px figure against a 12px tracked uppercase label — rather than from weight, colour or enclosure.
- **Do** keep each mark to its single job: oxblood for the stamp, today's rule and the cap boundary; green for money in and within-budget; amber for awaiting your check.
- **Do** give a quieter state its own colour token, the way future rows use Future Ink.
- **Do** reserve the dashed 1.5px edge for anything unverified, and switch it to a solid stamp the moment it is checked.
- **Do** resolve every spacing value to the 4dp cell scale, with a 20px gutter on every screen, and size every control to the 48px target with an 8px gap to its neighbour.
- **Do** keep anything that carries state at 11px or above, including the stamp.
- **Do** measure each new text-on-background pair and keep it above 4.5:1 in both schemes before shipping it.
- **Do** draw magnitude into the row itself (the 2px bar) rather than adding a separate chart.
- **Do** read `design/ADAPTIVE.md` before building any screen, and use the platform's own navigation, sheets, dialogs, controls, shapes, back behaviour and transition motion, painted with these tokens.

### Don't:
- **Don't** use Stardos Stencil outside a stamp or a stamp field.
- **Don't** reimplement `.mt-nav`, `.mt-fab`, `.mt-chip`, `.mt-seg` or `.mt-btn` as custom Flutter widgets — they are painted stand-ins for native controls in these HTML references.
- **Don't** apply the 2–4px radii to a native control; a control's corner belongs to the platform.
- **Don't** fade text to signal a quieter state. An opacity on type is how the old future row failed AA.
- **Don't** put a date on the small row stamp; the date gutter already carries it. The dated form is the large mark, at the moment of stamping.
- **Don't** add a progress bar, ring, donut or percentage-as-fill anywhere — budget state is graded tiers against a drawn cap, and category magnitude is the in-row bar.
- **Don't** let a budget at 123% of cap render the same as one at exactly 100%.
- **Don't** repeat in a foot line what the figures directly above it already say.
- **Don't** introduce a gradient, a hero wash, an account carousel or a tile dashboard; the ledger is one continuous list through today.
- **Don't** give a colour a second meaning or add a fourth accent. If a new state needs expressing, find a shape or a word for it.
- **Don't** use a shadow without an offset, or a coloured spread; depth is tonal steps and hairlines, and the only spread in the system is the ground-coloured ring that cuts the floating control free.
- **Don't** carry meaning in colour alone — every state must survive greyscale.
- **Don't** add a second animated moment. The stamp press is the app's only authored motion; everything else is the platform's.
- **Don't** write a hex literal into a component. Every colour in `app.css` resolves to a token, hover and backdrop values included, and new work keeps it that way.
