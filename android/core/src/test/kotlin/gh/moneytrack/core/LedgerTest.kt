package gh.moneytrack.core

import java.nio.file.Files
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withTimeout
import uniffi.mt_ffi.AccountKind
import uniffi.mt_ffi.BalanceBasis
import uniffi.mt_ffi.ChangedTable
import uniffi.mt_ffi.Direction
import uniffi.mt_ffi.StoreException
import uniffi.mt_ffi.TxStatus

/**
 * The round trip, from Kotlin: open a real encrypted database, write to it, and read a
 * balance back out of Rust.
 *
 * This is the test phase 1a existed for. It runs on the JVM against the library built for
 * this laptop, so it takes a second rather than an emulator boot — and the same calls run on
 * a handset against the same Rust.
 */
class LedgerTest {
    private val directory = Files.createTempDirectory("moneytrack").toFile()
    private val file get() = directory.resolve("ledger.db")
    private val key = ByteArray(LedgerKey.BYTE_COUNT) { 7 }

    @AfterTest
    fun cleanUp() {
        directory.deleteRecursively()
    }

    @Test
    fun `a balance comes back from rust`() = runTest {
        val ledger = Ledger.open(file, key)

        val account = ledger.openAccount("Wallet", AccountKind.MOMO, "GHS", 25_000)
        ledger.record(
            accountId = account.id,
            direction = Direction.OUT,
            minorUnits = 4_050,
            currency = "GHS",
            label = "Trotro",
        )
        val balance = ledger.balance(account.id)

        assertEquals(20_950, balance.minorUnits)
        assertEquals("GHS", balance.currency)
        assertEquals("209.50", balance.plainText())
    }

    @Test
    fun `the file is encrypted at rest`() = runTest {
        val ledger = Ledger.open(file, key)
        ledger.openAccount("Kɔkɔɔ savings", AccountKind.BANK, "GHS", 1)

        val raw = file.readBytes()

        assertFalse(
            raw.take(15).toByteArray().contentEquals("SQLite format 3".toByteArray()),
            "the database opens as a plain SQLite file",
        )
        assertFalse(
            String(raw, Charsets.ISO_8859_1).contains("Kɔkɔɔ savings"),
            "an account name is readable in the file on disk",
        )
    }

    @Test
    fun `the wrong key is refused`() = runTest {
        Ledger.open(file, key).openAccount("Wallet", AccountKind.MOMO, "GHS", 1)

        assertFailsWith<StoreException.Unreadable> {
            Ledger.open(file, ByteArray(LedgerKey.BYTE_COUNT) { 9 })
        }
    }

    @Test
    fun `a pending row is the forecast and not the balance`() = runTest {
        val ledger = Ledger.open(file, key)
        val account = ledger.openAccount("Wallet", AccountKind.MOMO, "GHS", 25_000)

        ledger.record(
            accountId = account.id,
            direction = Direction.OUT,
            minorUnits = 90_000,
            currency = "GHS",
            label = "Rent",
            bookedAt = java.time.Instant.now().plusSeconds(14 * 24 * 60 * 60),
            status = TxStatus.PENDING,
        )

        assertEquals(25_000, ledger.balance(account.id, BalanceBasis.CONFIRMED).minorUnits)
        assertEquals(-65_000, ledger.balance(account.id, BalanceBasis.PROJECTED).minorUnits)
    }

    @Test
    fun `a change reaches the flow`() = runBlocking {
        val ledger = Ledger.open(file, key)
        val received = async(Dispatchers.Default) { ledger.changes().first() }

        // The subscription is registered when the flow is collected, and that happens on
        // another thread. Rather than guess how long that takes, write until one lands.
        withTimeout(5_000) {
            while (!received.isCompleted) {
                ledger.openAccount("Wallet", AccountKind.MOMO, "GHS", 0)
                delay(20)
            }
        }

        assertEquals(listOf(ChangedTable.ACCOUNTS), received.await().tables)
    }

    @Test
    fun `an id that is not a uuid names the field`() = runTest {
        val ledger = Ledger.open(file, key)

        val error = assertFailsWith<StoreException.Value> { ledger.balance("wallet-1") }

        assertTrue(
            error.message!!.contains("account_id is not a UUID"),
            "the refusal does not name the field: ${error.message}",
        )
    }

    @Test
    fun `a row reads back as it was written`() = runTest {
        val ledger = Ledger.open(file, key)
        val account = ledger.openAccount("Wallet", AccountKind.MOMO, "GHS", 0)

        val written = ledger.record(
            accountId = account.id,
            direction = Direction.OUT,
            minorUnits = 1_234,
            currency = "GHS",
            label = "Kɔkɔɔ — ₵ fare 🚐",
        )

        assertContentEquals(listOf(written), ledger.transactions(account.id))
        assertEquals(ledger.deviceId, written.sync.deviceId)
    }
}
