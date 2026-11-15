package gh.moneytrack.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import gh.moneytrack.core.Ledger
import gh.moneytrack.core.LedgerKey
import gh.moneytrack.core.plainText
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.mt_ffi.AccountKind
import uniffi.mt_ffi.Direction

/**
 * The whole app, for now.
 *
 * Phase 1a's finish line is one call from Kotlin that opens a real encrypted database and
 * gets a real number back, so that is all this screen does: open the ledger, seed a wallet
 * the first time, and show what it holds. The screens proper — and the Material 3 theme
 * built from the tokens in `design/` — are phase 1b.
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                LedgerProof()
            }
        }
    }
}

/** What the screen can be showing. */
private sealed interface Showing {
    data object Opening : Showing
    data class Open(val accountId: String, val balance: String) : Showing
    data class Refused(val reason: String) : Showing
}

@Composable
private fun LedgerProof() {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var showing: Showing by remember { mutableStateOf(Showing.Opening) }
    var ledger: Ledger? by remember { mutableStateOf(null) }

    suspend fun read(open: Ledger): Showing = runCatching {
        val account = open.accounts().firstOrNull()
            ?: open.openAccount("Wallet", AccountKind.MOMO, "GHS", 0)
        Showing.Open(account.id, open.balance(account.id).plainText())
    }.getOrElse { failure -> Showing.Refused(failure.message ?: "the ledger would not open") }

    LaunchedEffect(Unit) {
        showing = runCatching {
            // Opening reads the key out of the Keystore and runs the migration, so it goes
            // off the main thread like every other call into the store.
            val opened = withContext(Dispatchers.IO) {
                Ledger.open(context.filesDir.resolve("ledger.db"), LedgerKey.fromKeystore(context))
            }
            ledger = opened
            read(opened)
        }.getOrElse { failure -> Showing.Refused(failure.message ?: "the ledger would not open") }
    }

    Column(
        modifier = Modifier.fillMaxSize().padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        when (val state = showing) {
            Showing.Opening -> Text("Opening the ledger…")

            is Showing.Refused -> Text(
                text = state.reason,
                style = MaterialTheme.typography.bodyMedium,
            )

            is Showing.Open -> {
                Text("GHS ${state.balance}", style = MaterialTheme.typography.displaySmall)
                Text(
                    "Walked in Rust, from an encrypted database on this device.",
                    style = MaterialTheme.typography.bodySmall,
                )
                Button(onClick = {
                    val open = ledger ?: return@Button
                    scope.launch {
                        runCatching {
                            open.record(
                                accountId = state.accountId,
                                direction = Direction.OUT,
                                minorUnits = 450,
                                currency = "GHS",
                                label = "Trotro",
                            )
                        }
                        showing = read(open)
                    }
                }) {
                    Text("Spend GHS 4.50")
                }
            }
        }
    }
}
