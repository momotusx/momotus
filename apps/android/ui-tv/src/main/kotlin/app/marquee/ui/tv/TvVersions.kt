@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.tv

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.LifecycleResumeEffect
import app.marquee.core.Format
import app.marquee.core.PluginCopy
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.VersionsCopy
import app.marquee.core.VersionsCopy.EmptyStep
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.EmptyVersions
import app.marquee.core.model.LearnedNote
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.views.VersionsView
import app.marquee.core.views.ViewStatus
import app.marquee.design.BadgeLook
import app.marquee.design.BadgeRow
import app.marquee.design.CapsLabel
import app.marquee.design.FocusInk
import app.marquee.design.GlassBar
import app.marquee.design.Icons
import app.marquee.design.LanguageTag
import app.marquee.design.LiveSpoken
import app.marquee.design.LocalReducedMotion
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
import app.marquee.design.focusClickable
import app.marquee.design.focusPalette
import app.marquee.design.marqueeFocus
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
 * @param onBack the empty state's Back.
 * @param onUndo Undo on the learned note.
 * @param held the page is open over the player: rows keep the order they opened in, new ones after them.
 */
@Composable
fun TvVersions(
    heading: String,
    versions: VersionsView?,
    focus: FocusMemory,
    onPlay: (String) -> Unit,
    onAddSource: () -> Unit,
    onBack: () -> Unit,
    onUndo: suspend (LearnedNote) -> Unit,
    modifier: Modifier = Modifier,
    held: Boolean = false,
) {
    val safe = Marquee.safe
    val snapshot = versions?.current()
    val failure = (versions?.status as? ViewStatus.Failed)?.reason
    val empty = snapshot?.empty?.takeIf { snapshot.recommended == null }
    val step = { it: EmptyStep ->
        when (it) {
            EmptyStep.PlayFromDevice -> {
                empty?.key?.let(onPlay)
            }

            EmptyStep.TryAgain -> {
                versions?.retry()
            }

            EmptyStep.Settings -> {
                // Back from Settings, the page asks again: an addon just added shows by itself.
                versions?.askAgain = true
                onAddSource()
            }

            EmptyStep.Back -> {
                onBack()
            }
        }
        Unit
    }
    // Back from Settings opened here: ask the core again when the page resumes, whether it stayed composed or not.
    LifecycleResumeEffect(versions) {
        if (versions?.askAgain == true) {
            versions.askAgain = false
            versions.retry()
        }
        onPauseOrDispose {}
    }
    // The list, for Right from the empty state's actions.
    val list = remember { FocusRequester() }
    val listState = rememberLazyListState()
    // The row the list last had focused and its place, so Right from the left column returns to it even once it
    // has scrolled out of composition (a section's heading or Play took focus and the list moved).
    var lastRow by remember { mutableStateOf<Pair<Int, String>?>(null) }
    val scope = rememberCoroutineScope()
    // The target exists once there is something to focus: the card's Play (or Try again, or the empty
    // state's first action), else the first row.
    val target =
        when {
            failure != null || snapshot?.recommended != null || empty != null -> PLAY
            else -> (snapshot?.rows?.firstOrNull() ?: snapshot?.plugins?.rows?.firstOrNull())?.let { rowKey(it) }
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
                    val back = lastRow
                    if (event.key == Key.DirectionRight && focus.focusedKey in left && back != null &&
                        focus.rowEntry(ROWS) == null
                    ) {
                        scope.launch {
                            listState.scrollToItem(back.first)
                            focus.focusWhenReady(back.second, focus.interactions, waitMs = 1_000)
                        }
                        return@onPreviewKeyEvent true
                    }
                }
                false
            }.padding(start = safe.side, end = safe.side, top = safe.top, bottom = safe.bottom),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.l),
    ) {
        Header(heading, snapshot)
        Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(Marquee.space.l)) {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                Pick(snapshot, failure, { versions?.retry() }, focus, onPlay) {
                    if (empty != null) Empty(empty, step, focus, list.takeIf { empty.rows.isNotEmpty() })
                }
                Shortcuts(snapshot, focus, list.takeIf { !snapshot?.rows.isNullOrEmpty() }, onPlay)
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                // The rows found when none plays are simply listed, dimmed, as they would rank once the reason is
                // fixed, each saying why it is dimmed.
                Sources(
                    snapshot,
                    empty?.rows ?: snapshot?.rows.orEmpty(),
                    empty?.kind,
                    held,
                    listState,
                    { at, key -> lastRow = at to key },
                    // Nothing to list yet and nothing said: the table's frame waits in place, so nothing moves.
                    failure == null && snapshot?.recommended == null && snapshot?.empty == null &&
                        snapshot?.rows.isNullOrEmpty(),
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

/** The left column's stops: Play (or the empty state's first action) and the shortcuts. */
private val left = setOf(PLAY, SHORT_SMALLER, SHORT_SHARPER)

private fun rowKey(row: VersionRow) = "row:${row.key}"

private fun headKey(section: String) = "head:$section"

private fun foldKey(section: String) = "fold:$section"

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
            MqText("Versions", Modifier.semantics { heading() }, role = TextRole.Title1, maxLines = 1)
        }
        Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            VersionsCopy.progress(snapshot)?.let { (text, state) -> StatusWord(text, tone(state)) }
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
    // Only the plugins' rows, never picked (4.4): no card and no block, one line over them.
    if (failure == null && VersionsCopy.onlyPlugins(snapshot)) {
        MqText(PluginCopy.CHOOSE, Modifier.width(620.dp), role = TextRole.Callout, color = colors.muted)
        return
    }
    Column(
        Modifier
            // Entering the card from the list lands on Play, never on the details above it.
            .then(if (best != null) Modifier.focusProperties { enter = { play } }.focusGroup() else Modifier)
            .width(620.dp)
            .clip(shape)
            .background(colors.surface)
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
                CardParts(
                    StreamChoiceCopy.BEST_TV,
                    VersionsCopy.badges(best),
                    VersionsCopy.why(best),
                    VersionsCopy.cardSpoken(StreamChoiceCopy.BEST_TV, best),
                ) {
                    PrimaryButton(
                        label = "Play this version",
                        icon = PlayFilled,
                        onClick = { onPlay(best.key) },
                        focusRequester = play,
                        onFocusChange = focus.watch(null, PLAY),
                    )
                }
            }

            snapshot?.empty != null -> {
                empty()
            }

            // Still asking, or every addon answered with nothing while the core checks what this device keeps.
            snapshot == null || !snapshot.done || snapshot.rows.isEmpty() -> {
                // The card's own parts, unseen, under one block: the pick lands with nothing below it moving.
                Box(Modifier.testTag(CARD_PLACEHOLDER)) {
                    Box(Modifier.alpha(0f).clearAndSetSemantics {}) {
                        CardParts(StreamChoiceCopy.BEST_TV, PLACEHOLDER_BADGES, "Starts quickly", "") {
                            PrimaryButton("Play this version", {}, icon = PlayFilled, enabled = false)
                        }
                    }
                    Skeleton(Modifier.matchParentSize(), colors.raised)
                }
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

/**
 * The shortcuts under the card, "Smaller file" and "Sharper", in the core's order, each only when the core sends it
 * (plans/versions-android-spec.md, section 1): one press plays it. Keyed `short:`, so a version that is also a section
 * row never shares a key; Right goes to the list.
 */
@Composable
private fun Shortcuts(
    snapshot: VersionsSnapshot?,
    focus: FocusMemory,
    list: FocusRequester?,
    onPlay: (String) -> Unit,
) {
    for ((label, row) in VersionsCopy.shortcuts(snapshot)) {
        val key = if (label == StreamChoiceCopy.SMALLER) SHORT_SMALLER else SHORT_SHARPER
        SecondaryButton(
            VersionsCopy.shortcutLabel(label, row),
            { onPlay(row.key) },
            Modifier.focusProperties { if (list != null) right = list },
            icon = if (key == SHORT_SMALLER) Icons.Gauge else Icons.Gem,
            focusRequester = focus.requesterFor(key),
            onFocusChange = focus.watch(null, key),
        )
    }
}

private const val SHORT_SMALLER = "short:smaller"
private const val SHORT_SHARPER = "short:sharper"

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
    why: EmptyKind?,
    held: Boolean,
    listState: LazyListState,
    onRowFocused: (Int, String) -> Unit,
    loading: Boolean,
    focus: FocusMemory,
    list: FocusRequester,
    onPlay: (String) -> Unit,
    modifier: Modifier,
) {
    val colors = Marquee.colors
    // The core's quality sections, or today's groups for rows they do not cover; held in their first order over the
    // player, so a re-rank under it never moves a row.
    val seen = remember { mutableStateMapOf<String, Int>() }
    val sections = VersionsCopy.sections(snapshot, rows).let { if (held) VersionsCopy.held(it, seen) else it }
    // The order the rows showed in, recorded after they show: held reads it, and composition changes nothing.
    if (held) LaunchedEffect(sections.flatMap { s -> s.rows.map { it.key } }) { VersionsCopy.record(sections, seen) }
    val open = remember { mutableStateMapOf<String, Boolean>() }
    val scope = rememberCoroutineScope()
    // Focus by key once its node is composed (an item a toggle just inserted is not yet attached).
    // Looked up when focus moves, so the builder below calls nothing composable.
    val toPlay = Modifier.focusProperties { left = focus.requester(PLAY) ?: FocusRequester.Default }

    /**
     * Left from the [at]th row of an open section reaches its heading: by focus search while the heading is
     * composed, else by scrolling it back in first (a long section leaves it out of composition, and the search
     * would land on Play).
     */
    fun toHeading(
        group: String,
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
            if (loading && rows.isEmpty()) {
                // A heading and four rows of the real heights, so the first rows land where these were.
                item(key = "skeleton:head") {
                    // A real heading's words, unseen, so it is a heading's height.
                    Heading {
                        Row(Modifier.weight(1f).alpha(0f).clearAndSetSemantics {}) {
                            CapsLabel("Best quality", Modifier.weight(1f))
                            MqText("0", role = TextRole.Callout, maxLines = 1)
                        }
                    }
                }
                items(SKELETON_ROWS, key = { "skeleton:$it" }) { RowSkeleton(Modifier.testTag("row-skeleton-$it")) }
            }
            for (section in sections) {
                val group = section.id
                val rows = section.rows
                val all = open[group] == true
                // An open section's heading is a stop holding Show fewer: Left from any of its rows, or Up from
                // the first, reaches it, so a long section folds in two presses (plans/versions-android-spec.md 2).
                val fold = {
                    open[group] = !all
                    // Back on the section's Show all once it has folded, its heading brought to the top first: from
                    // deep in a long section the Show all would land out of view and never take the focus.
                    if (all) {
                        val seen = listState.layoutInfo.visibleItemsInfo
                        val head =
                            seen.firstOrNull { it.key == group }?.index
                                ?: seen.firstOrNull { it.key == foldKey(group) }?.let { it.index - rows.size - 1 }
                        scope.launch {
                            head?.let {
                                listState.scrollToItem(it)
                                // Rows too tall for the heading and Show all on one screen: Show all at the top.
                                if (listState.layoutInfo.visibleItemsInfo.none { i -> i.key == foldKey(group) }) {
                                    listState.scrollToItem(it + 1 + SHOWN)
                                }
                            }
                            focus.focusWhenReady(foldKey(group), focus.interactions, waitMs = 1_000)
                        }
                    }
                }
                item(key = group) {
                    Heading {
                        // A heading to a screen reader: "1080p, 12 versions".
                        Row(
                            Modifier.weight(1f).clearAndSetSemantics {
                                heading()
                                contentDescription = VersionsCopy.sectionSpoken(section.title, rows.size)
                            },
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            CapsLabel(section.title, Modifier.weight(1f), colors.text)
                            MqText("${rows.size}", role = TextRole.Callout, maxLines = 1)
                        }
                        if (all) {
                            SecondaryButton(
                                StreamChoiceCopy.SHOW_FEWER,
                                { fold() },
                                toPlay,
                                icon = Icons.ChevronUp,
                                focusRequester = focus.requesterFor(headKey(group)),
                                spoken = VersionsCopy.foldSpoken(section.title, rows.size, open = true),
                            )
                        }
                    }
                }
                itemsIndexed(
                    if (all) rows else rows.take(SHOWN),
                    key = { _, row -> rowKey(row) },
                ) { at, row ->
                    SourceRow(row, why, focus, onPlay, if (all) toHeading(group, row, at) else toPlay) {
                        listState.layoutInfo.visibleItemsInfo
                            .firstOrNull { it.key == rowKey(row) }
                            ?.let { onRowFocused(it.index, rowKey(row)) }
                    }
                }
                if (rows.size > SHOWN) {
                    item(key = "fold:$group") {
                        SecondaryButton(
                            if (all) StreamChoiceCopy.SHOW_FEWER else StreamChoiceCopy.showAll(rows.size),
                            {
                                val opening = !all
                                // Where the fold sits now is where the 4th row lands once the section opens.
                                val at =
                                    listState.layoutInfo.visibleItemsInfo
                                        .firstOrNull { it.key == foldKey(group) }
                                        ?.index
                                fold()
                                // Opening: the 4th row, the first that was hidden, scrolled into view and focused once
                                // it is composed (rows are taller than the fold, so it may land below the screen).
                                if (opening) {
                                    scope.launch {
                                        at?.let { listState.scrollToItem(maxOf(0, it - 1)) }
                                        focus.focusWhenReady(rowKey(rows[SHOWN]), focus.interactions, waitMs = 1_000)
                                    }
                                }
                            },
                            Modifier
                                .padding(start = 22.dp, top = 4.dp)
                                .then(toPlay),
                            icon = if (all) Icons.ChevronUp else Icons.ChevronDown,
                            focusRequester = focus.requesterFor(foldKey(group)),
                            spoken = VersionsCopy.foldSpoken(section.title, rows.size, open = all),
                        )
                    }
                }
            }
        }
    }
}

/** A section's heading: plain, no outline (design/DECISION.md, Lines). */
@Composable
private fun Heading(content: @Composable RowScope.() -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .padding(top = Marquee.space.xs)
            .padding(horizontal = 22.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = Alignment.CenterVertically,
        content = content,
    )
}

/** How many rows the table's frame shows while versions are found. */
private const val SKELETON_ROWS = 4

/** A row's frame while versions are found: a real row's parts, unseen, under one block, so its height is a row's. */
@Composable
private fun RowSkeleton(modifier: Modifier = Modifier) {
    Box(modifier.fillMaxWidth().padding(horizontal = 22.dp, vertical = 14.dp)) {
        Column(Modifier.alpha(0f).clearAndSetSemantics {}, verticalArrangement = Arrangement.spacedBy(6.dp)) {
            BadgeRow(listOf("1080p" to BadgeLook.Filled))
            MqText("2 GB, ready now", role = TextRole.Callout, maxLines = 1)
            MqText("A.2010.1080p.mkv", role = TextRole.Caption, maxLines = 2, minLines = 2)
        }
        Skeleton(Modifier.matchParentSize())
    }
}

/**
 * One source (plans/versions-android-spec.md, section 1): its badges and language, one quiet line (size and source in
 * plain words, or the core's warning), and a details line kept for it at all times and faded in on focus, so a row is
 * one height whether focused or not and a held Down lays out nothing.
 */
@Composable
private fun SourceRow(
    row: VersionRow,
    why: EmptyKind?,
    focus: FocusMemory,
    onPlay: (String) -> Unit,
    modifier: Modifier = Modifier,
    onFocused: () -> Unit = {},
) {
    val key = rowKey(row)
    val shape = RoundedCornerShape(Marquee.radius.m)
    val watch = focus.watch(ROWS, key)
    var focused by remember { mutableStateOf(false) }
    // The focused row turns white with dark words (focusPalette); the rows of a page where none plays (the core's
    // empty rows) read dimmed, say [why] in place of their line, and stay startable.
    val colors = focusPalette(focused).let { if (why != null && !focused) it.dimmed() else it }
    val line = why?.let(StreamChoiceCopy::emptyRowLine) ?: StreamChoiceCopy.rowLine(row)
    val quiet = VersionsCopy.quiet(row).takeIf { why == null }
    val tag = VersionsCopy.languageTag(row)
    val reduced = LocalReducedMotion.current
    val details by animateFloatAsState(if (focused) 1f else 0f, tween(if (reduced) 0 else 180), label = "details")
    Column(
        modifier
            .fillMaxWidth()
            .focusRequester(focus.requesterFor(key))
            .marqueeFocus(Marquee.radius.m, Tokens.FOCUS_SCALE_TILE)
            .onFocusChanged { state ->
                watch(state.isFocused)
                focused = state.isFocused
                if (state.isFocused) onFocused()
            }.clip(shape)
            .background(if (focused) colors.raised else Color.Transparent)
            .semantics {
                contentDescription = VersionsCopy.rowSpoken(row)
                // A warning or why it is dimmed, then the details: what the eye gets from the row on focus.
                stateDescription =
                    listOfNotNull(line.text.takeIf { quiet == null }, VersionsCopy.details(row)).joinToString(". ")
            }.focusClickable({ onPlay(row.key) }, Role.Button)
            .padding(horizontal = 22.dp, vertical = 14.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        FocusInk(colors) {
            Row(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalAlignment = Alignment.CenterVertically) {
                BadgeRow(VersionsCopy.badges(row).map { it.text to badgeLook(it.look) })
                if (tag != null) LanguageTag(tag)
            }
            if (quiet != null) {
                MqText(VersionsCopy.rowWords(row), role = TextRole.Callout, color = colors.muted, maxLines = 1)
            } else {
                ToneWord(VersionsCopy.rowWords(row, why), tone(line.tone), maxLines = 1, icon = why?.let { Icons.Ban })
            }
            // Two lines kept, so a long release name shows whole on focus and every row stays one height.
            MqText(
                VersionsCopy.details(row),
                Modifier.graphicsLayer { alpha = details },
                role = TextRole.Caption,
                color = colors.muted,
                maxLines = 2,
                minLines = 2,
            )
        }
    }
}

/**
 * The card's parts (plans/versions-android-spec.md, section 1): the kicker, the version's badges, one why line clamped
 * to one line, then [play]. Said as one node, [spoken], with the whole why sentence; the button says itself.
 */
@Composable
private fun CardParts(
    kicker: String,
    badges: List<VersionsCopy.Badge>,
    why: String,
    spoken: String,
    play: @Composable () -> Unit,
) {
    Column(Modifier.testTag(CARD), verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        Column(
            Modifier.clearAndSetSemantics { if (spoken.isNotEmpty()) contentDescription = spoken },
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            MqText(
                kicker,
                role = TextRole.Callout,
                color = Marquee.colors.dim,
                weight = FontWeight.SemiBold,
                maxLines = 1,
            )
            BadgeRow(badges.map { it.text to badgeLook(it.look) })
            MqText(why, role = TextRole.Callout, color = Marquee.colors.muted, maxLines = 1)
        }
        play()
    }
}

/** The core's badge look in the design system's words. */
private fun badgeLook(look: VersionsCopy.Badge.Look) =
    when (look) {
        VersionsCopy.Badge.Look.Filled -> BadgeLook.Filled
        VersionsCopy.Badge.Look.Plain -> BadgeLook.Plain
        VersionsCopy.Badge.Look.Outlined -> BadgeLook.Outlined
    }

/** What the placeholder card holds, unseen, so it is the card's height. */
private val PLACEHOLDER_BADGES = listOf(VersionsCopy.Badge("4K", VersionsCopy.Badge.Look.Filled))

const val CARD_PLACEHOLDER = "versions-card-placeholder"
const val CARD = "versions-card"
