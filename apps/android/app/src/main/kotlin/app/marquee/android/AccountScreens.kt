package app.marquee.android

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.autofill.ContentType
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
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
import app.marquee.core.FirstRunCopy
import app.marquee.core.SyncCopy
import app.marquee.core.SyncRules
import app.marquee.core.model.IdentityLink
import app.marquee.core.model.PairingSnapshot
import app.marquee.core.session.CoreSession
import app.marquee.core.sync.AccountSignIn
import app.marquee.core.sync.AccountSignIn.Step
import app.marquee.core.sync.EmailCode
import app.marquee.core.views.ViewStatus
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.LiveSpoken
import app.marquee.design.Marks
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.MqTextField
import app.marquee.design.PrimaryButton
import app.marquee.design.Qr
import app.marquee.design.SecondaryButton
import app.marquee.design.Spinner
import app.marquee.design.Status
import app.marquee.design.SwitchRow
import app.marquee.design.TextRole
import app.marquee.design.focusClickable
import app.marquee.design.focusSoon
import app.marquee.design.marqueeFocus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
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
    /** Read out when it appears (a failure with no button to take the focus). */
    spoken: Boolean = false,
    onAct: (Act) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        if (spoken) {
            LiveSpoken(failure.text) { Callout(Icons.CircleAlert, failure.text) }
        } else {
            Callout(Icons.CircleAlert, failure.text)
        }
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
    val joined = view.current() is PairingSnapshot.Joined && !sync.joining
    LaunchedEffect(joined) {
        if (joined) {
            context.rememberJoin(server, name)
            onJoined()
        }
    }
    Session(view, sync, onDone = onJoined, how = AccountCopy.PHONE_SCAN_HOW)
}

/**
 * The phone's sign-in (4.1): Continue with each provider the relay offers (none: Continue, plans/tv-qr-login.md 5.4), Other
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
    /** From "I already use Momotus": another device's way first, and its line (plans/first-screens.md 3.3). */
    returning: Boolean = false,
    /** The grabber's drag down: back, as the system Back. */
    onBack: () -> Unit = {},
    /** The provider pressed, for Who's watching?'s line after a join. */
    onProvider: (String) -> Unit = {},
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
    val tv = Marquee.form == Form.Tv
    // Back on the email screen: the code stage to the address stage, the address stage to Sign in.
    BackHandler(enabled = step == Step.Email) { signIn.back() }
    LaunchedEffect(step) {
        when (step) {
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
            Act.NotNow -> {
                notNow()
            }

            Act.Kit -> {
                onKit()
            }

            // On `found` the code is on this screen already.
            Act.Scan -> {
                if (step !is Step.Found) onScan()
            }

            Act.Other -> {
                own = false
                signIn.act(a)
            }

            else -> {
                signIn.act(a)
            }
        }
    }
    var another by rememberSaveable { mutableStateOf(false) }
    var others by rememberSaveable { mutableStateOf(false) }
    val choose = { p: String ->
        onProvider(p)
        signIn.choose(p)
    }
    // A stage after the account was made (the passkey nag) answers Back as Later; a stage at work holds it, so Back
    // never leaves the sheet with an account the first run has not heard of.
    BackHandler(enabled = step is Step.SecondWay) { signIn.later() }
    BackHandler(enabled = step is Step.Working || step == Step.Loading || step is Step.Done) {}
    LoginSheet(
        if (step == Step.New) AccountCopy.CREATE_ACCOUNT else AccountCopy.SIGN_IN_TITLE,
        start = step is Step.Start,
        returning = returning,
        onNotNow = notNow.takeIf { step is Step.Start && !own },
        onBack = onBack,
    ) {
        if (!firstRun && step is Step.Start) {
            block("added") { MqText(AccountCopy.LIBRARY_ADDED, color = Marquee.colors.muted) }
        }
        signIn.failure?.let { f -> block("failure") { FailureNote(f, onAct = act) } }
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
                    loginStart(
                        step.providers,
                        email = step.email && !tv,
                        continueHere = signIn::continueHere.takeIf { signIn.failure == null },
                        returning = returning,
                        another = another,
                        onAnother = { another = !another },
                        pairing = { SignInPairing(session, sync, flows.server(context), onJoined = signIn::joined) },
                        choose = choose,
                        onEmail = { signIn.choose(AccountRules.EMAIL) },
                        others = others,
                        onOthers = { others = !others },
                    ) { otherWays(flows.passkeys, signIn::usePasskey, onScan, onKit, onOwnServer = { own = true }) }
                }
            }

            Step.Email -> {
                block("email") { EmailStages(signIn.email, act) }
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

/**
 * `new`: Create your Marquee account, or the pairing QR for "I already use Marquee on another device". After a
 * provider sign-in the account is created at once, as any app's does; the choice stays only when that failed.
 */
@Composable
private fun NewAccount(
    session: CoreSession,
    sync: SyncController,
    server: String,
    signIn: AccountSignIn,
) {
    var pairing by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(signIn.held) {
        if (signIn.held != null && signIn.failure == null) signIn.create()
    }
    if (pairing) {
        SignInPairing(session, sync, server, onJoined = signIn::joined)
        SecondaryButton(SyncCopy.CANCEL, { pairing = false })
    } else {
        MqText(AccountCopy.NO_ACCOUNT_YET, color = Marquee.colors.muted)
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

/**
 * Continue with a provider as the provider draws it (plans/first-screens.md 3.3): Google white with its "G", Facebook
 * blue with its mark; any other a secondary pill. The label is large text (20 bold), so white on Facebook's blue keeps
 * the 3:1 large text needs.
 */
@Composable
internal fun ProviderButton(
    provider: String,
    onClick: () -> Unit,
    focusRequester: FocusRequester? = null,
) {
    val label = AccountRules.continueWith(provider)
    val (mark, fill) =
        when (provider) {
            "google" -> Marks.Google to Color.White
            "facebook" -> Marks.Facebook to Color(Marks.FACEBOOK_BLUE)
            else -> return SecondaryButton(label, onClick, Modifier.fillMaxWidth(), focusRequester = focusRequester)
        }
    val ink = if (provider == "google") Marquee.colors.onPrimary else Color.White
    val tv = Marquee.form == Form.Tv
    Row(
        Modifier
            .fillMaxWidth()
            .then(focusRequester?.let { Modifier.focusRequester(it) } ?: Modifier)
            .marqueeFocus(Marquee.radius.pill)
            .clip(RoundedCornerShape(Marquee.radius.pill))
            .background(fill)
            .focusClickable(onClick, Role.Button)
            .heightIn(min = if (tv) 72.dp else 52.dp)
            .padding(horizontal = 20.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Image(mark, contentDescription = null, modifier = Modifier.size(if (tv) 32.dp else 22.dp))
        MqText(label, role = TextRole.Title3, weight = FontWeight.Bold, color = ink, maxLines = 2)
    }
}

/**
 * The Login sheet over the still wall, on first run and from Settings alike (plans/first-screens.md 3.3, D10): its
 * title, Not now at the right on the first stage ([onNotNow] null on the others), the line ([returning]: another
 * device's), the grabber whose drag goes back, and the stage's [items].
 */
@Composable
internal fun LoginSheet(
    title: String,
    start: Boolean,
    returning: Boolean,
    onNotNow: (() -> Unit)?,
    onBack: () -> Unit,
    items: LazyListScope.() -> Unit,
) {
    FirstSheet(
        title,
        width = 860.dp,
        corner = onNotNow?.let { { SheetLink(AccountCopy.NOT_NOW, it) } },
        line =
            when {
                !start -> null
                returning -> FirstRunCopy.LOGIN_RETURNING
                else -> FirstRunCopy.LOGIN_LINE
            },
        onDrag = onBack,
        scroll = false,
        pad = false,
    ) {
        LazyColumn(
            Modifier.focusRestorer(),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
            content = items,
        )
    }
}

/**
 * The Login sheet's first stage (3.3): each provider as it draws itself, Continue with email (the primary when it
 * is the only way; [email] is false on a TV, D6), Continue when the service offers no provider ([continueHere], none
 * while its failure shows), Use another device on the phone ([returning]: first) with the pairing code under it,
 * and Other ways to sign in, a 48 dp link that opens [otherWays].
 */
internal fun LazyListScope.loginStart(
    offered: List<String>,
    email: Boolean,
    continueHere: (() -> Unit)?,
    returning: Boolean,
    another: Boolean,
    onAnother: () -> Unit,
    pairing: @Composable () -> Unit,
    choose: (String) -> Unit,
    onEmail: () -> Unit,
    others: Boolean,
    onOthers: () -> Unit,
    otherWays: LazyListScope.() -> Unit,
) {
    val device: LazyListScope.() -> Unit = {
        block("another") {
            // The phone only: TV and wide show the QR itself.
            if (Marquee.form != Form.Tv) {
                SecondaryButton(
                    FirstRunCopy.USE_ANOTHER_DEVICE,
                    onAnother,
                    Modifier.fillMaxWidth(),
                    icon = Icons.QrCode,
                    selected = another,
                )
                if (another) pairing()
            }
        }
    }
    // "I already use Momotus": the other device's way first, one more press on that path.
    if (returning) device()
    providers(offered, choose)
    if (email) {
        block("email-row") {
            if (offered.isEmpty()) {
                PrimaryButton(AccountCopy.CONTINUE_WITH_EMAIL, onEmail, icon = Icons.Mail, focusRequester = stepFocus())
            } else {
                SecondaryButton(AccountCopy.CONTINUE_WITH_EMAIL, onEmail, icon = Icons.Mail)
            }
        }
    }
    // No provider on this sync service: Continue, a passkey here or a new account (plans/tv-qr-login.md 5.4); under
    // email when email is the way in.
    if (offered.isEmpty() && continueHere != null) {
        block("continue") {
            if (email) {
                SecondaryButton(AccountCopy.CONTINUE, continueHere)
            } else {
                PrimaryButton(AccountCopy.CONTINUE, continueHere, focusRequester = stepFocus())
            }
        }
    }
    if (!returning) device()
    block("other-link") { SheetLink(AccountCopy.OTHER_WAYS, onOthers) }
    if (others) otherWays()
}

private fun LazyListScope.providers(
    providers: List<String>,
    choose: (String) -> Unit,
) {
    providers.forEachIndexed { i, p ->
        block("provider-$p") {
            if (i == 0) {
                ProviderButton(p, { choose(p) }, stepFocus())
            } else {
                ProviderButton(p, { choose(p) })
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
 * A TV's Login sheet over the still wall (plans/first-screens.md 3.3, D3; plans/tv-qr-login.md 5.5): the live QR, "Sign
 * in to Momotus" and its line, the three steps, the waiting line, then Not now (the first focus) and Other ways to sign
 * in (Account: a recovery kit, one's own server; no email on a TV). A phone on the TV's network signs it in with one
 * tap. With no QR (no sync service, a code that did not come, or one the service refused) the sheet says so in one
 * line with the reason under it, Try again and Not now, Not now focused.
 */
@Composable
fun TvSignInScreen(
    session: CoreSession,
    sync: SyncController,
    server: String,
    onNotNow: () -> Unit,
    onOther: () -> Unit,
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
        if (holder.view == null && server.isNotEmpty()) {
            context.rememberJoin(server, name)
            holder.start(server, name)
        }
    }
    val view = holder.view
    val p = view?.current()
    // A code that has not come after PAIRING_LATE_MS is no code: the sheet says so, and Try again waits that long again.
    var late by remember(view) { mutableStateOf(false) }
    LaunchedEffect(view) {
        delay(SyncRules.PAIRING_LATE_MS)
        late = true
    }
    val failed = (view?.status as? ViewStatus.Failed)?.reason
    val noQr = server.isEmpty() || failed != null || (late && (p == null || p == PairingSnapshot.Starting))
    if (p is PairingSnapshot.Joined && !sync.joining) LaunchedEffect(Unit) { onIn() }
    TvLoginSheet(
        qr = (p as? PairingSnapshot.Waiting)?.qr,
        approving = p == PairingSnapshot.Joining || p is PairingSnapshot.Joined,
        noQr = if (noQr) failed?.let(SyncCopy::pairingProblem) ?: AccountCopy.UNREACHABLE else null,
        onNotNow = onNotNow,
        onOther = onOther,
        onRetry = if (server.isEmpty()) null else ({ holder.start(server, name) }),
    )
}

/**
 * The TV Login sheet's look: the QR [qr] (none yet: an empty white square), the steps and the waiting line, or with
 * [noQr] (its reason) the one line, Try again ([onRetry]) and Not now. Not now has the first focus either way.
 */
@Composable
internal fun TvLoginSheet(
    qr: String?,
    approving: Boolean,
    noQr: String?,
    onNotNow: () -> Unit,
    onOther: () -> Unit,
    onRetry: (() -> Unit)?,
) {
    val notNow = remember { FocusRequester() }
    LaunchedEffect(noQr != null) { focusSoon(notNow, 60) }
    FirstSheet(null, width = 1200.dp) {
        Row(horizontalArrangement = Arrangement.spacedBy(48.dp), verticalAlignment = Alignment.CenterVertically) {
            if (noQr == null) {
                Box(
                    Modifier
                        .size(296.dp)
                        .clip(RoundedCornerShape(20.dp))
                        .background(Color.White)
                        .padding(18.dp),
                    contentAlignment = Alignment.Center,
                ) { qr?.let { Qr(it, AccountCopy.TV_QR_LABEL, 260.dp, pairing = true) } }
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                MqText(
                    FirstRunCopy.TV_LOGIN_TITLE,
                    Modifier.semantics { heading() },
                    role = TextRole.Title1,
                    color = Color.White,
                    weight = FontWeight.Bold,
                )
                if (noQr != null) {
                    LiveSpoken(FirstRunCopy.TV_NO_QR) { MqText(FirstRunCopy.TV_NO_QR, role = TextRole.Body) }
                    MqText(
                        noQr,
                        role = TextRole.Callout,
                        color = Marquee.colors.muted,
                    )
                    Row(Modifier.padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                        onRetry?.let { SecondaryButton(FirstRunCopy.TRY_AGAIN, it, icon = Icons.RefreshCw) }
                        SecondaryButton(AccountCopy.NOT_NOW, onNotNow, focusRequester = notNow)
                    }
                } else {
                    MqText(FirstRunCopy.LOGIN_LINE, role = TextRole.Body, color = Marquee.colors.muted)
                    FirstRunCopy.TV_STEPS.forEachIndexed { i, step ->
                        Row(
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Box(
                                Modifier.size(40.dp).clip(CircleShape).background(Color.White.copy(alpha = 0.16f)),
                                contentAlignment = Alignment.Center,
                            ) { MqText("${i + 1}", role = TextRole.Callout, weight = FontWeight.Bold) }
                            MqText(step, role = TextRole.Callout, color = Marquee.colors.muted)
                        }
                    }
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Spinner(size = 24.dp)
                        MqText(
                            if (approving) AccountCopy.APPROVE_SIGNING_IN else FirstRunCopy.QR_WAITING,
                            role = TextRole.Callout,
                            color = Marquee.colors.muted,
                        )
                    }
                    Row(Modifier.padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                        SecondaryButton(AccountCopy.NOT_NOW, onNotNow, focusRequester = notNow)
                        SecondaryButton(AccountCopy.OTHER_WAYS, onOther)
                    }
                }
            }
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
        if (actions.providers.isNotEmpty() || (actions.emailOffered && !tv) || links.isNotEmpty()) {
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
            // E12, E13: one Email row at most, and none on a TV.
            val email = !tv && actions.emailLinkable
            val emailLink = actions.emailLink
            if (rows.links && (more.isNotEmpty() || email)) {
                MqText(AccountCopy.LINK_ANOTHER, role = TextRole.Callout)
                if (emailLink != null) {
                    BackHandler { if (!emailLink.back()) actions.closeEmail() }
                    // The address lives only while its screen does (E12).
                    DisposableEffect(Unit) { onDispose(actions::closeEmail) }
                    EmailStages(emailLink) { a ->
                        when (a) {
                            Act.Retry -> emailLink.retry()
                            Act.Resend -> emailLink.resend()
                            Act.OtherAddress -> emailLink.open()
                            else -> actions.closeEmail()
                        }
                    }
                    SecondaryButton(SyncCopy.CANCEL, actions::closeEmail)
                } else {
                    FlowRow(
                        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                    ) {
                        for (p in more) {
                            SecondaryButton(AccountRules.continueWith(p), { actions.link(p) }, icon = Icons.Link)
                        }
                        if (email) {
                            SecondaryButton(AccountCopy.CONTINUE_WITH_EMAIL, actions::linkEmail, icon = Icons.Mail)
                        }
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
    val owner = rememberOwnerCheck()
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
    // Remove from this device empties it: the screen lock first, as Sign out with removal (D13). Without a lock
    // the ask above the button is the step.
    val removeHere: () -> Unit = {
        if (!flows.removal.running) {
            error = null
            scope.launch {
                error =
                    try {
                        guarded(owner, tv, SyncCopy.AUTH_REMOVE_HERE, askAgain = null) { flows.removeHere(debrid) }
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        wordsOf(e, SyncCopy.Action.Remove)
                    }
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
                        SecondaryButton(AccountRules.cancelUnlink(p), { go { sync.cancelDeletion(unlink = true) } })
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
                    removeHere,
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

/**
 * The email screen (plans/accounts-email.md 4): the address, then the 6-digit code. A failure stays on the stage
 * it came from, under its field; each stage's title names the pane, so TalkBack says it as the focus moves to
 * the new stage's field. The address and the code live in [form], in memory only (E12).
 */
@Composable
internal fun EmailStages(
    form: EmailCode,
    onAct: (Act) -> Unit,
) {
    when (form.stage) {
        EmailCode.Stage.Address -> EmailAddressStage(form, onAct)
        EmailCode.Stage.Code -> EmailCodeStage(form, onAct)
    }
}

@Composable
private fun EmailAddressStage(
    form: EmailCode,
    onAct: (Act) -> Unit,
) {
    val field = stepFocus()
    // A refused address gives the field the focus again, which reads the line it is described by.
    LaunchedEffect(form.said) { if (form.said > 0) runCatching { field.requestFocus() } }
    Column(
        Modifier.semantics { paneTitle = AccountCopy.EMAIL_TITLE },
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        StageTitle(AccountCopy.EMAIL_TITLE)
        FieldLabel(AccountCopy.EMAIL_LABEL)
        MqTextField(
            form.address,
            { form.address = it },
            label = AccountCopy.EMAIL_LABEL,
            placeholder = "",
            modifier = Modifier.focusRequester(field),
            keyboardOptions =
                KeyboardOptions(
                    capitalization = KeyboardCapitalization.None,
                    autoCorrectEnabled = false,
                    keyboardType = KeyboardType.Email,
                    imeAction = ImeAction.Send,
                ),
            keyboardActions = KeyboardActions(onSend = { form.send() }),
            // Read-only, not disabled, while the code is asked for: the focus stays.
            readOnly = form.busy,
            contentType = ContentType.EmailAddress,
            error = form.fieldError,
        )
        form.fieldError?.let { Said(it, Status.Bad) }
        form.failure?.let { FailureNote(it, spoken = it.actions.isEmpty(), onAct = onAct) }
        PrimaryButton(AccountCopy.EMAIL_SEND, form::send, icon = Icons.Mail, busy = form.busy)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun EmailCodeStage(
    form: EmailCode,
    onAct: (Act) -> Unit,
) {
    val field = stepFocus()
    // A wrong code clears the field and gives it the focus again.
    LaunchedEffect(form.said) { if (form.said > 0) runCatching { field.requestFocus() } }
    // Send a new code counts down from the newest mail's time, so time in the background counts.
    var left by remember { mutableIntStateOf(form.resendIn()) }
    LaunchedEffect(form.sentAt) {
        while (true) {
            left = form.resendIn()
            if (left == 0) break
            delay(1_000)
        }
    }
    val ready = left == 0
    val resend =
        if (ready) AccountCopy.CODE_RESEND else AccountRules.fill(AccountCopy.CODE_RESEND_IN, mapOf("seconds" to left))
    Column(
        Modifier.semantics { paneTitle = AccountCopy.CODE_TITLE },
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        StageTitle(AccountCopy.CODE_TITLE)
        MqText(AccountRules.fill(AccountCopy.CODE_SENT_TO, mapOf("email" to form.sentTo)), role = TextRole.Callout)
        FieldLabel(AccountCopy.CODE_LABEL)
        MqTextField(
            form.code,
            form::type,
            label = AccountCopy.CODE_LABEL,
            placeholder = "",
            modifier = Modifier.focusRequester(field),
            keyboardOptions =
                KeyboardOptions(
                    autoCorrectEnabled = false,
                    keyboardType = KeyboardType.Number,
                    imeAction = ImeAction.Go,
                ),
            keyboardActions = KeyboardActions(onGo = { form.submit() }),
            readOnly = form.busy || form.ended,
            // Fills from an SMS only: a typed or pasted code is the main path.
            contentType = ContentType.SmsOtpCode,
            error = form.fieldError,
        )
        form.fieldError?.let { Said(it, Status.Bad) }
        if (form.busy) Spinner()
        if (form.resent && form.failure == null) Note(AccountCopy.CODE_RESENT, side = 0.dp, spoken = true)
        form.failure?.let { FailureNote(it, spoken = it.actions.isEmpty(), onAct = onAct) }
        // Always shown, at normal weight: the mail may land in spam (plans/accounts-email.md 3).
        MqText(AccountCopy.CODE_MISSING, role = TextRole.Callout, color = Marquee.colors.muted)
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            // The countdown is read once with the button, not on each tick; the button turning on is said once.
            Box {
                LiveSpoken(if (ready) AccountCopy.CODE_RESEND else "") { Box(Modifier.size(1.dp)) }
                SecondaryButton(resend, form::resend, icon = Icons.Mail, enabled = ready && !form.busy)
            }
            SecondaryButton(AccountCopy.CODE_OTHER_ADDRESS, form::open)
        }
    }
}

@Composable
private fun StageTitle(text: String) {
    MqText(text, Modifier.semantics { heading() }, role = TextRole.Title3)
}

/** A field's visible label (the field says its own name to TalkBack, so this one is not read twice). */
@Composable
private fun FieldLabel(text: String) {
    MqText(text, Modifier.clearAndSetSemantics { }, role = TextRole.Callout, weight = FontWeight.SemiBold)
}
