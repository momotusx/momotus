@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.tv

import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
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
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import app.marquee.core.Format
import app.marquee.core.VersionsCopy
import app.marquee.core.model.Group
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.views.VersionsView
import app.marquee.core.views.ViewStatus
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.PrimaryButton
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.TextRole
import app.marquee.design.focusClickable
import app.marquee.design.marqueeFocus

private const val PLAY = "play"
private const val ROWS = "rows"

/**
 * The ranked sources of one video: a recommendation card on the left with up to three plain
 * reasons, and every source grouped on the right, with what is hidden and why. The first key press
 * freezes the recommendation, so the pick under the cursor never changes while results still
 * stream in. The release name of the focused row shows at the foot, whole.
 *
 * @param heading what the screen is about ("Sintel" or "Sintel, S1:E2").
 * @param onPlay a version was chosen.
 */
@Composable
fun TvVersions(
    heading: String,
    versions: VersionsView?,
    focus: FocusMemory,
    onPlay: (VersionRow) -> Unit,
    modifier: Modifier = Modifier,
) {
    val safe = Marquee.safe
    val snapshot = versions?.snapshot
    val failure = (versions?.status as? ViewStatus.Failed)?.reason
    var focusedRow by remember { mutableStateOf<VersionRow?>(null) }
    // The target exists once there is something to focus: the card's Play (or Try again), else the first row.
    val target =
        if (failure != null ||
            snapshot?.recommended != null
        ) {
            PLAY
        } else {
            snapshot?.rows?.firstOrNull()?.let { rowKey(it) }
        }
    LaunchedEffect(target) {
        // A remembered row that is no longer in the list (it moved into the card) is not a target.
        val wanted = focus.focusedKey?.takeIf { focus.requester(it) != null } ?: target
        if (wanted != null) focus.focusWhenReady(wanted, waitMs = 10_000)
    }

    Row(
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
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.l),
    ) {
        Column(Modifier.width(640.dp).fillMaxHeight(), verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            MqText("Choose a version", role = TextRole.Title2, maxLines = 1)
            MqText(heading, role = TextRole.Callout, color = Marquee.colors.muted, maxLines = 2)
            Recommendation(snapshot, failure, { versions?.retry() }, focus, onPlay)
        }
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            Sources(snapshot, focus, onPlay, onFocusedRow = { focusedRow = it }, modifier = Modifier.weight(1f))
            Footer(focusedRow ?: snapshot?.recommended, snapshot)
        }
    }
}

private val arrows = setOf(Key.DirectionLeft, Key.DirectionRight, Key.DirectionUp, Key.DirectionDown)

private fun rowKey(row: VersionRow) = "row:${row.key}"

@Composable
private fun Recommendation(
    snapshot: VersionsSnapshot?,
    failure: String?,
    onRetry: () -> Unit,
    focus: FocusMemory,
    onPlay: (VersionRow) -> Unit,
) {
    val colors = Marquee.colors
    val best = snapshot?.recommended
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Marquee.radius.l))
            .background(colors.surface)
            .padding(Marquee.space.m),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        when {
            failure != null -> {
                StateMessage(
                    title = "Sources could not load",
                    body = failure,
                    icon = Icons.CircleAlert,
                    actionLabel = "Try again",
                    onAction = onRetry,
                    actionFocus = focus.requesterFor(PLAY),
                    modifier = Modifier.padding(0.dp),
                )
            }

            snapshot == null -> {
                Skeleton(Modifier.size(300.dp, 40.dp))
                Skeleton(Modifier.size(420.dp, 28.dp))
                Skeleton(Modifier.size(260.dp, 72.dp))
            }

            best != null -> {
                MqText("Recommended", role = TextRole.Caption, color = colors.muted)
                MqText(VersionsCopy.headline(best), role = TextRole.Title2, maxLines = 2)
                MqText(VersionsCopy.source(best), role = TextRole.Callout, color = colors.muted, maxLines = 1)
                for (reason in best.reasons.take(3)) {
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        MqIcon(Icons.Check, tint = colors.ok)
                        MqText(VersionsCopy.reason(reason), role = TextRole.Callout, maxLines = 2)
                    }
                }
                PrimaryButton(
                    label = "Play this version",
                    icon = Icons.Play,
                    onClick = { onPlay(best) },
                    focusRequester = focus.requesterFor(PLAY),
                    onFocusChange = focus.watch(null, PLAY),
                )
            }

            else -> {
                StateMessage(
                    title = VersionsCopy.waitingTitle(snapshot),
                    body = VersionsCopy.waitingBody(snapshot),
                    icon = if (snapshot.done) Icons.CircleAlert else null,
                    modifier = Modifier.padding(0.dp),
                )
            }
        }
    }
}

@Composable
private fun Sources(
    snapshot: VersionsSnapshot?,
    focus: FocusMemory,
    onPlay: (VersionRow) -> Unit,
    onFocusedRow: (VersionRow?) -> Unit,
    modifier: Modifier,
) {
    val colors = Marquee.colors
    val groups = snapshot?.rows.orEmpty().groupBy { it.group }
    LazyColumn(
        modifier =
            modifier
                .fillMaxWidth()
                .focusProperties { enter = { focus.rowEntry(ROWS) ?: FocusRequester.Default } }
                .focusGroup(),
        contentPadding = PaddingValues(vertical = Marquee.space.xs),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        for (group in Group.entries) {
            val rows = groups[group].orEmpty()
            if (rows.isEmpty()) continue
            item(key = "group:$group") {
                MqText(
                    VersionsCopy.group(group),
                    role = TextRole.Caption,
                    color = colors.muted,
                    modifier = Modifier.padding(top = Marquee.space.s, start = 4.dp),
                )
            }
            items(rows, key = { rowKey(it) }) { row -> SourceRow(row, focus, onPlay, onFocusedRow) }
        }
        val addons = snapshot?.let(VersionsCopy::addons)
        val hidden = snapshot?.hidden?.let(VersionsCopy::hidden)
        if (hidden != null || addons != null) {
            item(key = "notes") {
                Column(
                    Modifier.padding(top = Marquee.space.m),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                ) {
                    hidden?.let { MqText(it, role = TextRole.Callout, color = colors.muted) }
                    addons?.let { MqText(it, role = TextRole.Callout, color = colors.muted) }
                }
            }
        }
    }
}

@Composable
private fun SourceRow(
    row: VersionRow,
    focus: FocusMemory,
    onPlay: (VersionRow) -> Unit,
    onFocusedRow: (VersionRow?) -> Unit,
) {
    val colors = Marquee.colors
    val key = rowKey(row)
    val shape = RoundedCornerShape(Marquee.radius.m)
    val watch = focus.watch(ROWS, key)
    val spoken = "${VersionsCopy.headline(row)}, ${VersionsCopy.source(row)}"
    Row(
        Modifier
            .fillMaxWidth()
            .focusRequester(focus.requesterFor(key))
            .marqueeFocus(Marquee.radius.m, scale = 1.02f)
            .onFocusChanged { state ->
                watch(state.isFocused)
                onFocusedRow(if (state.isFocused) row else null)
            }.clip(shape)
            .background(colors.surface)
            .semantics {
                contentDescription = spoken
                stateDescription = VersionsCopy.status(row.status)
            }.focusClickable({ onPlay(row) }, Role.Button)
            .padding(horizontal = Marquee.space.m, vertical = Marquee.space.s),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.m),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqText(row.quality, weight = FontWeight.SemiBold, modifier = Modifier.width(110.dp), maxLines = 1)
        MqText(
            row.features.joinToString(", "),
            role = TextRole.Callout,
            color = colors.muted,
            modifier = Modifier.weight(1f),
            maxLines = 1,
        )
        MqText(
            row.sizeBytes?.let(Format::bytes).orEmpty(),
            role = TextRole.Callout,
            modifier = Modifier.width(110.dp),
            maxLines = 1,
        )
        MqText(row.via, role = TextRole.Callout, modifier = Modifier.width(170.dp), maxLines = 1)
        MqText(
            VersionsCopy.status(row.status),
            role = TextRole.Callout,
            color = colors.muted,
            modifier = Modifier.width(240.dp),
            maxLines = 1,
        )
    }
}

/** The release name of the focused row, in the pieces it breaks into, so it is never cut off. */
@Composable
private fun Footer(
    row: VersionRow?,
    snapshot: VersionsSnapshot?,
) {
    if (row == null || snapshot == null) return
    MqText(
        Format.releaseWrapped(row.releaseName),
        role = TextRole.Caption,
        color = Marquee.colors.muted,
        maxLines = 3,
        modifier = Modifier.padding(top = Marquee.space.xs).fillMaxWidth(),
    )
}
