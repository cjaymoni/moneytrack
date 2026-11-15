import Foundation
import Security

/// The key the ledger file is encrypted under.
///
/// Generated once on this device, kept in the Keychain, and never written anywhere else.
/// Rust is handed the bytes at open and never stores them; this is the only code in the
/// project that touches the secure enclave's doorbell.
public enum LedgerKey {
    /// Bytes the cipher wants: a 256-bit raw key.
    public static let byteCount = 32

    /// Where it lives. The service string is the bundle's, so two builds of the app on one
    /// device do not read each other's key and then fail to open each other's file.
    private static var service: String {
        (Bundle.main.bundleIdentifier ?? "moneytrack") + ".ledger-key"
    }

    private static let account = "ledger"

    /// Reads the key, generating and storing one on first run.
    public static func fromKeychain() throws -> Data {
        if let existing = try read() {
            return existing
        }
        let fresh = try generate()
        try store(fresh)
        return fresh
    }

    /// Removes the key. The ledger file becomes unreadable, which is the point: this is
    /// what "delete my data" does to anything a backup already took a copy of.
    public static func forget() throws {
        let status = SecItemDelete(query() as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw LedgerKeyError.keychain(status)
        }
    }

    private static func query() -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
    }

    private static func read() throws -> Data? {
        var lookup = query()
        lookup[kSecReturnData as String] = true
        lookup[kSecMatchLimit as String] = kSecMatchLimitOne

        var item: CFTypeRef?
        let status = SecItemCopyMatching(lookup as CFDictionary, &item)
        switch status {
        case errSecSuccess:
            guard let data = item as? Data, data.count == byteCount else {
                // A key of the wrong length cannot open the file and cannot be repaired by
                // guessing. Say so rather than generating a second key that would make the
                // existing ledger permanently unreadable.
                throw LedgerKeyError.wrongLength
            }
            return data
        case errSecItemNotFound:
            return nil
        default:
            throw LedgerKeyError.keychain(status)
        }
    }

    private static func generate() throws -> Data {
        var bytes = [UInt8](repeating: 0, count: byteCount)
        let status = SecRandomCopyBytes(kSecRandomDefault, byteCount, &bytes)
        guard status == errSecSuccess else { throw LedgerKeyError.keychain(status) }
        return Data(bytes)
    }

    private static func store(_ key: Data) throws {
        var item = query()
        item[kSecValueData as String] = key
        // After first unlock, so a sync woken in the background can open the file, and
        // this-device-only so the key never travels in an iCloud Keychain backup to a
        // device the ledger file is not on.
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly

        let status = SecItemAdd(item as CFDictionary, nil)
        guard status == errSecSuccess else { throw LedgerKeyError.keychain(status) }
    }
}

/// What the Keychain would not do.
public enum LedgerKeyError: Error, Equatable {
    /// The Keychain refused, with its own status code.
    case keychain(OSStatus)
    /// A key is stored, and it is not the length the cipher wants.
    case wrongLength
}
