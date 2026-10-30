package app.marquee.android

import android.graphics.Bitmap
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBars
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
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.isTraversalGroup
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.LayoutDirection
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
import coil3.compose.AsyncImage
import coil3.compose.LocalPlatformContext
import coil3.request.ImageRequest
import coil3.request.transformations
import coil3.transform.Transformation
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import kotlin.math.abs

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
    /** The still behind the step (V1); null keeps it black. */
    still: FirstRunStill? = null,
) {
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
        Doors {
            onImport?.let {
                Door(
                    "I use Stremio",
                    "Bring your addons, library and Continue Watching over with a code.",
                    Icons.DoorOpen,
                    it,
                    first,
                )
            }
            Door(
                "Start fresh",
                "Choose from the recommended addons, then start watching.",
                Icons.Sparkles,
                onDefaults,
                first.takeIf { onImport == null },
            )
            onPair?.let {
                Door(
                    "I already use Momotus",
                    "Join your other device's sync account with a code. Library and addons follow.",
                    Icons.QrCode,
                    it,
                    null,
                )
            }
        }
    }
}

/** The doors, stacked in the step's block on every form. */
@Composable
private fun Doors(doors: @Composable ColumnScope.() -> Unit) {
    Column(
        Modifier.fillMaxWidth().semantics { isTraversalGroup = true },
        verticalArrangement = Arrangement.spacedBy(if (Marquee.form == Form.Tv) 12.dp else 10.dp),
        content = doors,
    )
}

/**
 * A first-run question step (V1 Full Bleed, the owner's pick of 2026-10-30, design/exploration/onboarding-v2): one
 * still fills the screen behind the step's header and its block, the question (a heading), its lead line, the
 * choices ([bar]) and [foot]. The phone keeps the block at the foot on a ramp rising into black, the still showing
 * above; TV and wide windows keep it in a column at the lower left over Apple's wash. A block taller than the screen
 * scrolls, the still staying behind. With no still the screen stays black.
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
    BoxWithConstraints(
        Modifier
            .fillMaxSize()
            .background(Color.Black)
            // A step change is announced by its question (TalkBack reads a new pane by its title).
            .semantics { paneTitle = question }
            .testTag(FILM_FRAME),
    ) {
        val wide = tv || (maxWidth >= 840.dp && maxHeight >= 480.dp)
        val side =
            when {
                tv -> Marquee.safe.side
                wide -> 56.dp
                else -> 16.dp
            }
        val column = if (tv) 840.dp else 580.dp
        Still(still, wide, side + minOf(column, maxWidth - side * 2))
        val bottom = if (tv) Marquee.safe.bottom else 34.dp
        val below = bottom + WindowInsets.systemBars.asPaddingValues().calculateBottomPadding()
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .heightIn(min = maxHeight)
                .windowInsetsPadding(WindowInsets.systemBars)
                .padding(start = side, end = side, top = Marquee.safe.top, bottom = bottom),
            verticalArrangement = Arrangement.SpaceBetween,
        ) {
            // Over the art the header's words are white: the wash under the header is there for them.
            FocusInk(Marquee.colors.copy(muted = Color.White, dim = Color.White)) {
                StepHeader(step, skip, onSkip, onBack, Modifier.testTag(STEP_HEADER))
            }
            Column(
                Modifier
                    .padding(top = Marquee.space.m)
                    .then(if (wide) Modifier.widthIn(max = column) else Modifier)
                    .fillMaxWidth()
                    .then(if (wide) Modifier else Modifier.ramp(side, below))
                    .testTag(FILM_BAR),
            ) {
                MqText(
                    question,
                    Modifier.semantics { heading() },
                    role = TextRole.Hero,
                    color = Color.White,
                    weight = FontWeight.Bold,
                )
                MqText(
                    lead,
                    Modifier.padding(top = if (wide) 14.dp else 10.dp),
                    role = if (tv) TextRole.Body else TextRole.Callout,
                    color = Marquee.colors.muted,
                )
                Column(
                    Modifier.padding(top = if (wide) 34.dp else 22.dp),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
                    content = bar,
                )
                foot?.let { Box(Modifier.padding(top = 14.dp)) { it() } }
            }
        }
    }
}

/**
 * The phone's ramp under the step's block, over the still: clear 170 dp over the question, black 82 percent at its top
 * line, black from 135 dp under it down [below] past the block (the page's foot), [side] past either edge.
 */
private fun Modifier.ramp(
    side: Dp,
    below: Dp,
) = drawBehind {
    val top = -170.dp.toPx()
    drawRect(
        Brush.verticalGradient(
            0f to Color.Transparent,
            170f / 305f to Color.Black.copy(alpha = 0.82f),
            1f to Color.Black,
            startY = top,
            endY = top + 305.dp.toPx(),
        ),
        topLeft = Offset(-side.toPx(), top),
        size = Size(size.width + 2 * side.toPx(), size.height - top + below.toPx()),
    )
}

/**
 * The still behind the whole step, decorative, faded in by opacity, under its wash: on TV and wide windows Apple's
 * (from the leading edge, dark to 60 dp past the column's end at [columnEnd], from the foot and under the header), on
 * the phone one under the header. Every word over it keeps 4.5:1 over a white still. Black with none.
 */
@Composable
private fun BoxScope.Still(
    still: FirstRunStill?,
    wide: Boolean,
    columnEnd: Dp,
) {
    still?.let { s ->
        var landed by remember(s.background) { mutableStateOf(false) }
        val alpha by animateFloatAsState(if (landed) 1f else 0f, tween(Marquee.panelMs), label = "still")
        AsyncImage(
            model =
                ImageRequest
                    .Builder(LocalPlatformContext.current)
                    .data(s.background)
                    .transformations(TrimFlatEdges)
                    .build(),
            contentDescription = null,
            contentScale = ContentScale.Crop,
            alignment = if (wide) BiasAlignment(0.44f, -0.2f) else BiasAlignment(0.28f, 0f),
            onSuccess = { landed = true },
            modifier = Modifier.matchParentSize().graphicsLayer { this.alpha = alpha },
        )
    }
    Box(
        Modifier.matchParentSize().drawBehind {
            val black = Color.Black
            if (!wide) {
                drawRect(
                    Brush.verticalGradient(
                        0f to black.copy(alpha = 0.72f),
                        120f / 220f to black.copy(alpha = 0.66f),
                        1f to Color.Transparent,
                        endY = 220.dp.toPx(),
                    ),
                )
                return@drawBehind
            }
            val ltr = layoutDirection == LayoutDirection.Ltr
            // A narrow wide window keeps its whole column dark: the 86 percent stop never falls inside it.
            val dark = maxOf(0.5f, (columnEnd + 60.dp).toPx() / size.width).coerceAtMost(1f)
            drawRect(
                Brush.horizontalGradient(
                    0f to black.copy(alpha = 0.92f),
                    dark to black.copy(alpha = 0.86f),
                    (dark + 0.28f).coerceAtMost(1f) to Color.Transparent,
                    startX = if (ltr) 0f else size.width,
                    endX = if (ltr) size.width else 0f,
                ),
            )
            drawRect(Brush.verticalGradient(0.6f to Color.Transparent, 1f to black.copy(alpha = 0.7f)))
            drawRect(
                Brush.verticalGradient(
                    0f to black.copy(alpha = 0.74f),
                    0.2f to black.copy(alpha = 0.66f),
                    0.36f to Color.Transparent,
                ),
            )
        },
    )
}

/** Crops the still's flat margin before the frame's crop ([trimFlatEdges]). */
private object TrimFlatEdges : Transformation() {
    override val cacheKey = "trimFlatEdges"

    override suspend fun transform(
        input: Bitmap,
        size: coil3.size.Size,
    ) = trimFlatEdges(input)
}

/**
 * [b] without its flat margin: the rows and columns at its edges in one colour, its corners' colour, such as a promo
 * collage's cream ground, which in the black film frame reads as a light strip. A framed picture has the same colour at
 * opposite corners; a side whose run reaches a third of the picture is the picture itself, not a margin, and stays.
 */
internal fun trimFlatEdges(b: Bitmap): Bitmap {
    val w = b.width
    val h = b.height
    val ground = b.getPixel(0, 0)

    fun near(p: Int) =
        abs(android.graphics.Color.red(p) - android.graphics.Color.red(ground)) <= FLAT_SPREAD &&
            abs(android.graphics.Color.green(p) - android.graphics.Color.green(ground)) <= FLAT_SPREAD &&
            abs(android.graphics.Color.blue(p) - android.graphics.Color.blue(ground)) <= FLAT_SPREAD
    if (!near(b.getPixel(w - 1, h - 1))) return b
    val line = IntArray(maxOf(w, h))

    fun row(y: Int) = b.getPixels(line, 0, w, 0, y, w, 1).let { (0 until w).all { near(line[it]) } }

    fun column(x: Int) = b.getPixels(line, 0, 1, x, 0, 1, h).let { (0 until h).all { near(line[it]) } }

    // The run from one edge, or 0 when it reaches a third of the side.
    fun run(
        count: Int,
        flat: (Int) -> Boolean,
    ) = (0 until count / 3).firstOrNull { !flat(it) } ?: 0

    val top = run(h, ::row)
    val bottom = run(h) { row(h - 1 - it) }
    val left = run(w, ::column)
    val right = run(w) { column(w - 1 - it) }
    if (top + bottom + left + right == 0) return b
    return Bitmap.createBitmap(b, left, top, w - left - right, h - top - bottom)
}

/** How far a margin pixel may stray from its ground per channel: JPEG noise, not a picture's edge. */
private const val FLAT_SPREAD = 12

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

/** The question's page: the still, the two answers in its block, and why a save failed ([problem]). */
@Composable
internal fun StreamQuestionPage(
    still: FirstRunStill?,
    problem: String?,
    onBack: (() -> Unit)?,
    onSkip: () -> Unit,
    onAnswer: (StreamChoiceCopy.Answer) -> Unit,
) {
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
        Doors {
            Door(StreamChoiceCopy.FAST.label, StreamChoiceCopy.FAST.text, Icons.Zap, {
                onAnswer(StreamChoiceCopy.FAST)
            }, fast)
            Door(StreamChoiceCopy.BEST.label, StreamChoiceCopy.BEST.text, Icons.Gem, {
                onAnswer(StreamChoiceCopy.BEST)
            }, null)
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
 * writes nothing. A profile with subtitle languages already passes by. Continue sits under the list; on
 * TV it has focus first, so one press keeps the device's language.
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

/** The subtitles step's page over [step]: the still, the languages in its block, Continue and More languages. */
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
            val actions: @Composable (Modifier) -> Unit = { fill ->
                PrimaryButton(
                    DefaultsCopy.CONTINUE,
                    { step.save(onDone) },
                    fill,
                    icon = Icons.Check,
                    // Not before the language list: the device's language is unknown until then.
                    enabled = !step.busy && step.loaded,
                    focusRequester = go,
                )
                SecondaryButton(if (more) SubtitleStep.FEWER else SubtitleStep.MORE, { more = !more }, fill)
            }
            // The phone stacks them the block's width; TV keeps them side by side.
            if (tv) {
                Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) { actions(Modifier) }
            } else {
                Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    actions(Modifier.fillMaxWidth())
                }
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
        LanguageRows(rows)
        if (more) {
            Kicker(SubtitleStep.MORE, side = 0.dp)
            LanguageRows(step.rest.map { Language(it.name, null, step.chosen == it.code) { step.choose(it.code) } })
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

/** The languages, two columns on every form. */
@Composable
private fun LanguageRows(rows: List<Language>) {
    val columns = 2
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        for (line in rows.chunked(columns)) {
            Row(Modifier.height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
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
 * is a fill, not an edge (design/DECISION.md, Selected): white 24 percent under the chosen row, 12 under the rest.
 */
@Composable
private fun LanguageRow(
    name: String,
    line: String?,
    chosen: Boolean,
    modifier: Modifier,
    onClick: () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    var focused by remember { mutableStateOf(false) }
    val radius = if (tv) 22.dp else 16.dp
    val shape = RoundedCornerShape(radius)
    // On TV the focused row turns white with dark words (focusPalette).
    FocusInk(focusPalette(focused)) {
        val colors = Marquee.colors
        Row(
            modifier
                .marqueeFocus(radius, Tokens.FOCUS_SCALE_TILE)
                .onFocusChanged { focused = it.isFocused }
                .clip(shape)
                .background(
                    when {
                        focused && tv -> colors.surface
                        chosen -> CHOSEN_FILL
                        else -> DOOR_FILL
                    },
                ).focusSelectable(chosen, onClick)
                .heightIn(min = if (tv) 72.dp else 52.dp)
                .padding(horizontal = if (tv) 22.dp else 14.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                MqText(name, role = TextRole.Headline, weight = FontWeight.SemiBold, maxLines = 1)
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
            content = words,
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
 * One of the ways to begin, stacked in the step's block: a pill filled white 12 percent, its icon in a circle, a title,
 * one line and a chevron. On TV the focused one turns white.
 */
@Composable
private fun Door(
    title: String,
    body: String,
    icon: ImageVector,
    onClick: () -> Unit,
    focusRequester: FocusRequester?,
) {
    val tv = Marquee.form == Form.Tv
    var focused by remember { mutableStateOf(false) }
    val radius = if (tv) Marquee.radius.pill else 26.dp
    val shape = RoundedCornerShape(radius)
    // On TV the focused row turns white with dark words (focusPalette).
    FocusInk(focusPalette(focused)) {
        val colors = Marquee.colors
        val lit = focused && tv
        Row(
            Modifier
                .fillMaxWidth()
                .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
                .marqueeFocus(radius)
                .onFocusChanged { focused = it.isFocused }
                .clip(shape)
                .background(if (lit) colors.surface else DOOR_FILL)
                .focusClickable(onClick, Role.Button)
                .heightIn(min = if (tv) 120.dp else 72.dp)
                .padding(
                    start = if (tv) 20.dp else 12.dp,
                    end = if (tv) 32.dp else 14.dp,
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
                MqText(body, role = if (tv) TextRole.Callout else TextRole.Caption, color = colors.muted)
            }
            MqIcon(Icons.ChevronRight, tint = (if (lit) Color.Black else Color.White).copy(alpha = 0.4f))
        }
    }
}

/** A quiet text action in a screen's corner; [back] puts its chevron first, pointing back. */
@Composable
internal fun SkipLink(
    label: String,
    onClick: () -> Unit,
    back: Boolean = false,
    focusRequester: FocusRequester? = null,
) {
    var focused by remember { mutableStateOf(false) }
    // On TV the focused link turns white with dark words (focusPalette).
    val colors = focusPalette(focused)
    Row(
        Modifier
            .heightIn(min = 48.dp)
            .then(focusRequester?.let { Modifier.focusRequester(it) } ?: Modifier)
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

/** A door's pill and its icon's circle, and a language's row; the chosen language's row. */
private val DOOR_FILL = Color.White.copy(alpha = 0.12f)
private val CHOSEN_FILL = Color.White.copy(alpha = 0.24f)

/** The step's whole screen, the still behind it; and its block, from the question's top line to the last choice. */
internal const val FILM_FRAME = "film-frame"
internal const val FILM_BAR = "film-bar"
internal const val STEP_HEADER = "step-header"
