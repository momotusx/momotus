package app.marquee.android

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.AccountCopy
import app.marquee.core.AddonsCopy
import app.marquee.core.DebridCopy
import app.marquee.core.DefaultsCopy
import app.marquee.core.FindAddonsCopy
import app.marquee.core.LanguagePicks
import app.marquee.core.LinkInstall
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.SyncCopy
import app.marquee.core.TraktCopy
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.AddonInfo
import app.marquee.core.model.LanguageChoice
import app.marquee.core.model.SubtitleSize
import app.marquee.core.session.CoreSession
import app.marquee.core.views.AddonsView
import app.marquee.core.views.SettingsView
import app.marquee.core.views.ViewStatus
import app.marquee.design.BrandLockup
import app.marquee.design.Chip
import app.marquee.design.FocusInk
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.LiveSpoken
import app.marquee.design.Marks
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.MqTextField
import app.marquee.design.PrimaryButton
import app.marquee.design.RoundButton
import app.marquee.design.SecondaryButton
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.SwitchRow
import app.marquee.design.TextRole
import app.marquee.design.Tokens
import app.marquee.design.focusClickable
import app.marquee.design.focusPalette
import app.marquee.design.focusSoon
import app.marquee.design.marqueeFocus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

// Settings (plans/android-design-v2.md 5.5): addons, playback and about, as sections on glass.
// Profiles, sync and debrid sign-in arrive with their own slices.

class SettingsHolder(
    val session: CoreSession,
) : ViewModel() {
    val prefs: SettingsView = session.settings()
    val addons: AddonsView = session.addons()

    override fun onCleared() {
        prefs.close()
        addons.close()
    }
}

/** The parts of the page an operation can come from. */
internal enum class Place { Addons, Install, Playback }

/** What an operation ended with, shown under the part of the page it came from. */
internal class Said(
    val place: Place,
    val text: String,
    val failed: Boolean,
    /** Still running: the line says what is under way, as the web's "Installing". */
    val pending: Boolean = false,
)

@Composable
private fun SaidLine(
    said: Said,
    side: Dp = Marquee.safe.side,
) {
    when {
        said.failed -> Note(said.text, failed = true, side = side)
        else -> StatusWord(said.text, if (said.pending) Status.Warn else Status.Ok, Modifier.padding(horizontal = side))
    }
}

@Composable
fun SettingsScreen(
    session: CoreSession,
    gate: ProfileGate,
    onProfiles: () -> Unit,
    onEditProfile: (String) -> Unit,
    onManageProfiles: () -> Unit,
    sync: SyncController,
    onImport: () -> Unit,
    onSync: () -> Unit,
    onDebrid: () -> Unit,
    onTrakt: () -> Unit,
    onHomeRows: () -> Unit,
    onFindAddons: () -> Unit,
    /** Settings > Stream choice; null where it is not offered (the sources page). */
    onStreamChoice: (() -> Unit)? = null,
    /** The recommended addons step with every switch; null where it is not offered (the sources page). */
    onDefaults: (() -> Unit)? = null,
    /** Opened from "Add a source": the link field first and only the source addons, as on Apple. */
    sourcesOnly: Boolean = false,
    link: LinkInstall,
) {
    val holder =
        viewModel<SettingsHolder>(
            key = "settings",
            factory = viewModelFactory { initializer { SettingsHolder(session) } },
        )
    val scope = rememberCoroutineScope()
    // One operation at a time; what it ends with stays on screen in plain words, under the part of
    // the page it came from (the top of the page is out of sight from the install field).
    var busy by remember { mutableStateOf<String?>(null) }
    var said by remember { mutableStateOf<Said?>(null) }
    val requesters = remember { mutableStateMapOf<String, FocusRequester>() }
    val installFocus = remember { FocusRequester() }

    fun run(
        what: String,
        place: Place,
        then: () -> Unit = {},
        op: suspend () -> Unit,
    ) {
        if (busy != null) return
        busy = what
        said = Said(place, what, failed = false, pending = true)
        scope.launch {
            try {
                op()
                // An operation that says nothing on success (all but an install) leaves no line.
                if (said?.pending == true) said = null
                then()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                said =
                    Said(
                        place,
                        "$what did not work: ${(e as? CoreError)?.message ?: CoreLog.describe(e)}",
                        failed = true,
                    )
            } finally {
                busy = null
            }
        }
    }

    val addons =
        holder.addons.snapshot
            ?.addons
            .orEmpty()
            .filter { !sourcesOnly || "stream" in it.provides }
    // A remote types; only a phone can paste.
    val linkVerb = if (Marquee.form == Form.Tv) "Type" else "Paste"
    if (sourcesOnly) {
        LaunchedEffect(Unit) {
            // The field is laid out in the first frame; focus it after that.
            focusSoon(installFocus)
        }
    }
    LazyColumn(
        // Down from the TV pill comes back to what had focus (plans 2.1).
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        if (sourcesOnly) {
            item(key = "head") { BrowseHeader("Add a source") }
            item(key = "sources-why") {
                Note(
                    "A source is an addon that finds a playable stream for a title. " +
                        "$linkVerb a source addon's link to add one.",
                )
            }
            item(key = "install") {
                LinkField(session, link, installFocus)
            }
            item(key = "find") {
                LinkRow(
                    Icons.PackageSearch,
                    FindAddonsCopy.TITLE,
                    FindAddonsCopy.LEDE,
                    onFindAddons,
                    lines = Int.MAX_VALUE,
                )
            }
        } else {
            item(key = "head") { BrowseHeader("Your Momotus") }
        }
        if (!sourcesOnly) {
            gate.snapshot?.let { s ->
                val me = s.profiles.firstOrNull { it.id == s.active }
                if (me != null) {
                    item(key = "profiles-head") { Kicker("Profiles") }
                    item(key = "profiles") {
                        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                            ProfileLinkRow(
                                me,
                                if (s.profiles.size > 1) {
                                    AccountCopy.SWITCH_PROFILE
                                } else {
                                    "Add a profile for someone else, or switch"
                                },
                                onProfiles,
                            )
                            LinkRow(
                                Icons.UserPen,
                                "Edit profile",
                                "Name, picture and PIN.",
                                onClick = { onEditProfile(me.id) },
                            )
                            if (me.isOwner) {
                                LinkRow(
                                    Icons.Users,
                                    "Manage profiles",
                                    "Add, edit and remove profiles on this account.",
                                    onManageProfiles,
                                )
                            }
                        }
                    }
                }
            }
        }
        item(key = "addons-head") { Kicker(if (sourcesOnly) "Your sources" else "Addons") }
        // Find addons leads the section, as on the web.
        if (!sourcesOnly) {
            item(key = "find") {
                LinkRow(
                    Icons.PackageSearch,
                    FindAddonsCopy.TITLE,
                    FindAddonsCopy.LEDE,
                    onFindAddons,
                    lines = Int.MAX_VALUE,
                )
            }
            // A kids profile reads the owner's addons: the defaults are not its to change.
            if (onDefaults != null && gate.snapshot?.kids == false) {
                item(key = "defaults") {
                    LinkRow(Icons.ListChecks, DefaultsCopy.SETTINGS_ROW, DefaultsCopy.SETTINGS_ROW_LINE, onDefaults)
                }
            }
        }
        when {
            holder.addons.status is ViewStatus.Failed -> {
                item(key = "addons-failed") { Note("Addons could not load", failed = true) }
            }

            addons.isEmpty() && holder.addons.snapshot != null -> {
                item(key = "addons-none") {
                    Note(
                        if (sourcesOnly) {
                            "No source yet. $linkVerb a source addon's link above."
                        } else {
                            "No addon yet. $linkVerb an addon link below to add one."
                        },
                    )
                }
            }
        }
        itemsIndexed(addons, key = { _, addon -> "addon:${addon.url}" }) { index, addon ->
            val requester = remember(addon.url) { requesters.getOrPut(addon.url) { FocusRequester() } }
            AddonRow(
                addon = addon,
                first = index == 0,
                last = index == addons.lastIndex,
                busy = busy != null,
                focusRequester = requester,
                onToggle = {
                    run(if (addon.enabled) "Turning ${addon.name} off" else "Turning ${addon.name} on", Place.Addons) {
                        session.ops.setAddonEnabled(addon.url, !addon.enabled)
                    }
                },
                onMove = { by ->
                    run("Moving ${addon.name}", Place.Addons) { session.ops.moveAddon(addon.url, index + by) }
                },
                onRemove = {
                    // Focus falls to the next addon, or to the link field when this was the last.
                    val next = addons.getOrNull(index + 1) ?: addons.getOrNull(index - 1)
                    run(
                        "Removing ${addon.name}",
                        Place.Addons,
                        then = {
                            val target = next?.let { requesters[it.url] } ?: installFocus
                            runCatching { target.requestFocus() }
                        },
                    ) { session.ops.removeAddon(addon.url) }
                },
            )
        }
        said?.takeIf { it.place == Place.Addons }?.let { item(key = "addons-said") { SaidLine(it) } }
        if (sourcesOnly) return@LazyColumn
        item(key = "install") {
            LinkField(session, link, installFocus)
        }
        item(key = "import") {
            LinkRow(
                Icons.Download,
                "Import from Stremio",
                "Addons, library and progress, with a code. No password typed here.",
                onImport,
            )
        }
        item(key = "debrid") {
            val count =
                session.debrid.snapshot
                    ?.accounts
                    ?.size ?: 0
            LinkRow(
                Icons.KeyRound,
                "Debrid services",
                if (count ==
                    0
                ) {
                    "Play torrents as fast direct links with a debrid account."
                } else {
                    DebridCopy.inUse(count)
                },
                onDebrid,
            )
        }
        item(key = "trakt") {
            val trakt = (LocalContext.current.applicationContext as MarqueeApp).trakt
            // The state is said whole, never cut short.
            LinkRow(Icons.ListChecks, TraktCopy.TITLE, TraktCopy.row(trakt.status, ::clockTime), onTrakt, lines = 4)
        }
        item(key = "devices-head") { Kicker("Account") }
        item(key = "devices") {
            // One page for the account: its state when joined, else the ways to join one (the web's line).
            // What is wrong or still waits first, never "Synced" over a round that failed or waits (the web's words).
            val last = sync.lastSynced
            val words =
                if (sync.syncing && sync.joined && sync.problem == null) {
                    SyncCopy.SYNCING
                } else {
                    SyncCopy.summary(
                        joined = sync.joined,
                        problem = sync.problem?.words,
                        failed = SyncCopy.failedSince(sync.failed.refused, sync.failed.unreadable, sync.failed.newer),
                        report = sync.lastReport,
                        time =
                            last?.let {
                                java.text.DateFormat
                                    .getTimeInstance(
                                        java.text.DateFormat.SHORT,
                                    ).format(java.util.Date(it))
                            },
                    )
                }
            LinkRow(Icons.MonitorSmartphone, SyncCopy.TITLE, words, onSync)
        }
        item(key = "home-head") { Kicker("Home") }
        item(key = "home-rows") {
            LinkRow(
                Icons.Rows3,
                "Home rows",
                "Which catalogs Home shows after Continue Watching, and in what order.",
                onHomeRows,
            )
        }
        item(key = "playback-head") { Kicker("Playback") }
        onStreamChoice?.let { open ->
            item(key = "stream-choice") {
                val streams = holder.prefs.snapshot?.streams
                LinkRow(
                    Icons.SlidersHorizontal,
                    StreamChoiceCopy.TITLE,
                    streams?.let(StreamChoiceCopy::summary) ?: StreamChoiceCopy.LEDE,
                    open,
                )
            }
        }
        item(key = "playback") { Playback(holder, busy != null) { what, op -> run(what, Place.Playback, op = op) } }
        said?.takeIf { it.place == Place.Playback }?.let { item(key = "playback-said") { SaidLine(it) } }
        item(key = "about-head") { Kicker("About") }
        item(key = "about") { About() }
    }
}

/** A section card (Home's defaults card among them): solid `surface`, no edge (design/DECISION.md, Surfaces). */
@Composable
internal fun Section(
    side: Dp = Marquee.safe.side,
    content: @Composable () -> Unit,
) {
    val shape = RoundedCornerShape(Marquee.radius.l)
    Box(
        Modifier
            .padding(horizontal = side)
            .fillMaxWidth()
            .clip(shape)
            .background(Marquee.colors.surface)
            .padding(if (Marquee.form == Form.Tv) 24.dp else 16.dp),
    ) { content() }
}

/** The active profile's row: its avatar in place of an icon. */
@Composable
private fun ProfileLinkRow(
    p: app.marquee.core.model.ProfileInfo,
    text: String,
    onClick: () -> Unit,
) {
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Marquee.radius.l)
    // On TV the focused row turns white with dark words (focusPalette).
    FocusInk(focusPalette(focused)) {
        val colors = Marquee.colors
        Row(
            Modifier
                .padding(horizontal = Marquee.safe.side)
                .fillMaxWidth()
                .marqueeFocus(Marquee.radius.l, Tokens.FOCUS_SCALE_TILE)
                .onFocusChanged { focused = it.isFocused }
                .clip(shape)
                .background(colors.surface)
                .semantics(mergeDescendants = true) {}
                .focusClickable(onClick, Role.Button)
                .padding(if (Marquee.form == Form.Tv) 24.dp else 16.dp),
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ProfileAvatar(p, if (Marquee.form == Form.Tv) 40.dp else 28.dp)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                MqText(p.name, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 1)
                MqText(text, role = TextRole.Caption, color = colors.muted, maxLines = 2)
            }
            MqIcon(Icons.ChevronRight, tint = colors.muted)
        }
    }
}

/** A row that opens another page: its icon, name and what it does, and a chevron (the web's LinkRow); no edge. */
@Composable
internal fun LinkRow(
    icon: ImageVector,
    title: String,
    text: String,
    onClick: () -> Unit,
    /** Lines the text may take; Find addons' lede is never cut. */
    lines: Int = 2,
    /** The page's own side margin; a grid that pads its items already gives none. */
    side: Dp = Marquee.safe.side,
) {
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Marquee.radius.l)
    // On TV the focused row turns white with dark words (focusPalette).
    FocusInk(focusPalette(focused)) {
        val colors = Marquee.colors
        Row(
            Modifier
                .padding(horizontal = side)
                .fillMaxWidth()
                .marqueeFocus(Marquee.radius.l, Tokens.FOCUS_SCALE_TILE)
                .onFocusChanged { focused = it.isFocused }
                .clip(shape)
                .background(colors.surface)
                .semantics(mergeDescendants = true) {}
                .focusClickable(onClick, Role.Button)
                .padding(if (Marquee.form == Form.Tv) 24.dp else 16.dp),
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            MqIcon(icon, tint = colors.muted)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                MqText(title, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 1)
                MqText(text, role = TextRole.Caption, color = colors.muted, maxLines = lines)
            }
            // Points the way the page goes: left in a right-to-left layout.
            val rtl = LocalLayoutDirection.current == LayoutDirection.Rtl
            MqIcon(
                Icons.ChevronRight,
                modifier = Modifier.graphicsLayer { scaleX = if (rtl) -1f else 1f },
                tint = colors.muted,
            )
        }
    }
}

@Composable
private fun AddonRow(
    addon: AddonInfo,
    first: Boolean,
    last: Boolean,
    busy: Boolean,
    focusRequester: FocusRequester,
    onToggle: () -> Unit,
    onMove: (Int) -> Unit,
    onRemove: () -> Unit,
) {
    val colors = Marquee.colors
    Section {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            // The name line is one switch (as the recommended addons): TalkBack reads the name with its state.
            SwitchRow(
                addon.enabled,
                onToggle,
                Modifier
                    .focusRequester(focusRequester)
                    .marqueeFocus(Marquee.radius.m, Tokens.FOCUS_SCALE_TILE)
                    .clip(RoundedCornerShape(Marquee.radius.m)),
            ) {
                Column(Modifier.weight(1f)) {
                    MqText(addon.name, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 1)
                    // The URL may hold keys: the host stands in for it, or where a built-in row runs.
                    MqText(
                        AddonsCopy.caption(addon),
                        role = TextRole.Caption,
                        // Read here: a TV focus turns the switch row white, and its words dark.
                        color = Marquee.colors.dim,
                        maxLines = 1,
                    )
                }
            }
            addon.description?.let { MqText(it, role = TextRole.Caption, color = colors.muted, maxLines = 2) }
            // Wraps on a small phone or a large font instead of cutting a label.
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.m),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
            ) {
                // Spoken with the addon's name: TalkBack moves between several of these rows.
                val name = addon.name
                RoundButton(
                    Icons.ChevronUp,
                    "Move up",
                    { onMove(-1) },
                    showLabel = true,
                    enabled = !first && !busy,
                    spoken = "Move $name up",
                )
                RoundButton(
                    Icons.ChevronDown,
                    "Move down",
                    { onMove(1) },
                    showLabel = true,
                    enabled = !last && !busy,
                    spoken = "Move $name down",
                )
                // Removing takes a second press within 4 s, as on the web (Apple asks in a dialog).
                var armed by remember(addon.url) { mutableStateOf(false) }
                LaunchedEffect(armed) {
                    if (armed) {
                        delay(4_000)
                        armed = false
                    }
                }
                RoundButton(
                    Icons.Trash,
                    // Short, so the circle barely moves under a finger that presses again.
                    if (armed) "Press again" else "Remove",
                    {
                        if (armed) {
                            armed = false
                            onRemove()
                        } else {
                            armed = true
                        }
                    },
                    showLabel = true,
                    selected = armed,
                    enabled = !busy,
                    spoken = if (armed) "Press again to remove $name" else "Remove $name",
                )
            }
        }
    }
}

/**
 * The link field with the one add-by-link flow ([LinkInstall]): Settings and Find addons' Paste a
 * link show the same request, which goes on when the page closes.
 */
@Composable
internal fun LinkField(
    session: CoreSession,
    link: LinkInstall,
    focus: FocusRequester,
    side: Dp = Marquee.safe.side,
    compact: Boolean = false,
) {
    val said = link.said?.let { Said(Place.Install, it.words, it.failed, it.pending) }
    // A link whose settings carry a sign-in is refused first; the user's confirm sends the field's
    // link (an edit drops the warning, so it is the link warned about), acknowledged.
    InstallField(
        link.busy,
        focus,
        said,
        side,
        compact,
        warned = link.said?.token == true,
        onEdit = link::edited,
        offered = link.offered,
        onTaken = { link.offered = null },
    ) { url, acknowledged, clear ->
        link.run({ ack -> session.ops.installAddon(url, acknowledgeToken = ack).name }, clear, acknowledged)
    }
}

/** A field for an addon link (https, http or stremio://) and the button that adds it. */
@Composable
internal fun InstallField(
    busy: Boolean,
    focusRequester: FocusRequester,
    /** How the last add went, shown under the button. */
    status: Said?,
    side: Dp = Marquee.safe.side,
    /** Find addons' phone page: no heading and the button beside the field, so the cards start higher. */
    compact: Boolean = false,
    /** The last add was refused for a sign-in in the link: offer to add it anyway. */
    warned: Boolean = false,
    /** The text changed. */
    onEdit: () -> Unit = {},
    /** A link another app handed over: it fills the field (once, [onTaken]), and the user's Add adds it. */
    offered: String? = null,
    onTaken: () -> Unit = {},
    /** The link, whether the user confirmed its sign-in, and what clears the field once it is added. */
    onInstall: (String, Boolean, () -> Unit) -> Unit,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    var text by remember { mutableStateOf("") }
    LaunchedEffect(offered) {
        if (offered != null) {
            text = offered
            onEdit()
            onTaken()
        }
    }
    val keyboard = LocalSoftwareKeyboardController.current
    // The keyboard goes, so how it went (under the button) is in sight.
    val add = {
        if (!busy && text.isNotBlank()) {
            keyboard?.hide()
            onInstall(text.trim(), false) { text = "" }
        }
    }
    val field: @Composable (Modifier) -> Unit = { modifier ->
        MqTextField(
            value = text,
            onValueChange = {
                text = it
                onEdit()
            },
            label = "Addon link",
            placeholder = "https://... manifest.json",
            modifier = modifier,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
            // The keyboard's Go adds it, as the button does.
            keyboardActions = KeyboardActions(onGo = { add() }),
        )
    }
    val button: @Composable () -> Unit = {
        PrimaryButton(
            label = "Add",
            icon = Icons.Plus,
            enabled = !busy && text.isNotBlank(),
            onClick = add,
            focusRequester = focusRequester,
        )
    }
    Section(side) {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            if (compact) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    field(Modifier.weight(1f))
                    button()
                }
            } else {
                MqText("Add an addon", role = TextRole.Headline, weight = FontWeight.Bold)
                field(Modifier)
                button()
            }
            // Spoken as it changes (TalkBack): the sign-in warning above all, but also how an add went.
            if (status != null) LiveSpoken(status.text) { SaidLine(status, side = 0.dp) }
            if (warned) {
                SecondaryButton(
                    LinkInstall.TOKEN_CONFIRM,
                    { if (!busy) onInstall(text.trim(), true) { text = "" } },
                    icon = Icons.ShieldAlert,
                )
            }
        }
    }
}

@Composable
private fun Playback(
    holder: SettingsHolder,
    busy: Boolean,
    run: (String, suspend () -> Unit) -> Unit,
) {
    val prefs = holder.prefs.snapshot ?: return
    val ops = holder.session.ops
    Section {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            // The core marks the device's language as it lists it ("nb" reads as "no").
            val device = DeviceCaps.languageCode(LocalConfiguration.current.locales[0])
            val load = remember(ops, device) { suspend { ops.languages(device) } }
            SubtitleLanguage(prefs.subtitleLanguages, load) { list ->
                if (!busy) {
                    run("Changing the subtitle languages") {
                        ops.setPrefs(buildJsonObject { put("subtitleLanguages", json(list)) })
                    }
                }
            }
            MqText("Subtitle size", role = TextRole.Headline, weight = FontWeight.Bold)
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            ) {
                for ((size, words) in SUBTITLE_SIZES) {
                    Chip(
                        words,
                        selected = prefs.subtitleSize == size,
                        onClick = {
                            if (!busy && prefs.subtitleSize != size) {
                                run("Changing the subtitle size") {
                                    ops.setPrefs(buildJsonObject { put("subtitleSize", JsonPrimitive(size.wire)) })
                                }
                            }
                        },
                    )
                }
            }
            MqText("Next episode", role = TextRole.Headline, weight = FontWeight.Bold)
            Chip(
                if (prefs.autoplayNext) "Plays on its own" else "Waits for you",
                selected = prefs.autoplayNext,
                onClick = {
                    if (!busy) {
                        run("Changing autoplay") {
                            ops.setPrefs(buildJsonObject { put("autoplayNext", JsonPrimitive(!prefs.autoplayNext)) })
                        }
                    }
                },
                switch = true,
            )
        }
    }
}

private fun json(list: List<String>) = JsonArray(list.map(::JsonPrimitive))

/** Settings > Playback's Subtitle language: the first chosen language named, then the ordered picker. */
@Composable
internal fun SubtitleLanguage(
    chosen: List<String>,
    load: suspend () -> List<LanguageChoice>,
    onChange: (List<String>) -> Unit,
) {
    MqText("Subtitle language", role = TextRole.Headline, weight = FontWeight.Bold)
    Languages(chosen, load, onChange, first = { name -> name?.let { "$it first" } ?: "None, subtitles start off" })
    MqText(
        "Languages are listed in the order you turn them on; the first one wins. " +
            "A subtitle you pick while watching moves its language first.",
        role = TextRole.Caption,
        color = Marquee.colors.muted,
    )
}

/**
 * A language picker over every language the core knows ([load]): the chosen ones first, in the order
 * they were turned on (the first wins), each a press away from off; then a search by name, which
 * offers the common ones before anything is typed. A pick clears the search. [first] words a line over
 * the chips from the first chosen language's name (null for none).
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun Languages(
    chosen: List<String>,
    load: suspend () -> List<LanguageChoice>,
    onChange: (List<String>) -> Unit,
    /** Audio takes the base codes ("pt" for "pt-BR"); subtitles take the core's codes as they are. */
    audio: Boolean = false,
    first: ((String?) -> String)? = null,
) {
    val all by produceState(emptyList<LanguageChoice>(), load) {
        value =
            try {
                load().let { if (audio) LanguagePicks.audio(it) else it }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                emptyList()
            }
    }
    var query by remember { mutableStateOf("") }
    // The device's own language is offered first, by the code the core reads ("he", never Java's old "iw").
    val device = DeviceCaps.languageCode(LocalConfiguration.current.locales[0])
    // On a remote, focus follows a picked language to its new place with the chosen ones, so a second
    // press of select never lands on the language that took its slot.
    val tv = Marquee.form == Form.Tv
    val focusers = remember { HashMap<String, FocusRequester>() }
    var follow by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(follow, chosen) {
        val code = follow?.takeIf { it in chosen } ?: return@LaunchedEffect
        withFrameNanos { }
        runCatching { focusers[code]?.requestFocus() }
        follow = null
    }
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        first?.let {
            MqText(
                it(
                    chosen.firstOrNull()?.let { code ->
                        LanguagePicks.name(code, all)
                    },
                ),
                role = TextRole.Callout,
            )
        }
        if (chosen.isNotEmpty()) {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            ) {
                for (code in chosen) {
                    key(code) {
                        val requester = remember { FocusRequester() }
                        DisposableEffect(requester) {
                            focusers[code] = requester
                            onDispose { focusers.remove(code) }
                        }
                        Chip(
                            LanguagePicks.name(code, all),
                            selected = true,
                            onClick = { onChange(chosen - code) },
                            focusRequester = requester,
                            switch = true,
                        )
                    }
                }
            }
        }
        MqTextField(
            value = query,
            onValueChange = { query = it },
            label = "Find a language",
            placeholder = "Polish, Korean, Arabic",
        )
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            for (choice in LanguagePicks.search(all, query, chosen, device)) {
                key(choice.code) {
                    Chip(
                        choice.name,
                        selected = false,
                        onClick = {
                            onChange(chosen + choice.code)
                            query = ""
                            if (tv) follow = choice.code
                        },
                        switch = true,
                    )
                }
            }
        }
    }
}

@Composable
private fun About() {
    val context = LocalContext.current
    val version =
        remember {
            runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull()
        }
    Section {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            // The headline under it says the name.
            BrandLockup(
                if (Marquee.form == Form.Tv) 48.dp else 36.dp,
                Modifier.padding(bottom = Marquee.space.xs),
                spoken = false,
            )
            MqText("Momotus ${version.orEmpty()}", role = TextRole.Headline, weight = FontWeight.Bold)
            MqText("Free software under the GPL 3.", role = TextRole.Caption, color = Marquee.colors.muted)
            MqText(
                "Type: Inter and Unbounded (SIL Open Font License). " +
                    "Icons: Lucide (ISC). Flags: circle-flags (MIT).",
                role = TextRole.Caption,
                color = Marquee.colors.muted,
            )
            MqText(AddonsCopy.TURKCEALTYAZI_CREDIT, role = TextRole.Caption, color = Marquee.colors.muted)
            // TMDB's terms: its logo and this line wherever its data shows.
            Row(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Image(Marks.Tmdb, contentDescription = null, modifier = Modifier.height(16.dp))
                MqText(TMDB_CREDIT, role = TextRole.Caption, color = Marquee.colors.muted)
            }
            if (CoreBuild.missing) MqText(CoreBuild.ABOUT_LINE, role = TextRole.Caption, color = Marquee.colors.muted)
        }
    }
}

/** TMDB's attribution line (its terms ask for it with the logo). */
internal const val TMDB_CREDIT = "This product uses the TMDB API but is not endorsed or certified by TMDB."

/** The subtitle sizes Settings offers, in order (the profile's, on every device). */
internal val SUBTITLE_SIZES =
    listOf(
        SubtitleSize.Small to "Small",
        SubtitleSize.Medium to "Medium",
        SubtitleSize.Large to "Large",
        SubtitleSize.ExtraLarge to "Extra large",
    )
