package app.marquee.android

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.BiasAlignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.isTraversalGroup
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.DefaultsCopy
import app.marquee.core.DefaultsStep
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.StreamChoiceSettings
import app.marquee.core.SubtitleStep
import app.marquee.core.model.DefaultAddonInfo
import app.marquee.core.model.DefaultAddons
import app.marquee.core.model.DefaultInstalled
import app.marquee.core.model.FirstRunStill
import app.marquee.core.model.Prefs
import app.marquee.core.route.DefaultsMode
import app.marquee.core.session.CoreSession
import app.marquee.design.BrandLockup
import app.marquee.design.FocusInk
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.KeyPresses
import app.marquee.design.LiveSpoken
import app.marquee.design.MarkTile
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.PrimaryButton
import app.marquee.design.SecondaryButton
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.SwitchRow
import app.marquee.design.TextRole
import app.marquee.design.Tokens
import app.marquee.design.focusClickable
import app.marquee.design.focusPalette
import app.marquee.design.focusSelectable
import app.marquee.design.focusSoon
import app.marquee.design.marqueeFocus
import app.marquee.design.windowPx
import coil3.compose.AsyncImage
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

/**
 * A new user's first screen, one decision: how to begin. "I use Stremio" brings a library over (only
 * offered when [onImport] is given), "Start fresh" and "Browse first" open the recommended addons
 * ([onDefaults]), and "I already use Marquee" joins that device's sync account (only offered when
 * [onPair] is given). Nothing is typed here, and nothing is installed before the addons step.
 */
@Composable
fun OnboardingScreen(
    onDefaults: () -> Unit,
    onImport: (() -> Unit)? = null,
    onPair: (() -> Unit)? = null,
    /** The frame's still (B4); null keeps it black. */
    still: FirstRunStill? = null,
) {
    val tv = Marquee.form == Form.Tv
    val first = remember { FocusRequester() }
    AutoFocus(first)
    FramedPage(
        1,
        "Browse first",
        onDefaults,
        null,
        still,
        "How would you like to begin?",
        "No account needed. Everything stays on this device until you turn on sync.",
    ) {
        val doors: @Composable (Modifier) -> Unit = { door ->
            onImport?.let {
                Door(
                    "I use Stremio",
                    "Bring your addons, library and Continue Watching over with a code.",
                    Icons.DoorOpen,
                    it,
                    door,
                    first,
                )
            }
            Door(
                "Start fresh",
                "Choose from the recommended addons, then start watching.",
                Icons.Sparkles,
                onDefaults,
                door,
                first.takeIf { onImport == null },
            )
            onPair?.let {
                Door(
                    "I already use Momotus",
                    "Join your other device's sync account with a code. Library and addons follow.",
                    Icons.QrCode,
                    it,
                    door,
                    null,
                )
            }
        }
        Doors(tv, listOfNotNull(onImport, onDefaults, onPair).size, doors)
    }
}

/** The doors in the bar under the frame: side by side on TV, sharing the column's width; stacked on the phone. */
@Composable
private fun Doors(
    tv: Boolean,
    count: Int,
    doors: @Composable (Modifier) -> Unit,
) {
    if (tv) {
        // An exact width each, so the row's one height (the tallest door's) is measured at the width they get.
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val gap = Marquee.space.m
            val width = (maxWidth - gap * (count - 1)) / count
            Row(
                Modifier.fillMaxWidth().height(IntrinsicSize.Min).semantics { isTraversalGroup = true },
                horizontalArrangement = Arrangement.spacedBy(gap),
            ) { doors(Modifier.width(width).fillMaxHeight()) }
        }
    } else {
        Column(
            Modifier.semantics { isTraversalGroup = true },
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) { doors(Modifier.fillMaxWidth()) }
    }
}

/**
 * A first-run question step (B4, the owner's pick of 2026-10-28): the step's header, then a film frame on black
 * holding the still with the question as its subtitle, and the choices in the bar under it ([bar]); [foot] stays in
 * sight under it all. The frame is as wide as the screen while the height the header and the bar leave allows, else
 * that height at 2.39:1, centered; never under [FILM_FLOOR] of the screen, the page scrolling then. The header and
 * the bar keep one column edge.
 */
@Composable
private fun FramedPage(
    step: Int,
    skip: String,
    onSkip: () -> Unit,
    onBack: (() -> Unit)?,
    still: FirstRunStill?,
    question: String,
    lead: String,
    foot: (@Composable () -> Unit)? = null,
    bar: @Composable ColumnScope.() -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val side = Marquee.safe.side
    Column(
        Modifier
            .fillMaxSize()
            .background(Color.Black)
            // A step change is announced by its question (TalkBack reads a new pane by its title).
            .semantics { paneTitle = question }
            .windowInsetsPadding(WindowInsets.systemBars)
            .padding(vertical = Marquee.safe.top),
    ) {
        StepHeader(step, skip, onSkip, onBack, Modifier.padding(horizontal = side).testTag(STEP_HEADER))
        val density = LocalDensity.current
        val floor = with(density) { windowPx().height.toDp() } * FILM_FLOOR
        val pad = Marquee.space.m
        val gap = if (tv) 48.dp else 20.dp
        val scroll = rememberScrollState()
        BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
            val room = maxHeight
            SubcomposeLayout(Modifier.fillMaxWidth().verticalScroll(scroll)) { constraints ->
                val width = constraints.maxWidth
                val choices =
                    subcompose("bar") {
                        Column(
                            Modifier
                                .fillMaxWidth()
                                .padding(horizontal = side)
                                .padding(top = gap)
                                .testTag(FILM_BAR),
                            verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
                        ) {
                            // The phone's lead line sits under the frame, over the choices.
                            if (!tv) {
                                MqText(
                                    lead,
                                    Modifier.fillMaxWidth(),
                                    role = TextRole.Callout,
                                    color = Marquee.colors.dim,
                                    textAlign = TextAlign.Center,
                                )
                            }
                            bar()
                        }
                    }.map { it.measure(Constraints(maxWidth = width)) }
                val barHeight = choices.sumOf { it.height }
                val edge = pad.roundToPx()
                val left = room.roundToPx() - barHeight - 2 * edge
                val height = minOf(width / FILM_RATIO, maxOf(left, floor.roundToPx()).toFloat())
                val frameWidth = minOf(width, (height * FILM_RATIO).toInt())
                val frameHeight = (frameWidth / FILM_RATIO).toInt()
                val frame =
                    subcompose("frame") { FilmFrame(still, question, lead) }
                        .map { it.measure(Constraints.fixed(frameWidth, frameHeight)) }
                val content = edge + frameHeight + barHeight + edge
                val total = maxOf(content, room.roundToPx())
                // What is left when the width binds goes above and below alike on TV, below the bar on the phone.
                val top = edge + if (tv) (total - content) / 2 else 0
                layout(width, total) {
                    frame.forEach { it.place((width - frameWidth) / 2, top) }
                    var y = top + frameHeight
                    choices.forEach {
                        it.place(0, y)
                        y += it.height
                    }
                }
            }
        }
        foot?.let { Box(Modifier.fillMaxWidth().padding(horizontal = side)) { it() } }
    }
}

/**
 * The film frame, at the size its page gives it: the still fills it, faded in by opacity; with none it stays black.
 * The question is its subtitle, one 60 percent box per line, and a real heading; on TV the lead line rides under it.
 */
@Composable
internal fun FilmFrame(
    still: FirstRunStill?,
    question: String,
    lead: String,
) {
    val tv = Marquee.form == Form.Tv
    BoxWithConstraints(Modifier.fillMaxSize().background(Color.Black).testTag(FILM_FRAME)) {
        still?.let { s ->
            var landed by remember(s.background) { mutableStateOf(false) }
            val alpha by animateFloatAsState(if (landed) 1f else 0f, tween(Marquee.panelMs), label = "still")
            AsyncImage(
                model = s.background,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                alignment = BiasAlignment(0f, -0.2f),
                onSuccess = { landed = true },
                modifier = Modifier.matchParentSize().graphicsLayer { this.alpha = alpha },
            )
        }
        Column(
            Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .padding(horizontal = if (tv) maxWidth * 0.06f else 14.dp)
                .padding(bottom = maxHeight * if (tv) 0.09f else 0.12f),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Cue(
                question,
                if (tv) TextRole.Title3 else TextRole.Title2,
                FontWeight.SemiBold,
                Modifier.semantics { heading() },
            )
            if (tv) Cue(lead, TextRole.Headline, FontWeight.Medium)
        }
    }
}

/** A line set as a film's subtitle: white, one box of the player's 60 percent scrim behind each line. */
@Composable
private fun Cue(
    text: String,
    role: TextRole,
    weight: FontWeight,
    modifier: Modifier = Modifier,
) {
    var layout by remember { mutableStateOf<TextLayoutResult?>(null) }
    MqText(
        text,
        modifier.drawBehind {
            val l = layout ?: return@drawBehind
            val pad = l.size.height / l.lineCount.coerceAtLeast(1) * 0.22f
            for (i in 0 until l.lineCount) {
                val left = l.getLineLeft(i) - pad
                val right = l.getLineRight(i) + pad
                drawRect(
                    FILM_SCRIM,
                    Offset(left, l.getLineTop(i)),
                    Size(
                        right - left,
                        l.getLineBottom(i) - l.getLineTop(i),
                    ),
                )
            }
        },
        role = role,
        color = Color.White,
        weight = weight,
        textAlign = TextAlign.Center,
        onTextLayout = { layout = it },
    )
}

/**
 * A first-run step's header: the mark and name at the left; Back (from step 2), "Step N of 4" with its dots and the
 * step's skip at the right. The phone reads the count without showing it.
 */
@Composable
private fun StepHeader(
    step: Int,
    skip: String?,
    onSkip: () -> Unit,
    onBack: (() -> Unit)?,
    modifier: Modifier = Modifier,
) {
    val tv = Marquee.form == Form.Tv
    Row(
        modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(if (tv) Marquee.space.m else 0.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.weight(1f)) { BrandLockup(if (tv) 44.dp else 32.dp) }
        onBack?.let { SkipLink("Back", it, back = true) }
        StepCount(step)
        skip?.let { SkipLink(it, onSkip) }
    }
}

/** "Step N of 4": its dots and words on TV; the phone has no room for them and reads them only. */
@Composable
private fun StepCount(step: Int) {
    val words = "Step $step of $FIRST_RUN_STEPS"
    if (Marquee.form != Form.Tv) {
        Box(Modifier.size(1.dp).semantics { contentDescription = words })
        return
    }
    Row(horizontalArrangement = Arrangement.spacedBy(14.dp), verticalAlignment = Alignment.CenterVertically) {
        Row(Modifier.clearAndSetSemantics { }, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            for (i in 1..FIRST_RUN_STEPS) {
                Box(
                    Modifier
                        .size(if (i == step) 28.dp else 10.dp, 10.dp)
                        .clip(RoundedCornerShape(5.dp))
                        .background(if (i == step) Color.White else Color.White.copy(alpha = 0.4f)),
                )
            }
        }
        MqText(words, role = TextRole.Caption, weight = FontWeight.SemiBold, color = Marquee.colors.dim)
    }
}

/** The question's write for the page's life. */
class StreamQuestionHolder(
    session: CoreSession,
) : ViewModel() {
    val page = StreamChoiceSettings(session.ops, viewModelScope)
}

/**
 * The one question of a first run, after the recommended addons (plans/m13-stream-choice.md 4.5): how
 * Marquee should pick what to play. Start fast has focus; each answer writes its patch, then [onDone].
 * Skip writes nothing, since the defaults are Start fast's.
 */
@Composable
fun StreamQuestionScreen(
    session: CoreSession,
    still: FirstRunStill? = null,
    onBack: (() -> Unit)? = null,
    onDone: () -> Unit,
) {
    val holder =
        viewModel<StreamQuestionHolder>(
            key = "stream-question",
            factory = viewModelFactory { initializer { StreamQuestionHolder(session) } },
        )
    val page = holder.page
    StreamQuestionPage(still, page.problem, onBack, onDone) { page.answer(it, onDone) }
}

/** The question's page: the frame, the two answers in its bar, and why a save failed ([problem]). */
@Composable
internal fun StreamQuestionPage(
    still: FirstRunStill?,
    problem: String?,
    onBack: (() -> Unit)?,
    onSkip: () -> Unit,
    onAnswer: (StreamChoiceCopy.Answer) -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val fast = remember { FocusRequester() }
    AutoFocus(fast)
    FramedPage(
        3,
        StreamChoiceCopy.SKIP,
        onSkip,
        onBack,
        still,
        StreamChoiceCopy.QUESTION_TITLE,
        StreamChoiceCopy.QUESTION_LEAD,
    ) {
        Doors(tv, 2) { door ->
            Door(StreamChoiceCopy.FAST.label, StreamChoiceCopy.FAST.text, Icons.Zap, {
                onAnswer(StreamChoiceCopy.FAST)
            }, door, fast)
            Door(StreamChoiceCopy.BEST.label, StreamChoiceCopy.BEST.text, Icons.Gem, {
                onAnswer(StreamChoiceCopy.BEST)
            }, door, null)
        }
        problem?.let { StatusWord("Could not save the choice: $it", Status.Bad) }
    }
}

/** The subtitles step for the page's life (the state and the write live in [SubtitleStep]). */
class SubtitleStepHolder(
    session: CoreSession,
    device: String?,
) : ViewModel() {
    val step = SubtitleStep(session.ops, viewModelScope, device).also { it.load() }
}

/**
 * A first run's subtitles step, after the question (plans/m13-subtitle-languages.md 7): the language
 * subtitles start in, one choice, this device's chosen at first. Continue writes it, then [onDone]; Skip
 * writes nothing. A profile with subtitle languages already passes by. Continue stays in sight under the
 * list; on TV it has focus first, so one press keeps the device's language.
 */
@Composable
fun SubtitleStepScreen(
    session: CoreSession,
    still: FirstRunStill? = null,
    onBack: (() -> Unit)? = null,
    onDone: () -> Unit,
) {
    val device = DeviceCaps.languageCode(LocalConfiguration.current.locales[0])
    val holder =
        viewModel<SubtitleStepHolder>(
            key = "subtitle-step",
            factory = viewModelFactory { initializer { SubtitleStepHolder(session, device) } },
        )
    SubtitleStepGate({ prefsNow(session)?.subtitleLanguages.isNullOrEmpty() }, holder.step, still, onBack, onDone)
}

/** Asks [asks] once, before anything shows: the page when it says yes, else [onDone] at once. */
@Composable
internal fun SubtitleStepGate(
    asks: suspend () -> Boolean,
    step: SubtitleStep,
    still: FirstRunStill? = null,
    onBack: (() -> Unit)? = null,
    onDone: () -> Unit,
) {
    val leave by rememberUpdatedState(onDone)
    // Answered once: a step that passes by leaves once.
    val shows by produceState<Boolean?>(null) { value = asks().also { if (!it) leave() } }
    if (shows == true) SubtitleStepPage(step, still, onBack, onDone)
}

/** The subtitles step's page over [step]: the frame, the languages in its bar, Continue and More languages. */
@Composable
internal fun SubtitleStepPage(
    step: SubtitleStep,
    still: FirstRunStill? = null,
    onBack: (() -> Unit)? = null,
    onDone: () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val go = remember { FocusRequester() }
    AutoFocus(go)
    // A disabled Continue cannot take focus: on TV it is asked again once the list is in, unless a key has
    // moved the focus since the page came.
    val keys = remember { KeyPresses.count }
    LaunchedEffect(step.loaded) {
        if (tv && step.loaded && KeyPresses.count == keys) focusSoon(go, 60)
    }
    var more by remember { mutableStateOf(false) }
    // Not while Continue saves: the step leaves once.
    FramedPage(
        4,
        SubtitleStep.SKIP,
        { if (!step.busy) onDone() },
        onBack,
        still,
        SubtitleStep.TITLE,
        SubtitleStep.LEAD,
        foot = {
            Row(
                Modifier.fillMaxWidth().padding(top = Marquee.space.s),
                horizontalArrangement =
                    Arrangement.spacedBy(
                        Marquee.space.xs,
                        if (tv) Alignment.CenterHorizontally else Alignment.Start,
                    ),
            ) {
                PrimaryButton(
                    DefaultsCopy.CONTINUE,
                    { step.save(onDone) },
                    icon = Icons.Check,
                    // Not before the language list: the device's language is unknown until then.
                    enabled = !step.busy && step.loaded,
                    focusRequester = go,
                )
                SecondaryButton(if (more) SubtitleStep.FEWER else SubtitleStep.MORE, { more = !more })
            }
        },
    ) {
        val rows =
            step.common.map { language ->
                Language(
                    language.name,
                    SubtitleStep.DEVICE.takeIf { language.code == step.mine },
                    step.chosen == language.code,
                ) {
                    step.choose(language.code)
                }
            } + Language(SubtitleStep.NONE, SubtitleStep.NONE_LINE, step.chosen == null) { step.choose(null) }
        LanguageRows(rows, tv)
        if (more) {
            Kicker(SubtitleStep.MORE, side = 0.dp)
            LanguageRows(step.rest.map { Language(it.name, null, step.chosen == it.code) { step.choose(it.code) } }, tv)
        }
        step.listProblem?.let { StatusWord("${SubtitleStep.LIST_FAILED}: $it", Status.Bad) }
        step.problem?.let { StatusWord("${SubtitleStep.FAILED}: $it", Status.Bad) }
    }
}

/** One language of the subtitles step, as a row shows it. */
private class Language(
    val name: String,
    val line: String?,
    val chosen: Boolean,
    val onClick: () -> Unit,
)

/** The languages: three columns on TV, one on the phone. */
@Composable
private fun LanguageRows(
    rows: List<Language>,
    tv: Boolean,
) {
    val columns = if (tv) 3 else 1
    Column(verticalArrangement = Arrangement.spacedBy(if (tv) Marquee.space.s else Marquee.space.xs)) {
        for (line in rows.chunked(columns)) {
            Row(Modifier.height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(Marquee.space.l)) {
                for (row in line) {
                    LanguageRow(
                        row.name,
                        row.line,
                        row.chosen,
                        Modifier.weight(1f).fillMaxHeight(),
                        row.onClick,
                    )
                }
                // A short last line keeps its rows the width of the others.
                repeat(columns - line.size) { Box(Modifier.weight(1f)) }
            }
        }
    }
}

/**
 * One language of the subtitles step: its name, a line when it has one, a check when chosen. One focus stop. Chosen
 * is a fill, not an edge (design/DECISION.md, Selected): `raised` under the chosen row, `surface` under the rest.
 */
@Composable
private fun LanguageRow(
    name: String,
    line: String?,
    chosen: Boolean,
    modifier: Modifier,
    onClick: () -> Unit,
) {
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Marquee.radius.l)
    // On TV the focused row turns white with dark words (focusPalette).
    FocusInk(focusPalette(focused)) {
        val colors = Marquee.colors
        Row(
            modifier
                .marqueeFocus(Marquee.radius.l, Tokens.FOCUS_SCALE_TILE)
                .onFocusChanged { focused = it.isFocused }
                .clip(shape)
                .background(if (chosen) colors.raised else colors.surface)
                .focusSelectable(chosen, onClick)
                .heightIn(min = 56.dp)
                .padding(horizontal = Marquee.space.s, vertical = 10.dp),
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                MqText(name, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 1)
                line?.let { MqText(it, role = TextRole.Caption, color = colors.muted) }
            }
            if (chosen) MqIcon(Icons.Check)
        }
    }
}

/** The defaults step for the page's life (the state and the install live in [DefaultsStep]). */
class DefaultsHolder(
    session: CoreSession,
) : ViewModel() {
    val step = DefaultsStep(session.ops, viewModelScope).also { it.load() }
}

/**
 * The recommended addons (plans/m13-default-addons.md 6): the core's list grouped, a switch per row,
 * on unless the profile has it off, and one Continue that installs them in one write. Continue stays
 * in sight under the list; on TV it has focus first, so one press accepts them all, and down from the
 * last row reaches it.
 */
@Composable
fun DefaultsScreen(
    session: CoreSession,
    mode: DefaultsMode,
    /** Back, in the first run's header (step 2 of 4). */
    onBack: (() -> Unit)? = null,
    onDone: () -> Unit,
) {
    val holder =
        viewModel<DefaultsHolder>(
            key = "defaults",
            factory = viewModelFactory { initializer { DefaultsHolder(session) } },
        )
    val tv = Marquee.form == Form.Tv
    val go = remember { FocusRequester() }
    val step = holder.step
    val answer = step.answer
    if (answer != null) AutoFocus(go)
    val title = if (mode == DefaultsMode.Review) DefaultsCopy.REVIEW_TITLE else DefaultsCopy.TITLE
    val rows = DefaultsCopy.rows(answer?.addons.orEmpty(), mode)
    Column(
        Modifier
            .fillMaxSize()
            .semantics { paneTitle = title }
            .windowInsetsPadding(WindowInsets.systemBars)
            .padding(horizontal = Marquee.safe.side, vertical = Marquee.safe.top),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        // The first run counts this step as its second, with no frame: the list is not a question.
        if (mode == DefaultsMode.FirstRun || mode == DefaultsMode.AfterImport) StepHeader(2, null, {}, onBack)
        val width = Modifier.widthIn(max = 900.dp).fillMaxWidth()
        // More air between groups than between the rows of a group.
        Column(
            width.weight(1f).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.l),
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                MqText(
                    title,
                    role = if (tv) TextRole.Title1 else TextRole.Title2,
                )
                MqText(
                    when (mode) {
                        DefaultsMode.Review -> DefaultsCopy.REVIEW_LEAD
                        DefaultsMode.Settings -> DefaultsCopy.SETTINGS_LEAD
                        else -> DefaultsCopy.LEAD
                    },
                    role = TextRole.Body,
                    color = Marquee.colors.muted,
                )
            }
            step.problem?.let {
                StateMessage(
                    DefaultsCopy.NOT_LOADED,
                    it,
                    icon = Icons.CircleAlert,
                    actionLabel = "Try again",
                    onAction = { step.load() },
                )
            }
            if (answer == null && step.problem == null) StatusWord("Loading", Status.Warn)
            for ((group, addons) in rows.groupBy { it.group }) {
                Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    // The group's label sits over the marks.
                    Kicker(DefaultsCopy.group(group), side = 0.dp)
                    for (addon in addons) {
                        // After an import, the ones it brought keep how they are and say where they came from.
                        val on = DefaultsCopy.on(addon, mode, step.switches)
                        DefaultRow(addon, on.takeIf { DefaultsCopy.hasSwitch(addon, mode) }) {
                            if (!step.busy) step.switches[addon.key] = !on
                        }
                    }
                    if (group == DefaultsCopy.CATALOGS &&
                        DefaultsCopy.catalogMissing(answer?.addons.orEmpty(), mode, step.switches)
                    ) {
                        LiveSpoken(
                            DefaultsCopy.NO_CATALOG,
                        ) { Note(DefaultsCopy.NO_CATALOG, failed = true, side = 0.dp) }
                    }
                }
            }
            step.failed?.let {
                StateMessage(DefaultsCopy.FAILED, "$it. Try again.", icon = Icons.CircleAlert)
            }
            if (tv && (mode == DefaultsMode.FirstRun || mode == DefaultsMode.AfterImport)) {
                Note(DefaultsCopy.TV_MORE, side = 0.dp)
            }
        }
        Row(width) {
            PrimaryButton(
                when {
                    step.busy -> DefaultsCopy.WORKING
                    mode == DefaultsMode.Settings -> DefaultsCopy.SAVE
                    else -> DefaultsCopy.CONTINUE
                },
                { step.save(mode, onDone) },
                icon = Icons.Check,
                enabled = answer != null && !step.busy,
                focusRequester = go,
            )
        }
    }
}

/**
 * One default: its mark, its name and line from the core, and its switch ([on] null: no switch, it
 * came from Stremio). The whole row is the switch, one focus stop on TV; TalkBack reads the name, the
 * line and the state.
 */
@Composable
private fun DefaultRow(
    addon: DefaultAddonInfo,
    on: Boolean?,
    onToggle: () -> Unit,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    val shape = RoundedCornerShape(Marquee.radius.l)
    val card =
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surface)
            .border(1.dp, colors.edge, shape)
    val padding = PaddingValues(if (tv) 16.dp else 12.dp)
    val words: @Composable RowScope.() -> Unit = {
        MarkTile(addon.logo, addon.name)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            MqText(addon.name, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 1)
            // Read here: a TV focus turns the switch row white, and its words dark.
            MqText(addon.line, role = TextRole.Caption, color = Marquee.colors.muted)
        }
    }
    if (on != null) {
        SwitchRow(
            on,
            onToggle,
            Modifier.marqueeFocus(Marquee.radius.l, Tokens.FOCUS_SCALE_TILE).then(card),
            padding,
            words,
        )
    } else {
        Row(
            card.semantics(mergeDescendants = true) {}.padding(padding),
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            words()
            MqText(DefaultsCopy.FROM_STREMIO, role = TextRole.Caption, color = colors.muted)
        }
    }
}

/** The profile's `defaultsSeen`, read once from the settings view (null when it does not answer in time). */
internal suspend fun defaultsSeen(session: CoreSession): Int? = prefsNow(session)?.defaultsSeen

/** The profile's preferences, read once from the settings view (null when it does not answer in time). */
private suspend fun prefsNow(session: CoreSession): Prefs? {
    val view = session.settings()
    return try {
        withTimeoutOrNull(OFFER_WAIT_MS) { snapshotFlow { view.snapshot }.filterNotNull().first() }
    } finally {
        view.close()
    }
}

private const val OFFER_WAIT_MS = 5_000L

/** The one-time Home card: the defaults this profile does not have, Review or Not now. */
@Composable
internal fun RecommendedCard(
    answer: DefaultAddons,
    onReview: () -> Unit,
    onNotNow: () -> Unit,
) {
    val missing = answer.addons.filter { it.installed == DefaultInstalled.Absent }.map { it.name }
    // A remote landing on a button brings the whole card into view, so its words are read first.
    val whole = remember { BringIntoViewRequester() }
    val scope = rememberCoroutineScope()
    Box(
        Modifier
            .bringIntoViewRequester(whole)
            .onFocusChanged { if (it.hasFocus) scope.launch { whole.bringIntoView() } },
    ) {
        // Secondary pills: Home's one white pill is the hero's Play.
        Section {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                MqText(DefaultsCopy.REVIEW_TITLE, role = TextRole.Title3)
                MqText(DefaultsCopy.cardLine(missing), role = TextRole.Callout, color = Marquee.colors.muted)
                Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    SecondaryButton(DefaultsCopy.REVIEW, onReview, icon = Icons.ListChecks)
                    SecondaryButton(DefaultsCopy.NOT_NOW, onNotNow)
                }
            }
        }
    }
}

/**
 * One of the ways to begin, in the bar under the frame: a pill filled white 12 percent, its icon in a circle, a title
 * and one line. On TV the focused one turns white.
 */
@Composable
private fun Door(
    title: String,
    body: String,
    icon: ImageVector,
    onClick: () -> Unit,
    modifier: Modifier,
    focusRequester: FocusRequester?,
) {
    val tv = Marquee.form == Form.Tv
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Marquee.radius.pill)
    // On TV the focused row turns white with dark words (focusPalette).
    FocusInk(focusPalette(focused)) {
        val colors = Marquee.colors
        val lit = focused && tv
        Row(
            modifier
                .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
                .marqueeFocus(Marquee.radius.pill)
                .onFocusChanged { focused = it.isFocused }
                .clip(shape)
                .background(if (lit) colors.surface else DOOR_FILL)
                .focusClickable(onClick, Role.Button)
                .heightIn(min = if (tv) 120.dp else 72.dp)
                .padding(
                    start = if (tv) 20.dp else 14.dp,
                    end = if (tv) 36.dp else 18.dp,
                    top = if (tv) 18.dp else 12.dp,
                    bottom = if (tv) 18.dp else 12.dp,
                ),
            horizontalArrangement = Arrangement.spacedBy(if (tv) Marquee.space.m else 12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier
                    .size(if (tv) 76.dp else 44.dp)
                    .clip(CircleShape)
                    .background(if (lit) Color.Black.copy(alpha = 0.08f) else DOOR_FILL),
                contentAlignment = Alignment.Center,
            ) { MqIcon(icon, size = if (tv) 36.dp else 20.dp) }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(if (tv) 4.dp else 2.dp)) {
                MqText(title, role = TextRole.Headline, weight = FontWeight.SemiBold, maxLines = 2)
                MqText(body, role = if (tv) TextRole.Callout else TextRole.Caption, color = colors.dim)
            }
        }
    }
}

/** A quiet text action in a screen's corner; [back] puts its chevron first, pointing back. */
@Composable
private fun SkipLink(
    label: String,
    onClick: () -> Unit,
    back: Boolean = false,
) {
    var focused by remember { mutableStateOf(false) }
    // On TV the focused link turns white with dark words (focusPalette).
    val colors = focusPalette(focused)
    Row(
        Modifier
            .heightIn(min = 48.dp)
            .marqueeFocus(Marquee.radius.m)
            .onFocusChanged { focused = it.isFocused }
            .clip(RoundedCornerShape(Marquee.radius.m))
            .background(if (focused && Marquee.form == Form.Tv) colors.surface else Color.Transparent)
            .focusClickable(onClick, Role.Button)
            .padding(horizontal = Marquee.space.s),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (back) MqIcon(Icons.ChevronLeft, tint = colors.muted)
        MqText(label, role = TextRole.Callout, weight = FontWeight.SemiBold, color = colors.muted)
        if (!back) MqIcon(Icons.ChevronRight, tint = colors.muted)
    }
}

/** The first run's steps that count: the doors, the recommended addons, the stream question and subtitles. */
internal const val FIRST_RUN_STEPS = 4

/** The film frame's shape, and its least share of the screen's height (a bar too tall for the rest scrolls). */
internal const val FILM_RATIO = 2.39f
internal const val FILM_FLOOR = 0.4f

/** The player's subtitle scrim, behind each line of the frame's words: 5.7:1 for white over a white still. */
internal val FILM_SCRIM = Color.Black.copy(alpha = 0.6f)

/** A door's pill and its icon's circle. */
private val DOOR_FILL = Color.White.copy(alpha = 0.12f)

internal const val FILM_FRAME = "film-frame"
internal const val FILM_BAR = "film-bar"
internal const val STEP_HEADER = "step-header"
