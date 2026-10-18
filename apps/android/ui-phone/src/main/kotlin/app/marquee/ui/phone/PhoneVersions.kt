package app.marquee.ui.phone

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
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
import kotlinx.coroutines.flow.first

/** How many rows of a group show before "Show all", as on the web. */
private const val SHOWN = 3

/**
 * The ranked sources of one video on a phone (plans/android-design-v2.md 5.6, the web's v2
 * screen): the heading, how the search goes and what this phone plays; the recommendation as a
 * glass card with its facts; every other source by group; and what is hidden and why. The first
 * scroll freezes the recommendation so it does not change while results still stream in.
 *
 * @param heading what the screen is about ("Sintel" or "Sintel, S1:E2").
 * @param device what the ranking was made for ("This screen: 1080p, plays H.264 in MP4").
 * @param onPlay a version was chosen.
 * @param onAddSource no addon gave a version: go where one is added.
 */
@Composable
fun PhoneVersions(
    heading: String,
    device: String?,
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
    val open = remember { mutableStateMapOf<Group, Boolean>() }
    LazyColumn(
        state = listState,
        modifier = modifier.fillMaxSize().windowInsetsPadding(WindowInsets.statusBars),
        contentPadding = PaddingValues(start = side, end = side, top = Marquee.space.m, bottom = Marquee.space.xl),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        item(key = "head") {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Eyebrow(heading)
                MqText("Versions", role = TextRole.Title1, maxLines = 1)
                val (text, tone) = VersionsCopy.progress(snapshot)
                StatusWord(text, tone(tone), Modifier.padding(top = 4.dp))
                if (device != null) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.Top) {
                        MqIcon(Icons.Monitor, tint = colors.muted, size = 16.dp)
                        MqText(device, role = TextRole.Caption, color = colors.muted)
                    }
                }
            }
        }
        item(key = "best") {
            Pick(
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
            items(if (all) rows else rows.take(SHOWN), key = { "row:${it.key}" }) { row -> SourceRow(row, onPlay) }
            if (rows.size > SHOWN) {
                item(key = "fold:$group") {
                    SecondaryButton(
                        if (all) "Show the best $SHOWN" else "Show all ${rows.size}",
                        { open[group] = !all },
                        icon = if (all) Icons.ChevronUp else Icons.ChevronDown,
                    )
                }
            }
        }
        val hidden = snapshot?.hidden?.takeIf { it.count > 0 }
        if (hidden != null) {
            item(key = "hidden") {
                GlassBar(Modifier.padding(top = Marquee.space.s)) {
                    MqIcon(Icons.EyeOff, tint = colors.muted)
                    MqText(
                        "${VersionsCopy.hiddenTitle(hidden)}  ${VersionsCopy.hiddenDetail(hidden)}",
                        role = TextRole.Callout,
                        color = colors.muted,
                        maxLines = 3,
                        modifier = Modifier.weight(1f),
                    )
                }
            }
        }
    }
}

private fun tone(tone: VersionsCopy.Tone) =
    when (tone) {
        VersionsCopy.Tone.Idle -> Status.Idle
        VersionsCopy.Tone.Ok -> Status.Ok
        VersionsCopy.Tone.Warn -> Status.Warn
        VersionsCopy.Tone.Bad -> Status.Bad
    }

private fun statusTone(row: VersionRow) = if (VersionsCopy.good(row.status)) Status.Ok else Status.Warn

@Composable
private fun Pick(
    snapshot: VersionsSnapshot?,
    failure: String?,
    onRetry: () -> Unit,
    onAddSource: () -> Unit,
    onPlay: (VersionRow) -> Unit,
    modifier: Modifier,
) {
    val colors = Marquee.colors
    val shape = RoundedCornerShape(Marquee.radius.l)
    val best = snapshot?.recommended
    Column(
        modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.glassStrong)
            .border(1.dp, colors.edge, shape)
            .padding(20.dp),
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
                    modifier = Modifier.padding(0.dp),
                )
            }

            best != null -> {
                CapsLabel("Recommended for this screen")
                MqText(VersionsCopy.pickHeading(best), role = TextRole.Title1, maxLines = 2)
                FactsGrid(VersionsCopy.facts(best), tones = mapOf("Status" to statusTone(best)))
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    for (reason in best.reasons.take(3)) {
                        Row(
                            horizontalArrangement = Arrangement.spacedBy(10.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            MqIcon(Icons.Check, tint = colors.ok)
                            MqText(VersionsCopy.reason(reason, best.status), role = TextRole.Callout, maxLines = 2)
                        }
                    }
                }
                // ponytail: the system monospace face; IBM Plex Mono as on the web when Android bundles it.
                MqText(
                    Format.releaseWrapped(best.releaseName),
                    role = TextRole.Caption,
                    color = colors.muted,
                    maxLines = 3,
                    fontFamily = FontFamily.Monospace,
                )
                PrimaryButton(
                    label = "Play this version",
                    icon = PlayFilled,
                    onClick = { onPlay(best) },
                    modifier = Modifier.padding(top = 4.dp).fillMaxWidth(),
                )
                Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    MqIcon(Icons.RefreshCw, tint = colors.muted, size = 16.dp)
                    MqText(VersionsCopy.failover(best), role = TextRole.Caption, color = colors.muted)
                }
            }

            snapshot == null || !snapshot.done -> {
                CapsLabel("Recommended for this screen")
                Skeleton(Modifier.size(160.dp, 36.dp))
                Skeleton(Modifier.size(240.dp, 20.dp))
                Skeleton(Modifier.size(200.dp, 20.dp))
                Skeleton(Modifier.fillMaxWidth().height(48.dp))
            }

            else -> {
                val none = snapshot.rows.isEmpty()
                StateMessage(
                    title = VersionsCopy.waitingTitle(snapshot),
                    body = VersionsCopy.waitingBody(snapshot),
                    icon = Icons.CircleAlert,
                    actionLabel = "Add a source".takeIf { none },
                    onAction = onAddSource.takeIf { none },
                    modifier = Modifier.padding(0.dp),
                )
            }
        }
    }
}

/** One source: the quality, then its status and size, where from and its features, on a hairline. */
@Composable
private fun SourceRow(
    row: VersionRow,
    onPlay: (VersionRow) -> Unit,
) {
    val colors = Marquee.colors
    val spoken =
        listOfNotNull(
            VersionsCopy.headline(row),
            row.sizeBytes?.let(Format::bytes),
            "via ${row.via}",
        ).joinToString(", ")
    Row(
        Modifier
            .fillMaxWidth()
            .semantics {
                contentDescription = spoken
                stateDescription = VersionsCopy.status(row.status)
            }.focusClickable({ onPlay(row) }, Role.Button)
            .padding(horizontal = 12.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // One width for every quality ("Unknown" the widest), so the statuses line up.
        MqText(
            VersionsCopy.cell(row.quality),
            Modifier.widthIn(min = 76.dp),
            weight = FontWeight.SemiBold,
            maxLines = 1,
        )
        // The status with the size at its end, then the source and features: both wrap on a small
        // phone or a large font instead of cutting what helps the choice.
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                StatusWord(VersionsCopy.status(row.status), statusTone(row), Modifier.weight(1f))
                row.sizeBytes?.let { MqText(Format.bytes(it), role = TextRole.Callout, maxLines = 1) }
            }
            MqText(
                (listOf("via ${row.via}") + row.features).joinToString(" · "),
                role = TextRole.Caption,
                color = colors.muted,
                maxLines = 2,
            )
        }
    }
}
