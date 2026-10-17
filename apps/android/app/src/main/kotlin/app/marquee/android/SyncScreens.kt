package app.marquee.android

import android.content.Context
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.SyncCopy
import app.marquee.core.model.PairingSnapshot
import app.marquee.core.session.CoreSession
import app.marquee.core.views.PairingView
import app.marquee.core.views.ViewStatus
import app.marquee.design.CodeText
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.MqTextField
import app.marquee.design.PrimaryButton
import app.marquee.design.Qr
import app.marquee.design.SecondaryButton
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.TextRole
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.filterIsInstance
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import java.net.URI
import java.text.DateFormat
import java.util.Date

// Pair this device and Sync (plans/android-design-v2.md 13c, the web's pairing screen): this device
// joins a sync account by showing a code that a device already in it approves.

private const val PREFS = "marquee"
private const val SERVER = "syncServer"

/** The sync server this device used before; not a secret, so plain preferences hold it. */
private fun Context.knownServer(): String =
    getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(SERVER, null).orEmpty()

private fun Context.rememberServer(server: String) =
    getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(SERVER, server).apply()

/**
 * One pairing attempt at a time; a new one closes the last. The holder, not the screen, stores a
 * join: the record is kept as soon as it arrives, whether or not its part of the page is composed.
 */
class PairingHolder(
    private val session: CoreSession,
    private val sync: SyncController,
) : ViewModel() {
    var view: PairingView? by mutableStateOf(null)
        private set
    private var watch: Job? = null

    fun start(
        server: String,
        name: String,
    ) {
        view?.close()
        watch?.cancel()
        val next = session.pairing(server, name)
        view = next
        watch =
            viewModelScope.launch {
                val joined = snapshotFlow { next.snapshot }.filterIsInstance<PairingSnapshot.Joined>().first()
                sync.didJoin(joined.record)
            }
    }

    override fun onCleared() {
        view?.close()
    }
}

@Composable
fun PairingScreen(
    session: CoreSession,
    sync: SyncController,
    onDone: () -> Unit,
) {
    val context = LocalContext.current
    val holder =
        viewModel<PairingHolder>(
            key = "pairing",
            factory = viewModelFactory { initializer { PairingHolder(session, sync) } },
        )
    val tv = Marquee.form == Form.Tv
    val known = remember(context) { context.knownServer() }
    var server by rememberSaveable { mutableStateOf(known) }
    var name by rememberSaveable { mutableStateOf(if (tv) "Marquee TV" else "Marquee phone") }
    // The fields are open only while no server is known: with one, the code shows at once.
    var advanced by rememberSaveable { mutableStateOf(known.isEmpty()) }
    LaunchedEffect(Unit) { if (known.isNotEmpty() && holder.view == null) holder.start(known, name) }
    var badAddress by rememberSaveable { mutableStateOf(false) }
    val begin = {
        val s = server.trim()
        // An address with a user name or password is never kept or sent.
        badAddress = s.isNotEmpty() && !plainAddress(s)
        if (s.isNotEmpty() && !badAddress) {
            context.rememberServer(s)
            holder.start(s, name.trim().ifEmpty { "Marquee" })
        }
    }
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                BrowseHeader("Settings", "Pair this device")
                MqText(
                    "Joins your sync account so library, progress and addons follow you. Nothing is kept until both " +
                        "screens show the same code.",
                    modifier = Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 900.dp),
                    role = TextRole.Callout,
                    color = Marquee.colors.muted,
                )
            }
        }
        holder.view?.let { view ->
            item(key = "session") {
                Column(Modifier.padding(horizontal = Marquee.safe.side)) {
                    Session(view, sync, server, onAgain = begin, onDone = onDone)
                }
            }
        }
        if (known.isEmpty()) item(key = "once") { Note("Enter your sync server once; this device remembers it.") }
        item(key = "advanced") {
            Column(
                Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 900.dp),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
            ) {
                SecondaryButton("Advanced", { advanced = !advanced }, icon = Icons.Settings, selected = advanced)
                if (advanced) {
                    MqTextField(
                        server,
                        { server = it },
                        label = "Sync server",
                        placeholder = "https://sync.example.org",
                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next),
                    )
                    if (badAddress) {
                        Note(
                            "Enter the address without a user name or password.",
                            failed = true,
                            side = 0.dp,
                        )
                    }
                    MqTextField(name, { name = it }, label = "This device", placeholder = "Marquee")
                    PrimaryButton("Show a code", begin, icon = Icons.QrCode, enabled = server.isNotBlank())
                }
            }
        }
    }
}

/** One attempt, from the code to the join. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Session(
    view: PairingView,
    sync: SyncController,
    server: String,
    onAgain: () -> Unit,
    onDone: () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val status = view.status
    if (status is ViewStatus.Failed) {
        StateMessage(
            "Pairing could not start",
            SyncCopy.pairingProblem(status.reason),
            icon = Icons.CircleAlert,
            actionLabel = "Start again",
            onAction = onAgain,
        )
        return
    }
    when (val p = view.snapshot) {
        null, PairingSnapshot.Starting -> {
            StatusWord("Contacting ${hostOf(server)}", Status.Warn)
        }

        is PairingSnapshot.Waiting -> {
            val until = DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(p.expiresMs))
            val steps = @Composable {
                Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                    MqText(
                        "On a device that already syncs, open Settings, then add a device, and scan this code.",
                        role = TextRole.Callout,
                        modifier = Modifier.widthIn(max = 720.dp),
                    )
                    StatusWord("Waiting for the other device. The code works until $until.", Status.Warn)
                }
            }
            if (tv) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.xl),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Qr(p.qr, "Pairing QR code", 320.dp)
                    steps()
                }
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                    Qr(p.qr, "Pairing QR code", 240.dp)
                    steps()
                }
            }
        }

        is PairingSnapshot.Confirm -> {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                MqText("Does the other device show the same six digits?", role = TextRole.Callout)
                CodeText(p.code)
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                ) {
                    PrimaryButton("Yes, they match", view::confirm, icon = Icons.Check)
                    SecondaryButton("No, they differ", view::reject, icon = Icons.CircleX)
                }
            }
        }

        is PairingSnapshot.Joined -> {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    MqIcon(Icons.CircleCheck, tint = Marquee.colors.ok)
                    MqText("This device joined your sync account.", role = TextRole.Headline)
                }
                SyncState(sync)
                PrimaryButton("Done", onDone)
            }
        }

        PairingSnapshot.Expired -> {
            StateMessage(
                "The code expired",
                "Start again for a new code.",
                icon = Icons.Clock,
                actionLabel = "Start again",
                onAction = onAgain,
            )
        }

        is PairingSnapshot.Failed -> {
            StateMessage(
                "Pairing stopped",
                SyncCopy.pairingProblem(p.reason),
                icon = Icons.CircleAlert,
                actionLabel = "Start again",
                onAction = onAgain,
            )
        }
    }
}

/** The server's host name only: never a path, a query or a user name. */
private fun hostOf(url: String) = runCatching { URI(url).host }.getOrNull() ?: "the sync server"

/** An address with a host and no user name or password in it. */
private fun plainAddress(url: String) =
    runCatching { URI(url).let { it.host != null && it.rawUserInfo == null } }.getOrDefault(false)

/** How sync stands: running, what went wrong, or when it last went through, with what a round left to say. */
@Composable
internal fun SyncState(sync: SyncController) {
    val problem = sync.problem
    val last = sync.lastSynced
    when {
        sync.syncing -> {
            StatusWord("Syncing", Status.Warn)
        }

        problem != null -> {
            Note(problem, failed = true, side = 0.dp)
        }

        last != null -> {
            StatusWord(
                "Synced at ${DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(last))}",
                Status.Ok,
            )
        }

        else -> {
            StatusWord("Not synced yet", Status.Idle)
        }
    }
    sync.lastReport?.let { report ->
        for (note in SyncCopy.notes(report)) MqText(note, role = TextRole.Caption, color = Marquee.colors.muted)
    }
}

/** The sync account on this device: how it stands, a round on demand, and the fingerprint to compare. */
@Composable
fun SyncScreen(sync: SyncController) {
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") { BrowseHeader("Settings", "Sync") }
        item(key = "state") {
            Column(
                Modifier.padding(horizontal = Marquee.safe.side),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
            ) {
                MqText(
                    "Library, progress and addons follow you between the devices in your sync account.",
                    role = TextRole.Callout,
                    color = Marquee.colors.muted,
                )
                SyncState(sync)
                PrimaryButton(
                    "Sync now",
                    sync::request,
                    icon = Icons.RefreshCw,
                    enabled = !sync.syncing,
                )
                sync.lastReport?.let {
                    MqText("Fingerprint ${it.fingerprint}", role = TextRole.Caption, color = Marquee.colors.dim)
                }
            }
        }
    }
}
