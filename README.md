# moneytrack (working name)

Personal income and spending tracker. Android, iOS, desktop and web, with automatic
capture from MoMo and bank SMS on Android. Intended for publication, not just personal use.

Nothing is built yet. The thinking so far:

- [docs/PLAN.md](docs/PLAN.md) — what it does, in what order, and what will bite.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — how it is put together and why.

Decided: a Rust core and server, a Flutter UI, integer minor units for money, on-device
SMS parsing, and pending transactions as the forecast.
