package gh.moneytrack.core

import java.io.File
import java.time.Instant
import java.time.format.DateTimeFormatter
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import uniffi.mt_ffi.AccountBalanceDto
import uniffi.mt_ffi.AccountDto
import uniffi.mt_ffi.AccountKind
import uniffi.mt_ffi.BalanceBasis
import uniffi.mt_ffi.ChangeDto
import uniffi.mt_ffi.ChangeObserver
import uniffi.mt_ffi.Direction
import uniffi.mt_ffi.MoneyDto
import uniffi.mt_ffi.NewAccountDto
import uniffi.mt_ffi.NewTransactionDto
import uniffi.mt_ffi.Store
import uniffi.mt_ffi.TransactionDto
import uniffi.mt_ffi.TxSource
import uniffi.mt_ffi.TxStatus
import uniffi.mt_ffi.formatAmount
import uniffi.mt_ffi.openStore

/**
 * One person's ledger, open.
 *
 * A thin wrapper over the Rust store, and deliberately thin: everything that decides what a
 * number is lives below this, so the same question asked from iOS gets the same answer. What
 * is here is what is genuinely the platform's — where the file lives, how the key is kept,
 * and turning a stream of changes into a [Flow] the screens can collect.
 */
class Ledger private constructor(private val store: Store) {

    /** This installation's id, as carried on every row it writes. */
    val deviceId: String get() = store.deviceId()

    /** The revision of the last committed write. */
    val revision: ULong get() = store.revision()

    /** Opens an account. */
    suspend fun openAccount(
        name: String,
        kind: AccountKind,
        currency: String,
        openingMinorUnits: Long,
    ): AccountDto = store.createAccount(
        NewAccountDto(
            name = name,
            kind = kind,
            currency = currency,
            openingBalance = MoneyDto(minorUnits = openingMinorUnits, currency = currency),
        )
    )

    /** Every account, in display order. */
    suspend fun accounts(): List<AccountDto> = store.accounts()

    /**
     * Records a transaction. [bookedAt] is the caller's: an entry typed this evening for
     * yesterday's fare is booked yesterday, and a future-dated pending row is the forecast.
     */
    suspend fun record(
        accountId: String,
        direction: Direction,
        minorUnits: Long,
        currency: String,
        label: String,
        bookedAt: Instant = Instant.now(),
        status: TxStatus = TxStatus.CONFIRMED,
        source: TxSource = TxSource.MANUAL,
    ): TransactionDto = store.recordTransaction(
        NewTransactionDto(
            accountId = accountId,
            direction = direction,
            amount = MoneyDto(minorUnits = minorUnits, currency = currency),
            bookedAt = WIRE.format(bookedAt),
            categoryId = null,
            label = label,
            counterparty = null,
            note = null,
            status = status,
            source = source,
        )
    )

    /** Every live row in one account, oldest first. */
    suspend fun transactions(accountId: String): List<TransactionDto> =
        store.transactionsForAccount(accountId)

    /** What one account holds. */
    suspend fun balance(
        accountId: String,
        basis: BalanceBasis = BalanceBasis.CONFIRMED,
    ): MoneyDto = store.accountBalance(accountId, basis)

    /** What every account holds, in one pass over the rows. */
    suspend fun balances(
        basis: BalanceBasis = BalanceBasis.CONFIRMED,
    ): List<AccountBalanceDto> = store.accountBalances(basis)

    /**
     * Writes, as they commit.
     *
     * This is what replaces drift's live queries on this platform: a screen collects the
     * flow and re-runs the queries the change touched. Collection ending unsubscribes.
     */
    fun changes(): Flow<ChangeDto> = callbackFlow {
        val observer = object : ChangeObserver {
            override fun changed(change: ChangeDto) {
                trySend(change)
            }
        }
        val subscription = store.subscribe(observer)
        awaitClose { store.unsubscribe(subscription) }
    }

    companion object {
        /** The wire spelling of an instant: RFC 3339, UTC, which is what the store reads. */
        private val WIRE: DateTimeFormatter =
            DateTimeFormatter.ISO_INSTANT

        /** Opens the ledger at [file], encrypted under [key]: 32 raw bytes. */
        suspend fun open(file: File, key: ByteArray): Ledger =
            Ledger(openStore(file.absolutePath, key))
    }
}

/**
 * The amount as the currency's own number of decimal places, formatted in Rust.
 *
 * Where the decimal point goes is a property of the currency — two places for the cedi, none
 * for the CFA franc — and the registry that knows it is in `mt-core`. A screen that divides
 * by a hundred in Kotlin is right until the first West African row arrives, and then quietly
 * wrong by a factor of a hundred.
 */
fun MoneyDto.plainText(): String = runCatching { formatAmount(this) }.getOrDefault("—")
