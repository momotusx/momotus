package app.marquee.android

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
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
import app.marquee.design.MqText
import app.marquee.design.PrimaryButton
import app.marquee.design.RoundButton
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.TextRole
import app.marquee.design.dpadExits
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
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
)

@Composable
private fun SaidLine(said: Said) {
    if (said.failed) {
        Note(
            said.text,
            failed = true,
        )
    } else {
        StatusWord(said.text, Status.Ok, Modifier.padding(horizontal = Marquee.safe.side))
    }
}

@Composable
fun SettingsScreen(session: CoreSession) {
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
        said = null
        scope.launch {
            try {
                op()
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
        Modifier.fillMaxSize(),
        contentPadding = pagePadding(),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") { BrowseHeader("Settings", "Your Marquee") }
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
        item(key = "playback-head") { Kicker("Playback") }
        item(key = "playback") { Playback(holder, busy != null) { what, op -> run(what, Place.Playback, op = op) } }
        said?.takeIf { it.place == Place.Playback }?.let { item(key = "playback-said") { SaidLine(it) } }
        item(key = "about-head") { Kicker("About") }
        item(key = "about") { About() }
    }
}

/** A section card on glass. */
@Composable
private fun Section(content: @Composable () -> Unit) {
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
                Chip(
                    if (addon.enabled) "On" else "Off",
                    selected = addon.enabled,
                    onClick = onToggle,
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
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Marquee.radius.pill)
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
            Box(
                Modifier
                    .fillMaxWidth()
                    .height(if (tv) 72.dp else 48.dp)
                    .clip(shape)
                    .background(colors.raised)
                    .border(2.dp, if (focused) colors.accent else colors.control, shape)
                    .padding(horizontal = if (tv) 24.dp else 16.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                BasicTextField(
                    value = text,
                    onValueChange = { text = it },
                    singleLine = true,
                    textStyle =
                        TextStyle(
                            color = colors.text,
                            fontSize = Marquee.type.callout,
                            fontFamily = MarqueeFonts.text,
                        ),
                    cursorBrush = SolidColor(colors.accent),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Go),
                    // The keyboard's Go adds it, as the button does.
                    keyboardActions = KeyboardActions(onGo = { add() }),
                    modifier =
                        Modifier
                            .fillMaxWidth()
                            .dpadExits()
                            .then(Modifier.onFocusChanged { focused = it.isFocused })
                            .semantics { contentDescription = "Addon link" },
                    decorationBox = { inner ->
                        if (text.isEmpty()) MqText("https://... manifest.json", color = colors.dim, maxLines = 1)
                        inner()
                    },
                )
            }
            PrimaryButton(
                label = "Add",
                icon = Icons.Plus,
                enabled = !busy && text.isNotBlank(),
                onClick = add,
                focusRequester = focusRequester,
            )
            if (status != null) {
                if (status.failed) Note(status.text, failed = true, side = 0.dp) else StatusWord(status.text, Status.Ok)
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
            MqText("Quality", role = TextRole.Headline, weight = FontWeight.Bold)
            Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
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
