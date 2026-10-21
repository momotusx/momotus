package app.marquee.android

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.DebridCopy
import app.marquee.core.ProfileCopy
import app.marquee.core.TraktCopy
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.TraktLoginSnapshot
import app.marquee.core.model.TraktStatus
import app.marquee.core.model.TraktUnmapped
import app.marquee.core.session.CoreSession
import app.marquee.core.trakt.TraktAccount
import app.marquee.core.views.TraktLoginView
import app.marquee.design.FocusInk
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.LiveSpoken
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.PrimaryButton
import app.marquee.design.Qr
import app.marquee.design.SecondaryButton
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.TextRole
import app.marquee.design.Tokens
import app.marquee.design.focusPalette
import app.marquee.design.marqueeFocus
import app.marquee.design.openLink
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.text.DateFormat
import java.util.Calendar
import java.util.Date

// Trakt (plans/m7-trakt.md section 3): the profile's account, its state in words, a sign-in by code,
// Sync now, Sign out and what did not match. The record never passes through this page: TraktAccount
// stores what the `trakt` view offers.

/** A wall clock as the page says it: the time today, else the date and time. */
internal fun clockTime(ms: Long): String {
    val day = { at: Long -> Calendar.getInstance().apply { timeInMillis = at }.get(Calendar.DAY_OF_YEAR) }
    val today = System.currentTimeMillis().let { now -> now - ms < 86_400_000 && day(now) == day(ms) }
    val format =
        if (today) {
            DateFormat.getTimeInstance(
                DateFormat.SHORT,
            )
        } else {
            DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT)
        }
    return format.format(Date(ms))
}

/** The page's state: the sign-in under way and what the last action said. */
class TraktHolder(
    private val session: CoreSession,
    private val trakt: TraktAccount,
) : ViewModel() {
    /** The sign-in by code; closed with the page (and so by a lock or a switch). */
    var login: TraktLoginView? by mutableStateOf(null)
        private set

    var said: Pair<String, Status>? by mutableStateOf(null)
        private set

    /** The core signed out but the file kept the record: Try again deletes it. */
    var notStored by mutableStateOf(false)
        private set

    var busy by mutableStateOf(false)
        private set

    /** Sign out asks first. */
    var asking by mutableStateOf(false)

    fun signIn() {
        closeLogin()
        said = null
        login = session.traktLogin()
    }

    fun closeLogin() {
        login?.close()
        login = null
    }

    fun syncNow() =
        act(TraktCopy::syncFailed) {
            TraktCopy.synced(trakt.syncNow()) to Status.Ok
        }

    /** Signed out only once the record is gone from the file; else it says so, with Try again. */
    fun signOut() {
        if (busy) return
        asking = false
        act(TraktCopy::signOutFailed) {
            val stored = trakt.signOut()
            notStored = !stored
            if (stored) TraktCopy.SIGNED_OUT to Status.Idle else TraktCopy.SIGN_OUT_NOT_STORED to Status.Warn
        }
    }

    private fun act(
        failed: (String) -> String,
        op: suspend () -> Pair<String, Status>,
    ) {
        if (busy) return
        busy = true
        said = null
        notStored = false
        viewModelScope.launch {
            said =
                try {
                    op()
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    failed((e as? CoreError)?.message ?: CoreLog.describe(e)) to Status.Bad
                } finally {
                    busy = false
                }
        }
    }

    override fun onCleared() = closeLogin()
}

private fun toneOf(state: String) =
    when (state) {
        "connected" -> Status.Ok
        "failing", "thisDeviceOff" -> Status.Warn
        "reconnect" -> Status.Bad
        else -> Status.Idle
    }

@Composable
fun TraktScreen(
    session: CoreSession,
    trakt: TraktAccount,
) {
    val holder =
        viewModel<TraktHolder>(
            key = "trakt",
            factory = viewModelFactory { initializer { TraktHolder(session, trakt) } },
        )
    val status = trakt.status
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                BrowseHeader(TraktCopy.EYEBROW, TraktCopy.TITLE)
                MqText(
                    TraktCopy.LEDE,
                    modifier = Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 900.dp),
                    role = TextRole.Callout,
                    color = Marquee.colors.muted,
                )
            }
        }
        item(key = "account") { Account(status, trakt.memoryOnly, holder) }
        val unmapped = status?.unmapped.orEmpty()
        if (unmapped.isNotEmpty()) {
            item(key = "unmapped-head") {
                Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    Kicker(TraktCopy.UNMAPPED_TITLE)
                    MqText(
                        TraktCopy.UNMAPPED_LEDE,
                        modifier = Modifier.padding(horizontal = Marquee.safe.side),
                        role = TextRole.Callout,
                        color = Marquee.colors.muted,
                    )
                }
            }
            items(unmapped, key = { "unmapped:${it.itemId}" }) { item -> Unmapped(item) }
        }
    }
}

/** One item Trakt and Marquee could not agree on: one stop for a remote and a screen reader, the name then why. */
@Composable
private fun Unmapped(item: TraktUnmapped) {
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Marquee.radius.l)
    val tv = Marquee.form == Form.Tv
    // On TV the focused row turns white with dark words (focusPalette).
    FocusInk(focusPalette(focused)) {
        Column(
            Modifier
                .padding(horizontal = Marquee.safe.side)
                .fillMaxWidth()
                .marqueeFocus(Marquee.radius.l, Tokens.FOCUS_SCALE_TILE)
                .onFocusChanged { focused = it.isFocused }
                .clip(shape)
                .background(Marquee.colors.surface)
                .border(1.dp, Marquee.colors.edge, shape)
                .semantics(mergeDescendants = true) {}
                // A remote moves by focus: each row takes it, so the list can be read to the end.
                .then(if (tv) Modifier.focusable() else Modifier)
                .padding(if (tv) 24.dp else 16.dp),
            verticalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            MqText(item.name, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 2)
            MqText(
                item.reason.replaceFirstChar { it.uppercase() },
                role = TextRole.Caption,
                color = Marquee.colors.muted,
            )
        }
    }
}

/** The account card: the state in words, its facts, the sign-in or its actions, and what the last action said. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Account(
    status: TraktStatus?,
    memoryOnly: Boolean,
    holder: TraktHolder,
) {
    val scope = rememberCoroutineScope()
    val actionFocus = remember { FocusRequester() }
    val tv = Marquee.form == Form.Tv
    Section {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            if (status == null) return@Column
            // With no action (a build without Trakt), the state line takes focus on a TV, so a remote reads it.
            val noAction = TraktCopy.signIn(status.state) == null && !TraktCopy.signedIn(status.state)
            StatusWord(
                TraktCopy.state(status, ::clockTime),
                toneOf(status.state),
                modifier =
                    if (tv && noAction) {
                        Modifier
                            .marqueeFocus(Marquee.radius.pill, scale = 1f)
                            .semantics(mergeDescendants = true) {}
                            .focusable()
                    } else {
                        Modifier
                    },
                maxLines = Int.MAX_VALUE,
            )
            if (TraktCopy.signedIn(status.state)) {
                val facts =
                    listOfNotNull(
                        TraktCopy.lastSynced(status.lastOkMs, ::clockTime),
                        TraktCopy.pending(status.pendingWrites),
                        status.importing?.let(TraktCopy::importing),
                        // Failing and reconnect say their reason in the state line already.
                        status.lastError?.takeIf { status.state == "connected" }?.let(TraktCopy::lastError),
                    )
                for (fact in facts) MqText(fact, role = TextRole.Callout, color = Marquee.colors.muted)
            }
            if (memoryOnly) Said(TraktCopy.MEMORY_ONLY, Status.Warn)
            val login = holder.login
            when {
                login != null -> {
                    // Connected: the state line says so; the focus goes to the account's first action.
                    SignIn(login, status, holder) {
                        holder.closeLogin()
                        scope.refocus(actionFocus)
                    }
                }

                holder.asking -> {
                    MqText(TraktCopy.SIGN_OUT_ASK, role = TextRole.Callout)
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                        // While Sync now runs the ask stays, its Sign out waiting until the round ends.
                        SecondaryButton(
                            TraktCopy.SIGN_OUT,
                            holder::signOut,
                            icon = Icons.LogOut,
                            enabled = !holder.busy,
                        )
                        SecondaryButton(TraktCopy.CANCEL, {
                            holder.asking = false
                            scope.refocus(actionFocus)
                        }, focusRequester = stepFocus())
                    }
                }

                else -> {
                    FlowRow(
                        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                    ) {
                        TraktCopy.signIn(status.state)?.let { label ->
                            PrimaryButton(label, holder::signIn, icon = Icons.QrCode, focusRequester = actionFocus)
                        }
                        if (TraktCopy.signedIn(status.state)) {
                            // A sign-in to do again comes first: until then there is nothing to sync.
                            if (TraktCopy.signIn(status.state) == null) {
                                SecondaryButton(
                                    if (holder.busy) TraktCopy.SYNCING else TraktCopy.SYNC_NOW,
                                    holder::syncNow,
                                    icon = Icons.RefreshCw,
                                    busy = holder.busy,
                                    focusRequester = actionFocus,
                                )
                            }
                            SecondaryButton(TraktCopy.SIGN_OUT, { holder.asking = true }, icon = Icons.LogOut)
                        }
                    }
                }
            }
            // What an action ended with, read out once by TalkBack (Synced, Sync did not work, Signed out).
            val said = holder.said
            LiveSpoken(said?.first.orEmpty()) { said?.let { (text, tone) -> Said(text, tone) } }
            if (holder.notStored) SecondaryButton(TraktCopy.TRY_AGAIN, holder::signOut, icon = Icons.RefreshCw)
        }
    }
}

/** A sign-in by code: the code large, the address, a QR on a TV, the time left; then connected or why not. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun SignIn(
    view: TraktLoginView,
    status: TraktStatus,
    holder: TraktHolder,
    onDone: () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val context = LocalContext.current
    val colors = Marquee.colors
    val phase = view.snapshot ?: TraktLoginSnapshot.Starting
    // Connected once the record is stored and acknowledged: the view no longer offers it.
    val done = phase is TraktLoginSnapshot.Done && status.state == "connected" && status.record == null
    LaunchedEffect(done) { if (done) onDone() }
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        when (phase) {
            is TraktLoginSnapshot.Code -> {
                var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
                LaunchedEffect(phase.expiresMs) {
                    while (true) {
                        now = System.currentTimeMillis()
                        delay(1_000)
                    }
                }
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.m),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
                ) {
                    // A TV signs in from a phone: the QR leads. A phone opens the page itself.
                    if (tv) Qr(phase.qr, "Trakt sign-in QR code", 240.dp)
                    // Read out once when the code comes (where to go, then the code a character at a time),
                    // never the time left: the focus is on Cancel, so a live region carries it.
                    val address = DebridCopy.address(phase.url)
                    val lead = if (tv) TraktCopy.SCAN_OR_OPEN else TraktCopy.OPEN
                    LiveSpoken("$lead $address ${TraktCopy.ENTER_CODE} ${TraktCopy.spokenCode(phase.userCode)}") {
                        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            MqText(lead, role = TextRole.Callout, color = colors.muted)
                            MqText(address, role = TextRole.Headline, weight = FontWeight.Bold)
                            MqText(TraktCopy.ENTER_CODE, role = TextRole.Callout, color = colors.muted)
                            MqText(
                                phase.userCode,
                                role = TextRole.Title1,
                                weight = FontWeight.Bold,
                                fontFamily = FontFamily.Monospace,
                                letterSpacing = 4.sp,
                            )
                            MqText(
                                TraktCopy.goodFor(ProfileCopy.waitText(phase.expiresMs - now)),
                                role = TextRole.Caption,
                                color = colors.muted,
                            )
                        }
                    }
                }
                FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    if (!tv) {
                        PrimaryButton(TraktCopy.OPEN_PAGE, { openLink(context, phase.url) }, icon = Icons.Link)
                    }
                    SecondaryButton(TraktCopy.CANCEL, holder::closeLogin, focusRequester = stepFocus())
                }
            }

            TraktLoginSnapshot.Starting, is TraktLoginSnapshot.Done -> {
                StatusWord(TraktCopy.login(phase), Status.Idle)
                SecondaryButton(TraktCopy.CANCEL, holder::closeLogin, focusRequester = stepFocus())
            }

            TraktLoginSnapshot.NotConfigured -> {
                Said(TraktCopy.login(phase), Status.Bad)
                SecondaryButton(TraktCopy.CLOSE, holder::closeLogin, focusRequester = stepFocus())
            }

            else -> {
                // Expired, denied, failed: a new code is one press away.
                Said(TraktCopy.login(phase), Status.Bad)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    PrimaryButton(
                        TraktCopy.NEW_CODE,
                        holder::signIn,
                        icon = Icons.RefreshCw,
                        focusRequester = stepFocus(),
                    )
                    SecondaryButton(TraktCopy.CLOSE, holder::closeLogin)
                }
            }
        }
    }
}
