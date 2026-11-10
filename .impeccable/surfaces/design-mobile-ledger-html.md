---
version: 1
slug: "design-mobile-ledger-html"
primary_target: "design/mobile/ledger.html"
related_targets: ["design/mobile","design/tokens"]
---

Scope: the moneytrack phone app — ledger, review inbox, quick add, budgets, outlook. Visitor mode: Operate.

Audience: people in Ghana tracking cash, MoMo and a bank account. Job: log or confirm a transaction in seconds, then see whether the month holds. Constraints: one-handed on the move, direct sunlight, low-end Android on small screens, and an unhurried evening review. Platform is adaptive — Material 3 on Android, HIG on iOS — so navigation, sheets, controls and back behaviour stay native on each OS. The world is not allowed to supply them.

## Direction contract

THESIS: A ledger where the act of checking is visible. This surface refuses the money-app default — a gradient hero balance over a carousel of account cards and a donut of categories — because that arrangement shows a total and hides the one thing that matters here: which rows have been verified by a human and which are still a machine's guess. The screen's organising idea is a single continuous ledger through today, not a dashboard of tiles.

OWN-WORLD: Ghana's cocoa grading and bagging line. Oat ground (#EFE7D7) with near-black stencil ink (#17150F); grade marks in oxblood (#8C3A1E), leaf green (#2F5B42) for income, and a tally amber (#C2762B) that only ever means "awaiting your check". Archivo throughout — small uppercase labels tracked to +0.07em for the stencilled register, and amounts set as monumental tabular figures. Stardos Stencil appears nowhere except inside a stamp. Rules are hairlines on a 4dp cell grid; corners are 2–3px, never pill-rounded. Recognisable with every word removed by the oat ground, the tracked micro-labels against huge figures, and the stamp.

STORY: The visitor opens the app, sees what they actually have, and immediately sees that two alerts the phone parsed are still unstamped. They tap one, read it, correct the category if it's wrong, and stamp it. The ledger below runs on past today into rent and salary, so they understand their position without opening anything else.

FIRST VIEWPORT: Native status bar, then a compact bar with the screen name and an account filter. The balance occupies the top sixth at 54px tabular, GHS set small beside it, with one tracked line underneath naming the accounts and the date. Directly below, a dashed amber review strip carrying an empty stamp field and the count of parsed alerts — the first coloured thing on screen. The rest is the ledger: date gutter, label, counterparty, amount right-aligned, and a hairline magnitude bar under each row so the list is its own chart. A ruled oxblood TODAY line sits mid-screen with future pending rows continuing below it at reduced ink. Native bottom navigation, with the add action as the platform's primary action control.

FORM: Cocoa Grade Stencil, candidate 4 of seven grounded directions (ordered: lorry-park signwriting, cedi banknote, MoMo kiosk tariff board, cocoa grading stencil, adinkra stamping, civic timetable print, wax print). Chosen by the user over Lorry Park Signwriter, Gate Board and Warm Consumer Surface. seed:bcf3a0ae7fea624c

SIGNATURE INTERACTION: The stamp. A parsed row carries a dashed empty stamp field; confirming it presses a dated grade mark into the row, rotated a few degrees off square, with a short ink-settle. Budget state reuses the same grammar as graded tiers rather than a progress bar. Raises carried in from declined challengers: scale contrast alone for hierarchy, magnitude drawn into each row, one fixed scale across months so they compare honestly, and a 4dp cell grid nothing sits off.

Unresolved: whether the stamp also appears on manually entered rows (currently no — manual entry is already a human act), and the iOS stamp's motion under Reduce Motion.
