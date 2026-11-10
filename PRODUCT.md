# Product

<!-- impeccable:product-schema 1 -->

## Platform

adaptive

## Stack

Flutter for all clients (Android, iOS, desktop, web) with a Rust core and Axum server; see
`docs/ARCHITECTURE.md`. Design artifacts and screen mockups are built as local static HTML/CSS
in `design/`, then implemented in Flutter. No hosted artifacts — everything stays in the repo.

## Users

Primary: people in Ghana managing their own money across cash, mobile money and a bank
account, who currently have no reliable picture of where it goes. They receive MoMo and bank
alerts by SMS constantly and either ignore them or mentally discard them.

Secondary, later: the same person's household, through shared visibility. Not scoped yet.

## Product Purpose

Track income and spending accurately enough to answer four questions: where the money went,
whether a budget has been breached, what the coming weeks look like, and how much is held in
each account and currency.

Success is the user still logging transactions in month three. Most trackers fail on
discipline, not features, which is why automatic capture is the centre of the product rather
than a convenience on the side.

## Positioning

Automatic capture from MoMo and bank SMS, parsed entirely on the device, with every parsed
transaction reviewed before it enters the ledger. Competitors either require manual entry,
or require linking a bank account through an aggregator that does not meaningfully cover
Ghanaian mobile money.

The mechanism a neighbouring product cannot copy honestly: the SMS alerts a Ghanaian user
already receives become the ledger, without the raw messages ever leaving the phone.

## Operating Context

Four distinct usage scenes, all confirmed and all load-bearing:

- **One-handed, on the move.** Logging a fare or a purchase while standing or walking,
  often holding something else. Primary actions must be thumb-reachable.
- **Outdoors in bright sunlight.** The screen fights daylight. Rules out low-contrast greys,
  thin type at small sizes, and colour as the sole carrier of meaning.
- **Low-end Android, small screens.** Budget devices, ~5-inch displays, modest GPUs. Layouts
  must survive narrow widths; motion must stay cheap.
- **Sit-down review at home.** Unhurried, in the evening. Where the breakdown, the budget
  state and the outlook are actually read.

Transactions arrive from four sources: manual entry, parsed SMS, CSV/OFX statement import,
and receipt capture. Everything parsed lands in a review inbox before it reaches the ledger.

## Capabilities and Constraints

- Accounts of kind cash, MoMo, bank or card; each has one currency. GHS is the default.
- Money is integer minor units with a currency attached. No floating point anywhere.
- A transaction's status is pending or confirmed; future-dated pending rows are the forecast,
  so history and outlook are one continuous list, not two screens.
- Transfers between the user's own accounts are paired and netted — neither income nor expense.
- Categorisation ladder: explicit source category, then stored pattern rules, then the user's
  own most recent transaction with the same label or counterparty, then Uncategorised.
- SMS capture is Android only. iOS gives apps no SMS access and never will, so the iOS build
  must be fully usable on manual entry, import and receipts alone.
- SMS parsing runs on-device. Raw message text never leaves the phone, is not retained after
  parsing, and unmatched messages are counted rather than stored. This is a Google Play policy
  requirement for the "SMS-based money management" permission exception, not a preference.
- Budgets are per category and per currency; a cedi cap and a dollar cap are separate budgets.
- Bank and wallet SMS formats ship as versioned rule packs fetched at runtime, so supporting a
  new bank is a server release rather than a store review.
- Undecided: whether the web client runs the core as WASM or defers to the server; whether a
  self-hosted option ships alongside the published product.

## Brand Commitments

`moneytrack` is a working name, not a confirmed one. No logo, wordmark, palette or typeface
has been chosen. Nothing visual is binding yet.

## Evidence on Hand

None. There are no users, no testimonials, no screenshots, no usage data, no press and no
launch date. Future work must not fabricate any of these, and must not imply the product is
in use.

Written context in the repo: `docs/PLAN.md`, `docs/ARCHITECTURE.md`, `README.md`.

## Product Principles

1. **Automatic capture is the product.** Everything else is table stakes; this is why someone
   installs it and why they are still using it in month three.
2. **Nothing parsed enters the ledger unreviewed.** SMS formats change without warning, and a
   silent mis-parse corrupts the numbers quietly, which is worse than missing one.
3. **Privacy is structural, not promised.** Parsing happens on-device because the data should
   never be somewhere it could leak, not because a policy requires a claim.
4. **Entry must be faster than not bothering.** Manual logging under five seconds, or the
   discipline fails and the ledger becomes fiction.
5. **The outlook changes behaviour; the breakdown only confirms it.** Forward-looking balance
   earns the prominence, not the category pie chart.

## Accessibility & Inclusion

Driven by the operating context rather than a chosen standard: high contrast as a baseline
because of sunlight use, generous thumb-reachable targets for one-handed entry, meaning never
carried by colour alone (income versus expense must be legible in monochrome), and layouts
that hold at small widths and large system text sizes. Target WCAG 2.2 AA contrast as the
floor, not the goal.
