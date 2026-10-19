package app.marquee.android

import android.Manifest
import android.content.Context
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.SyncCopy
import app.marquee.core.SyncCopy.Action
import app.marquee.core.SyncRules
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.LostPreview
import app.marquee.core.model.MemberInfo
import app.marquee.core.model.PairingSnapshot
import app.marquee.core.session.CoreSession
import app.marquee.core.sync.SyncAccount
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
import app.marquee.design.focusClickable
import app.marquee.design.marqueeFocus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.filterIsInstance
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import java.text.DateFormat
import java.util.Date

// Devices and sync (plans/m7-sync.md, plans/core-protocol.md; the web's screens/sync.tsx and Apple's
// Sync/*): start an account, join one by pairing or with a recovery kit, see this device's state and
// the others, add a device, make a kit, remove a device or report one lost, stop syncing. The record
// lives in the encrypted secret store ([SyncController]); a kit is shown once and kept nowhere.
// No key, token, record or kit reaches a log.

private const val PREFS = "marquee"
private const val SERVER = "syncServer"
private const val NAME = "syncName"

/** The sync server this device used before; not a secret, so plain preferences hold it. */
private fun Context.knownServer(): String =
    getSharedPreferences(
        PREFS,
        Context.MODE_PRIVATE,
    ).getString(SERVER, null).orEmpty().trim().takeIf(SyncRules::plainAddress).orEmpty()

/** The server and this device's name, for the next pairing or kit: neither is a secret. */
private fun Context.rememberJoin(
    server: String,
    name: String,
) = getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    .edit()
    .putString(SERVER, server)
    .putString(NAME, name)
    .apply()

/** How other devices list this one: the name it joined with last, else one by its form. */
private fun Context.deviceName(tv: Boolean): String =
    getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(NAME, null)?.trim()?.ifEmpty { null }
        ?: if (tv) "Marquee TV" else "Marquee phone"

private fun clock(ms: Long) = DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(ms))

/** A line of what happened: a dot in the tone and the words. */
@Composable
private fun Said(
    text: String,
    tone: Status,
) {
    if (tone == Status.Bad) Note(text, failed = true, side = 0.dp) else StatusWord(text, tone)
}

/** A field with its name shown above it, so a filled field still says what it holds. The field speaks the name. */
@Composable
private fun Labeled(
    label: String,
    field: @Composable () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        MqText(
            label,
            role = TextRole.Caption,
            color = Marquee.colors.muted,
            modifier = Modifier.clearAndSetSemantics {},
        )
        field()
    }
}

/** A highlighted line with its icon: a warning or a step the user should take. */
@Composable
private fun Callout(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    text: String,
) {
    val shape = RoundedCornerShape(Marquee.radius.card)
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(Marquee.colors.glass)
            .border(1.dp, Marquee.colors.edge, shape)
            .padding(16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        MqIcon(icon, tint = Marquee.colors.text)
        MqText(text, role = TextRole.Callout, color = Marquee.colors.muted)
    }
}

/** A row a TV cannot act on: focusable so the remote reads it, with the line that says where to do it. */
@Composable
private fun ElsewhereRow(
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    title: String,
    text: String,
) {
    val shape = RoundedCornerShape(Marquee.radius.l)
    Row(
        Modifier
            .padding(horizontal = Marquee.safe.side)
            .fillMaxWidth()
            .marqueeFocus(Marquee.radius.l, scale = 1.01f)
            .clip(shape)
            .background(Marquee.colors.surface)
            .border(1.dp, Marquee.colors.edge, shape)
            .semantics(mergeDescendants = true) {}
            .focusable()
            .padding(24.dp),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(icon, tint = Marquee.colors.muted)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            MqText(title, role = TextRole.Headline, weight = FontWeight.Bold)
            MqText(text, role = TextRole.Caption, color = Marquee.colors.muted)
        }
    }
}

/** A page's own column: the gutter on both sides and a readable width. */
private fun LazyListScope.block(
    key: String,
    content: @Composable () -> Unit,
) = item(key = key) {
    Column(
        Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 900.dp),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) { content() }
}

/** The 12 characters in groups of four, read out one at a time. */
@Composable
private fun Fingerprint(fingerprint: String) {
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        Kicker(SyncCopy.COMPARE_LABEL, side = 0.dp)
        MqText(
            SyncRules.fingerprintGroups(fingerprint),
            modifier =
                Modifier.semantics {
                    contentDescription = "${SyncCopy.COMPARE_LABEL}: ${fingerprint.toList().joinToString(" ")}"
                },
            role = TextRole.Title2,
            fontFamily = FontFamily.Monospace,
        )
        MqText(SyncCopy.COMPARE_HELP, role = TextRole.Callout, color = Marquee.colors.muted)
    }
}

/** What a failed action reads as. */
private fun wordsOf(
    e: Throwable,
    action: Action,
): String {
    // The core's failures are worded; anything else is the app's own, and its kind goes to the log.
    if (e !is CoreError) CoreLog.w("sync ${action.name.lowercase()}: ${e.javaClass.simpleName}")
    return SyncAccount.problemOf(e, action).words
}

/**
 * Asks who is there before [run]: the system prompt when the device has a screen lock; a phone with
 * none refuses (there is nothing to ask with), and a TV with none takes the page's second step,
 * [askAgain]. Returns the words of why nothing ran, or null when [run] ran (it may throw).
 */
private suspend fun guarded(
    owner: OwnerCheck,
    tv: Boolean,
    title: String,
    askAgain: () -> Unit,
    run: suspend () -> Unit,
): String? {
    if (!owner.secure) {
        if (tv) {
            askAgain()
            return null
        }
        return SyncCopy.problem("noLock", "")
    }
    return when (owner.confirm(title)) {
        Owner.Confirmed -> {
            run()
            null
        }

        Owner.Cancelled -> {
            SyncCopy.problem("unconfirmed", "")
        }

        Owner.NoLock -> {
            SyncCopy.problem("noLock", "")
        }
    }
}

/** The TV's second step, where no screen lock can say who is there. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun AskAgain(
    yes: String,
    onYes: () -> Unit,
    onCancel: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        MqText(SyncCopy.TV_SURE, role = TextRole.Callout)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            SecondaryButton(yes, onYes, icon = Icons.ShieldAlert, focusRequester = stepFocus())
            SecondaryButton(SyncCopy.CANCEL, onCancel)
        }
    }
}

/** The first button of a step that replaces the focused one takes the focus, so a remote keeps its place. */
@Composable
private fun stepFocus(): FocusRequester {
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) {
        withFrameNanos { }
        runCatching { focus.requestFocus(FocusDirection.Enter) }
    }
    return focus
}

/** Once a step that held the focus has gone, the focus goes to the first of [targets] that takes it. */
private fun CoroutineScope.refocus(vararg targets: FocusRequester) {
    launch {
        withFrameNanos { }
        targets.any { runCatching { it.requestFocus(FocusDirection.Enter) }.getOrDefault(false) }
    }
}

// The hub

/**
 * Devices and sync: not joined, the three ways in (pair, start an account, use a kit); joined, this
 * device's state with every fact of the last round and the 12 characters to compare, the devices with
 * Remove and Report lost, Add a device, Make a recovery kit, and stopping.
 */
@Composable
fun SyncScreen(
    sync: SyncController,
    onPair: () -> Unit,
    onJoin: (kit: Boolean) -> Unit,
    onAdd: () -> Unit,
    onKit: () -> Unit,
    onLost: (String) -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    var left by remember { mutableStateOf(false) }
    val addFocus = remember { FocusRequester() }
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") { BrowseHeader("Settings", SyncCopy.TITLE) }
        block("lede") { MqText(SyncCopy.LEDE, color = Marquee.colors.muted) }
        if (!sync.joined) {
            block("not-joined") {
                if (left) StatusWord(SyncCopy.LEFT, Status.Ok)
                sync.problem?.let { Said(it.words, Status.Bad) }
            }
            item(key = "pair") { LinkRow(Icons.QrCode, SyncCopy.PAIR_TITLE, SyncCopy.PAIR_TEXT, onPair) }
            item(key = "start") { LinkRow(Icons.Plus, SyncCopy.START_TITLE, SyncCopy.START_TEXT, { onJoin(false) }) }
            item(
                key = "recover",
            ) { LinkRow(Icons.LifeBuoy, SyncCopy.RECOVER_TITLE, SyncCopy.RECOVER_TEXT, { onJoin(true) }) }
        } else {
            joined(sync, tv, onAdd, onKit, onLost, onLeft = { left = true }, addFocus)
        }
    }
}

private fun LazyListScope.joined(
    sync: SyncController,
    tv: Boolean,
    onAdd: () -> Unit,
    onKit: () -> Unit,
    onLost: (String) -> Unit,
    onLeft: () -> Unit,
    addFocus: FocusRequester,
) {
    item(key = "state") { StateCard(sync) }
    item(key = "devices") { DeviceList(sync, onLost, addFocus) }
    item(key = "add") {
        Box(Modifier.focusRequester(addFocus)) {
            LinkRow(Icons.QrCode, SyncCopy.ADD_TITLE, if (tv) SyncCopy.TV_ADD else SyncCopy.ADD_TEXT, onAdd)
        }
    }
    if (tv) {
        item(key = "kit") { ElsewhereRow(Icons.KeyRound, SyncCopy.KIT_TITLE, SyncCopy.TV_KIT) }
    } else {
        item(key = "kit") { LinkRow(Icons.KeyRound, SyncCopy.KIT_TITLE, SyncCopy.KIT_TEXT, onKit) }
    }
    block("leave") { Leave(sync, onLeft) }
}

/** This device: the state, every fact of the last round, Sync now and a fix, and the 12 characters. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun StateCard(sync: SyncController) {
    val tv = Marquee.form == Form.Tv
    val scope = rememberCoroutineScope()
    val now = remember { FocusRequester() }
    var said by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) {
        if (tv) {
            withFrameNanos { }
            runCatching { now.requestFocus() }
        }
    }
    Section {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            val problem = sync.problem
            val report = sync.lastReport
            val last = sync.lastSynced
            when {
                problem != null -> Said(problem.words, Status.Bad)
                sync.syncing -> StatusWord(SyncCopy.SYNCING, Status.Warn)
                last != null && report != null -> StatusWord(SyncCopy.syncedLine(clock(last), report), Status.Ok)
                else -> StatusWord(SyncCopy.NOT_SYNCED, Status.Idle)
            }
            report?.let { for (note in SyncCopy.notes(it)) Note(note, side = 0.dp) }
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            ) {
                PrimaryButton(
                    if (sync.syncing) SyncCopy.SYNCING else SyncCopy.SYNC_NOW,
                    sync::request,
                    icon = Icons.RefreshCw,
                    focusRequester = now,
                )
                when (problem?.code) {
                    "fork" -> {
                        SecondaryButton(SyncCopy.USE_SERVER, {
                            scope.launch {
                                said =
                                    try {
                                        sync.useServerRoster()
                                        runCatching { sync.loadDevices() }
                                        null
                                    } catch (e: CancellationException) {
                                        throw e
                                    } catch (e: Exception) {
                                        wordsOf(e, Action.Devices)
                                    }
                            }
                        }, icon = Icons.MonitorSmartphone)
                    }

                    // Forgetting is for a relay that cannot take this device back: it keeps the library.
                    "wrongRelay", "relayBehind", "notJoined" -> {
                        SecondaryButton(SyncCopy.FORGET_HERE, sync::forgetHere, icon = Icons.Trash)
                    }
                }
            }
            said?.let { Said(it, Status.Bad) }
            // The latest device list's 12 characters: a round's own value can be a step behind it.
            (sync.devices?.fingerprint ?: report?.fingerprint)?.let { Fingerprint(it) }
        }
    }
}

/** The account's devices and kits, with Remove and Report lost; a callout when there is no kit. */
@Composable
private fun DeviceList(
    sync: SyncController,
    onLost: (String) -> Unit,
    afterList: FocusRequester,
) {
    val tv = Marquee.form == Form.Tv
    val scope = rememberCoroutineScope()
    val listFocus = remember { FocusRequester() }
    var listError by remember { mutableStateOf<String?>(null) }
    var said by remember { mutableStateOf<Pair<String, String>?>(null) }
    LaunchedEffect(Unit) {
        listError =
            try {
                sync.loadDevices()
                null
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                wordsOf(e, Action.Devices)
            }
    }
    val members = SyncRules.sortMembers(sync.devices?.members.orEmpty())
    val noKit = sync.devices != null && members.none { it.kit }
    Column(
        Modifier
            .padding(horizontal = Marquee.safe.side)
            .widthIn(max = 900.dp)
            .fillMaxWidth()
            .focusRequester(listFocus),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        if (sync.recovered || noKit) {
            val text =
                when {
                    !sync.recovered -> SyncCopy.NO_KIT
                    tv -> SyncCopy.TV_AFTER_RECOVER
                    else -> SyncCopy.AFTER_RECOVER
                }
            Callout(Icons.KeyRound, text)
        }
        Kicker(SyncCopy.DEVICES, side = 0.dp)
        said?.takeIf { it.first == "list" }?.let { StatusWord(it.second, Status.Ok) }
        listError?.let { Said(it, Status.Bad) }
        if (sync.devices == null && listError == null) StatusWord(SyncCopy.LOADING_DEVICES, Status.Warn)
        for (member in members) {
            androidx.compose.runtime.key(member.id) {
                MemberRow(
                    member,
                    members,
                    sync,
                    said = said?.takeIf { it.first == member.id }?.second,
                    onSaid = { said = it },
                    onLost = onLost,
                    onRemoved = { scope.refocus(listFocus, afterList) },
                )
            }
        }
    }
}

/** One device or kit: its name, when and through which device it came, and its actions. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun MemberRow(
    member: MemberInfo,
    members: List<MemberInfo>,
    sync: SyncController,
    said: String?,
    onSaid: (Pair<String, String>?) -> Unit,
    onLost: (String) -> Unit,
    onRemoved: () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val scope = rememberCoroutineScope()
    val owner = rememberOwnerCheck()
    val removeFocus = remember { FocusRequester() }
    val name = SyncRules.memberName(member)
    var asking by remember { mutableStateOf(false) }
    var again by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Marquee.radius.card)
    val remove: suspend () -> Unit = {
        busy = true
        try {
            sync.removeDevice(member.id)
            onSaid("list" to "${SyncCopy.REMOVED}: $name")
            onRemoved()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            onSaid(member.id to wordsOf(e, Action.Remove))
        } finally {
            busy = false
        }
    }
    Column(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(Marquee.colors.glass)
            .border(1.dp, Marquee.colors.edge, shape)
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            MqText(name, role = TextRole.Headline, weight = FontWeight.Bold)
            if (member.thisDevice) StatusWord(SyncCopy.THIS_DEVICE, Status.Ok)
        }
        MqText(
            SyncCopy.addedLine(member.kit, member.addedMs, SyncRules.sponsorName(member, members)),
            role = TextRole.Caption,
            color = Marquee.colors.muted,
        )
        said?.let { Said(it, Status.Bad) }
        if (member.thisDevice) return@Column
        when {
            again -> {
                AskAgain("${SyncCopy.REMOVE} $name", onYes = {
                    again = false
                    scope.launch { remove() }
                }, onCancel = {
                    again = false
                    asking = false
                    scope.refocus(removeFocus)
                })
            }

            asking -> {
                MqText(
                    if (member.kit) SyncCopy.REMOVE_KIT_ASK else SyncCopy.removeDeviceAsk(name),
                    role = TextRole.Callout,
                )
                FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    SecondaryButton("${SyncCopy.REMOVE} $name", {
                        if (busy) return@SecondaryButton
                        onSaid(null)
                        scope.launch {
                            guarded(owner, tv, SyncCopy.AUTH_REMOVE, askAgain = { again = true }) { remove() }
                                ?.let { onSaid(member.id to it) }
                        }
                    }, icon = Icons.Trash, busy = busy, focusRequester = stepFocus())
                    SecondaryButton(SyncCopy.CANCEL, {
                        asking = false
                        scope.refocus(removeFocus)
                    })
                }
            }

            else -> {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    SecondaryButton(SyncCopy.REMOVE, {
                        onSaid(null)
                        asking = true
                    }, icon = Icons.Trash, spoken = "${SyncCopy.REMOVE} $name", focusRequester = removeFocus)
                    if (!member.kit) {
                        SecondaryButton(
                            SyncCopy.REPORT_LOST,
                            { onLost(member.id) },
                            icon = Icons.ShieldAlert,
                            spoken = "${SyncCopy.REPORT_LOST}: $name",
                        )
                    }
                }
            }
        }
    }
}

/** Stop syncing here: edits not sent go first; with some unsent, sync again or stop anyway. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Leave(
    sync: SyncController,
    onLeft: () -> Unit,
) {
    val scope = rememberCoroutineScope()
    var unsent by remember { mutableStateOf(false) }

    /** Leaving needs the sync server; when it cannot be reached, the account can be forgotten here only. */
    var unreachable by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val leaveFocus = remember { FocusRequester() }

    fun leave(force: Boolean) {
        if (busy) return
        busy = true
        error = null
        scope.launch {
            try {
                sync.signOut(force)
                onLeft()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                val problem = SyncAccount.problemOf(e, Action.Leave)
                unsent = problem.code == "unsent"
                unreachable = problem.code == null
                error = problem.words
            } finally {
                busy = false
            }
        }
    }
    Kicker(SyncCopy.LEAVE_TITLE, side = 0.dp)
    MqText(SyncCopy.LEAVE_TEXT, role = TextRole.Callout, color = Marquee.colors.muted)
    error?.let { Said(it, Status.Bad) }
    FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        if (unsent) {
            SecondaryButton(SyncCopy.SYNC_AGAIN, {
                unsent = false
                error = null
                sync.request()
                scope.refocus(leaveFocus)
            }, icon = Icons.RefreshCw, focusRequester = stepFocus())
            SecondaryButton(SyncCopy.LEAVE_ANYWAY, { leave(true) }, icon = Icons.LogOut, busy = busy)
        } else {
            SecondaryButton(
                SyncCopy.LEAVE_BUTTON,
                { leave(false) },
                icon = Icons.LogOut,
                busy = busy,
                focusRequester = leaveFocus,
            )
            if (unreachable) SecondaryButton(SyncCopy.FORGET_HERE, sync::forgetHere, icon = Icons.Trash)
        }
    }
    if (unreachable) MqText(SyncCopy.FORGET_HERE_TEXT, role = TextRole.Callout, color = Marquee.colors.muted)
}

/** A page that needs a joined device says so instead of failing on its first op. */
@Composable
private fun NeedsAccount(
    sync: SyncController,
    onBack: () -> Unit,
    content: @Composable () -> Unit,
) {
    if (sync.joined) {
        content()
    } else {
        StateMessage(SyncCopy.TITLE, SyncCopy.OFF_LINE, actionLabel = SyncCopy.TITLE, onAction = onBack)
    }
}

// Start an account, or join one with a kit

/**
 * Start an account (the first device) or join one with a kit: both take the server and this
 * device's name. A join over a live account asks first. The kit is held only while the page is open.
 */
@Composable
fun SyncJoinScreen(
    sync: SyncController,
    kit: Boolean,
    onDone: () -> Unit,
) {
    val context = LocalContext.current
    val tv = Marquee.form == Form.Tv
    val scope = rememberCoroutineScope()
    var server by rememberSaveable { mutableStateOf(context.knownServer()) }
    var name by rememberSaveable { mutableStateOf(context.deviceName(tv)) }
    // The kit or the invite: the kit stays out of saved state, which can outlive the page.
    var extra by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var replacing by remember { mutableStateOf(false) }
    val first = remember { FocusRequester() }
    val submitFocus = remember { FocusRequester() }

    fun submit(confirmed: Boolean) {
        if (busy) return
        val s = server.trim()
        // An address with a user name or password is never kept or sent.
        if (!SyncRules.plainAddress(s)) {
            error = SyncCopy.BAD_ADDRESS
            return
        }
        if (kit && extra.isBlank()) {
            error = SyncCopy.NO_KIT_ENTERED
            return
        }
        if (sync.joined && !confirmed) {
            replacing = true
            return
        }
        replacing = false
        busy = true
        error = null
        scope.launch {
            try {
                val n = name.trim().ifEmpty { "Marquee" }
                if (kit) sync.recover(s, extra.trim(), n) else sync.createAccount(s, extra.trim(), n)
                context.rememberJoin(s, n)
                // The kit leaves the page with it: nothing keeps it.
                extra = ""
                onDone()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                error = wordsOf(e, if (kit) Action.Recover else Action.Start)
            } finally {
                busy = false
            }
        }
    }
    LaunchedEffect(Unit) {
        if (tv) {
            withFrameNanos { }
            runCatching { first.requestFocus() }
        }
    }
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") { BrowseHeader(SyncCopy.TITLE, if (kit) SyncCopy.RECOVER_TITLE else SyncCopy.START_TITLE) }
        block("lede") { MqText(if (kit) SyncCopy.RECOVER_LEDE else SyncCopy.START_TEXT, color = Marquee.colors.muted) }
        block("form") {
            Labeled(SyncCopy.SERVER) {
                MqTextField(
                    server,
                    { server = it },
                    label = SyncCopy.SERVER,
                    placeholder = SyncCopy.SERVER_HINT,
                    modifier = Modifier.focusRequester(first),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next),
                )
            }
            if (kit) {
                Labeled(SyncCopy.KIT) {
                    MqTextField(
                        extra,
                        { extra = it },
                        label = SyncCopy.KIT,
                        placeholder = SyncCopy.KIT_HINT,
                        keyboardOptions =
                            KeyboardOptions(
                                capitalization = KeyboardCapitalization.Characters,
                                autoCorrectEnabled = false,
                                keyboardType = KeyboardType.Password,
                                imeAction = ImeAction.Next,
                            ),
                    )
                }
            }
            Labeled(SyncCopy.DEVICE) {
                MqTextField(name, { name = it }, label = SyncCopy.DEVICE, placeholder = "Marquee")
            }
            MqText(SyncCopy.DEVICE_HELP, role = TextRole.Caption, color = Marquee.colors.muted)
            if (!kit) {
                Labeled(SyncCopy.INVITE) {
                    MqTextField(
                        extra,
                        { extra = it },
                        label = SyncCopy.INVITE,
                        placeholder = SyncCopy.INVITE_HINT,
                        keyboardOptions = KeyboardOptions(autoCorrectEnabled = false, imeAction = ImeAction.Done),
                    )
                }
            }
            error?.let { Said(it, Status.Bad) }
            if (replacing) {
                Callout(Icons.TriangleAlert, "${SyncCopy.REPLACE_ASK}. ${SyncCopy.REPLACE_TEXT}")
                Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    SecondaryButton(SyncCopy.REPLACE_BUTTON, {
                        submit(true)
                        scope.refocus(submitFocus)
                    }, icon = Icons.Check, focusRequester = stepFocus())
                    SecondaryButton(SyncCopy.CANCEL, {
                        replacing = false
                        scope.refocus(submitFocus)
                    })
                }
            } else {
                PrimaryButton(
                    when {
                        busy && kit -> SyncCopy.RECOVERING
                        busy -> SyncCopy.STARTING
                        kit -> SyncCopy.RECOVER_BUTTON
                        else -> SyncCopy.START_BUTTON
                    },
                    { submit(false) },
                    icon = if (kit) Icons.LifeBuoy else Icons.Check,
                    focusRequester = submitFocus,
                )
            }
        }
    }
}

// Add a device

/**
 * Approve a new device: scan its code (a phone with a camera) or paste it, then a step that names its
 * sync server, then the six digits the new device must show. A scanned or pasted code never goes
 * straight to approving. A TV says to use a phone, and still takes a pasted code.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun AddDeviceScreen(
    sync: SyncController,
    onDone: () -> Unit,
) {
    val context = LocalContext.current
    val tv = Marquee.form == Form.Tv
    val scope = rememberCoroutineScope()
    val canScan = !tv && context.hasCamera()
    var code by remember { mutableStateOf("") }
    var scanning by remember { mutableStateOf(false) }
    var noCamera by remember { mutableStateOf(false) }

    /** The code waiting for the step that names its sync server. */
    var reviewing by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var digits by remember { mutableStateOf<String?>(null) }
    val fieldFocus = remember { FocusRequester() }
    val permission =
        rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
            scanning = granted
            noCamera = !granted
        }

    fun take(text: String) {
        val trimmed = text.trim()
        if (!SyncRules.isPairingCode(trimmed) || SyncRules.pairingHost(trimmed) == null) {
            error = SyncCopy.NOT_A_CODE
            return
        }
        error = null
        reviewing = trimmed
    }

    fun approve(qr: String) {
        if (busy) return
        busy = true
        error = null
        scope.launch {
            try {
                digits = sync.approvePairing(qr)
                code = ""
                reviewing = null
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                error = wordsOf(e, Action.Approve)
            } finally {
                busy = false
            }
        }
    }
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") { BrowseHeader(SyncCopy.TITLE, SyncCopy.ADD_TITLE) }
        block("body") {
            NeedsAccount(sync, onDone) {
                val shown = digits
                val pending = reviewing
                when {
                    shown != null -> {
                        MqText(SyncCopy.APPROVED_STEP)
                        CodeText(shown)
                        MqText(SyncCopy.APPROVED_HELP, role = TextRole.Callout, color = Marquee.colors.muted)
                        PrimaryButton(SyncCopy.DONE, onDone, icon = Icons.Check, focusRequester = stepFocus())
                    }

                    pending != null -> {
                        MqText(SyncCopy.approveAsk(SyncRules.pairingHost(pending).orEmpty()), role = TextRole.Title3)
                        Callout(Icons.ShieldAlert, SyncCopy.ADD_WARNING)
                        error?.let { Said(it, Status.Bad) }
                        FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                            PrimaryButton(
                                if (busy) SyncCopy.APPROVING else SyncCopy.APPROVE,
                                { approve(pending) },
                                icon = Icons.Check,
                                focusRequester = stepFocus(),
                            )
                            SecondaryButton(SyncCopy.CANCEL, {
                                if (!busy) {
                                    reviewing = null
                                    error = null
                                    scope.refocus(fieldFocus)
                                }
                            })
                        }
                    }

                    else -> {
                        MqText(if (tv) SyncCopy.TV_ADD else SyncCopy.ADD_LEDE, color = Marquee.colors.muted)
                        Callout(Icons.ShieldAlert, SyncCopy.ADD_WARNING)
                        if (scanning) {
                            QrScanner(
                                onCode = { found ->
                                    // Cancel or leaving may have come first: a code that arrives after does nothing.
                                    if (scanning) {
                                        scanning = false
                                        take(found)
                                    }
                                },
                                onFailed = {
                                    scanning = false
                                    noCamera = true
                                },
                                modifier =
                                    Modifier
                                        .fillMaxWidth()
                                        .aspectRatio(
                                            1f,
                                        ).clip(RoundedCornerShape(Marquee.radius.card)),
                            )
                            MqText(SyncCopy.SCANNING, role = TextRole.Callout)
                            SecondaryButton(SyncCopy.CANCEL, { scanning = false })
                        } else if (canScan) {
                            PrimaryButton(SyncCopy.SCAN, {
                                error = null
                                noCamera = false
                                if (context.cameraAllowed()) {
                                    scanning = true
                                } else {
                                    permission.launch(
                                        Manifest.permission.CAMERA,
                                    )
                                }
                            }, icon = Icons.Camera)
                        }
                        if (noCamera) Said(SyncCopy.NO_CAMERA, Status.Bad)
                        Labeled(if (canScan) SyncCopy.PASTE_LABEL else SyncCopy.PASTE_ONLY) {
                            MqTextField(
                                code,
                                { code = it },
                                label = if (canScan) SyncCopy.PASTE_LABEL else SyncCopy.PASTE_ONLY,
                                placeholder = SyncCopy.PASTE_HINT,
                                modifier = Modifier.focusRequester(fieldFocus),
                                keyboardOptions =
                                    KeyboardOptions(
                                        autoCorrectEnabled = false,
                                        keyboardType = KeyboardType.Uri,
                                    ),
                            )
                        }
                        if (canScan) {
                            SecondaryButton(
                                SyncCopy.APPROVE,
                                { take(code) },
                                icon = Icons.Check,
                                enabled = code.isNotBlank(),
                            )
                        } else {
                            PrimaryButton(
                                SyncCopy.APPROVE,
                                { take(code) },
                                icon = Icons.Check,
                                enabled = code.isNotBlank(),
                            )
                        }
                        error?.let { Said(it, Status.Bad) }
                    }
                }
            }
        }
    }
}

// Recovery kit

/**
 * Make a recovery kit, behind the screen-lock prompt: shown once, kept nowhere, copied only marked
 * sensitive with a warning, and cleared from the clipboard when the page closes. Not on a TV.
 */
@Composable
fun RecoveryKitScreen(
    sync: SyncController,
    onDone: () -> Unit,
) {
    val context = LocalContext.current
    val tv = Marquee.form == Form.Tv
    val scope = rememberCoroutineScope()
    val owner = rememberOwnerCheck()
    // The kit lives here only, for as long as the page is open.
    var kit by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var copied by remember { mutableStateOf(false) }
    DisposableEffect(Unit) {
        onDispose {
            kit = null
            SyncClipboard.clearKit(context)
        }
    }

    fun make() {
        if (busy) return
        busy = true
        error = null
        scope.launch {
            try {
                error = guarded(owner, tv = false, SyncCopy.AUTH_KIT, askAgain = {}) { kit = sync.createRecoveryKit() }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                error = wordsOf(e, Action.Kit)
            } finally {
                busy = false
            }
        }
    }
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") { BrowseHeader(SyncCopy.TITLE, SyncCopy.KIT_TITLE) }
        if (tv) {
            item(key = "tv") { ElsewhereRow(Icons.KeyRound, SyncCopy.TV_ONLY, SyncCopy.TV_KIT) }
            return@LazyColumn
        }
        block("body") {
            NeedsAccount(sync, onDone) {
                val shown = kit
                if (shown == null) {
                    MqText(SyncCopy.KIT_LEDE, color = Marquee.colors.muted)
                    MqText(SyncCopy.KIT_ONCE, role = TextRole.Callout, color = Marquee.colors.muted)
                    error?.let { Said(it, Status.Bad) }
                    Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                        PrimaryButton(
                            if (busy) SyncCopy.MAKING_KIT else SyncCopy.MAKE_KIT,
                            ::make,
                            icon = Icons.KeyRound,
                        )
                        SecondaryButton(SyncCopy.CANCEL, onDone)
                    }
                } else {
                    Kicker(SyncCopy.KIT_SHOWN, side = 0.dp)
                    MqText(
                        SyncRules.kitLines(shown),
                        modifier =
                            Modifier.semantics {
                                contentDescription =
                                    "${SyncCopy.KIT_SHOWN}: ${shown.toList().joinToString(" ")}"
                            },
                        role = TextRole.Title3,
                        fontFamily = FontFamily.Monospace,
                    )
                    Callout(Icons.TriangleAlert, SyncCopy.KIT_WARNING)
                    MqText(
                        if (copied) SyncCopy.KIT_COPIED else SyncCopy.COPY_KIT_HELP,
                        role = TextRole.Callout,
                        color = Marquee.colors.muted,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                        SecondaryButton(if (copied) SyncCopy.COPIED else SyncCopy.COPY_KIT, {
                            SyncClipboard.copyKit(context, shown)
                            copied = true
                        }, icon = if (copied) Icons.Check else Icons.Copy)
                        PrimaryButton(SyncCopy.SAVED_KIT, onDone, icon = Icons.Check)
                    }
                }
            }
        }
    }
}

// Report lost

/**
 * Report a device lost: it goes, with the devices it added (from a fresh device list, ticked, each
 * one untickable; an id the list does not have yet is listed as an unknown device) and always the kits
 * they made. Behind the screen-lock prompt; a TV with none asks a second time.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ReportLostScreen(
    sync: SyncController,
    lost: String,
    onDone: () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val scope = rememberCoroutineScope()
    val owner = rememberOwnerCheck()
    var loaded by remember { mutableStateOf<Pair<List<MemberInfo>, LostPreview>?>(null) }
    var loadError by remember { mutableStateOf<String?>(null) }
    var keep by remember { mutableStateOf(emptySet<String>()) }
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var again by remember { mutableStateOf(false) }
    var tries by remember { mutableStateOf(0) }
    val lostFocus = remember { FocusRequester() }
    // The names come from a device list fetched now, never a cached one: a device added since the
    // page opened must not go unseen.
    LaunchedEffect(tries) {
        loadError = null
        try {
            val list = sync.loadDevices()
            loaded = list.members to sync.lostPreview(lost)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            loadError = wordsOf(e, Action.Lost)
        }
    }
    val members = loaded?.first ?: sync.devices?.members.orEmpty()
    val name = members.firstOrNull { it.id == lost }?.let(SyncRules::memberName) ?: SyncCopy.UNNAMED
    val drop = SyncRules.lostDrop(lost, loaded?.second?.devices.orEmpty(), keep)
    val report: suspend () -> Unit = {
        busy = true
        try {
            sync.reportLost(lost, keep.toList())
            onDone()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            error = wordsOf(e, Action.Lost)
        } finally {
            busy = false
        }
    }
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") { BrowseHeader(SyncCopy.TITLE, "${SyncCopy.REPORT_LOST}: $name") }
        block("lede") { MqText(SyncCopy.named(SyncCopy.LOST_LEDE, name), color = Marquee.colors.muted) }
        val shown = loaded
        when {
            loadError != null -> {
                block("failed") {
                    Said(loadError.orEmpty(), Status.Bad)
                    Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                        PrimaryButton(SyncCopy.RETRY, { tries++ }, icon = Icons.RefreshCw, focusRequester = stepFocus())
                        SecondaryButton(SyncCopy.CANCEL, onDone)
                    }
                }
            }

            shown == null -> {
                block("checking") { StatusWord(SyncCopy.named(SyncCopy.LOST_CHECKING, name), Status.Warn) }
            }

            else -> {
                val (list, preview) = shown
                val rows = SyncRules.lostRows(preview, list)
                if (rows.isNotEmpty()) {
                    block("added-head") {
                        Kicker(SyncCopy.named(SyncCopy.LOST_ADDED, name), side = 0.dp)
                        MqText(
                            SyncCopy.named(SyncCopy.LOST_ADDED_HELP, name),
                            role = TextRole.Callout,
                            color = Marquee.colors.muted,
                        )
                    }
                    // On a remote the page opens on what goes with the lost device, not on the button.
                    itemsIndexed(rows, key = { _, row -> "row:${row.id}" }) { i, row ->
                        val going = row.id !in keep
                        TickRow(
                            row,
                            going,
                            if (i ==
                                0
                            ) {
                                stepFocus()
                            } else {
                                null
                            },
                            lostFocus.takeIf { i == rows.lastIndex },
                        ) {
                            keep = if (going) keep + row.id else keep - row.id
                        }
                    }
                }
                block("go") {
                    // With nothing else going, the page opens on the button.
                    if (rows.isEmpty()) LaunchedEffect(Unit) { refocus(lostFocus) }
                    val kits = SyncRules.kitsGoing(preview.kits, list, drop)
                    if (kits.isNotEmpty()) Callout(Icons.KeyRound, SyncCopy.lostKits(kits.size))
                    error?.let { Said(it, Status.Bad) }
                    if (again) {
                        AskAgain(SyncCopy.lostButton(drop.size), onYes = {
                            again = false
                            scope.launch { report() }
                        }, onCancel = {
                            again = false
                            scope.refocus(lostFocus)
                        })
                    } else {
                        FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                            SecondaryButton(SyncCopy.lostButton(drop.size), {
                                if (busy) return@SecondaryButton
                                error = null
                                scope.launch {
                                    guarded(
                                        owner,
                                        tv,
                                        SyncCopy.AUTH_LOST,
                                        askAgain = { again = true },
                                    ) { report() }?.let {
                                        error =
                                            it
                                    }
                                }
                            }, icon = Icons.ShieldAlert, busy = busy, focusRequester = lostFocus)
                            SecondaryButton(SyncCopy.CANCEL, onDone)
                        }
                    }
                }
            }
        }
    }
}

/** One device that goes unless the user unticks it. */
@Composable
private fun TickRow(
    row: SyncRules.LostRow,
    going: Boolean,
    focus: FocusRequester?,
    /** Where Down goes from the last row: the button, not the nearer Cancel. */
    down: FocusRequester?,
    onToggle: () -> Unit,
) {
    val shape = RoundedCornerShape(Marquee.radius.card)
    Row(
        Modifier
            .padding(horizontal = Marquee.safe.side)
            .widthIn(max = 900.dp)
            .fillMaxWidth()
            .marqueeFocus(Marquee.radius.card, scale = 1.01f)
            .clip(shape)
            .background(Marquee.colors.glass)
            .border(1.dp, Marquee.colors.edge, shape)
            .semantics(mergeDescendants = true) { stateDescription = if (going) "Removed with it" else "Kept" }
            .then(if (focus != null) Modifier.focusRequester(focus) else Modifier)
            .focusProperties { down?.let { this.down = it } }
            .focusClickable(onToggle, androidx.compose.ui.semantics.Role.Checkbox)
            .padding(16.dp),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(
            if (going) Icons.CircleCheck else Icons.Circle,
            tint = if (going) Marquee.colors.bad else Marquee.colors.muted,
        )
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            MqText(row.name, role = TextRole.Headline, weight = FontWeight.Bold)
            MqText(row.detail, role = TextRole.Caption, color = Marquee.colors.muted)
        }
    }
}

// Pair this device (the joining side)

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

    /** The view whose join went to the sync controller: each join is handed over once. */
    private var handed: PairingView? = null

    fun start(
        server: String,
        name: String,
    ) {
        hand()
        view?.close()
        watch?.cancel()
        val next = session.pairing(server, name)
        view = next
        watch =
            viewModelScope.launch {
                snapshotFlow { next.snapshot }.filterIsInstance<PairingSnapshot.Joined>().first()
                hand()
            }
    }

    /** Gives a joined record to the app's sync controller, which stores it whatever this page does next. */
    private fun hand() {
        val current = view ?: return
        val joined = current.snapshot as? PairingSnapshot.Joined ?: return
        if (handed === current) return
        handed = current
        sync.didJoin(joined.record)
    }

    override fun onCleared() {
        // Leaving before the watcher ran (Back right after Joined) still hands the join over.
        hand()
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
    // A remembered address with a user name or password (an older version saved it) is not used.
    val known = remember(context) { context.knownServer() }
    var server by rememberSaveable { mutableStateOf(known) }
    var name by rememberSaveable { mutableStateOf(context.deviceName(tv)) }
    // The fields are open only while no server is known: with one, the code shows at once.
    var advanced by rememberSaveable { mutableStateOf(known.isEmpty()) }
    // A device that already syncs asks before a pairing replaces its account (checked once, at open).
    val wasJoined = remember { sync.joined && !sync.joining }
    var replaceOk by rememberSaveable { mutableStateOf(!wasJoined) }
    var badAddress by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(replaceOk) {
        if (replaceOk && known.isNotEmpty() && holder.view == null) holder.start(known, name)
    }
    val begin = {
        val s = server.trim()
        // An address with a user name or password is never kept or sent.
        badAddress = s.isNotEmpty() && !SyncRules.plainAddress(s)
        if (s.isNotEmpty() && !badAddress) {
            val n = name.trim().ifEmpty { "Marquee" }
            context.rememberJoin(s, n)
            holder.start(s, n)
        }
    }
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                BrowseHeader("Sync", SyncCopy.PAIR_TITLE)
                MqText(
                    "Joins your sync account so library, progress and addons follow you. Nothing is kept until both " +
                        "screens show the same code.",
                    modifier = Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 900.dp),
                    role = TextRole.Callout,
                    color = Marquee.colors.muted,
                )
            }
        }
        if (!replaceOk) {
            block("replace") {
                Callout(Icons.TriangleAlert, "${SyncCopy.REPLACE_ASK}. ${SyncCopy.REPLACE_TEXT}")
                Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    SecondaryButton(SyncCopy.REPLACE_BUTTON, { replaceOk = true }, icon = Icons.Check)
                    SecondaryButton(SyncCopy.CANCEL, onDone)
                }
            }
            return@LazyColumn
        }
        holder.view?.let { view ->
            item(key = "session") {
                Column(Modifier.padding(horizontal = Marquee.safe.side)) {
                    Session(view, sync, server, onAgain = begin, onDone = onDone)
                }
            }
        }
        // Once joined, the page is the result and Done: no second pairing starts from it.
        val joined = holder.view?.snapshot is PairingSnapshot.Joined
        if (known.isEmpty() && !joined) {
            item(key = "once") { Note("Enter your sync server once; this device remembers it.") }
        }
        if (!joined) {
            item(key = "advanced") {
                Column(
                    Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 900.dp),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
                ) {
                    SecondaryButton("Advanced", { advanced = !advanced }, icon = Icons.Settings, selected = advanced)
                    if (advanced) {
                        Labeled(SyncCopy.SERVER) {
                            MqTextField(
                                server,
                                { server = it },
                                label = SyncCopy.SERVER,
                                placeholder = SyncCopy.SERVER_HINT,
                                keyboardOptions =
                                    KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next),
                            )
                        }
                        if (badAddress) Note(SyncCopy.BAD_ADDRESS, failed = true, side = 0.dp)
                        Labeled(SyncCopy.DEVICE) {
                            MqTextField(name, { name = it }, label = SyncCopy.DEVICE, placeholder = "Marquee")
                        }
                        PrimaryButton("Show a code", begin, icon = Icons.QrCode, enabled = server.isNotBlank())
                    }
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
    val context = LocalContext.current
    val status = view.status
    // Each step that asks something takes focus to its answer: on a TV the remote is never left on
    // the form below while "Yes, they match" waits.
    val answer = remember { FocusRequester() }
    var copied by remember(view) { mutableStateOf(false) }
    // Named by hand, not by class name: a release build shortens class names.
    val step =
        if (status is ViewStatus.Failed) {
            "could not start"
        } else {
            when (view.snapshot) {
                is PairingSnapshot.Confirm -> "confirm"
                is PairingSnapshot.Joined -> if (sync.joining) null else "joined"
                PairingSnapshot.Expired -> "expired"
                is PairingSnapshot.Failed -> "stopped"
                else -> null
            }
        }
    LaunchedEffect(step) {
        if (tv && step != null) {
            withFrameNanos { }
            runCatching { answer.requestFocus() }
        }
    }
    if (status is ViewStatus.Failed) {
        StateMessage(
            "Pairing could not start",
            SyncCopy.pairingProblem(status.reason),
            icon = Icons.CircleAlert,
            actionLabel = "Start again",
            onAction = onAgain,
            actionFocus = answer,
        )
        return
    }
    when (val p = view.snapshot) {
        null, PairingSnapshot.Starting -> {
            StatusWord("Contacting ${SyncRules.hostOf(server)}", Status.Warn)
        }

        is PairingSnapshot.Waiting -> {
            val until = DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(p.expiresMs))
            val steps = @Composable {
                Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                    MqText(
                        "On a device that already syncs, open Settings, then Devices and sync, then Add a device, " +
                            "and scan this code.",
                        role = TextRole.Callout,
                        modifier = Modifier.widthIn(max = 720.dp),
                    )
                    StatusWord("Waiting for the other device. The code works until $until.", Status.Warn)
                    // A phone can hand the code over as text; it holds public keys only.
                    if (!tv) {
                        SecondaryButton(if (copied) SyncCopy.COPIED else SyncCopy.COPY_CODE, {
                            SyncClipboard.copyCode(context, p.qr)
                            copied = true
                        }, icon = if (copied) Icons.Check else Icons.Copy)
                        MqText(SyncCopy.COPY_HELP, role = TextRole.Caption, color = Marquee.colors.muted)
                    }
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
                    PrimaryButton("Yes, they match", view::confirm, icon = Icons.Check, focusRequester = answer)
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
                // Done waits until the join is stored on this device.
                if (sync.joining) {
                    StatusWord("Saving the account on this device", Status.Warn)
                } else {
                    SyncState(sync)
                    PrimaryButton("Done", onDone, focusRequester = answer)
                }
            }
        }

        PairingSnapshot.Expired -> {
            StateMessage(
                "The code expired",
                "Start again for a new code.",
                icon = Icons.Clock,
                actionLabel = "Start again",
                onAction = onAgain,
                actionFocus = answer,
            )
        }

        is PairingSnapshot.Failed -> {
            StateMessage(
                "Pairing stopped",
                SyncCopy.pairingProblem(p.reason),
                icon = Icons.CircleAlert,
                actionLabel = "Start again",
                onAction = onAgain,
                actionFocus = answer,
            )
        }
    }
}

/** How sync stands: running, what went wrong, or when it last went through, with what a round left to say. */
@Composable
internal fun SyncState(sync: SyncController) {
    val problem = sync.problem
    val last = sync.lastSynced
    when {
        sync.syncing -> StatusWord(SyncCopy.SYNCING, Status.Warn)
        problem != null -> Note(problem.words, failed = true, side = 0.dp)
        last != null -> StatusWord("Synced at ${clock(last)}", Status.Ok)
        else -> StatusWord(SyncCopy.NOT_SYNCED, Status.Idle)
    }
    sync.lastReport?.let { report ->
        for (note in SyncCopy.notes(report)) MqText(note, role = TextRole.Caption, color = Marquee.colors.muted)
    }
}
