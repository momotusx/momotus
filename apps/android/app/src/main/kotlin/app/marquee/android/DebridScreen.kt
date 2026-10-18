package app.marquee.android

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.DebridCopy
import app.marquee.core.ProfileCopy
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.AccountState
import app.marquee.core.model.DebridAccount
import app.marquee.core.model.DebridLoginSnapshot
import app.marquee.core.secrets.SecretStore
import app.marquee.core.session.CoreSession
import app.marquee.core.views.DebridLoginView
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.MqTextField
import app.marquee.design.PrimaryButton
import app.marquee.design.Qr
import app.marquee.design.SecondaryButton
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.TextRole
import app.marquee.design.focusClickable
import app.marquee.design.marqueeFocus
import app.marquee.design.openLink
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

// Debrid services (plans/c10-handoff.md, phase 3a): each account is a record in the encrypted
// keystore, made by a sign-in with a code or a typed key. No key, token or code reaches a log.

/** The screen's state: drafts, the open sign-in, and what the last action said. Never a stored key. */
class DebridHolder(
    private val session: CoreSession,
) : ViewModel() {
    val drafts = mutableStateMapOf<String, String>()

    /** Services showing their key field ("Use a key instead"). */
    val keyOpen = mutableStateMapOf<String, Boolean>()

    /** The sign-in by code under way, and its service. Closed with the screen (and so by a lock or a switch). */
    var login: DebridLoginView? by mutableStateOf(null)
        private set
    var loginService: String? by mutableStateOf(null)
        private set

    var said: DebridSaid? by mutableStateOf(null)
        private set

    private val one = Mutex()

    fun signIn(service: String) {
        closeLogin()
        said = null
        loginService = service
        login = session.debridLogin(service)
    }

    /** Closes the sign-in. A record it already collected still arrives through the `debrid` view. */
    fun closeLogin() {
        login?.close()
        login = null
        loginService = null
    }

    fun saveKey(service: String) {
        val key = drafts[service]?.trim().orEmpty()
        if (key.isEmpty()) return
        drafts.remove(service)
        // One change at a time, in the order pressed.
        viewModelScope.launch {
            one.withLock {
                said =
                    try {
                        session.ops.setDebridKey(service, key)
                        keyOpen.remove(service)
                        null
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        DebridSaid(service, "The key was not taken: ${why(e)}.", Status.Bad)
                    }
            }
        }
    }

    /** Tombstone, core, record, tombstone: the helper's order. Nothing changes when the tombstone cannot be stored. */
    fun disconnect(account: DebridAccount) {
        viewModelScope.launch {
            one.withLock {
                said =
                    try {
                        session.debridAccounts.store
                            .disconnect(account.service, account.generation)
                            .await()
                        DebridSaid(account.service, "Disconnected", Status.Idle)
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        DebridSaid(
                            account.service,
                            "Not disconnected yet: ${why(e)}. It finishes at the next start.",
                            Status.Warn,
                        )
                    }
            }
        }
    }

    fun connected(service: String) {
        said = DebridSaid(service, "Connected", Status.Ok)
    }

    private fun why(e: Exception) = (e as? CoreError)?.message ?: CoreLog.describe(e)

    override fun onCleared() {
        closeLogin()
    }
}

class DebridSaid(
    val service: String,
    val text: String,
    val tone: Status,
)

@Composable
fun DebridScreen(session: CoreSession) {
    val holder =
        viewModel<DebridHolder>(
            key = "debrid",
            factory = viewModelFactory { initializer { DebridHolder(session) } },
        )
    val snapshot = session.debrid.snapshot
    val durable = session.secrets.health != SecretStore.Health.MemoryOnly
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                BrowseHeader("Settings", "Debrid services")
                MqText(
                    "A debrid account plays torrents as fast direct links. Sign-ins and keys stay on this device, " +
                        "encrypted, and are never shown again or synced.",
                    modifier = Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 900.dp),
                    role = TextRole.Callout,
                    color = Marquee.colors.muted,
                )
            }
        }
        items(DebridCopy.services, key = { it.id }) { service ->
            Service(
                service,
                account = snapshot?.accounts?.firstOrNull { it.service == service.id },
                pending = snapshot?.pending?.any { it.service == service.id } == true,
                durable = durable,
                holder = holder,
            )
        }
    }
}

/** One service: its state, a sign-in by code where the service has one, a key, and Disconnect. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Service(
    service: DebridCopy.Service,
    account: DebridAccount?,
    pending: Boolean,
    durable: Boolean,
    holder: DebridHolder,
) {
    val tv = Marquee.form == Form.Tv
    val context = LocalContext.current
    val keyboard = LocalSoftwareKeyboardController.current
    val byCode = service.id in DebridCopy.codeServices
    val state = DebridCopy.state(account?.state, pending, durable)
    val signingIn = holder.loginService == service.id
    // A key field: Premiumize has nothing else; the others open it with "Use a key instead", or when
    // the service asked for a new sign-in and has no code sign-in.
    val keyField =
        !signingIn &&
            (
                holder.keyOpen[service.id] == true ||
                    (!byCode && (account == null || account.state == AccountState.Reconnect))
            )
    val draft = holder.drafts[service.id].orEmpty()
    val save = {
        keyboard?.hide()
        holder.saveKey(service.id)
    }
    Section {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            // The name and its state take the room; the key link keeps its place at the end.
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Row(
                    Modifier.weight(1f),
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    MqText(
                        service.name,
                        modifier = Modifier.weight(1f, fill = false),
                        role = TextRole.Headline,
                        weight = FontWeight.Bold,
                        maxLines = 1,
                    )
                    if (state != null) StatusWord(state, toneOf(account?.state, pending, durable))
                }
                if (keyField && !tv) KeyPage("Get a key for ${service.name}") { openLink(context, service.keyPage) }
            }
            if (signingIn) {
                holder.login?.let { view ->
                    SignIn(service, view, account, pending, durable, holder)
                }
            } else {
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                ) {
                    if (byCode && (account == null || account.state == AccountState.Reconnect)) {
                        PrimaryButton(
                            if (account == null) "Sign in with a code" else "Sign in again",
                            { holder.signIn(service.id) },
                            Modifier.semantics { contentDescription = "Sign in to ${service.name} with a code" },
                            icon = Icons.QrCode,
                        )
                        if (holder.keyOpen[service.id] != true) {
                            SecondaryButton(
                                "Use a key instead",
                                { holder.keyOpen[service.id] = true },
                                Modifier.semantics { contentDescription = "Use a key for ${service.name} instead" },
                                icon = Icons.KeyRound,
                            )
                        }
                    }
                    if (account != null) {
                        SecondaryButton(
                            "Disconnect",
                            { holder.disconnect(account) },
                            Modifier.semantics { contentDescription = "Disconnect ${service.name}" },
                            icon = Icons.Trash,
                        )
                    }
                }
            }
            if (keyField) {
                // A TV has no browser: it names the page for a phone or computer.
                if (tv) {
                    MqText(
                        "Get a key at ${DebridCopy.page(service)}",
                        role = TextRole.Caption,
                        color = Marquee.colors.muted,
                    )
                }
                MqTextField(
                    draft,
                    { holder.drafts[service.id] = it },
                    label = "${service.name} API key",
                    placeholder = "API key",
                    keyboardOptions =
                        KeyboardOptions(
                            keyboardType = KeyboardType.Password,
                            autoCorrectEnabled = false,
                            imeAction = ImeAction.Done,
                        ),
                    keyboardActions = KeyboardActions(onDone = { save() }),
                    visualTransformation = PasswordVisualTransformation(),
                )
                PrimaryButton(
                    "Save",
                    save,
                    Modifier.semantics { contentDescription = "Save ${service.name} key" },
                    icon = Icons.Check,
                    enabled = draft.isNotBlank(),
                )
            }
            holder.said?.takeIf { it.service == service.id }?.let { StatusWord(it.text, it.tone) }
        }
    }
}

private fun toneOf(
    state: AccountState?,
    pending: Boolean,
    durable: Boolean,
) = when {
    pending && !durable -> Status.Warn
    state == AccountState.Reconnect -> Status.Bad
    state == AccountState.Failing || pending -> Status.Warn
    else -> Status.Ok
}

/** A sign-in by code: the QR and the code while it waits, then connected or why not. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun SignIn(
    service: DebridCopy.Service,
    view: DebridLoginView,
    account: DebridAccount?,
    pending: Boolean,
    durable: Boolean,
    holder: DebridHolder,
) {
    val tv = Marquee.form == Form.Tv
    val context = LocalContext.current
    val colors = Marquee.colors
    val phase = view.snapshot ?: DebridLoginSnapshot.Starting
    val stored = account != null && !pending
    // Done counts once the record is stored: then the sheet closes and the card says Connected.
    LaunchedEffect(phase, stored) {
        if (phase == DebridLoginSnapshot.Done && stored) {
            holder.closeLogin()
            holder.connected(service.id)
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        when (phase) {
            is DebridLoginSnapshot.Code -> {
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
                    if (tv) Qr(phase.qr, "Sign-in QR code for ${service.name}", 240.dp)
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        MqText(
                            if (tv) "Scan the code with your phone, or open" else "Open",
                            role = TextRole.Callout,
                            color = colors.muted,
                        )
                        MqText(DebridCopy.address(phase.url), role = TextRole.Headline, weight = FontWeight.Bold)
                        MqText("and enter this code:", role = TextRole.Callout, color = colors.muted)
                        MqText(
                            phase.userCode,
                            modifier =
                                Modifier.semantics {
                                    contentDescription =
                                        "Code ${phase.userCode.toList().joinToString(" ")}"
                                },
                            role = TextRole.Title1,
                            weight = FontWeight.Bold,
                            fontFamily = FontFamily.Monospace,
                            letterSpacing = 4.sp,
                        )
                        MqText(
                            "Good for ${ProfileCopy.waitText(phase.expiresMs - now)}",
                            role = TextRole.Caption,
                            color = colors.muted,
                        )
                    }
                }
                FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    if (!tv) PrimaryButton("Open the page", { openLink(context, phase.url) }, icon = Icons.Link)
                    SecondaryButton("Cancel", { holder.closeLogin() })
                }
            }

            DebridLoginSnapshot.Starting -> {
                MqText(DebridCopy.login(phase), role = TextRole.Callout, color = colors.muted)
                SecondaryButton("Cancel", { holder.closeLogin() })
            }

            DebridLoginSnapshot.Done -> {
                val words =
                    if (pending && !durable) "Kept for this session only: this device could not store it." else "Saving"
                StatusWord(words, if (durable) Status.Idle else Status.Warn)
                if (!durable) SecondaryButton("Close", { holder.closeLogin() })
            }

            else -> {
                StatusWord(DebridCopy.login(phase), Status.Bad)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    PrimaryButton("Get a new code", { holder.signIn(service.id) }, icon = Icons.RefreshCw)
                    SecondaryButton("Close", { holder.closeLogin() })
                }
            }
        }
    }
}

/** "Get a key" as a quiet link in the card's corner: the key page opens in the browser. */
@Composable
private fun KeyPage(
    spoken: String,
    onClick: () -> Unit,
) {
    val colors = Marquee.colors
    Row(
        Modifier
            .heightIn(min = 48.dp)
            .marqueeFocus(Marquee.radius.pill)
            .clip(RoundedCornerShape(Marquee.radius.pill))
            .semantics(mergeDescendants = true) { contentDescription = spoken }
            .focusClickable(onClick, Role.Button)
            .padding(horizontal = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(Icons.Link, tint = colors.muted, size = 16.dp)
        MqText("Get a key", role = TextRole.Callout, color = colors.muted, weight = FontWeight.Medium, maxLines = 1)
    }
}
