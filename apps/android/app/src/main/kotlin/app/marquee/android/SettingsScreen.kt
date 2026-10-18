package app.marquee.android

import androidx.compose.foundation.background
import androidx.compose.foundation.border
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
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.DebridCopy
import app.marquee.core.VersionsCopy
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.AddonInfo
import app.marquee.core.model.QualityPref
import app.marquee.core.session.CoreSession
import app.marquee.core.views.AddonsView
import app.marquee.core.views.SettingsView
import app.marquee.core.views.ViewStatus
import app.marquee.design.Chip
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MarqueeFonts
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.MqTextField
import app.marquee.design.PrimaryButton
import app.marquee.design.RoundButton
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.TextRole
import app.marquee.design.dpadExits
import app.marquee.design.focusClickable
import app.marquee.design.marqueeFocus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
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
private enum class Place { Addons, Install, Playback }

/** What an operation ended with, shown under the part of the page it came from. */
private class Said(
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
    onHomeRows: () -> Unit,
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
    LazyColumn(
        // Down from the TV pill comes back to what had focus (plans 2.1).
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") { BrowseHeader("Settings", "Your Marquee") }
        gate.snapshot?.let { s ->
            val me = s.profiles.firstOrNull { it.id == s.active }
            if (me != null) {
                item(key = "profiles-head") { Kicker("Profiles") }
                item(key = "profiles") {
                    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                        ProfileLinkRow(
                            me,
                            if (s.profiles.size > 1) "Switch profile" else "Add a profile for someone else, or switch",
                            onProfiles,
                        )
                        LinkRow(Icons.UserPen, "Edit profile", "Name, picture and PIN.") { onEditProfile(me.id) }
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
        item(key = "addons-head") { Kicker("Addons") }
        when {
            holder.addons.status is ViewStatus.Failed -> {
                item(key = "addons-failed") { Note("Addons could not load", failed = true) }
            }

            addons.isEmpty() && holder.addons.snapshot != null -> {
                item(key = "addons-none") { Note("No addon yet. Paste an addon link below to add one.") }
            }
        }
        itemsIndexed(addons, key = { _, addon -> "addon:${addon.url}" }) { index, addon ->
            val requester = requesters.getOrPut(addon.url) { FocusRequester() }
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
        item(key = "install") {
            InstallField(busy != null, installFocus, said?.takeIf { it.place == Place.Install }) { url, clear ->
                run("Adding the addon", Place.Install, then = clear) {
                    val added = session.ops.installAddon(url)
                    said = Said(Place.Install, "Added ${added.name}", failed = false)
                }
            }
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
                session.secrets
                    .load()
                    .debridKeys.size
            LinkRow(
                Icons.KeyRound,
                "Debrid services",
                if (count == 0) "Play torrents as fast direct links with a debrid key." else DebridCopy.inUse(count),
                onDebrid,
            )
        }
        item(key = "devices-head") { Kicker("Devices and sync") }
        item(key = "devices") {
            if (sync.joined) {
                val words = sync.problem ?: if (sync.syncing) "Syncing" else "Library, progress and addons follow you."
                LinkRow(Icons.RefreshCw, "Sync", words, onSync)
            } else {
                LinkRow(
                    Icons.QrCode,
                    "Pair this device",
                    "Join your sync account by scanning a code on a device that already syncs.",
                    onSync,
                )
            }
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
        item(key = "playback") { Playback(holder, busy != null) { what, op -> run(what, Place.Playback, op = op) } }
        said?.takeIf { it.place == Place.Playback }?.let { item(key = "playback-said") { SaidLine(it) } }
        item(key = "about-head") { Kicker("About") }
        item(key = "about") { About() }
    }
}

/** A section card on glass. */
@Composable
internal fun Section(content: @Composable () -> Unit) {
    val shape = RoundedCornerShape(Marquee.radius.l)
    val colors = Marquee.colors
    Box(
        Modifier
            .padding(horizontal = Marquee.safe.side)
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surface)
            .border(1.dp, colors.edge, shape)
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
    val shape = RoundedCornerShape(Marquee.radius.l)
    val colors = Marquee.colors
    Row(
        Modifier
            .padding(horizontal = Marquee.safe.side)
            .fillMaxWidth()
            .marqueeFocus(Marquee.radius.l, scale = 1.01f)
            .clip(shape)
            .background(colors.surface)
            .border(1.dp, colors.edge, shape)
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

/** A row that opens another page: its icon, name and what it does, and a chevron (the web's LinkRow). */
@Composable
private fun LinkRow(
    icon: ImageVector,
    title: String,
    text: String,
    onClick: () -> Unit,
) {
    val shape = RoundedCornerShape(Marquee.radius.l)
    val colors = Marquee.colors
    Row(
        Modifier
            .padding(horizontal = Marquee.safe.side)
            .fillMaxWidth()
            .marqueeFocus(Marquee.radius.l, scale = 1.01f)
            .clip(shape)
            .background(colors.surface)
            .border(1.dp, colors.edge, shape)
            .semantics(mergeDescendants = true) {}
            .focusClickable(onClick, Role.Button)
            .padding(if (Marquee.form == Form.Tv) 24.dp else 16.dp),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(icon, tint = colors.muted)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            MqText(title, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 1)
            MqText(text, role = TextRole.Caption, color = colors.muted, maxLines = 2)
        }
        MqIcon(Icons.ChevronRight, tint = colors.muted)
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
            Row(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    MqText(addon.name, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 1)
                    // The URL may hold keys: the host stands in for it.
                    MqText(
                        "${addon.host} · ${addon.version}",
                        role = TextRole.Caption,
                        color = colors.dim,
                        maxLines = 1,
                    )
                }
                // Named for its addon, so TalkBack moving between switches says which one it is.
                Chip(
                    if (addon.enabled) "On" else "Off",
                    selected = addon.enabled,
                    onClick = onToggle,
                    modifier = Modifier.semantics { contentDescription = addon.name },
                    focusRequester = focusRequester,
                    switch = true,
                )
            }
            addon.description?.let { MqText(it, role = TextRole.Caption, color = colors.muted, maxLines = 2) }
            Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.m)) {
                RoundButton(Icons.ChevronUp, "Move up", { onMove(-1) }, showLabel = true, enabled = !first && !busy)
                RoundButton(Icons.ChevronDown, "Move down", { onMove(1) }, showLabel = true, enabled = !last && !busy)
                RoundButton(Icons.Trash, "Remove", onRemove, showLabel = true, enabled = !busy)
            }
        }
    }
}

/** A field for an addon link (https, http or stremio://) and the button that adds it. */
@Composable
private fun InstallField(
    busy: Boolean,
    focusRequester: FocusRequester,
    /** How the last add went, shown under the button. */
    status: Said?,
    /** The link, and what clears the field once it is added. */
    onInstall: (String, () -> Unit) -> Unit,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    var text by remember { mutableStateOf("") }
    val keyboard = LocalSoftwareKeyboardController.current
    // The keyboard goes, so how it went (under the button) is in sight.
    val add = {
        if (!busy && text.isNotBlank()) {
            keyboard?.hide()
            onInstall(text.trim()) { text = "" }
        }
    }
    Section {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            MqText("Add an addon", role = TextRole.Headline, weight = FontWeight.Bold)
            MqTextField(
                value = text,
                onValueChange = { text = it },
                label = "Addon link",
                placeholder = "https://... manifest.json",
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
                // The keyboard's Go adds it, as the button does.
                keyboardActions = KeyboardActions(onGo = { add() }),
            )
            PrimaryButton(
                label = "Add",
                icon = Icons.Plus,
                enabled = !busy && text.isNotBlank(),
                onClick = add,
                focusRequester = focusRequester,
            )
            if (status != null) SaidLine(status, side = 0.dp)
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
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
            MqText("Quality", role = TextRole.Headline, weight = FontWeight.Bold)
            // Wraps on a narrow phone rather than squeezing the last chip.
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            ) {
                for ((value, label) in QUALITIES) {
                    Chip(label, selected = prefs.streams.quality == value, onClick = {
                        if (!busy) {
                            run("Changing the quality") {
                                ops.setPrefs(
                                    buildJsonObject { put("streams", buildJsonObject { put("quality", wire(value)) }) },
                                )
                            }
                        }
                    })
                }
            }
            MqText("Up to", role = TextRole.Headline, weight = FontWeight.Bold)
            // Wraps on a narrow phone rather than squeezing the last chip.
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            ) {
                for ((value, label) in HEIGHTS) {
                    Chip(label, selected = prefs.streams.maxHeight == value, onClick = {
                        if (!busy) {
                            run("Changing the resolution") {
                                val height = value?.let(::JsonPrimitive) ?: JsonNull
                                ops.setPrefs(
                                    buildJsonObject { put("streams", buildJsonObject { put("maxHeight", height) }) },
                                )
                            }
                        }
                    })
                }
            }
            // Several may be on; the list keeps the order they were turned on in, the first wins.
            MqText("Audio", role = TextRole.Headline, weight = FontWeight.Bold)
            Languages(LANGS.map { it.first }, prefs.streams.languages) { list ->
                if (!busy) {
                    run("Changing the audio languages") {
                        ops.setPrefs(
                            buildJsonObject { put("streams", buildJsonObject { put("languages", json(list)) }) },
                        )
                    }
                }
            }
            MqText("Subtitles", role = TextRole.Headline, weight = FontWeight.Bold)
            Languages(LANGS.map { it.second }, prefs.subtitleLanguages) { list ->
                if (!busy) {
                    run("Changing the subtitle languages") {
                        ops.setPrefs(buildJsonObject { put("subtitleLanguages", json(list)) })
                    }
                }
            }
            MqText(
                "Languages are listed in the order you turn them on; the first one wins.",
                role = TextRole.Caption,
                color = Marquee.colors.muted,
            )
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

private val QUALITIES =
    listOf(
        QualityPref.Best to "Best",
        QualityPref.Balanced to "Balanced",
        QualityPref.DataSaver to "Data saver",
    )

private val HEIGHTS = listOf(null to "Any resolution", 2160 to "4K", 1080 to "1080p", 720 to "720p")

/** The web's languages: the audio code (ISO 639-1) and the subtitle code addons use (ISO 639-2). */
private val LANGS =
    listOf(
        "en" to "eng",
        "tr" to "tur",
        "es" to "spa",
        "fr" to "fre",
        "de" to "ger",
        "it" to "ita",
        "pt" to "por",
        "ja" to "jpn",
    )

private fun json(list: List<String>) = JsonArray(list.map(::JsonPrimitive))

/** A chip per language, each on or off on its own; turning one on puts it last, so the order is the user's. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Languages(
    codes: List<String>,
    chosen: List<String>,
    onChange: (List<String>) -> Unit,
) {
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        for (code in codes) {
            val on = code in chosen
            Chip(
                VersionsCopy.languageName(code),
                selected = on,
                onClick = { onChange(if (on) chosen - code else chosen + code) },
                switch = true,
            )
        }
    }
}

/** The wire name of a quality ("dataSaver"). */
private fun wire(value: QualityPref): String = value.name.replaceFirstChar { it.lowercase() }

@Composable
private fun About() {
    val context = LocalContext.current
    val version =
        remember {
            runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull()
        }
    Section {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            MqText("Marquee ${version.orEmpty()}", role = TextRole.Headline, weight = FontWeight.Bold)
            MqText("Free software under the GPL 3.", role = TextRole.Caption, color = Marquee.colors.muted)
            MqText(
                "Type: Inter (SIL Open Font License). Icons: Lucide (ISC).",
                role = TextRole.Caption,
                color = Marquee.colors.muted,
            )
        }
    }
}
