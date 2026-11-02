@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.phone

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
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
import app.marquee.core.model.SourceKind
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.views.VersionsView
import app.marquee.core.views.ViewStatus
import app.marquee.design.BadgeLook
import app.marquee.design.BadgeRow
import app.marquee.design.CapsLabel
import app.marquee.design.Chip
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
import app.marquee.design.RoundButton
import app.marquee.design.SecondaryButton
import app.marquee.design.SegmentTrack
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.TextRole
import app.marquee.design.ToneWord
import app.marquee.design.dimmed
import app.marquee.design.focusClickable
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

/** How many rows of a group show before "Show all", as on the web. */
private const val SHOWN = 3

/**
 * The ranked sources of one video on a phone (plans/android-design-v2.md 5.6, the web's v2
 * screen): the heading, how the search goes and what the ranking read of this phone; the
 * recommendation as a glass card with its facts; every other source by group with its line (a tone
 * and plain words, plans/m13-stream-choice.md 3.7). A tap opens a row to the line's details and its
 * own Play, so the card's white Play stays the one primary. When nothing can play, the card says why
 * in the core's words with its actions (plans/play-every-version.md 2a), the rows found listed
 * dimmed. A note says once what the picks taught, with Undo. The first scroll freezes the recommendation so it does not change
 * while results still stream in.
 *
 * @param heading what the screen is about ("Sintel" or "Sintel, S1:E2").
 * @param onPlay a version was chosen, by its key.
 * @param onAddSource no addon is installed: go where one is added; null (a kids profile) leaves Settings out.
 * @param onBack the empty state's Back.
 * @param onUndo Undo on the learned note.
 * @param held the page is open over the player: rows keep the order they opened in, new ones after them.
 * @param download the versions to keep for a download (plans/offline-downloads.md 3.2): each row's action
 *   downloads it, and [onPlay] gets the key to download.
 */
@Composable
fun PhoneVersions(
    heading: String,
    versions: VersionsView?,
    onPlay: (String) -> Unit,
    onAddSource: (() -> Unit)?,
    onBack: () -> Unit,
    onUndo: suspend (LearnedNote) -> Unit,
    modifier: Modifier = Modifier,
    held: Boolean = false,
    download: Boolean = false,
) {
    val side = Marquee.safe.side
    val action = if (download) Action(DOWNLOAD_THIS, Icons.ArrowDownToLine) else Action(PLAY_THIS, PlayFilled)
    val colors = Marquee.colors
    val snapshot = versions?.current()
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
                onAddSource?.invoke()
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
    // The row whose details sheet is open, and each row's info button, which takes the focus back when it closes.
    var sheet by remember { mutableStateOf<VersionRow?>(null) }
    var lastSheet by remember { mutableStateOf<String?>(null) }
    val infos = remember { mutableMapOf<String, FocusRequester>() }
    LaunchedEffect(sheet) {
        if (sheet == null) lastSheet?.let { key -> infos[key]?.let { runCatching { it.requestFocus() } } }
    }
    // A press pins the pick, so the version under the finger at the press is the one its release plays.
    val press = { if (snapshot?.recommended != null && snapshot.frozen != true) versions?.freeze() }
    // The kind chips (plans/plugins-and-marketplace.md 4.2): the core counts the kinds and, under one (`setKind`, which
    // freezes the pick), sends that kind's own card, rows and sections. They show from two kinds on, then stay while the
    // page is open. The kind holds while this video's Versions page or player is on the stack (AppRoot).
    var chipsShown by rememberSaveable { mutableStateOf(false) }
    val showsKinds = VersionsCopy.showsKinds(snapshot)
    LaunchedEffect(showsKinds) { if (showsKinds) chipsShown = true }
    val chips = if (chipsShown || showsKinds) VersionsCopy.kindChips(snapshot) else emptyList()
    // The chip that is on again asks nothing.
    val chooseKind: (SourceKind?) -> Unit = { k ->
        if (k != snapshot?.selectedKind) versions?.setKind(k)
    }
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
    // The rows found when none plays are simply listed, dimmed, as they would rank once the reason is fixed.
    // The core's quality sections, or today's groups for rows they do not cover.
    val seen = remember { mutableStateMapOf<String, Int>() }
    val sections =
        VersionsCopy.sections(snapshot, empty?.rows ?: snapshot?.rows.orEmpty()).let {
            if (held) VersionsCopy.held(it, seen) else it
        }
    // The order the rows showed in, recorded after they show: held reads it, and composition changes nothing.
    if (held) LaunchedEffect(sections.flatMap { s -> s.rows.map { it.key } }) { VersionsCopy.record(sections, seen) }
    val open = remember { mutableStateMapOf<String, Boolean>() }
    // A tapped fold keeps its place under the finger: its offset before the change, put back once the list holds the
    // change. The index is counted as the list below is built (head, card, shortcuts, the kind chips, then each section's heading,
    // shown rows and fold); an item added above a fold has to be counted here too.
    var anchor by remember { mutableStateOf<Pair<String, Int>?>(null) }
    LaunchedEffect(anchor) {
        val (group, offset) = anchor ?: return@LaunchedEffect
        var index = 2 + VersionsCopy.shortcuts(snapshot).size + if (chips.isEmpty()) 0 else 1
        for (section in sections) {
            index += 1 + if (open[section.id] == true) section.rows.size else minOf(section.rows.size, SHOWN)
            if (section.id == group) break
            if (section.rows.size > SHOWN) index++
        }
        listState.scrollToItem(index, -offset)
        anchor = null
    }
    Box(modifier.fillMaxSize()) {
        LazyColumn(
            state = listState,
            // Under an open sheet the page is out of a screen reader's reach, so the sheet reads as a modal.
            modifier =
                Modifier
                    .fillMaxSize()
                    .then(if (sheet != null) Modifier.clearAndSetSemantics {} else Modifier),
            contentPadding =
                PaddingValues(start = side, end = side, top = pageTop() + Marquee.space.m, bottom = Marquee.space.xl),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            item(key = "head") {
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    MqText(
                        heading,
                        role = TextRole.Callout,
                        color = colors.dim,
                        weight = FontWeight.SemiBold,
                        maxLines = 1,
                    )
                    MqText("Versions", Modifier.semantics { heading() }, role = TextRole.Title1, maxLines = 1)
                    if (download) MqText(KEEP_LINE, role = TextRole.Callout, color = colors.muted)
                    VersionsCopy.progress(snapshot)?.let { (text, tone) ->
                        StatusWord(text, tone(tone), Modifier.padding(top = 4.dp))
                    }
                }
            }
            item(key = "best") {
                Pick(
                    snapshot,
                    VersionsCopy.kicker(snapshot, StreamChoiceCopy.BEST_PHONE),
                    failure,
                    { versions?.retry() },
                    onPlay,
                    Modifier.padding(vertical = Marquee.space.s),
                    action,
                ) {
                    if (empty != null) Empty(empty, step, addSource = onAddSource != null)
                }
            }
            // The shortcuts under the card, each one tap to play; keyed apart from the section rows they may repeat.
            // The shortcuts play: a download's pick is the card or a row.
            for ((label, row) in if (download) emptyList() else VersionsCopy.shortcuts(snapshot)) {
                val key = if (label == StreamChoiceCopy.SMALLER) "short:smaller" else "short:sharper"
                item(key = key) {
                    SecondaryButton(
                        VersionsCopy.shortcutLabel(label, row),
                        { onPlay(row.key) },
                        Modifier.fillMaxWidth(),
                        icon = if (label == StreamChoiceCopy.SMALLER) Icons.Gauge else Icons.Gem,
                    )
                }
            }
            // The kind chips under the card and its shortcuts, at the head of the list: a row that arrives late moves the
            // list, never Play under a finger.
            if (chips.isNotEmpty()) item(key = "kinds") { KindChips(chips, snapshot?.selectedKind, chooseKind) }
            for (section in sections) {
                val group = section.id
                val rows = section.rows
                val all = open[group] == true
                item(key = group) {
                    // A plain heading, no outline (design/DECISION.md, Lines).
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .padding(top = Marquee.space.xs)
                            .padding(horizontal = 16.dp, vertical = 10.dp),
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
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
                    }
                }
                items(if (all) rows else rows.take(SHOWN), key = { "row:${it.key}" }) { row ->
                    SourceRow(
                        row,
                        empty?.kind,
                        onPlay,
                        {
                            lastSheet = row.key
                            sheet = row
                        },
                        press,
                        infos,
                    )
                }
                if (rows.size > SHOWN) {
                    item(key = "fold:$group") {
                        SecondaryButton(
                            if (all) StreamChoiceCopy.SHOW_FEWER else StreamChoiceCopy.showAll(rows.size),
                            {
                                val at = listState.layoutInfo.visibleItemsInfo.firstOrNull { it.key == "fold:$group" }
                                open[group] = !all
                                if (at != null) anchor = group to at.offset
                            },
                            icon = if (all) Icons.ChevronUp else Icons.ChevronDown,
                            spoken = VersionsCopy.foldSpoken(section.title, rows.size, open = all),
                        )
                    }
                }
            }
            snapshot?.note?.let { note -> item(key = "note") { LearnedLine(note, onUndo) } }
        }
        sheet?.let { row ->
            DetailsSheet(row, why = empty?.kind, onPlay = onPlay, onClose = { sheet = null }, action = action)
        }
    }
}

/**
 * The kind chips (plans/plugins-and-marketplace.md 4.2), only with two kinds or more: one choice at a time, each with
 * its count.
 */
@Composable
private fun KindChips(
    chips: List<VersionsCopy.KindLabel>,
    on: SourceKind?,
    onChoose: (SourceKind?) -> Unit,
) {
    if (chips.isEmpty()) return
    // Named as a group: "Sources". The chip that is on stays in view on a narrow track or at a large font, by the
    // track's own scroll.
    SegmentTrack(
        Modifier.selectableGroup().semantics { contentDescription = StreamChoiceCopy.KIND_LABEL },
        kept = chips.indexOfFirst { it.kind == on },
    ) {
        for (chip in chips) {
            Chip(
                chip.label,
                chip.kind == on,
                { onChoose(chip.kind) },
                detail = chip.note,
                inTrack = true,
                spoken = chip.spoken,
            )
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
    kicker: String,
    failure: String?,
    onRetry: () -> Unit,
    onPlay: (String) -> Unit,
    modifier: Modifier,
    action: Action,
    empty: @Composable () -> Unit,
) {
    val colors = Marquee.colors
    val shape = RoundedCornerShape(Marquee.radius.l)
    val best = snapshot?.recommended
    // Only the plugins' rows, never picked (4.4): no card and no block, one line over them.
    if (failure == null && VersionsCopy.onlyPlugins(snapshot)) {
        MqText(PluginCopy.CHOOSE, modifier, role = TextRole.Callout, color = colors.muted)
        return
    }
    Column(
        modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surface)
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
                CardParts(
                    kicker,
                    VersionsCopy.badges(best),
                    VersionsCopy.why(best),
                    VersionsCopy.cardSpoken(kicker, best),
                ) {
                    PrimaryButton(
                        label = action.label,
                        icon = action.icon,
                        onClick = { onPlay(best.key) },
                        modifier = Modifier.fillMaxWidth().heightIn(min = 48.dp),
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
                        CardParts(StreamChoiceCopy.BEST_PHONE, PLACEHOLDER_BADGES, "Starts quickly", "") {
                            PrimaryButton(
                                "Play this version",
                                {},
                                Modifier.fillMaxWidth().heightIn(min = 48.dp),
                                icon = PlayFilled,
                                enabled = false,
                            )
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
 * Why nothing can play, in the core's words, said once when it appears, with its actions in the shared order under
 * it (plans/versions-android-spec.md, section 5), over the rows listed dimmed when there are some.
 */
@Composable
private fun Empty(
    empty: EmptyVersions,
    onStep: (EmptyStep) -> Unit,
    addSource: Boolean,
) {
    val text = StreamChoiceCopy.emptyLine(empty.kind)
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        LiveSpoken(text, ON_ARRIVAL_MS) { MqText(text, role = TextRole.Headline, weight = FontWeight.SemiBold) }
        for ((i, step) in VersionsCopy.emptySteps(empty, addSource).withIndex()) {
            val icon =
                when (step) {
                    EmptyStep.PlayFromDevice -> PlayFilled
                    EmptyStep.TryAgain -> Icons.RefreshCw
                    EmptyStep.Settings -> Icons.Settings
                    EmptyStep.Back -> Icons.ArrowLeft
                }
            if (i == 0) {
                PrimaryButton(step.label, { onStep(step) }, Modifier.fillMaxWidth(), icon = icon)
            } else {
                SecondaryButton(step.label, { onStep(step) }, icon = icon)
            }
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
    val text = StreamChoiceCopy.learnedNote(note.choice, note.up, tv = false, note.prefs)
    if (undone || text == null) return
    GlassBar(Modifier.padding(bottom = Marquee.space.xs)) {
        MqIcon(Icons.Sparkles, tint = colors.muted)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            // Said once when it appears, after the screen's first focus.
            LiveSpoken(text, ON_ARRIVAL_MS) { MqText(text, role = TextRole.Callout) }
            SecondaryButton(StreamChoiceCopy.UNDO, {
                undone = true
                scope.launch { runCatching { onUndo(note) }.onFailure { undone = false } }
            }, icon = Icons.Undo2)
        }
    }
}

/**
 * One source (plans/versions-android-spec.md, sections 1 and 3): its badges and language, then one quiet line (size and
 * source in plain words, or the core's warning). One tap plays it; the trailing info button, a sibling in the same
 * slot (not a button inside a button), opens its details. A screen reader hears the row as one stop and reaches the
 * details through its Details action, the info button hidden from it.
 */
@Composable
private fun SourceRow(
    row: VersionRow,
    why: EmptyKind?,
    onPlay: (String) -> Unit,
    onInfo: () -> Unit,
    onPress: () -> Unit,
    infos: MutableMap<String, FocusRequester>,
) {
    // The info button's requester, known by the row's key while it is composed: Back from the sheet comes back to it.
    val info = remember { FocusRequester() }
    DisposableEffect(row.key, info) {
        infos[row.key] = info
        onDispose { if (infos[row.key] === info) infos.remove(row.key) }
    }
    val line = why?.let(StreamChoiceCopy::emptyRowLine) ?: StreamChoiceCopy.rowLine(row)
    val quiet = VersionsCopy.quiet(row).takeIf { why == null }
    val spoken = listOfNotNull(VersionsCopy.rowSpoken(row), line.text.takeIf { quiet == null }).joinToString(", ")
    // The rows of a page where none plays (the core's empty rows) are dimmed, say [why] in place of their line,
    // and stay startable.
    FocusInk(if (why != null) Marquee.colors.dimmed() else Marquee.colors) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Column(
                Modifier
                    .weight(1f)
                    // Not consumed: the tap still plays; the press only pins the pick first.
                    .pointerInput(row.key) {
                        awaitEachGesture {
                            awaitFirstDown(requireUnconsumed = false)
                            onPress()
                        }
                    }.semantics {
                        contentDescription = spoken
                        customActions =
                            listOf(
                                CustomAccessibilityAction(StreamChoiceCopy.DETAILS) {
                                    onInfo()
                                    true
                                },
                            )
                    }.focusClickable({ onPlay(row.key) }, Role.Button)
                    .padding(horizontal = 12.dp, vertical = 12.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                RowFace(row, why)
            }
            Box(Modifier.size(48.dp).clearAndSetSemantics {}, contentAlignment = Alignment.Center) {
                RoundButton(Icons.Info, StreamChoiceCopy.DETAILS, onInfo, focusRequester = info)
            }
        }
    }
}

/**
 * What a row shows, the same in the row and atop its details sheet, so the sheet drops nothing: its badges and
 * language, then its one line (the size first, then its source, the core's warning or why it is dimmed).
 */
@Composable
private fun RowFace(
    row: VersionRow,
    why: EmptyKind?,
) {
    val line = why?.let(StreamChoiceCopy::emptyRowLine) ?: StreamChoiceCopy.rowLine(row)
    val tag = VersionsCopy.languageTag(row)
    Row(
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BadgeRow(VersionsCopy.badges(row).map { it.text to badgeLook(it.look) })
        if (tag != null) LanguageTag(tag)
    }
    if (VersionsCopy.quiet(row) != null && why == null) {
        MqText(VersionsCopy.rowWords(row), role = TextRole.Callout, color = Marquee.colors.muted, maxLines = 2)
    } else {
        ToneWord(VersionsCopy.rowWords(row, why), tone(line.tone), icon = why?.let { Icons.Ban })
    }
}

/**
 * A row's details in a sheet from the foot (plans/versions-android-spec.md, section 3): what the row shows first
 * ([RowFace]: its badges, its size and its warning), then the people sharing it, the release name, its languages and
 * sources, and its own Play. Back and a tap outside close it.
 */
@Composable
private fun DetailsSheet(
    row: VersionRow,
    why: EmptyKind?,
    onPlay: (String) -> Unit,
    onClose: () -> Unit,
    action: Action,
) {
    BackHandler(onBack = onClose)
    val colors = Marquee.colors
    // The focus goes into the sheet at once and stays in it until the sheet closes.
    val play = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { play.requestFocus() } }
    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black.copy(alpha = 0.5f))
            .pointerInput(Unit) { detectTapGestures { onClose() } },
    ) {
        Column(
            Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .clip(RoundedCornerShape(topStart = 10.dp, topEnd = 10.dp))
                .background(colors.surface)
                // Taps on the sheet stay in it.
                .pointerInput(Unit) { detectTapGestures { } }
                .testTag(DETAILS_SHEET)
                .semantics { paneTitle = StreamChoiceCopy.DETAILS }
                .focusProperties { exit = { FocusRequester.Cancel } }
                .focusGroup()
                .windowInsetsPadding(WindowInsets.navigationBars)
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
        ) {
            RowFace(row, why)
            // The core's details after the face, but what the face already said (VersionsCopy.sheetDetails).
            VersionsCopy.sheetDetails(row, why).takeIf { it.isNotEmpty() }?.let { MqText(it, role = TextRole.Callout) }
            // ponytail: the system monospace face; IBM Plex Mono as on the web when Android bundles it.
            MqText(
                Format.releaseWrapped(row.releaseName),
                role = TextRole.Caption,
                color = colors.muted,
                fontFamily = FontFamily.Monospace,
            )
            MqText(VersionsCopy.detail(row), role = TextRole.Caption, color = colors.muted)
            PrimaryButton(
                action.label,
                { onPlay(row.key) },
                Modifier.fillMaxWidth().heightIn(min = 48.dp),
                icon = action.icon,
                focusRequester = play,
            )
        }
    }
}

const val DETAILS_SHEET = "versions-details"

/** The pick's primary: play it, or keep it for a download. */
private class Action(
    val label: String,
    val icon: ImageVector,
)

private const val PLAY_THIS = "Play this version"
private const val DOWNLOAD_THIS = "Download this version"
private const val KEEP_LINE = "Pick the version to keep on this device"

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
            verticalArrangement = Arrangement.spacedBy(6.dp),
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
