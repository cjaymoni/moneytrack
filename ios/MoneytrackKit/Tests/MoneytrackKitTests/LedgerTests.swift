import XCTest
@testable import MoneytrackKit

/// The round trip, from Swift: open a real encrypted database, write to it, and read a
/// balance back out of Rust.
///
/// This is the test phase 1a existed for. It runs against the macOS slice of the
/// XCFramework, so it takes a second rather than a simulator boot.
final class LedgerTests: XCTestCase {
    private var directory: URL!

    override func setUpWithError() throws {
        directory = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: directory)
    }

    private var path: URL { directory.appendingPathComponent("ledger.db") }
    private var key: Data { Data(repeating: 7, count: LedgerKey.byteCount) }

    func testABalanceComesBackFromRust() async throws {
        let ledger = try await Ledger.open(at: path, key: key)

        let account = try await ledger.openAccount(
            name: "Wallet",
            kind: .momo,
            currency: "GHS",
            openingMinorUnits: 25_000
        )
        _ = try await ledger.record(
            accountId: account.id,
            direction: .out,
            minorUnits: 4_050,
            currency: "GHS",
            label: "Trotro"
        )
        let balance = try await ledger.balance(of: account.id)

        XCTAssertEqual(balance.minorUnits, 20_950)
        XCTAssertEqual(balance.currency, "GHS")
        XCTAssertEqual(balance.plainText, "209.50")
    }

    func testTheFileIsEncryptedAtRest() async throws {
        let ledger = try await Ledger.open(at: path, key: key)
        _ = try await ledger.openAccount(
            name: "Kɔkɔɔ savings",
            kind: .bank,
            currency: "GHS",
            openingMinorUnits: 1
        )

        let raw = try Data(contentsOf: path)

        XCTAssertFalse(raw.starts(with: Array("SQLite format 3".utf8)),
                       "the database opens as a plain SQLite file")
        XCTAssertNil(raw.range(of: Data("Kɔkɔɔ savings".utf8)),
                     "an account name is readable in the file on disk")
    }

    func testTheWrongKeyIsRefused() async throws {
        let ledger = try await Ledger.open(at: path, key: key)
        _ = try await ledger.openAccount(
            name: "Wallet", kind: .momo, currency: "GHS", openingMinorUnits: 1
        )

        do {
            _ = try await Ledger.open(at: path, key: Data(repeating: 9, count: LedgerKey.byteCount))
            XCTFail("a ledger opened under the wrong key")
        } catch let error as StoreError {
            XCTAssertEqual(error, .Unreadable)
        }
    }

    func testAPendingRowIsTheForecastAndNotTheBalance() async throws {
        let ledger = try await Ledger.open(at: path, key: key)
        let account = try await ledger.openAccount(
            name: "Wallet", kind: .momo, currency: "GHS", openingMinorUnits: 25_000
        )

        _ = try await ledger.record(
            accountId: account.id,
            direction: .out,
            minorUnits: 90_000,
            currency: "GHS",
            label: "Rent",
            bookedAt: Date().addingTimeInterval(14 * 24 * 60 * 60),
            status: .pending
        )

        let confirmed = try await ledger.balance(of: account.id, basis: .confirmed)
        let projected = try await ledger.balance(of: account.id, basis: .projected)
        XCTAssertEqual(confirmed.minorUnits, 25_000)
        XCTAssertEqual(projected.minorUnits, -65_000)
    }

    func testAChangeReachesTheStream() async throws {
        let ledger = try await Ledger.open(at: path, key: key)
        var changes = ledger.changes().makeAsyncIterator()

        async let first = changes.next()
        _ = try await ledger.openAccount(
            name: "Wallet", kind: .momo, currency: "GHS", openingMinorUnits: 0
        )

        let change = await first
        XCTAssertEqual(change?.revision, 1)
        XCTAssertEqual(change?.tables, [.accounts])
    }

    func testAnIdThatIsNotAUuidNamesTheField() async throws {
        let ledger = try await Ledger.open(at: path, key: key)

        do {
            _ = try await ledger.balance(of: "wallet-1")
            XCTFail("a malformed id was accepted")
        } catch let error as StoreError {
            XCTAssertEqual(error, .Value(reason: "account_id is not a UUID: wallet-1"))
        }
    }
}
