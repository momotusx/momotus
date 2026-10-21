@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.tv

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
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
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.marquee.core.Format
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.VersionsCopy
import app.marquee.core.model.BlockedReason
import app.marquee.core.model.BlockedVersions
import app.marquee.core.model.Group
import app.marquee.core.model.LearnedNote
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.views.VersionsView
import app.marquee.core.views.ViewStatus
import app.marquee.design.CapsLabel
import app.marquee.design.Eyebrow
import app.marquee.design.FactsGrid
import app.marquee.design.GlassBar
import app.marquee.design.Icons
import app.marquee.design.LiveSpoken
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.ON_ARRIVAL_MS
import app.marquee.design.PlayFilled
import app.marquee.design.PrimaryButton
import app.marquee.design.SecondaryButton
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.TextRole
import app.marquee.design.ToneWord
import app.marquee.design.endFade
import app.marquee.design.focusClickable
import app.marquee.design.marqueeFocus
import app.marquee.design.scrollByKey
import kotlinx.coroutines.launch

private const val PLAY = "play"
private const val ADD = "add"
private const val ROWS = "rows"

/** How many rows of a group show before "Show all", as on the web. */
private const val SHOWN = 3

/**
 * The ranked sources of one video (plans/android-design-v2.md 5.6, the web's v2 screen): the
 * heading, how the search goes and what the ranking read of this TV; the recommendation as a glass
 * card with its facts and plain reasons; every other source in a table by group with its line (a
 * tone and plain words, plans/m13-stream-choice.md 3.7), the focused row opened to the line's
 * details and its release name; and what is hidden and why. When versions were found and none plays
 * here, the card says why with its way on (3.11). A note says once what the picks taught, with
 * Undo. The first arrow key freezes the recommendation, so the pick under the cursor never changes
 * while results still stream in.
 *
 * @param heading what the screen is about ("Sintel" or "Sintel, S1:E2").
 * @param onPlay a version was chosen.
 * @param onAddSource no addon gave a version: go where one is added.
 * @param onAddAccount the versions play only through a streaming service account: go where one is added.
 * @param onUndo Undo on the learned note.
 */
@Composable
fun TvVersions(
    heading: String,
    versions: VersionsView?,
    focus: FocusMemory,
    onPlay: (VersionRow) -> Unit,
    onAddSource: () -> Unit,
    onAddAccount: () -> Unit,
    onUndo: suspend (LearnedNote) -> Unit,
    modifier: Modifier = Modifier,
) {
    val safe = Marquee.safe
    val snapshot = versions?.snapshot
    val failure = (versions?.status as? ViewStatus.Failed)?.reason
    val blocked = snapshot?.blocked?.takeIf { snapshot.recommended == null }
    val nothing =
        snapshot != null && snapshot.done && snapshot.recommended == null && snapshot.rows.isEmpty() && blocked == null
    // "Show them": the blocked rows, as they would rank once the reason is fixed.
    var showBlocked by remember { mutableStateOf(false) }
    // The list, for Right from Show them: Add an account reaches further right, so focus search alone stops there.
    val list = remember { FocusRequester() }
    // The target exists once there is something to focus: the card's Play (or Try again, or the blocked
    // card's action, or Add a source), else the first row.
    val target =
        when {
            failure != null || snapshot?.recommended != null || blocked != null -> PLAY
            nothing -> ADD
            else -> snapshot?.rows?.firstOrNull()?.let { rowKey(it) }
        }
    focus.FirstFocus(target, ask = failure != null)

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
        Header(heading, snapshot)
        Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(Marquee.space.l)) {
            Pick(snapshot, failure, { versions?.retry() }, onAddSource, focus, onPlay) {
                if (blocked != null) {
                    Blocked(snapshot, blocked, showBlocked, { showBlocked = !showBlocked }, onAddAccount, focus, list)
                }
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                // Once per screen: the card says it right before Play; the list only when there is no such card.
                if (snapshot?.nobodySharing == true && (failure != null || snapshot.recommended == null)) {
                    GlassBar { NobodySharing() }
                }
                snapshot?.note?.let { LearnedLine(it, onUndo) }
                Sources(
                    snapshot,
                    if (showBlocked && blocked != null) blocked.rows else snapshot?.rows.orEmpty(),
                    focus,
                    list,
                    onPlay,
                    Modifier.weight(1f),
                )
            }
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

/** The heading as an eyebrow over "Versions", and at the right how the search goes and what the ranking read of this TV. */
@Composable
private fun Header(
    heading: String,
    snapshot: VersionsSnapshot?,
) {
    val device = snapshot?.conditions?.let { StreamChoiceCopy.header(tv = true, it) }
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

/** The recommendation card, or why there is none yet ([blocked]: versions were found and none plays here). */
@Composable
private fun Pick(
    snapshot: VersionsSnapshot?,
    failure: String?,
    onRetry: () -> Unit,
    onAddSource: () -> Unit,
    focus: FocusMemory,
    onPlay: (VersionRow) -> Unit,
    blocked: @Composable () -> Unit,
) {
    val colors = Marquee.colors
    val shape = RoundedCornerShape(Marquee.radius.l)
    val best = snapshot?.recommended
    val play = focus.requesterFor(PLAY)
    Column(
        Modifier
            // Entering the card from the list lands on Play, never on the details above it.
            .then(if (best != null) Modifier.focusProperties { enter = { play } }.focusGroup() else Modifier)
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
                    actionFocus = play,
                    modifier = Modifier.padding(0.dp),
                )
            }

            best != null -> {
                // The details scroll when they run long (features, languages, three reasons, a long
                // release name); Play and its fallback line keep their place at the card's foot. Only
                // then do they take focus (Up from Play), and Up and Down scroll them until an edge,
                // where the key moves focus on as usual.
                val details = rememberScrollState()
                val scope = rememberCoroutineScope()
                // maxValue is Int.MAX_VALUE until the first measure: not an overflow yet.
                val overflows = details.maxValue in 1 until Int.MAX_VALUE
                Column(
                    Modifier
                        .weight(1f, fill = false)
                        .then(
                            if (overflows) {
                                Modifier
                                    .onPreviewKeyEvent { scrollByKey(it, details, scope) }
                                    .marqueeFocus(Marquee.radius.m, scale = 1f)
                                    .focusable()
                            } else {
                                Modifier
                            },
                        ).endFade(details.canScrollForward, vertical = true, length = 48.dp)
                        .verticalScroll(details),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
                ) {
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
                        // A version that plays in a window says so, with its minutes ahead.
                        StreamChoiceCopy.cardLine(best)?.let { line ->
                            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                MqIcon(Icons.Check, tint = colors.ok, size = 26.dp)
                                Column {
                                    MqText(line.text, role = TextRole.Callout, maxLines = 2)
                                    MqText(line.details, role = TextRole.Caption, color = colors.muted, maxLines = 2)
                                }
                            }
                        }
                    }
                    Release(best.releaseName, maxLines = 3)
                }
                // Outside the details, so it never scrolls away from Play; said when it comes.
                if (snapshot.nobodySharing) NobodySharing(Modifier.semantics { liveRegion = LiveRegionMode.Polite })
                PrimaryButton(
                    label = "Play this version",
                    icon = PlayFilled,
                    onClick = { onPlay(best) },
                    modifier = Modifier.padding(top = Marquee.space.xs),
                    focusRequester = play,
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

            snapshot?.blocked != null -> {
                blocked()
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

/**
 * Why no version is listed, in the core's words, with its way on: "Add an account" (and "Show them"
 * beside it) when the versions play only through a streaming service account, else "Show them".
 */
@Composable
private fun Blocked(
    snapshot: VersionsSnapshot,
    blocked: BlockedVersions,
    shown: Boolean,
    onShow: () -> Unit,
    onAddAccount: () -> Unit,
    focus: FocusMemory,
    list: FocusRequester,
) {
    val (text, action) =
        StreamChoiceCopy.blocked(blocked.reason, blocked.count, snapshot.conditions.window)
            ?: (null to StreamChoiceCopy.SHOW_ROWS)
    val account = blocked.reason == BlockedReason.NeedsAccount
    StateMessage(
        title = "No playable version found",
        // A reason from a newer core: the hidden rows say why.
        body = text ?: VersionsCopy.waitingBody(snapshot),
        actionLabel = action,
        onAction = if (account) onAddAccount else onShow,
        actionFocus = focus.requesterFor(PLAY),
        actionIcon =
            if (account) {
                Icons.KeyRound
            } else if (shown) {
                Icons.EyeOff
            } else {
                Icons.Eye
            },
        secondaryLabel = StreamChoiceCopy.SHOW_ROWS.takeIf { account },
        onSecondary = onShow.takeIf { account },
        secondaryModifier = Modifier.focusProperties { if (shown) right = list },
        secondaryIcon = if (shown) Icons.EyeOff else Icons.Eye,
        modifier = Modifier.padding(0.dp),
    )
}

/**
 * The core's `nobodySharing`: every version is a swarm with one seeder or none right now, said once: right
 * before Play on the recommended card, or over the list when there is no such card (plans/m13-stream-choice.md).
 */
@Composable
private fun NobodySharing(modifier: Modifier = Modifier) {
    val colors = Marquee.colors
    Row(
        modifier.semantics(mergeDescendants = true) {},
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.Top,
    ) {
        MqIcon(Icons.UserRoundX, tint = colors.warn, size = 26.dp)
        Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
            MqText(StreamChoiceCopy.NOBODY_SHARING, role = TextRole.Callout, weight = FontWeight.SemiBold, maxLines = 2)
            MqText(StreamChoiceCopy.NOBODY_SHARING_DETAILS, role = TextRole.Caption, color = colors.muted, maxLines = 3)
        }
    }
}

/** The one-time note of what the picks taught (3.9), until Undo. */
@Composable
private fun LearnedLine(
    note: LearnedNote,
    onUndo: suspend (LearnedNote) -> Unit,
) {
    val colors = Marquee.colors
    val scope = rememberCoroutineScope()
    var undone by remember(note) { mutableStateOf(false) }
    val text = StreamChoiceCopy.learnedNote(note.choice, note.up, tv = true, note.prefs)
    if (undone || text == null) return
    GlassBar {
        MqIcon(Icons.Sparkles, tint = colors.muted, size = 26.dp)
        // Said once when it appears, after the screen's first focus; the list under it keeps the focus.
        Box(
            Modifier.weight(1f),
        ) { LiveSpoken(text, ON_ARRIVAL_MS) { MqText(text, role = TextRole.Callout, maxLines = 2) } }
        SecondaryButton(StreamChoiceCopy.UNDO, {
            undone = true
            scope.launch { runCatching { onUndo(note) }.onFailure { undone = false } }
        }, icon = Icons.Undo2)
    }
}

@Composable
private fun Sources(
    snapshot: VersionsSnapshot?,
    rows: List<VersionRow>,
    focus: FocusMemory,
    list: FocusRequester,
    onPlay: (VersionRow) -> Unit,
    modifier: Modifier,
) {
    val colors = Marquee.colors
    val groups = rows.groupBy { it.group }
    val open = remember { mutableStateMapOf<Group, Boolean>() }
    Column(modifier, verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        if (rows.isNotEmpty()) {
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
                    .focusRequester(list)
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
                            VersionsCopy.hiddenDetail(hidden, snapshot.conditions.window),
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

/** One source: its cells on a hairline, its line in the Status column, opened to the line's details, the release name and its languages while focused. */
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
    val line = StreamChoiceCopy.line(row.line)
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
                // The line, then its details: what the eye gets from the row while it has focus.
                stateDescription = listOf(line.text, line.details).filter { it.isNotEmpty() }.joinToString(". ")
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
            // A long line ("Colors may look washed out on this screen") takes a second line, as on the web.
            ToneWord(line.text, tone(line.tone), Modifier.width(STATUS))
            MqText(row.via, Modifier.width(VIA), role = TextRole.Callout, maxLines = 1)
        }
        if (focused) {
            Column(Modifier.padding(start = QUALITY), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                if (line.details.isNotEmpty()) MqText(line.details, role = TextRole.Callout, maxLines = 2)
                Release(row.releaseName, maxLines = 2)
                MqText(VersionsCopy.detail(row), role = TextRole.Caption, color = colors.muted, maxLines = 1)
            }
        }
    }
}
