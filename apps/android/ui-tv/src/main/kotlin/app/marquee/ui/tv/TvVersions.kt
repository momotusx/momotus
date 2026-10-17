@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.tv

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.marquee.core.Format
import app.marquee.core.VersionsCopy
import app.marquee.core.model.Group
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.views.VersionsView
import app.marquee.core.views.ViewStatus
import app.marquee.design.CapsLabel
import app.marquee.design.Eyebrow
import app.marquee.design.FactsGrid
import app.marquee.design.GlassBar
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.PlayFilled
import app.marquee.design.PrimaryButton
import app.marquee.design.SecondaryButton
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.TextRole
import app.marquee.design.focusClickable
import app.marquee.design.marqueeFocus

private const val PLAY = "play"
private const val ADD = "add"
private const val ROWS = "rows"

/** How many rows of a group show before "Show all", as on the web. */
private const val SHOWN = 3

/**
 * The ranked sources of one video (plans/android-design-v2.md 5.6, the web's v2 screen): the
 * heading, how the search goes and what this screen plays; the recommendation as a glass card
 * with its facts and plain reasons; every other source in a table by group, the focused row
 * opened to its release name; and what is hidden and why. The first arrow key freezes the
 * recommendation, so the pick under the cursor never changes while results still stream in.
 *
 * @param heading what the screen is about ("Sintel" or "Sintel, S1:E2").
 * @param device what the ranking was made for ("This screen: 1080p, plays H.264 in MP4").
 * @param onPlay a version was chosen.
 * @param onAddSource no addon gave a version: go where one is added.
 */
@Composable
fun TvVersions(
    heading: String,
    device: String?,
    versions: VersionsView?,
    focus: FocusMemory,
    onPlay: (VersionRow) -> Unit,
    onAddSource: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val safe = Marquee.safe
    val snapshot = versions?.snapshot
    val failure = (versions?.status as? ViewStatus.Failed)?.reason
    val nothing = snapshot != null && snapshot.done && snapshot.recommended == null && snapshot.rows.isEmpty()
    // The target exists once there is something to focus: the card's Play (or Try again, or Add a source), else the first row.
    val target =
        when {
            failure != null || snapshot?.recommended != null -> PLAY
            nothing -> ADD
            else -> snapshot?.rows?.firstOrNull()?.let { rowKey(it) }
        }
    LaunchedEffect(target) {
        // A remembered row that is no longer in the list (it moved into the card) is not a target.
        val wanted = focus.focusedKey?.takeIf { focus.requester(it) != null } ?: target
        if (wanted != null) focus.focusWhenReady(wanted, waitMs = 10_000)
    }

    Column(
        modifier
            .fillMaxSize()
            .onPreviewKeyEvent { event ->
                if (event.type == KeyEventType.KeyDown) {
                    focus.interacted()
                    // First move of any kind pins the recommendation, once there is one to pin: a pin
                    // before that would keep the card empty while sources stream in.
                    if (event.key in arrows && snapshot?.recommended != null && !snapshot.frozen) versions?.freeze()
                }
                false
            }.padding(start = safe.side, end = safe.side, top = safe.top, bottom = safe.bottom),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.l),
    ) {
        Header(heading, device, snapshot)
        Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(Marquee.space.l)) {
            Pick(snapshot, failure, { versions?.retry() }, onAddSource, focus, onPlay)
            Sources(snapshot, focus, onPlay, Modifier.weight(1f))
        }
    }
}

private val arrows = setOf(Key.DirectionLeft, Key.DirectionRight, Key.DirectionUp, Key.DirectionDown)

private fun rowKey(row: VersionRow) = "row:${row.key}"

private fun tone(tone: VersionsCopy.Tone) =
    when (tone) {
        VersionsCopy.Tone.Idle -> Status.Idle
        VersionsCopy.Tone.Ok -> Status.Ok
        VersionsCopy.Tone.Warn -> Status.Warn
        VersionsCopy.Tone.Bad -> Status.Bad
    }

private fun statusTone(row: VersionRow) = if (VersionsCopy.good(row.status)) Status.Ok else Status.Warn

/** The heading as an eyebrow over "Versions", and at the right how the search goes and this screen. */
@Composable
private fun Header(
    heading: String,
    device: String?,
    snapshot: VersionsSnapshot?,
) {
    val colors = Marquee.colors
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Bottom) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            Eyebrow(heading)
            MqText("Versions", role = TextRole.Title1, maxLines = 1)
        }
        Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            val (text, state) = VersionsCopy.progress(snapshot)
            StatusWord(text, tone(state))
            if (device != null) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    MqIcon(Icons.Monitor, tint = colors.muted, size = 24.dp)
                    MqText(device, role = TextRole.Callout, color = colors.muted, maxLines = 1)
                }
            }
        }
    }
}

/** The recommendation card, or why there is none yet. */
@Composable
private fun Pick(
    snapshot: VersionsSnapshot?,
    failure: String?,
    onRetry: () -> Unit,
    onAddSource: () -> Unit,
    focus: FocusMemory,
    onPlay: (VersionRow) -> Unit,
) {
    val colors = Marquee.colors
    val shape = RoundedCornerShape(Marquee.radius.l)
    val best = snapshot?.recommended
    Column(
        Modifier
            .width(620.dp)
            .clip(shape)
            .background(colors.glassStrong)
            .border(1.dp, colors.edge, shape)
            .padding(32.dp),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        when {
            failure != null -> {
                StateMessage(
                    title = "Versions could not be loaded",
                    body = failure,
                    icon = Icons.CircleAlert,
                    actionLabel = "Try again",
                    onAction = onRetry,
                    actionFocus = focus.requesterFor(PLAY),
                    modifier = Modifier.padding(0.dp),
                )
            }

            best != null -> {
                CapsLabel("Recommended for this screen")
                MqText(VersionsCopy.pickHeading(best), role = TextRole.Title1, maxLines = 1)
                FactsGrid(VersionsCopy.facts(best), tones = mapOf("Status" to statusTone(best)))
                Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    for (reason in best.reasons.take(3)) {
                        Row(
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            MqIcon(Icons.Check, tint = colors.ok, size = 26.dp)
                            MqText(VersionsCopy.reason(reason, best.status), role = TextRole.Callout, maxLines = 2)
                        }
                    }
                }
                Release(best.releaseName, maxLines = 3)
                PrimaryButton(
                    label = "Play this version",
                    icon = PlayFilled,
                    onClick = { onPlay(best) },
                    modifier = Modifier.padding(top = Marquee.space.xs),
                    focusRequester = focus.requesterFor(PLAY),
                    onFocusChange = focus.watch(null, PLAY),
                )
                Row(
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    MqIcon(Icons.RefreshCw, tint = colors.muted, size = 22.dp)
                    MqText(VersionsCopy.failover(best), role = TextRole.Caption, color = colors.muted, maxLines = 2)
                }
            }

            snapshot == null || !snapshot.done -> {
                CapsLabel("Recommended for this screen")
                Skeleton(Modifier.size(260.dp, 56.dp))
                Skeleton(Modifier.size(420.dp, 28.dp))
                Skeleton(Modifier.size(360.dp, 28.dp))
                Skeleton(Modifier.size(300.dp, 28.dp))
            }

            else -> {
                val none = snapshot.rows.isEmpty()
                StateMessage(
                    title = VersionsCopy.waitingTitle(snapshot),
                    body = VersionsCopy.waitingBody(snapshot),
                    icon = Icons.CircleAlert,
                    actionLabel = "Add a source".takeIf { none },
                    onAction = onAddSource.takeIf { none },
                    actionFocus = focus.requesterFor(ADD).takeIf { none },
                    modifier = Modifier.padding(0.dp),
                )
            }
        }
    }
}

/** A release name in monospace, broken at its dots so it wraps whole. */
@Composable
private fun Release(
    name: String,
    maxLines: Int,
    modifier: Modifier = Modifier,
) {
    // ponytail: the system monospace face; IBM Plex Mono as on the web when Android bundles it.
    MqText(
        Format.releaseWrapped(name),
        modifier = modifier,
        role = TextRole.Caption,
        color = Marquee.colors.muted,
        maxLines = maxLines,
        fontFamily = FontFamily.Monospace,
    )
}

/** Column widths of the table; Features takes the rest. */
private val QUALITY: Dp = 150.dp
private val SIZE: Dp = 150.dp
private val STATUS: Dp = 300.dp
private val VIA: Dp = 220.dp

@Composable
private fun Sources(
    snapshot: VersionsSnapshot?,
    focus: FocusMemory,
    onPlay: (VersionRow) -> Unit,
    modifier: Modifier,
) {
    val colors = Marquee.colors
    val groups = snapshot?.rows.orEmpty().groupBy { it.group }
    val open = remember { mutableStateMapOf<Group, Boolean>() }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        if (snapshot?.rows.orEmpty().isNotEmpty()) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 22.dp),
            ) {
                CapsLabel("Quality", Modifier.width(QUALITY))
                CapsLabel("Features", Modifier.weight(1f))
                CapsLabel("Size", Modifier.width(SIZE))
                CapsLabel("Status", Modifier.width(STATUS))
                CapsLabel("Via", Modifier.width(VIA))
            }
            Line(colors.lineStrong)
        }
        LazyColumn(
            modifier =
                Modifier
                    .fillMaxWidth()
                    .focusProperties { enter = { focus.rowEntry(ROWS) ?: FocusRequester.Default } }
                    .focusGroup(),
            contentPadding = PaddingValues(vertical = Marquee.space.xs),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            for (group in Group.entries) {
                val rows = groups[group].orEmpty()
                if (rows.isEmpty()) continue
                val all = open[group] == true
                item(key = "group:$group") {
                    GlassBar(Modifier.padding(top = Marquee.space.xs)) {
                        CapsLabel(
                            VersionsCopy.group(group, snapshot?.recommended != null),
                            Modifier.weight(1f),
                            colors.text,
                        )
                        MqText("${rows.size}", role = TextRole.Callout, maxLines = 1)
                    }
                }
                items(
                    if (all) rows else rows.take(SHOWN),
                    key = { rowKey(it) },
                ) { row -> SourceRow(row, focus, onPlay) }
                if (rows.size > SHOWN) {
                    item(key = "fold:$group") {
                        SecondaryButton(
                            if (all) "Show the best $SHOWN" else "Show all ${rows.size}",
                            { open[group] = !all },
                            Modifier.padding(start = 22.dp, top = 4.dp),
                            icon = if (all) Icons.ChevronUp else Icons.ChevronDown,
                        )
                    }
                }
            }
            val hidden = snapshot?.hidden?.takeIf { it.count > 0 }
            if (hidden != null) {
                item(key = "hidden") {
                    GlassBar(Modifier.padding(top = Marquee.space.s)) {
                        MqIcon(Icons.EyeOff, tint = colors.muted, size = 26.dp)
                        MqText(
                            VersionsCopy.hiddenTitle(hidden),
                            role = TextRole.Callout,
                            weight = FontWeight.SemiBold,
                            maxLines = 1,
                        )
                        MqText(
                            VersionsCopy.hiddenDetail(hidden),
                            role = TextRole.Callout,
                            color = colors.muted,
                            maxLines = 2,
                            modifier = Modifier.weight(1f),
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun Line(color: Color) {
    Row(
        Modifier
            .fillMaxWidth()
            .height(1.dp)
            .background(color),
    ) {}
}

/** One source: its cells on a hairline, opened to the release name and its languages while focused. */
@Composable
private fun SourceRow(
    row: VersionRow,
    focus: FocusMemory,
    onPlay: (VersionRow) -> Unit,
) {
    val colors = Marquee.colors
    val key = rowKey(row)
    val shape = RoundedCornerShape(Marquee.radius.m)
    val watch = focus.watch(ROWS, key)
    var focused by remember { mutableStateOf(false) }
    val spoken =
        listOfNotNull(
            VersionsCopy.headline(row),
            row.sizeBytes?.let(Format::bytes),
            "via ${row.via}",
        ).joinToString(", ")
    Column(
        Modifier
            .fillMaxWidth()
            .focusRequester(focus.requesterFor(key))
            .marqueeFocus(Marquee.radius.m, scale = 1.02f)
            .onFocusChanged { state ->
                watch(state.isFocused)
                focused = state.isFocused
            }.clip(shape)
            .background(if (focused) colors.raised else Color.Transparent)
            .semantics {
                contentDescription = spoken
                stateDescription = VersionsCopy.status(row.status)
            }.focusClickable({ onPlay(row) }, Role.Button)
            .padding(horizontal = 22.dp, vertical = 14.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            MqText(VersionsCopy.cell(row.quality), Modifier.width(QUALITY), weight = FontWeight.SemiBold, maxLines = 1)
            MqText(
                row.features.joinToString(", "),
                Modifier.weight(1f),
                role = TextRole.Callout,
                color = colors.muted,
                maxLines = 1,
            )
            // Left blank when the source does not say, as on the web.
            MqText(
                row.sizeBytes?.let(Format::bytes).orEmpty(),
                Modifier.width(SIZE),
                role = TextRole.Callout,
                maxLines = 1,
            )
            StatusWord(VersionsCopy.status(row.status), statusTone(row), Modifier.width(STATUS))
            MqText(row.via, Modifier.width(VIA), role = TextRole.Callout, maxLines = 1)
        }
        if (focused) {
            Column(Modifier.padding(start = QUALITY), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Release(row.releaseName, maxLines = 2)
                MqText(VersionsCopy.detail(row), role = TextRole.Caption, color = colors.muted, maxLines = 1)
            }
        }
    }
}
