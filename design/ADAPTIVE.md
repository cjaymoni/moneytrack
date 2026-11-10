# Adaptive mapping

Platform is `adaptive`: Material 3 on Android, HIG on iOS. The Cocoa Grade Stencil
world lends **type, palette, density and one signature move** — the stamp. It does not
lend navigation, control shape, sheet behaviour, gestures or system motion.

The `.mt-*` classes in `tokens/app.css` are painted stand-ins so the HTML references
read as real screens. They are **not** a component library to port to Flutter. Every
row below says what the Flutter build uses instead.

| Job | Android (Material 3) | iOS (HIG) | What the world contributes |
|---|---|---|---|
| Screen context | Top app bar, small | Large title collapsing to inline on scroll | Title in Archivo 19px/600; account filter as a trailing control |
| Sections | Navigation bar, 3–5 destinations | Tab bar | Label type and the active-state ink. Icons are Material Symbols on Android, SF Symbols on iOS — never one set on both |
| Primary action | FAB, one per screen | Toolbar or navigation-bar primary action, or a prominent button in the sheet | Oxblood `--mt-grade` fill; platform decides shape and placement |
| Adding an entry | Full-screen destination | Detented sheet with Cancel / Save in the navigation bar | Entry figure type, category chips' palette |
| Stamping a row | Swipe action on the row, plus the button in Review | Swipe action on the row, plus the button in Review | The stamp mark and its press motion |
| Confirming a stamp | Snackbar with Undo | Toast-free: the row updates in place; undo lives in the row's swipe | Stamp ink |
| Choosing a category | Material chips (filter) | Segmented control or a picker, per the field | Chip palette and label tracking only |
| Direction (out/in/transfer) | Segmented button | Segmented control | Tracked uppercase labels |
| Back | System Back + predictive back gesture; never trapped | Left-edge swipe back; never disabled or overlaid | Nothing |
| Shape | Material 3 corner tokens | HIG continuous corners | Nothing. The 2–3px radii in `tokens.css` are for painted surfaces — the review strip, the stamp, sheets — not controls |
| Type scale | Display / Headline / Title / Body / Label roles, in `sp`, following the system font scale | Text styles Large Title → Caption, following Dynamic Type | The face (Archivo), the tracking, and the scale-contrast register |
| Elevation | Tonal surface levels | Materials and blur behind bars | Nothing hand-rolled |
| Motion | Material container transform, shared-axis, fade-through | System push, sheet rise, reverse on dismiss | The stamp press only, gated behind the system reduce-motion setting |

## Fixed obligations

- Touch targets: 48×48 dp on Android, 44×44 pt on iOS, with at least 8 dp between
  adjacent targets. Build to 48 and both are satisfied; `--mt-target` is that value.
- Type follows the system size setting on both platforms. Nothing that carries state
  may fall below 11 pt / 11 sp — including the stamp, which is why `--mt-stamp-face`
  is 11px and not the 8px it started at.
- Fonts ship as app assets. Archivo and Stardos Stencil are bundled, not fetched; the
  CDN links in the HTML references exist only because they are HTML references.
- Dark is a first-class scheme on both platforms and is tested, not inverted.
- SMS capture and anything it implies — the Review tab's badge, the stamp prompt —
  exists on Android only. The iOS build must be complete without it.

## Open

- Haptics on the stamp: the signature move on a phone, and nothing specifies the
  feedback yet. Likely a single medium impact on press.
- The iOS stamp under Reduce Motion: a crossfade to the settled mark, rather than the
  press animation. Not yet specified.
