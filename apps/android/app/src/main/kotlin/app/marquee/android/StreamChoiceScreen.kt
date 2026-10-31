package app.marquee.android

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.StreamChoiceSettings
import app.marquee.core.client.DeviceCapsParams
import app.marquee.core.model.Network
import app.marquee.core.model.Prefs
import app.marquee.core.model.Speed
import app.marquee.core.session.CoreSession
import app.marquee.core.views.SettingsView
import app.marquee.design.Chip
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.SecondaryButton
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.TextRole

class StreamChoiceHolder(
    session: CoreSession,
) : ViewModel() {
    val prefs: SettingsView = session.settings()
    val page = StreamChoiceSettings(session.ops, viewModelScope).also { it.load() }

    override fun onCleared() = prefs.close()
}

/**
 * Settings > Stream choice (plans/m13-stream-choice.md 4.4, the web's page): one glass panel of rows,
 * each a set of chips that writes one preference; what the picks taught, each with Undo, and Forget;
 * and "This device" with what the core knows of the connection and the disk. Every word comes from
 * `apps/shared/stream-choice-words.json` ([StreamChoiceCopy]).
 */
@Composable
fun StreamChoiceScreen(session: CoreSession) {
    val holder =
        viewModel<StreamChoiceHolder>(
            key = "stream-choice",
            factory = viewModelFactory { initializer { StreamChoiceHolder(session) } },
        )
    val prefs = holder.prefs.current()
    val page = holder.page
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        item(key = "head") {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                BrowseHeader(StreamChoiceCopy.TITLE)
                MqText(
                    StreamChoiceCopy.LEDE,
                    modifier = Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 900.dp),
                    role = TextRole.Callout,
                    color = Marquee.colors.muted,
                )
                page.problem?.let { Note(it, failed = true) }
                if (prefs == null) Note("Loading")
            }
        }
        if (prefs != null) {
            item(key = "rows") {
                val account =
                    (
                        session.debrid
                            .current()
                            ?.accounts
                            ?.size ?: 0
                    ) > 0
                Section { Rows(prefs, page, account) }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Rows(
    prefs: Prefs,
    page: StreamChoiceSettings,
    account: Boolean,
) {
    val s = prefs.streams
    // Undo and Forget take their row away: focus goes to the Learn choice, never to Forget (a second
    // press there would clear the rest).
    val learnFocus = remember { FocusRequester() }
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        Choices(StreamChoiceCopy.SIZE, { it == s.sizeLimitBytes }) { page.set("sizeLimitBytes", it) }
        Hairline()
        Choices(StreamChoiceCopy.SEEDERS, { it == s.minSeeders }) { page.set("minSeeders", it) }
        Hairline()
        Choices(StreamChoiceCopy.RESOLUTION, { StreamChoiceSettings.resolutionOn(it, s.maxHeight) }) {
            page.set("maxHeight", it)
        }
        Hairline()
        Choices(StreamChoiceCopy.HDR, { it == s.hdr }) { page.set("hdr", it) }
        if (account || !StreamChoiceCopy.READY_ONLY_WITH_ACCOUNT) {
            Hairline()
            Choices(StreamChoiceCopy.READY, { it == s.preferReady }) { page.set("preferReady", it) }
        }
        Hairline()
        Labelled(StreamChoiceCopy.LEARN.label, null) {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            ) {
                for (choice in StreamChoiceCopy.LEARN.choices) {
                    val on = choice.value == s.learn
                    Chip(
                        choice.label,
                        selected = on,
                        onClick = { page.set("learn", choice.value) },
                        focusRequester = learnFocus.takeIf { on },
                    )
                }
            }
            if (StreamChoiceCopy.anyLearned(prefs)) {
                val back = { runCatching { learnFocus.requestFocus() } }
                for (row in StreamChoiceCopy.learnedRows(prefs)) {
                    FlowRow(
                        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
                        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                        itemVerticalAlignment = Alignment.CenterVertically,
                    ) {
                        MqText(row.text, role = TextRole.Callout)
                        SecondaryButton(StreamChoiceCopy.UNDO, {
                            back()
                            page.undo(row)
                        }, icon = Icons.Undo2)
                    }
                }
                SecondaryButton(StreamChoiceCopy.FORGET, {
                    back()
                    page.forget()
                }, icon = Icons.RotateCcw)
            }
        }
        Hairline()
        Labelled(StreamChoiceCopy.DEVICE, null) { Device(page, account) }
    }
}

/** A row: its label (and help) beside the chips on TV, over them on a phone. */
@Composable
private fun Labelled(
    label: String,
    help: String?,
    content: @Composable () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val head: @Composable (Modifier) -> Unit = { modifier ->
        Column(modifier, verticalArrangement = Arrangement.spacedBy(4.dp)) {
            MqText(label, role = TextRole.Headline, weight = FontWeight.Bold)
            help?.let { MqText(it, role = TextRole.Caption, color = Marquee.colors.muted) }
        }
    }
    val body: @Composable (Modifier) -> Unit = { modifier ->
        Column(modifier, verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) { content() }
    }
    if (tv) {
        Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.m)) {
            head(Modifier.width(360.dp))
            body(Modifier.weight(1f))
        }
    } else {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            head(Modifier)
            body(Modifier)
        }
    }
}

/** One preference's chips, one of them on. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun <T> Choices(
    setting: StreamChoiceCopy.Setting<T>,
    on: (T) -> Boolean,
    onPick: (T) -> Unit,
) = Labelled(setting.label, setting.help) {
    // Wraps on a narrow phone rather than squeezing the last chip.
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        for (choice in setting.choices) {
            Chip(
                choice.label,
                selected = on(choice.value),
                onClick = { onPick(choice.value) },
            )
        }
    }
}

@Composable
private fun Hairline() =
    androidx.compose.foundation.layout.Box(
        Modifier
            .fillMaxWidth()
            .height(1.dp)
            .background(Marquee.colors.line),
    )

/** "This device": the screen, the connection and the disk; "Measure again" once a speed was measured. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Device(
    page: StreamChoiceSettings,
    account: Boolean,
) {
    val tv = Marquee.form == Form.Tv
    val context = LocalContext.current
    val caps by produceState<DeviceCapsParams?>(null) {
        value = runCatching { (context.applicationContext as MarqueeApp).deviceCaps() }.getOrNull()
    }
    val c = page.conditions
    val line =
        StreamChoiceCopy.conditionsLine(
            screenClass = StreamChoiceCopy.screenClass(caps?.displayHeight ?: 1080),
            hdr = caps?.hdr.orEmpty().isNotEmpty(),
            network = c?.network ?: caps?.network ?: Network.Other,
            speed = c?.let { StreamChoiceCopy.speedFor(it.http, it.p2p, it.engine, account) } ?: Speed(0f, 0),
        )
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        itemVerticalAlignment = Alignment.CenterVertically,
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
            MqIcon(if (tv) Icons.Tv else Icons.Smartphone, tint = Marquee.colors.muted)
            MqText(line, role = TextRole.Callout)
        }
        // It stays after a press (spent), so focus is not lost.
        if (page.measureShown) {
            SecondaryButton(StreamChoiceCopy.MEASURE_AGAIN, page::measureAgain, icon = Icons.RotateCcw)
        }
        if (page.measuring) StatusWord(StreamChoiceCopy.MEASURING, Status.Idle)
    }
}
