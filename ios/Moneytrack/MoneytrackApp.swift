import SwiftUI

/// The whole app, for now.
///
/// Phase 1a's finish line is one call from Swift that opens a real encrypted database and
/// gets a real number back, so that is all this screen does: open the ledger, seed a wallet
/// the first time, and show what it holds. The screens proper — and the HIG build of the
/// design in `design/` — are phase 1b.
@main
struct MoneytrackApp: App {
    var body: some Scene {
        WindowGroup {
            LedgerProof()
        }
    }
}
