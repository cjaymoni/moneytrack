import Foundation

/// One person's ledger, open.
///
/// A thin wrapper over the Rust store, and deliberately thin: everything that decides what a
/// number is lives below this, so the same question asked from Android gets the same answer.
/// What is here is what is genuinely the platform's — where the file lives, how the key is
/// kept, and turning a stream of changes into something SwiftUI can watch.
public final class Ledger: @unchecked Sendable {
    private let store: Store

    private init(store: Store) {
        self.store = store
    }

    /// Opens the ledger at `url`, encrypted under `key`.
    ///
    /// The key is 32 raw bytes, which `LedgerKey` reads from the Keychain. It is never
    /// written to disk by this layer and never leaves the device.
    public static func open(at url: URL, key: Data) async throws -> Ledger {
        Ledger(store: try await openStore(path: url.path, key: key))
    }

    /// Opens the ledger where the app keeps it, with the key from the Keychain, creating
    /// both on first run.
    public static func openDefault() async throws -> Ledger {
        let directory = try FileManager.default.url(
            for: .applicationSupportDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: true
        )
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return try await open(at: directory.appendingPathComponent("ledger.db"),
                              key: try LedgerKey.fromKeychain())
    }

    /// This installation's id, as carried on every row it writes.
    public var deviceId: String { store.deviceId() }

    /// The revision of the last committed write.
    public var revision: UInt64 { store.revision() }

    /// Opens an account.
    public func openAccount(
        name: String,
        kind: AccountKind,
        currency: String,
        openingMinorUnits: Int64
    ) async throws -> AccountDto {
        try await store.createAccount(
            account: NewAccountDto(
                name: name,
                kind: kind,
                currency: currency,
                openingBalance: MoneyDto(minorUnits: openingMinorUnits, currency: currency)
            )
        )
    }

    /// Every account, in display order.
    public func accounts() async throws -> [AccountDto] {
        try await store.accounts()
    }

    /// Records a transaction. `bookedAt` is the caller's: an entry typed this evening for
    /// yesterday's fare is booked yesterday, and a future-dated pending row is the forecast.
    public func record(
        accountId: String,
        direction: Direction,
        minorUnits: Int64,
        currency: String,
        label: String,
        bookedAt: Date = Date(),
        status: TxStatus = .confirmed,
        source: TxSource = .manual
    ) async throws -> TransactionDto {
        try await store.recordTransaction(
            transaction: NewTransactionDto(
                accountId: accountId,
                direction: direction,
                amount: MoneyDto(minorUnits: minorUnits, currency: currency),
                bookedAt: Self.wire.string(from: bookedAt),
                categoryId: nil,
                label: label,
                counterparty: nil,
                note: nil,
                status: status,
                source: source
            )
        )
    }

    /// Every live row in one account, oldest first.
    public func transactions(in accountId: String) async throws -> [TransactionDto] {
        try await store.transactionsForAccount(accountId: accountId)
    }

    /// What one account holds.
    public func balance(of accountId: String, basis: BalanceBasis = .confirmed) async throws -> MoneyDto {
        try await store.accountBalance(accountId: accountId, basis: basis)
    }

    /// What every account holds, in one pass over the rows.
    public func balances(basis: BalanceBasis = .confirmed) async throws -> [AccountBalanceDto] {
        try await store.accountBalances(basis: basis)
    }

    /// Writes, as they commit.
    ///
    /// This is what replaces drift's live queries on this platform: a screen awaits the
    /// stream and re-runs the queries the change touched. The sequence ends when the task
    /// holding it is cancelled, which also unsubscribes.
    public func changes() -> AsyncStream<ChangeDto> {
        AsyncStream { continuation in
            let observer = StreamObserver(continuation: continuation)
            do {
                let subscription = try store.subscribe(observer: observer)
                continuation.onTermination = { [store] _ in
                    try? store.unsubscribe(subscription: subscription)
                }
            } catch {
                // Subscribing fails only if the store's lock is poisoned, which means a
                // database thread has already failed. Finishing the stream is the honest
                // answer: there will be no changes, rather than changes nobody hears.
                continuation.finish()
            }
        }
    }

    /// The wire spelling of an instant: RFC 3339, UTC, which is what the store reads.
    private static let wire: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        formatter.timeZone = TimeZone(identifier: "UTC")
        return formatter
    }()
}

/// Hands each change to an `AsyncStream`.
private final class StreamObserver: ChangeObserver, @unchecked Sendable {
    private let continuation: AsyncStream<ChangeDto>.Continuation

    init(continuation: AsyncStream<ChangeDto>.Continuation) {
        self.continuation = continuation
    }

    func changed(change: ChangeDto) {
        continuation.yield(change)
    }
}

extension MoneyDto {
    /// The amount as the currency's own number of decimal places, formatted in Rust.
    ///
    /// Where the decimal point goes is a property of the currency — two places for the
    /// cedi, none for the CFA franc — and the registry that knows it is in `mt-core`. A
    /// screen that divides by a hundred in Swift is right until the first West African row
    /// arrives, and then quietly wrong by a factor of a hundred.
    public var plainText: String {
        (try? formatAmount(amount: self)) ?? "—"
    }
}
