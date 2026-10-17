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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.DebridCopy
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.secrets.SecretStore
import app.marquee.core.session.CoreSession
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.MqTextField
import app.marquee.design.PrimaryButton
import app.marquee.design.SecondaryButton
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.TextRole
import app.marquee.design.focusClickable
import app.marquee.design.marqueeFocus
import app.marquee.design.openLink
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

// Debrid services (plans/android-design-v2.md 13d, Apple's DebridScreen): a key per service in the
// encrypted SecretStore, handed to the core with `setDebridKeys`. A key is never shown again or logged.

/** Drafts live here, not in saved state: a typed key never leaves memory unless it is saved. */
class DebridHolder(
    private val session: CoreSession,
) : ViewModel() {
    val drafts = mutableStateMapOf<String, String>()
    var saved by mutableStateOf(savedNow())
        private set

    // What the last change ended with, shown in the card of the service it changed.
    var said: DebridSaid? by mutableStateOf(null)
        private set

    private fun savedNow() =
        session.secrets
            .load()
            .debridKeys
            .map { it.first }
            .toSet()

    /** Stores [key] for [service] (null removes it), then hands every key to the core. */
    fun apply(
        service: String,
        key: String?,
    ) {
        val keys =
            session.secrets
                .update {
                    it.copy(
                        debridKeys = DebridCopy.change(it.debridKeys, service, key),
                    )
                }.debridKeys
        drafts.remove(service)
        saved = savedNow()
        val memoryOnly = session.secrets.health == SecretStore.Health.MemoryOnly
        viewModelScope.launch {
            val (text, tone) =
                try {
                    val words = DebridCopy.inUse(session.ops.setDebridKeys(keys))
                    if (memoryOnly) {
                        "$words Kept for this session only: this device could not store it." to Status.Warn
                    } else {
                        words to if (keys.isEmpty()) Status.Idle else Status.Ok
                    }
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    "Saved, but the core did not take it: ${(e as? CoreError)?.message ?: CoreLog.describe(e)}" to
                        Status.Bad
                }
            said = DebridSaid(service, text, tone)
        }
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
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                BrowseHeader("Settings", "Debrid services")
                MqText(
                    "A debrid account plays torrents as fast direct links. Keys stay on this device, encrypted, " +
                        "and are never shown again or synced.",
                    modifier = Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 900.dp),
                    role = TextRole.Callout,
                    color = Marquee.colors.muted,
                )
            }
        }
        items(DebridCopy.services, key = { it.id }) { service -> Service(service, holder) }
    }
}

/** One service: its name, whether a key is saved, where to get one, and the field. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Service(
    service: DebridCopy.Service,
    holder: DebridHolder,
) {
    val tv = Marquee.form == Form.Tv
    val context = LocalContext.current
    val keyboard = LocalSoftwareKeyboardController.current
    val colors = Marquee.colors
    val saved = service.id in holder.saved
    val draft = holder.drafts[service.id].orEmpty()
    val save = {
        if (draft.isNotBlank()) {
            keyboard?.hide()
            holder.apply(service.id, draft)
        }
    }
    Section {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            // The name and its badge take the room; the link keeps its place at the end of every card.
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
                    if (saved) {
                        Row(
                            horizontalArrangement = Arrangement.spacedBy(6.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            MqIcon(Icons.KeyRound, tint = colors.ok, size = if (tv) 24.dp else 18.dp)
                            MqText("Key saved", role = TextRole.Caption, color = colors.ok, maxLines = 1)
                        }
                    }
                }
                if (!tv) KeyPage { openLink(context, service.keyPage) }
            }
            // A TV has no browser to sign in with: it names the page for a phone or computer.
            if (tv) {
                MqText("Get a key at ${DebridCopy.page(service)}", role = TextRole.Caption, color = colors.muted)
            }
            MqTextField(
                draft,
                { holder.drafts[service.id] = it },
                label = "${service.name} API key",
                placeholder = if (saved) "Replace the saved key" else "API key",
                keyboardOptions =
                    KeyboardOptions(
                        keyboardType = KeyboardType.Password,
                        autoCorrectEnabled = false,
                        imeAction = ImeAction.Done,
                    ),
                keyboardActions = KeyboardActions(onDone = { save() }),
                visualTransformation = PasswordVisualTransformation(),
            )
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            ) {
                PrimaryButton("Save", save, icon = Icons.Check, enabled = draft.isNotBlank())
                if (saved) SecondaryButton("Remove", { holder.apply(service.id, null) }, icon = Icons.Trash)
            }
            holder.said?.takeIf { it.service == service.id }?.let { StatusWord(it.text, it.tone) }
        }
    }
}

/** "Get a key" as a quiet link in the card's corner: the key page opens in the browser. */
@Composable
private fun KeyPage(onClick: () -> Unit) {
    val colors = Marquee.colors
    Row(
        Modifier
            .heightIn(min = 48.dp)
            .marqueeFocus(Marquee.radius.pill)
            .clip(RoundedCornerShape(Marquee.radius.pill))
            .focusClickable(onClick, Role.Button)
            .padding(horizontal = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(Icons.Link, tint = colors.muted, size = 16.dp)
        MqText("Get a key", role = TextRole.Callout, color = colors.muted, weight = FontWeight.Medium, maxLines = 1)
    }
}
