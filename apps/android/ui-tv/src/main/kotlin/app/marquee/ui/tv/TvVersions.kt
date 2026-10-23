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
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
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
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.marquee.core.Format
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.VersionsCopy
import app.marquee.core.VersionsCopy.EmptyStep
import app.marquee.core.model.EmptyAction
import app.marquee.core.model.EmptyVersions
import app.marquee.core.model.Group
import app.marquee.core.model.LearnedNote
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.views.VersionsView
import app.marquee.core.views.ViewStatus
import app.marquee.design.CapsLabel
import app.marquee.design.FactsGrid
import app.marquee.design.FocusInk
import app.marquee.design.GlassBar
import app.marquee.design.Icons
import app.marquee.design.LanguageTag
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
import app.marquee.design.Tokens
import app.marquee.design.ToneWord
import app.marquee.design.dimmed
import app.marquee.design.endFade
import app.marquee.design.focusClickable
import app.marquee.design.focusPalette
import app.marquee.design.marqueeFocus
import app.marquee.design.scrollByKey
import kotlinx.coroutines.launch

private const val PLAY = "play"
private const val ROWS = "rows"

/** How many rows of a group show before "Show all", as on the web. */
private const val SHOWN = 3

/**
 * The ranked sources of one video (plans/android-design-v2.md 5.6, the web's v2 screen): the
 * heading, how the search goes and what the ranking read of this TV; the recommendation as a glass
 * card with its facts and plain reasons; every other source in a table by group with its line (a
 * tone and plain words, plans/m13-stream-choice.md 3.7), the focused row opened to the line's
 * details and its release name. When nothing can play, the card says why in the core's words with
 * its actions (plans/play-every-version.md 2a), the rows found listed dimmed. A note says once what
 * the picks taught, with Undo. The first arrow key freezes the recommendation, so the pick under the
 * cursor never changes while results still stream in.
 *
 * @param heading what the screen is about ("Sintel" or "Sintel, S1:E2").
 * @param onPlay a version was chosen, by its key.
 * @param onAddSource no addon is installed: go where one is added.
 * @param onAddAccount the versions play only through a streaming service account: go where one is added.
 * @param onBack the empty state's Back.
 * @param onUndo Undo on the learned note.
 */
@Composable
fun TvVersions(
    heading: String,
    versions: VersionsView?,
    focus: FocusMemory,
    onPlay: (String) -> Unit,
    onAddSource: () -> Unit,
    onAddAccount: () -> Unit,
    onBack: () -> Unit,
    onUndo: suspend (LearnedNote) -> Unit,
    modifier: Modifier = Modifier,
) {
    val safe = Marquee.safe
    val snapshot = versions?.snapshot
    val failure = (versions?.status as? ViewStatus.Failed)?.reason
    val empty = snapshot?.empty?.takeIf { snapshot.recommended == null }
    val step = { it: EmptyStep ->
        when (it) {
            EmptyStep.PlayFromDevice -> empty?.key?.let(onPlay)
            EmptyStep.TryAgain -> versions?.retry()
            EmptyStep.Settings -> if (empty?.action == EmptyAction.SettingsAccounts) onAddAccount() else onAddSource()
            EmptyStep.Back -> onBack()
        }
        Unit
    }
    // The list, for Right from the empty state's actions.
    val list = remember { FocusRequester() }
    // The target exists once there is something to focus: the card's Play (or Try again, or the empty
    // state's first action), else the first row.
    val target =
        when {
            failure != null || snapshot?.recommended != null || empty != null -> PLAY
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
            Pick(snapshot, failure, { versions?.retry() }, focus, onPlay) {
                if (empty != null) Empty(empty, step, focus, list.takeIf { empty.rows.isNotEmpty() })
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                // The rows found when none plays are simply listed, dimmed, as they would rank once the reason is fixed.
                Sources(
                    snapshot,
                    empty?.rows ?: snapshot?.rows.orEmpty(),
                    empty != null,
                    focus,
                    list,
                    onPlay,
                    Modifier.weight(1f),
                )
                snapshot?.note?.let { LearnedLine(it, onUndo) }
            }
        }
    }
}

private val arrows = setOf(Key.DirectionLeft, Key.DirectionRight, Key.DirectionUp, Key.DirectionDown)

private fun rowKey(row: VersionRow) = "row:${row.key}"

private fun headKey(group: Group) = "head:$group"

private fun foldKey(group: Group) = "fold:$group"

private fun tone(tone: VersionsCopy.Tone) =
    when (tone) {
        VersionsCopy.Tone.Idle -> Status.Idle
        VersionsCopy.Tone.Ok -> Status.Ok
        VersionsCopy.Tone.Warn -> Status.Warn
        VersionsCopy.Tone.Bad -> Status.Bad
    }

private fun statusTone(row: VersionRow) = if (VersionsCopy.good(row.status)) Status.Ok else Status.Warn

/** The heading in a plain line over "Versions", and at the right how the search goes. */
@Composable
private fun Header(
    heading: String,
    snapshot: VersionsSnapshot?,
) {
    val colors = Marquee.colors
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.Bottom) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            MqText(heading, role = TextRole.Callout, color = colors.dim, weight = FontWeight.SemiBold, maxLines = 1)
            MqText("Versions", role = TextRole.Title1, maxLines = 1)
        }
        Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            val (text, state) = VersionsCopy.progress(snapshot)
            StatusWord(text, tone(state))
        }
    }
}

/** The recommendation card, or why there is none yet ([empty]: nothing can play). */
@Composable
private fun Pick(
    snapshot: VersionsSnapshot?,
    failure: String?,
    onRetry: () -> Unit,
    focus: FocusMemory,
    onPlay: (String) -> Unit,
    empty: @Composable () -> Unit,
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
                    PickLine()
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
                PrimaryButton(
                    label = "Play this version",
                    icon = PlayFilled,
                    onClick = { onPlay(best.key) },
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

            snapshot?.empty != null -> {
                empty()
            }

            // Still asking, or every addon answered with nothing while the core checks what this device keeps.
            snapshot == null || !snapshot.done || snapshot.rows.isEmpty() -> {
                PickLine()
                Skeleton(Modifier.size(260.dp, 56.dp))
                Skeleton(Modifier.size(420.dp, 28.dp))
                Skeleton(Modifier.size(360.dp, 28.dp))
                Skeleton(Modifier.size(300.dp, 28.dp))
            }

            else -> {
                StateMessage(
                    title = VersionsCopy.NOT_READY_TITLE,
                    body = VersionsCopy.NOT_READY_BODY,
                    icon = Icons.CircleAlert,
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

/**
 * Why nothing can play, in the core's words, said once when it appears, with its actions in the shared order under
 * it (plans/versions-android-spec.md, section 5). The first is focused, and Right goes to the rows listed beside it
 * ([list], when there are some).
 */
@Composable
private fun Empty(
    empty: EmptyVersions,
    onStep: (EmptyStep) -> Unit,
    focus: FocusMemory,
    list: FocusRequester?,
) {
    val text = StreamChoiceCopy.emptyLine(empty.kind)
    val right = Modifier.focusProperties { if (list != null) right = list }
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        LiveSpoken(text, ON_ARRIVAL_MS) {
            MqText(text, role = TextRole.Headline, weight = FontWeight.SemiBold, maxLines = 4)
        }
        for ((i, step) in VersionsCopy.emptySteps(empty).withIndex()) {
            if (i == 0) {
                PrimaryButton(
                    step.label,
                    { onStep(step) },
                    right,
                    icon = icon(step),
                    focusRequester = focus.requesterFor(PLAY),
                )
            } else {
                SecondaryButton(step.label, { onStep(step) }, right, icon = icon(step))
            }
        }
    }
}

private fun icon(step: EmptyStep) =
    when (step) {
        EmptyStep.PlayFromDevice -> PlayFilled
        EmptyStep.TryAgain -> Icons.RefreshCw
        EmptyStep.Settings -> Icons.Settings
        EmptyStep.Back -> Icons.ArrowLeft
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
    dimmed: Boolean,
    focus: FocusMemory,
    list: FocusRequester,
    onPlay: (String) -> Unit,
    modifier: Modifier,
) {
    val colors = Marquee.colors
    val groups = rows.groupBy { it.group }
    val open = remember { mutableStateMapOf<Group, Boolean>() }
    val scope = rememberCoroutineScope()
    // Focus by key once its node is composed (an item a toggle just inserted is not yet attached).
    val focusSoon = { key: String -> scope.launch { focus.focusWhenReady(key, focus.interactions, waitMs = 1_000) } }
    // Looked up when focus moves, so the builder below calls nothing composable.
    val toPlay = Modifier.focusProperties { left = focus.requester(PLAY) ?: FocusRequester.Default }
    val listState = rememberLazyListState()

    /**
     * Left from the [at]th row of an open section reaches its heading: by focus search while the heading is
     * composed, else by scrolling it back in first (a long section leaves it out of composition, and the search
     * would land on Play).
     */
    fun toHeading(
        group: Group,
        row: VersionRow,
        at: Int,
    ) = Modifier
        .onPreviewKeyEvent { event ->
            if (event.type != KeyEventType.KeyDown || event.key != Key.DirectionLeft) return@onPreviewKeyEvent false
            if (focus.requester(headKey(group)) != null) return@onPreviewKeyEvent false
            val index =
                listState.layoutInfo.visibleItemsInfo
                    .firstOrNull { it.key == rowKey(row) }
                    ?.index ?: return@onPreviewKeyEvent false
            scope.launch {
                listState.scrollToItem(index - at - 1)
                focus.focusWhenReady(headKey(group), focus.interactions, waitMs = 1_000)
            }
            true
        }.focusProperties { left = focus.requester(headKey(group)) ?: FocusRequester.Default }
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
            }
            Line(colors.lineStrong)
        }
        LazyColumn(
            state = listState,
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
                // An open section's heading is a stop holding Show fewer: Left from any of its rows, or Up from
                // the first, reaches it, so a long section folds in two presses (plans/versions-android-spec.md 2).
                val fold = {
                    open[group] = !all
                    // Back on the section's Show all once it has folded.
                    if (all) focusSoon(foldKey(group))
                }
                item(key = "group:$group") {
                    GlassBar(Modifier.padding(top = Marquee.space.xs)) {
                        CapsLabel(
                            VersionsCopy.group(group, snapshot?.recommended != null),
                            Modifier.weight(1f),
                            colors.text,
                        )
                        MqText("${rows.size}", role = TextRole.Callout, maxLines = 1)
                        if (all) {
                            SecondaryButton(
                                StreamChoiceCopy.SHOW_FEWER,
                                { fold() },
                                toPlay,
                                icon = Icons.ChevronUp,
                                focusRequester = focus.requesterFor(headKey(group)),
                            )
                        }
                    }
                }
                itemsIndexed(
                    if (all) rows else rows.take(SHOWN),
                    key = { _, row -> rowKey(row) },
                ) { at, row ->
                    SourceRow(row, dimmed, focus, onPlay, if (all) toHeading(group, row, at) else toPlay)
                }
                if (rows.size > SHOWN) {
                    item(key = "fold:$group") {
                        SecondaryButton(
                            if (all) StreamChoiceCopy.SHOW_FEWER else StreamChoiceCopy.showAll(rows.size),
                            {
                                val opening = !all
                                fold()
                                // Opening: the 4th row, the first that was hidden, once it is composed.
                                if (opening) focusSoon(rowKey(rows[SHOWN]))
                            },
                            Modifier
                                .padding(start = 22.dp, top = 4.dp)
                                .then(toPlay),
                            icon = if (all) Icons.ChevronUp else Icons.ChevronDown,
                            focusRequester = focus.requesterFor(foldKey(group)),
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
    dimmed: Boolean,
    focus: FocusMemory,
    onPlay: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val key = rowKey(row)
    val shape = RoundedCornerShape(Marquee.radius.m)
    val watch = focus.watch(ROWS, key)
    var focused by remember { mutableStateOf(false) }
    // The focused row turns white with dark words (focusPalette); the rows of a page where none plays (the core's
    // empty rows) read dimmed and stay startable.
    val colors = focusPalette(focused).let { if (dimmed && !focused) it.dimmed() else it }
    val line = StreamChoiceCopy.rowLine(row)
    val tag = VersionsCopy.languageTag(row)
    val spoken =
        listOfNotNull(
            VersionsCopy.headline(row),
            tag,
            row.sizeBytes?.let(Format::bytes),
        ).joinToString(", ")
    Column(
        modifier
            .fillMaxWidth()
            .focusRequester(focus.requesterFor(key))
            .marqueeFocus(Marquee.radius.m, Tokens.FOCUS_SCALE_TILE)
            .onFocusChanged { state ->
                watch(state.isFocused)
                focused = state.isFocused
            }.clip(shape)
            .background(if (focused) colors.raised else Color.Transparent)
            .semantics {
                contentDescription = spoken
                // The line, then its details: what the eye gets from the row while it has focus.
                stateDescription = listOf(line.text, line.details).filter { it.isNotEmpty() }.joinToString(". ")
            }.focusClickable({ onPlay(row.key) }, Role.Button)
            .padding(horizontal = 22.dp, vertical = 14.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        FocusInk(colors) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                // The language tag under the quality, inside its cell, so every column stays under its heading.
                Column(Modifier.width(QUALITY), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    MqText(VersionsCopy.cell(row.quality), weight = FontWeight.SemiBold, maxLines = 1)
                    if (tag != null) LanguageTag(tag)
                }
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
}

/** The pick card's first line, plain at 64 percent: the frames' words, never an eyebrow. */
@Composable
private fun PickLine() {
    MqText("Best for this screen", role = TextRole.Callout, color = Marquee.colors.dim, weight = FontWeight.SemiBold)
}
