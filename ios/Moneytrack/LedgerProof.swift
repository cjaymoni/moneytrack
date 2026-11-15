import MoneytrackKit
import SwiftUI

/// What the screen can be showing.
private enum Showing {
    case opening
    case open(accountId: String, balance: String)
    case refused(reason: String)
}

struct LedgerProof: View {
    @State private var showing: Showing = .opening
    @State private var ledger: Ledger?

    var body: some View {
        VStack(spacing: 16) {
            switch showing {
            case .opening:
                Text("Opening the ledger…")

            case .refused(let reason):
                Text(reason)
                    .font(.footnote)
                    .multilineTextAlignment(.center)

            case .open(let accountId, let balance):
                Text("GHS \(balance)")
                    .font(.system(.largeTitle, design: .default))
                    .monospacedDigit()
                Text("Walked in Rust, from an encrypted database on this device.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                Button("Spend GHS 4.50") {
                    Task { await spend(from: accountId) }
                }
                .buttonStyle(.borderedProminent)
            }
        }
        .padding(24)
        .task { await start() }
    }

    private func start() async {
        do {
            let opened = try await Ledger.openDefault()
            ledger = opened
            showing = try await read(from: opened)
        } catch {
            showing = .refused(reason: String(describing: error))
        }
    }

    private func read(from ledger: Ledger) async throws -> Showing {
        // Not written with `??`: the right-hand side is an `await`, and the autoclosure
        // behind that operator cannot hold one.
        let account: AccountDto
        if let existing = try await ledger.accounts().first {
            account = existing
        } else {
            account = try await ledger.openAccount(
                name: "Wallet", kind: .momo, currency: "GHS", openingMinorUnits: 0
            )
        }
        let balance = try await ledger.balance(of: account.id)
        return .open(accountId: account.id, balance: balance.plainText)
    }

    private func spend(from accountId: String) async {
        guard let ledger else { return }
        do {
            _ = try await ledger.record(
                accountId: accountId,
                direction: .out,
                minorUnits: 450,
                currency: "GHS",
                label: "Trotro"
            )
            showing = try await read(from: ledger)
        } catch {
            showing = .refused(reason: String(describing: error))
        }
    }
}
