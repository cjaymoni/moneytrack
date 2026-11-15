package gh.moneytrack.core

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * The key the ledger file is encrypted under.
 *
 * The Android Keystore will not hand back raw key bytes — that is the point of it — and
 * SQLCipher needs raw bytes. So there are two keys: a Keystore key that never leaves secure
 * hardware, and the ledger's own 32 random bytes, kept on disk wrapped under it. Getting the
 * file is not enough, and getting the file and the wrapped key is not enough either.
 */
object LedgerKey {
    /** Bytes the cipher wants: a 256-bit raw key. */
    const val BYTE_COUNT = 32

    private const val KEYSTORE = "AndroidKeyStore"
    private const val WRAPPING_ALIAS = "gh.moneytrack.ledger-wrapping-key"
    private const val PREFERENCES = "gh.moneytrack.ledger-key"
    private const val WRAPPED = "wrapped"
    private const val NONCE = "nonce"
    private const val TRANSFORMATION = "AES/GCM/NoPadding"
    private const val TAG_BITS = 128

    /** Reads the key, generating and wrapping one on first run. */
    fun fromKeystore(context: Context): ByteArray {
        val preferences = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)
        val wrapped = preferences.getString(WRAPPED, null)
        val nonce = preferences.getString(NONCE, null)

        if (wrapped != null && nonce != null) {
            return unwrap(decode(wrapped), decode(nonce))
        }

        val fresh = ByteArray(BYTE_COUNT).also { SecureRandom().nextBytes(it) }
        val cipher = Cipher.getInstance(TRANSFORMATION).apply {
            init(Cipher.ENCRYPT_MODE, wrappingKey())
        }
        val sealed = cipher.doFinal(fresh)
        // `commit`, not `apply`: the write has to be on disk before this key opens a ledger
        // file. `apply` returns immediately and writes in the background, so a process death
        // in that window would leave a database encrypted under a key nothing remembers —
        // which is not a crash, it is someone's records gone.
        val stored = preferences.edit()
            .putString(WRAPPED, encode(sealed))
            .putString(NONCE, encode(cipher.iv))
            .commit()
        check(stored) { "the ledger key could not be stored" }
        return fresh
    }

    private fun unwrap(sealed: ByteArray, nonce: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORMATION).apply {
            init(Cipher.DECRYPT_MODE, wrappingKey(), GCMParameterSpec(TAG_BITS, nonce))
        }
        val key = cipher.doFinal(sealed)
        // A key of the wrong length cannot open the file and cannot be repaired by guessing.
        // Failing here is better than generating a second key, which would make the ledger
        // already on disk permanently unreadable.
        require(key.size == BYTE_COUNT) { "the stored ledger key is ${key.size} bytes" }
        return key
    }

    private fun wrappingKey(): SecretKey {
        val keystore = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (keystore.getEntry(WRAPPING_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let {
            return it.secretKey
        }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(
                WRAPPING_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                // Not `setUserAuthenticationRequired`: a sync woken in the background has to
                // be able to open the file. Device authentication gates the app, which is
                // the screen's job rather than the cipher's.
                .build()
        )
        return generator.generateKey()
    }

    private fun encode(bytes: ByteArray): String =
        android.util.Base64.encodeToString(bytes, android.util.Base64.NO_WRAP)

    private fun decode(text: String): ByteArray =
        android.util.Base64.decode(text, android.util.Base64.NO_WRAP)
}
