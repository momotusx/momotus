package app.marquee.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.AccountCopy
import app.marquee.core.AccountRules
import app.marquee.core.AccountRules.Act
import app.marquee.core.AccountRules.Card
import app.marquee.core.SyncCopy
import app.marquee.core.SyncRules
import app.marquee.core.model.IdentityLink
import app.marquee.core.model.PairingSnapshot
import app.marquee.core.session.CoreSession
import app.marquee.core.sync.AccountSignIn
import app.marquee.core.sync.AccountSignIn.Step
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.MqTextField
import app.marquee.design.PrimaryButton
import app.marquee.design.SecondaryButton
import app.marquee.design.Spinner
import app.marquee.design.Status
import app.marquee.design.SwitchRow
import app.marquee.design.TextRole
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import java.text.DateFormat
import java.util.Date

// Accounts on Android (plans/accounts.md 4.1 to 4.3, 5): the phone's sign-in, the TV's first run, Account's
// sign-in rows and Home's account card. Every word is apps/shared/account-words.json's (AccountCopy), and every
// piece is one the app already has, so the v3 design restyles all of it.

internal fun accountDate(ms: Long): String = DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(ms))

/** A failure's words and its buttons; [onAct] does what each one does. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun FailureNote(
    failure: AccountRules.Failure,
    onAct: (Act) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        Callout(Icons.CircleAlert, failure.text)
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            failure.actions.forEachIndexed { i, a ->
                if (i == 0) {
                    PrimaryButton(a.label, { onAct(a.act) }, focusRequester = stepFocus())
                } else {
                    SecondaryButton(a.label, { onAct(a.act) })
                }
            }
        }
    }
}

/** A pairing QR inside the sign-in (found, and "I already use Marquee"): [onJoined] once this device joined. */
@Composable
private fun SignInPairing(
    session: CoreSession,
    sync: SyncController,
    server: String,
    onJoined: () -> Unit,
) {
    val context = LocalContext.current
    val holder =
        viewModel<PairingHolder>(
            key = "sign-in-pairing",
            factory =
                viewModelFactory {
                    initializer {
                        PairingHolder(
                            session,
                            sync,
                            deviceForm(context, tv = false),
                        )
                    }
                },
        )
    val name = remember { context.deviceName(tv = false) }
    LaunchedEffect(server) { if (holder.view == null) holder.start(server, name) }
    val view = holder.view ?: return
    val joined = view.snapshot is PairingSnapshot.Joined && !sync.joining
    LaunchedEffect(joined) {
        if (joined) {
            context.rememberJoin(server, name)
            onJoined()
        }
    }
    Session(view, sync, server, onAgain = { holder.start(server, name) }, onDone = onJoined)
}

/**
 * The phone's sign-in (4.1): Continue with each provider the relay offers (none: today's doors at once), Other
 * ways to sign in, and Not now; then `found` (the QR, a recovery kit, Start over) or `new` (Create, or I already
 * use Marquee), and the passkey or second-way step. [onCreated] goes on to the first-run steps, [onIn] past them.
 */
@Composable
fun SignInScreen(
    flows: AccountFlows,
    session: CoreSession,
    sync: SyncController,
    /** From the first screen; else a device with a library signs in, which the screen says first. */
    firstRun: Boolean,
    onNotNow: () -> Unit,
    onScan: () -> Unit,
    onKit: () -> Unit,
    /** A sync service the user typed that offers no sign-in: today's doors on Account. */
    onNoProviders: () -> Unit,
    /** [kit]: the second-way step's "Use a recovery kit instead", whose page comes next. */
    onCreated: (kit: Boolean) -> Unit,
    onIn: () -> Unit,
) {
    val context = LocalContext.current
    LaunchedEffect(Unit) { if (flows.signIn == null) flows.startSignIn(context) }
    // Done ends it: the screen leaves meanwhile.
    val signIn = flows.signIn ?: return
    LifecycleResumeEffect(signIn) {
        signIn.resumed()
        onPauseOrDispose { }
    }
    var own by rememberSaveable { mutableStateOf(false) }
    var kitNext by remember { mutableStateOf(false) }
    val notNow = {
        flows.endSignIn()
        onNotNow()
    }
    val step = signIn.step
    LaunchedEffect(step) {
        when (step) {
            // A relay with no provider: the sign-in screen is today's doors (4.1.1).
            is Step.Start -> {
                if (step.providers.isEmpty() && signIn.failure == null) notNow()
            }

            is Step.Done -> {
                context.rememberJoin(flows.server(context), context.deviceName(tv = false))
                flows.endSignIn()
                if (step.created) onCreated(kitNext) else onIn()
            }

            else -> {
                Unit
            }
        }
    }
    val act: (Act) -> Unit = { a ->
        when (a) {
            Act.NotNow -> notNow()

            Act.Kit -> onKit()

            // On `found` the code is on this screen already.
            Act.Scan -> if (step !is Step.Found) onScan()

            Act.Other -> own = false

            else -> signIn.act(a)
        }
    }
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") {
            BrowseHeader("Marquee", if (step == Step.New) AccountCopy.CREATE_ACCOUNT else AccountCopy.SIGN_IN_TITLE)
        }
        if (!firstRun && step is Step.Start) {
            block("added") { MqText(AccountCopy.LIBRARY_ADDED, color = Marquee.colors.muted) }
        }
        signIn.failure?.let { f -> block("failure") { FailureNote(f, act) } }
        when (step) {
            Step.Loading, is Step.Working, is Step.Done -> {
                block("working") { Spinner() }
            }

            is Step.Start -> {
                if (own) {
                    block("own") {
                        OwnServer(flows, onProviders = {
                            own = false
                            flows.endSignIn()
                            flows.startSignIn(context)
                        }, onNone = onNoProviders)
                    }
                } else {
                    providers(step.providers, signIn::choose)
                    otherWays(flows.passkeys, signIn::usePasskey, onScan, onKit, onOwnServer = { own = true })
                    block("not-now") { SecondaryButton(AccountCopy.NOT_NOW, notNow) }
                }
            }

            // The words and buttons are the failure's: "Scan this code with a device that's signed in."
            is Step.Found -> {
                block("found") {
                    SignInPairing(session, sync, flows.server(context), onJoined = signIn::joined)
                }
            }

            Step.New -> {
                block("new") { NewAccount(session, sync, flows.server(context), signIn) }
            }

            Step.StartOverAsk -> {
                block("start-over") {
                    Callout(Icons.TriangleAlert, AccountCopy.START_OVER_WARNING)
                    PrimaryButton(AccountCopy.START_OVER, signIn::confirmStartOver, focusRequester = stepFocus())
                    SecondaryButton(SyncCopy.CANCEL, { signIn.act(Act.Cancel) })
                }
            }

            Step.SecondWay -> {
                block("second-way") {
                    val (again, kit, later) = AccountCopy.NO_SECOND_WAY_IN_ACTIONS
                    Callout(Icons.ShieldAlert, AccountCopy.NO_SECOND_WAY_IN)
                    PrimaryButton(again, signIn::savePasskeyAgain, focusRequester = stepFocus())
                    SecondaryButton(kit, {
                        kitNext = true
                        signIn.later()
                    })
                    SecondaryButton(later, signIn::later)
                }
            }
        }
    }
}

/** `new`: Create your Marquee account, or the pairing QR for "I already use Marquee on another device". */
@Composable
private fun NewAccount(
    session: CoreSession,
    sync: SyncController,
    server: String,
    signIn: AccountSignIn,
) {
    var pairing by rememberSaveable { mutableStateOf(false) }
    if (pairing) {
        SignInPairing(session, sync, server, onJoined = signIn::joined)
        SecondaryButton(SyncCopy.CANCEL, { pairing = false })
    } else {
        PrimaryButton(AccountCopy.CREATE_ACCOUNT, signIn::create, icon = Icons.Plus, focusRequester = stepFocus())
        SecondaryButton(AccountCopy.ALREADY_USE, { pairing = true }, icon = Icons.QrCode)
    }
}

/** Use my own sync server: one that offers sign-in starts the sign-in there, one with none goes to Account. */
@Composable
private fun OwnServer(
    flows: AccountFlows,
    onProviders: () -> Unit,
    onNone: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var server by rememberSaveable { mutableStateOf(context.knownServer()) }
    var said by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    MqText(AccountCopy.USE_OWN_SERVER, role = TextRole.Title3)
    MqTextField(
        server,
        { server = it },
        label = AccountCopy.OWN_SERVER_LABEL,
        placeholder = SyncCopy.SERVER_HINT,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
    )
    said?.let { Said(it, Status.Bad) }
    PrimaryButton(AccountCopy.OWN_SERVER_USE, {
        val s = server.trim()
        // An address with a user name or password is never kept or sent.
        if (s.isEmpty() || !SyncRules.plainAddress(s)) {
            said = SyncCopy.BAD_ADDRESS
        } else if (!busy) {
            busy = true
            said = null
            scope.launch {
                val offered = flows.providers(s)
                context.rememberJoin(s, context.deviceName(tv = false))
                busy = false
                if (offered.isEmpty()) onNone() else onProviders()
            }
        }
    }, enabled = !busy, focusRequester = stepFocus())
}

private fun LazyListScope.providers(
    providers: List<String>,
    choose: (String) -> Unit,
) {
    providers.forEachIndexed { i, p ->
        block("provider-$p") {
            if (i == 0) {
                PrimaryButton(AccountRules.continueWith(p), { choose(p) }, focusRequester = stepFocus())
            } else {
                SecondaryButton(AccountRules.continueWith(p), { choose(p) })
            }
        }
    }
}

/** Other ways to sign in (4.1.1): a passkey, a signed-in device's scan, a recovery kit, one's own sync server. */
private fun LazyListScope.otherWays(
    passkeys: Boolean,
    usePasskey: () -> Unit,
    onScan: () -> Unit,
    onKit: () -> Unit,
    onOwnServer: () -> Unit,
) {
    item(key = "other") { Kicker(AccountCopy.OTHER_WAYS) }
    // Passkeys live on the link host: a build without one has none.
    if (passkeys) {
        item(key = "passkey") { LinkRow(Icons.KeyRound, AccountCopy.USE_PASSKEY, "", usePasskey) }
    }
    item(key = "scan") { LinkRow(Icons.QrCode, AccountCopy.SCAN_FROM_DEVICE, "", onScan) }
    item(key = "kit") { LinkRow(Icons.LifeBuoy, AccountCopy.USE_RECOVERY_KIT, "", onKit) }
    item(key = "own") { LinkRow(Icons.Settings, AccountCopy.USE_OWN_SERVER, "", onOwnServer) }
}

/**
 * A TV's first run (4.2, D6): Sign in with your phone, the pairing code and, once a phone approved it, the six
 * digits; Use without an account leads to the doors, Use a recovery kit stays for households with no phone.
 */
@Composable
fun TvSignInScreen(
    session: CoreSession,
    sync: SyncController,
    server: String,
    onDoors: () -> Unit,
    onKit: () -> Unit,
    onIn: () -> Unit,
) {
    val context = LocalContext.current
    val holder =
        viewModel<PairingHolder>(
            key = "tv-sign-in",
            factory = viewModelFactory { initializer { PairingHolder(session, sync, "tv") } },
        )
    val name = remember { context.deviceName(tv = true) }
    LaunchedEffect(server) {
        if (holder.view == null) {
            context.rememberJoin(server, name)
            holder.start(server, name)
        }
    }
    val joined = holder.view?.snapshot is PairingSnapshot.Joined
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") { BrowseHeader("Marquee", AccountCopy.TV_SIGN_IN) }
        block("how") { MqText(AccountCopy.TV_SIGN_IN_HOW, color = Marquee.colors.muted) }
        holder.view?.let { view ->
            item(key = "session") {
                Column(Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 1100.dp)) {
                    Session(view, sync, server, onAgain = { holder.start(server, name) }, onDone = onIn)
                }
            }
        }
        // A TV that joined skips the doors.
        if (!joined) {
            item(key = "without") { LinkRow(Icons.House, AccountCopy.TV_WITHOUT_ACCOUNT, "", onDoors) }
            item(key = "kit") { LinkRow(Icons.LifeBuoy, AccountCopy.USE_RECOVERY_KIT, "", onKit) }
        }
    }
}

/**
 * Account's sign-in rows (4.3, D16, D17): Signed in with (each link and its date; Unlink and Link another on the
 * owner, behind the screen lock), and Save a passkey here (the owner, on a phone or tablet).
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun AccountRows(
    flows: AccountFlows,
    sync: SyncController,
    rows: AccountRules.Rows,
) {
    val actions = flows.actions
    val tv = Marquee.form == Form.Tv
    val scope = rememberCoroutineScope()
    val owner = rememberOwnerCheck()
    var error by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(sync.joined) { if (sync.joined) actions.load() }
    val links = actions.links.orEmpty()
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        if (actions.providers.isNotEmpty() || links.isNotEmpty()) {
            Kicker(AccountCopy.SIGNED_IN_WITH, side = 0.dp)
            for (link in links) {
                LinkLine(
                    link,
                    // Unlinking changes how the account is reached: the screen lock first (Apple's revoke page
                    // follows for an Apple link).
                    onUnlink =
                        if (rows.links) {
                            {
                                scope.launch {
                                    error =
                                        guarded(owner, tv, AccountCopy.UNLINK, askAgain = null) {
                                            actions.unlink(link)
                                        }
                                }
                            }
                        } else {
                            null
                        },
                )
            }
            if (rows.links && links.size == 1) {
                MqText(AccountCopy.UNLINK_LAST, role = TextRole.Caption, color = Marquee.colors.muted)
            }
            val linked = links.map { it.provider }.toSet()
            val more = actions.providers.filter { it !in linked }
            if (rows.links && more.isNotEmpty()) {
                MqText(AccountCopy.LINK_ANOTHER, role = TextRole.Callout)
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                ) {
                    for (p in more) {
                        SecondaryButton(AccountRules.continueWith(p), { actions.link(p) }, icon = Icons.Link)
                    }
                }
            }
        }
        // Delete account's own failure shows under it.
        actions.failure?.takeUnless { actions.deleting }?.let { f ->
            FailureNote(f) { a ->
                when (a) {
                    Act.Anyway, Act.Move -> actions.goOn()
                    else -> actions.dismiss()
                }
            }
        }
        actions.said?.takeUnless { actions.deleting }?.let { Said(it, Status.Warn) }
        error?.let { Said(it, Status.Bad) }
        if (rows.savePasskey && flows.passkeys) {
            SecondaryButton(
                AccountCopy.SAVE_PASSKEY_HERE,
                { actions.savePasskeyHere() },
                icon = Icons.KeyRound,
                busy = actions.busy,
            )
        }
    }
}

@Composable
private fun LinkLine(
    link: IdentityLink,
    /** The owner's Unlink; null off the owner (D16). */
    onUnlink: (() -> Unit)?,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        MqText(AccountRules.providerName(link.provider), role = TextRole.Headline)
        MqText(
            AccountRules.fill(AccountCopy.LINKED_ON, mapOf("date" to accountDate(link.createdMs))),
            role = TextRole.Caption,
            color = Marquee.colors.muted,
        )
        onUnlink?.let { SecondaryButton(AccountCopy.UNLINK, it, icon = Icons.Unlink) }
    }
}

/**
 * Home's one account card (4.3), by [AccountRules.homeCard]'s priority: a scheduled deletion with Cancel, the
 * account deleted elsewhere, an announcement (OK on the owner only), the nudge for a second way in.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun AccountCard(
    flows: AccountFlows,
    sync: SyncController,
    card: Card,
    onSignIn: () -> Unit,
    onMakeKit: () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val scope = rememberCoroutineScope()
    var error by remember { mutableStateOf<String?>(null) }
    var removing by remember { mutableStateOf(false) }
    var removingPasskey by remember { mutableStateOf(false) }
    var debrid by remember { mutableStateOf(true) }
    val go: (suspend () -> Unit) -> Unit = { work ->
        error = null
        scope.launch {
            error =
                try {
                    work()
                    null
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    wordsOf(e, SyncCopy.Action.Remove)
                }
        }
    }
    Column(
        Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 900.dp),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        Callout(
            if (card is Card.Notice) Icons.Info else Icons.ShieldAlert,
            when (card) {
                is Card.Deletion -> card.text
                is Card.Gone -> card.text
                is Card.Notice -> card.text
                is Card.SecondWay -> "${card.text}. ${card.detail}"
            },
        )
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            when (card) {
                is Card.Deletion -> {
                    PrimaryButton(AccountCopy.DELETION_CANCEL, { go { sync.cancelDeletion() } })
                    card.provider?.let { p ->
                        SecondaryButton(
                            AccountRules.fill(
                                AccountCopy.DELETION_CANCEL_UNLINK,
                                mapOf("provider" to AccountRules.providerName(p)),
                            ),
                            { go { sync.cancelDeletion(unlink = true) } },
                        )
                    }
                }

                is Card.Gone -> {
                    val (signIn, remove, ok) = AccountCopy.ACCOUNT_GONE_ACTIONS
                    if (!removing) {
                        PrimaryButton(signIn, onSignIn)
                        if (card.remove) SecondaryButton(remove, { removing = true })
                        SecondaryButton(ok, sync::dismissGone)
                    }
                }

                is Card.Notice -> {
                    if (!removingPasskey) {
                        if (card.notYou && card.member != null) {
                            SecondaryButton(AccountCopy.ANNOUNCE_NOT_YOU, { removingPasskey = true })
                        }
                        if (card.ok) {
                            PrimaryButton(AccountCopy.ANNOUNCE_OK, { go { sync.ackAnnouncements(listOf(card.id)) } })
                        }
                    }
                }

                is Card.SecondWay -> {
                    // Kits are made on a phone or tablet; a TV's Account says where.
                    if (!tv) PrimaryButton(SyncCopy.KIT_TITLE, onMakeKit, icon = Icons.KeyRound)
                }
            }
        }
        if (removing && card is Card.Gone) {
            // D13's ask on the account-deleted card: the owner's, with the debrid line ticked.
            MqText(AccountCopy.REMOVE_LIBRARY_ASK, role = TextRole.Callout)
            SwitchRow(debrid, { debrid = !debrid }) { MqText(AccountCopy.REMOVE_LIBRARY_DEBRID) }
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            ) {
                SecondaryButton(
                    AccountCopy.ACCOUNT_GONE_ACTIONS[1],
                    { if (!flows.removal.running) go { flows.removeHere(debrid) } },
                    icon = Icons.Trash,
                    busy = flows.removal.running,
                )
                SecondaryButton(SyncCopy.CANCEL, { removing = false }, focusRequester = stepFocus())
            }
        }
        val member = (card as? Card.Notice)?.member
        if (removingPasskey && card is Card.Notice && member != null) {
            MqText(AccountCopy.REMOVE_PASSKEY_ASK, role = TextRole.Callout)
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            ) {
                SecondaryButton(SyncCopy.REMOVE, {
                    go {
                        sync.removeDevice(member)
                        sync.ackAnnouncements(listOf(card.id))
                        removingPasskey = false
                    }
                }, icon = Icons.Trash)
                SecondaryButton(SyncCopy.CANCEL, { removingPasskey = false }, focusRequester = stepFocus())
            }
        }
        if (flows.removal.running) Said(AccountCopy.WIPING, Status.Warn)
        flows.removal.problem?.let { Said(it, Status.Bad) }
        error?.let { Said(it, Status.Bad) }
    }
}
