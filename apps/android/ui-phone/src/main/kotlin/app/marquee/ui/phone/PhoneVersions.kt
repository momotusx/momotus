package app.marquee.ui.phone

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
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
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.TextRole
import app.marquee.design.focusClickable
import kotlinx.coroutines.flow.first

/**
 * The ranked sources of one video on a phone: the recommendation as a card with up to three
 * reasons, every source grouped under it, and what is hidden and why. The first scroll freezes the
 * recommendation so it does not change while results still stream in.
 *
 * @param heading what the screen is about ("Sintel" or "Sintel, S1:E2").
 * @param onPlay a version was chosen.
 * @param onAddSource no addon gave a version: go where one is added.
 */
@Composable
fun PhoneVersions(
    heading: String,
    versions: VersionsView?,
    onPlay: (VersionRow) -> Unit,
    onAddSource: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val side = Marquee.safe.side
    val colors = Marquee.colors
    val snapshot = versions?.snapshot
    val failure = (versions?.status as? ViewStatus.Failed)?.reason
    val listState = rememberLazyListState()
    // The first scroll pins the recommendation, once there is one: a pin before that would keep the
    // card empty while sources stream in. The flag comes from the core, so a restarted core is pinned again.
    LaunchedEffect(versions, snapshot?.frozen) {
        if (versions != null && snapshot?.frozen != true) {
            snapshotFlow { listState.isScrollInProgress && versions.snapshot?.recommended != null }.first { it }
            versions.freeze()
        }
    }
    val groups = snapshot?.rows.orEmpty().groupBy { it.group }
    LazyColumn(
        state = listState,
        modifier = modifier.fillMaxSize().windowInsetsPadding(WindowInsets.statusBars),
        contentPadding = PaddingValues(start = side, end = side, top = Marquee.space.m, bottom = Marquee.space.xl),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        item(key = "head") {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                MqText("Versions", role = TextRole.Title2)
                MqText(heading, role = TextRole.Callout, color = colors.muted, maxLines = 2)
            }
        }
        item(key = "best") {
            Recommendation(
                snapshot,
                failure,
                { versions?.retry() },
                onAddSource,
                onPlay,
                Modifier.padding(vertical = Marquee.space.s),
            )
        }
        for (group in Group.entries) {
            val rows = groups[group].orEmpty()
            if (rows.isEmpty()) continue
            item(key = "group:$group") {
                // Group headers are the one place that is set in capitals, with the count of rows in them.
                MqText(
                    "${VersionsCopy.group(group, snapshot?.recommended != null).uppercase()}  ${rows.size}",
                    role = TextRole.Caption,
                    color = colors.muted,
                    weight = FontWeight.SemiBold,
                    modifier = Modifier.padding(top = Marquee.space.s),
                )
            }
            items(rows, key = { "row:${it.key}" }) { row -> SourceRow(row, onPlay) }
        }
        val hidden = snapshot?.hidden?.takeIf { it.count > 0 }
        val addons = snapshot?.let(VersionsCopy::addons)
        if (hidden != null || addons != null) {
            item(key = "notes") {
                Column(
                    Modifier.padding(top = Marquee.space.s),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                ) {
                    hidden?.let { HiddenRow(VersionsCopy.hiddenTitle(it), VersionsCopy.hiddenDetail(it)) }
                    addons?.let { MqText(it, role = TextRole.Callout, color = colors.muted) }
                }
            }
        }
    }
}

@Composable
private fun Recommendation(
    snapshot: VersionsSnapshot?,
    failure: String?,
    onRetry: () -> Unit,
    onAddSource: () -> Unit,
    onPlay: (VersionRow) -> Unit,
    modifier: Modifier,
) {
    val colors = Marquee.colors
    val best = snapshot?.recommended
    Column(
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Marquee.radius.l))
            .background(colors.surface)
            .padding(Marquee.space.m),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        when {
            failure != null -> {
                StateMessage(
                    title = "Sources could not load",
                    body = failure,
                    icon = Icons.CircleAlert,
                    actionLabel = "Try again",
                    onAction = onRetry,
                    modifier = Modifier.padding(0.dp),
                )
            }

            snapshot == null -> {
                Skeleton(Modifier.size(180.dp, 28.dp))
                Skeleton(Modifier.size(240.dp, 20.dp))
                Skeleton(Modifier.size(160.dp, 48.dp))
            }

            best != null -> {
                MqText("Recommended for this screen", role = TextRole.Callout, color = colors.muted)
                MqText(VersionsCopy.headline(best), role = TextRole.Title3, maxLines = 2)
                MqText(VersionsCopy.source(best), role = TextRole.Callout, color = colors.muted)
                for (reason in best.reasons.take(3)) {
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        MqIcon(Icons.Check, tint = colors.ok)
                        MqText(VersionsCopy.reason(reason, best.status), role = TextRole.Callout, maxLines = 2)
                    }
                }
                PrimaryButton(
                    label = "Play this version",
                    icon = Icons.Play,
                    onClick = { onPlay(best) },
                    modifier = Modifier.padding(top = Marquee.space.xs).fillMaxWidth(),
                )
            }

            else -> {
                val nothing = snapshot.done && snapshot.rows.isEmpty()
                StateMessage(
                    title = VersionsCopy.waitingTitle(snapshot),
                    body = VersionsCopy.waitingBody(snapshot),
                    icon = if (snapshot.done) Icons.CircleAlert else null,
                    actionLabel = "Add a source".takeIf { nothing },
                    onAction = onAddSource.takeIf { nothing },
                    modifier = Modifier.padding(0.dp),
                )
            }
        }
    }
}

@Composable
private fun SourceRow(
    row: VersionRow,
    onPlay: (VersionRow) -> Unit,
) {
    val colors = Marquee.colors
    val spoken = "${VersionsCopy.headline(row)}, ${VersionsCopy.source(row)}"
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(Marquee.radius.m))
            .background(colors.surface)
            .semantics {
                contentDescription = spoken
                stateDescription = VersionsCopy.status(row.status)
            }.focusClickable({ onPlay(row) }, Role.Button)
            .padding(Marquee.space.s),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        // Line 1: "720p, HDR10 · 1.2 GB". Line 2: the status with its dot and where it comes from. Line 3: the release.
        MqText(
            listOfNotNull(
                (listOf(VersionsCopy.cell(row.quality)) + row.features).joinToString(", "),
                row.sizeBytes?.let(Format::bytes),
            ).joinToString(" · "),
            weight = FontWeight.SemiBold,
            maxLines = 1,
        )
        Row(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            StatusWord(VersionsCopy.status(row.status), if (VersionsCopy.good(row.status)) Status.Ok else Status.Warn)
            MqText("· ${row.via}", role = TextRole.Callout, color = colors.muted, maxLines = 1)
        }
        MqText(
            Format.releaseWrapped(row.releaseName),
            role = TextRole.Caption,
            color = colors.muted,
            maxLines = 2,
        )
    }
}

/** What is hidden and why, as a row with the eye-off icon. */
@Composable
private fun HiddenRow(
    title: String,
    detail: String,
) {
    val colors = Marquee.colors
    Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs), verticalAlignment = Alignment.Top) {
        MqIcon(Icons.EyeOff, tint = colors.muted)
        MqText(
            "$title  $detail",
            role = TextRole.Callout,
            color = colors.muted,
            maxLines = 3,
            modifier = Modifier.weight(1f),
        )
    }
}
