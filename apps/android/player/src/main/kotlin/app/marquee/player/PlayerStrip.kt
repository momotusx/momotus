package app.marquee.player

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.collapse
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.expand
import androidx.compose.ui.semantics.isTraversalGroup
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.marquee.core.Format
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.model.NextEpisode
import app.marquee.core.model.SubtitleBackground
import app.marquee.core.model.SubtitleSize
import app.marquee.design.ButtonFocused
import app.marquee.design.FocusInk
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.LiveSpoken
import app.marquee.design.Marquee
import app.marquee.design.MarqueeFonts
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.Tokens
import app.marquee.design.focusClickable
import app.marquee.design.focusPalette
import app.marquee.design.marqueeFocus

// The player bar's strips and its quick subtitle shift (plans/player-bar.md, the picked "Toolbar strip").

private val Bar = StreamChoiceCopy.Bar
private val Names = StreamChoiceCopy.Bar.Names

/** Which strip is open over the bar: one at a time (plans/player-bar.md section 5). */
enum class Strip { Subtitles, Audio }

/** Under this the phone's scrubber is too short to aim at, and the bar takes two rows (D17). */
val MIN_SCRUB = 120.dp

/** How the phone's bar fits its width (D17): one row or two, the shift's Earlier and Later in words or chevrons. */
data class PhoneBarFit(
    val rows: Int,
    val shiftWords: Boolean,
)

/**
 * The phone's bar for [room] (its width inside the margins), decided from the width and film or series alone, with the
 * shift's room always counted, so turning a subtitle on or off never moves a row (D17). [groups] is both groups of
 * pills without the shift, with their gaps; [track] the clocks and their gaps beside the scrubber. One row with the
 * shift in words while the scrubber keeps [MIN_SCRUB]; else with chevrons; else two rows (the scrubber row, then the
 * control row), in words when they fit there. Targets never shrink.
 */
fun phoneBarFit(
    room: Dp,
    groups: Dp,
    track: Dp,
    shiftWords: Dp,
    shiftChevrons: Dp,
): PhoneBarFit =
    when {
        room - groups - track - shiftWords >= MIN_SCRUB -> PhoneBarFit(1, true)
        room - groups - track - shiftChevrons >= MIN_SCRUB -> PhoneBarFit(1, false)
        else -> PhoneBarFit(2, room - groups >= shiftWords)
    }

/** How one side of the TV bar fits its column (D16): its pills with their words, and the shift in words. */
data class SideFit(
    val words: Boolean,
    val shiftWords: Boolean,
)

/**
 * One side of the TV bar in a [column] (the row is the pick's `1fr auto 1fr`, so each side has half of what the
 * transport leaves): its pills with their words and values when they fit ([words], counting the shift in words),
 * else icons alone ([icons]); and if that still overflows, the shift's words become chevrons (D16 a and b). The
 * shift's room is always counted, so the side never flips when a subtitle turns on.
 * ponytail: D16's third step (two rows) is not built on Android TV: on its 1920-point canvas a series' right side as
 * icons with chevrons takes about 605 of the about 700 points it has (PlayerBarRulesTest pins it). Add it if the canvas
 * ever narrows.
 */
fun sideFit(
    column: Dp,
    words: Dp,
    icons: Dp,
    shiftWords: Dp = 0.dp,
): SideFit =
    when {
        words + shiftWords <= column -> SideFit(words = true, shiftWords = true)
        icons + shiftWords <= column -> SideFit(words = false, shiftWords = true)
        else -> SideFit(words = false, shiftWords = false)
    }

/**
 * The language chips of the subtitle strip after Off (D5): the language on now, then the others in the picker's order
 * (the profile's languages first, then the core's), at most [cap]. They come from the picker's own list, so offline
 * they show what it shows. "More languages" opens the picker for the rest.
 */
fun stripLanguages(
    rows: List<TrackChoice>,
    cap: Int,
): List<SubtitleLanguage> {
    val all = subtitleLanguages(rows)
    return (all.filter { it.chosen } + all.filterNot { it.chosen }).take(cap)
}

/**
 * Whether Off is where a remote enters the subtitle strip ([languages] its chips): it is checked, or the track on has
 * no chip (captions in the video that name no language), so the strip always has a way in.
 */
fun offIsEntry(
    rows: List<TrackChoice>,
    languages: List<SubtitleLanguage>,
): Boolean = rows.none { it.selected && it.id != TRACK_OFF } || languages.none { it.chosen }

/** Next's spoken name: "Next episode, S1:E3 In Perpetuity" (section 8). */
fun nextName(next: NextEpisode): String =
    Names.NEXT_EPISODE.replace("{episode}", "${Format.episodeLabel(next.season, next.episode)} ${next.title}".trim())

/** How many language chips a strip shows: 3 on phones, 5 on TV (D5). */
@Composable
private fun chipCap(): Int = if (Marquee.form == Form.Tv) 5 else 3

/**
 * A foot scrim tall enough that [cover] (from the screen's foot to the top of the bar, or of an open strip and its
 * Style row) lies where it is at least 80% of its peak, so white words on the strip's chips keep 4.5:1 over a white
 * picture (plans/player-bar.md section 5, scrim; acceptance 8). Never under [least].
 */
fun footScrim(
    cover: Dp,
    least: Dp,
): Dp = maxOf(least, cover / (1f - FOOT_RAMP))

/** The share of a foot scrim, from its top, over which it rises to 80% of its peak ([scrimStops]). */
const val FOOT_RAMP = 0.25f

/** The shift's value box: wide enough for "−10.25 s" and, on TV, the chevrons adjust mode lights. */
@Composable
private fun valueWidth(): Dp = if (Marquee.form == Form.Tv) 200.dp else 76.dp

/** Beside Earlier and Later in words. */
@Composable
private fun shiftPad(): Dp = if (Marquee.form == Form.Tv) 15.dp else 10.dp

/** The widths a bar needs, measured in its own type, for the fit rules ([phoneBarFit], [sideFit]). */
class BarWidths(
    private val measure: (String) -> Dp,
    /** A pill's height: an icon-only pill is as wide. */
    val pill: Dp,
    private val icon: Dp,
    private val pad: Dp,
    private val gap: Dp,
    private val value: Dp,
    private val shiftPad: Dp,
    /** The widest a pill's value draws ([ActionButton]: twice [PILL_VALUE_MAX] on TV). */
    private val valueMax: Dp,
    /** The busy mark after a value ([ActionButton]'s spinner). */
    private val spinner: Dp,
) {
    /** The width of [text] in the bar's words. */
    fun text(text: String): Dp = measure(text)

    /** A pill with its words: icon, name and value, the value as wide as the widest of [details]; [busy] with its mark. */
    fun pill(
        label: String,
        details: List<String?>,
        busy: Boolean = false,
    ): Dp =
        pad * 2 + icon + gap + measure(label) +
            (details.filterNotNull().maxOfOrNull { gap + minOf(measure(it), valueMax) } ?: 0.dp) +
            (if (busy) gap + spinner else 0.dp)

    fun pill(
        label: String,
        detail: String?,
    ): Dp = pill(label, listOf(detail))

    /** The shift with Earlier and Later in words. */
    val shiftWords: Dp
        get() =
            maxOf(pill, measure(Bar.EARLIER) + shiftPad * 2) + value + maxOf(pill, measure(Bar.LATER) + shiftPad * 2)

    /** The shift with chevrons. */
    val shiftChevrons: Dp get() = pill * 2 + value
}

/** [BarWidths] for this form, measured in the bar's type. */
@Composable
fun rememberBarWidths(): BarWidths {
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val tv = Marquee.form == Form.Tv
    val size = if (tv) Marquee.type.body else Marquee.type.callout
    val style = TextStyle(fontFamily = MarqueeFonts.text, fontSize = size, fontWeight = FontWeight.SemiBold)
    val pill = pillHeight()
    val icon = pillIcon()
    val value = valueWidth()
    val pad = shiftPad()
    return remember(measurer, density, style, pill) {
        BarWidths(
            measure = { text ->
                with(density) {
                    measurer
                        .measure(text, style, softWrap = false, maxLines = 1)
                        .size.width
                        .toDp()
                }
            },
            pill = pill,
            icon = icon,
            pad = if (tv) 24.dp else 12.dp,
            gap = if (tv) 14.dp else 6.dp,
            value = value,
            shiftPad = pad,
            valueMax = if (tv) PILL_VALUE_MAX * 2 else PILL_VALUE_MAX,
            spinner = if (tv) 20.dp else 10.dp,
        )
    }
}

/**
 * Every value Subtitles can show in this play: what it shows now, Off's ("Off, 3 languages") and each language's name,
 * so a side's fit is decided once and turning a subtitle on or off never flips it (D16).
 */
fun PlaybackController.subtitleValues(): List<String> =
    listOf(subtitlesDetail(), subtitlesDetail(text.map { it.copy(selected = it.id == TRACK_OFF) }, subtitlesPending)) +
        subtitleLanguages(text).map { it.name }

/**
 * The quick shift beside Subtitles and in its strip (D2, D3): Earlier, the value, Later in one capsule. A tap moves
 * 0.25 s; held, it repeats ([QUICK_START_MS], then every [QUICK_EVERY_MS], growing); the value ("+0.25 s") resets to 0
 * when pressed. One polite line says the value once it settles (D12), never once a tick.
 *
 * On TV ([valueFocus] given) Earlier and Later are no focus stops: the value is the one stop, where Left and Right move
 * the focus on as from any control. OK enters adjust mode ([adjusting], hoisted so Back leaves it first): the
 * chevrons light, Left is earlier and Right later, held or pressed, the remote's own repeats ignored; OK, Up or Down
 * leave it and keep the focus, as does the focus going (section 6).
 */
@Composable
fun QuickShift(
    delayMs: Long,
    onDelay: (Long) -> Unit,
    modifier: Modifier = Modifier,
    words: Boolean = true,
    valueFocus: FocusRequester? = null,
    adjusting: Boolean = false,
    onAdjust: (Boolean) -> Unit = {},
    /** On the value: where Up goes from it on TV (the scrubber, or the open strip). */
    valueModifier: Modifier = Modifier,
) {
    val tv = Marquee.form == Form.Tv
    val delay = rememberUpdatedState(delayMs)
    // The settled value, said once it rests (D12); nothing is said before the viewer shifts.
    var moved by remember { mutableStateOf<Long?>(null) }
    var said by remember { mutableStateOf("") }
    LaunchedEffect(moved) {
        val at = moved ?: return@LaunchedEffect
        kotlinx.coroutines.delay(SHIFT_SETTLE_MS)
        said = shiftSaid(at)
    }
    val set =
        rememberUpdatedState { ms: Long ->
            onDelay(ms)
            moved = ms
        }
    val scope = rememberCoroutineScope()
    val hold = remember { DelayHold(scope, delay, set, QUICK_START_MS, QUICK_EVERY_MS, ::quickDelay) }
    val reset = { if (delay.value != 0L) set.value(0L) }
    val adjust = rememberUpdatedState(adjusting)
    Row(
        modifier
            .height(pillHeight())
            .clip(RoundedCornerShape(Marquee.radius.pill))
            .background(SECONDARY)
            .semantics { isTraversalGroup = true }
            .testTag(SHIFT_TAG),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // The settled line, said by a polite live region of no size inside the capsule.
        LiveSpoken(said, quietMs = 0L) {}
        ShiftButton(-1, words, hold, tv)
        var focused by remember { mutableStateOf(false) }
        var eatRelease by remember { mutableStateOf(false) }
        ButtonFocused(focused)
        val ink = focusPalette(focused)
        val actions =
            buildList {
                add(CustomAccessibilityAction(Names.EARLIER) { set.value(quickDelay(delay.value, -1)).let { true } })
                add(CustomAccessibilityAction(Names.LATER) { set.value(quickDelay(delay.value, 1)).let { true } })
                if (tv && delayMs != 0L) add(CustomAccessibilityAction(Bar.RESET) { reset().let { true } })
            }
        Row(
            Modifier
                .then(if (valueFocus != null) Modifier.focusRequester(valueFocus) else Modifier)
                .then(valueModifier)
                .onFocusChanged {
                    focused = it.isFocused
                    if (!it.isFocused && adjust.value) {
                        hold.release(step = false)
                        onAdjust(false)
                    }
                }.onPreviewKeyEvent { event ->
                    // The release of the OK that ended adjust mode is eaten, so it never presses the value again.
                    if (eatRelease && event.type == KeyEventType.KeyUp && event.key in OK_KEYS) {
                        eatRelease = false
                        return@onPreviewKeyEvent true
                    }
                    if (!tv || !adjust.value) return@onPreviewKeyEvent false
                    when (event.key) {
                        Key.DirectionLeft, Key.DirectionRight -> {
                            // The remote's own repeats of a held key are swallowed: the hold keeps its own pace.
                            if (event.nativeKeyEvent.repeatCount == 0 || event.type == KeyEventType.KeyUp) {
                                delayKey(event, hold)
                            }
                            true
                        }

                        Key.DirectionCenter, Key.Enter, Key.NumPadEnter, Key.DirectionUp, Key.DirectionDown -> {
                            // Adjust mode ends on the press; an OK's release is eaten above.
                            if (event.type == KeyEventType.KeyDown) {
                                hold.release(step = false)
                                onAdjust(false)
                                eatRelease = event.key in OK_KEYS
                            }
                            true
                        }

                        else -> {
                            false
                        }
                    }
                }.marqueeFocus(Marquee.radius.pill, scale = 1.04f)
                .fillMaxHeight()
                .width(valueWidth())
                .clip(RoundedCornerShape(Marquee.radius.pill))
                .background(if (tv && focused) ink.raised else Color.Transparent)
                .semantics {
                    contentDescription = shiftName(delayMs, tv)
                    stateDescription = delayText(delayMs)
                    customActions = actions
                }.focusClickable({ if (tv) onAdjust(true) else reset() }),
            horizontalArrangement = Arrangement.spacedBy(4.dp, Alignment.CenterHorizontally),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            FocusInk(ink) {
                // Only the capsule that holds the focus lights (the bar's and the strip's share one adjust mode).
                val lit = tv && adjusting && focused
                if (lit) MqIcon(Icons.ChevronLeft, tint = ink.text, size = 24.dp)
                MqText(delayText(delayMs), role = pillText(), weight = FontWeight.SemiBold, maxLines = 1)
                if (lit) MqIcon(Icons.ChevronRight, tint = ink.text, size = 24.dp)
            }
        }
        ShiftButton(1, words, hold, tv)
    }
}

/** How long a shift rests before its line is said (D12): the same 700 ms its delay waits before it goes to the core. */
const val SHIFT_SETTLE_MS = 700L

/** The quick shift's capsule, for tests. */
const val SHIFT_TAG = "quick-shift"

/** An open strip, with its Style row, for tests. */
const val STRIP_TAG = "player-strip"

/** The keys a remote's OK sends. */
private val OK_KEYS = setOf(Key.DirectionCenter, Key.Enter, Key.NumPadEnter)

/** Earlier or Later, a word or a chevron: a tap steps, a hold repeats ([DelayHold]), a screen reader presses once. */
@Composable
private fun ShiftButton(
    direction: Int,
    words: Boolean,
    hold: DelayHold,
    tv: Boolean,
) {
    val word = if (direction < 0) Bar.EARLIER else Bar.LATER
    Box(
        Modifier
            .fillMaxHeight()
            .widthIn(min = pillHeight())
            .clip(RoundedCornerShape(Marquee.radius.pill))
            .semantics {
                contentDescription = if (direction < 0) Names.EARLIER else Names.LATER
                role = Role.Button
                onClick {
                    hold.press(direction)
                    hold.release()
                    true
                }
            }.pointerInput(Unit) {
                detectTapGestures(
                    onPress = {
                        hold.press(direction)
                        hold.release(step = tryAwaitRelease())
                    },
                )
            }
            // On a remote the value is the one stop (section 6).
            .then(if (tv) Modifier.focusProperties { canFocus = false } else Modifier)
            .padding(horizontal = shiftPad()),
        contentAlignment = Alignment.Center,
    ) {
        if (words) {
            MqText(word, role = pillText(), weight = FontWeight.SemiBold, maxLines = 1)
        } else {
            MqIcon(if (direction < 0) Icons.ChevronLeft else Icons.ChevronRight, tint = Color.White, size = pillIcon())
        }
    }
}

/**
 * A chip of a strip: a pill with its words, white at 16 percent; the checked one ([checked] true, a radio) in the
 * selected grey with a white edge; a [ghost] one without a fill. On TV the focused chip turns white with dark words,
 * as every focused control. Chips never cut their words: the strip's row scrolls.
 */
@Composable
fun StripChip(
    text: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    checked: Boolean? = null,
    ghost: Boolean = false,
    expanded: Boolean? = null,
    enabled: Boolean = true,
    spoken: String? = null,
    leading: (@Composable () -> Unit)? = null,
    icon: ImageVector? = null,
    trailing: ImageVector? = null,
    /** Shows the focused look without focus (screenshots and tests), as [ActionButton]'s. */
    forceFocus: Boolean = false,
) {
    val tv = Marquee.form == Form.Tv
    var focusHeld by remember { mutableStateOf(false) }
    val focused = focusHeld || forceFocus
    ButtonFocused(focused)
    val colors = focusPalette(focused)
    val shape = RoundedCornerShape(Marquee.radius.pill)
    val fill =
        when {
            tv && focused -> colors.raised
            checked == true || expanded == true -> Tokens.selectedFill
            ghost -> Color.Transparent
            else -> SECONDARY
        }
    Row(
        modifier
            .marqueeFocus(Marquee.radius.pill, scale = 1.04f, forced = forceFocus)
            .onFocusChanged { focusHeld = it.isFocused }
            .height(pillHeight())
            .clip(shape)
            .background(fill)
            .then(if (checked == true) Modifier.border(2.dp, Color.White.copy(alpha = 0.7f), shape) else Modifier)
            .semantics {
                spoken?.let { contentDescription = it }
                if (checked != null) selected = checked
                when (expanded) {
                    true -> collapse { onClick().let { true } }
                    false -> expand { onClick().let { true } }
                    null -> Unit
                }
            }.focusClickable(onClick, if (checked != null) Role.RadioButton else Role.Button, enabled)
            .alpha(if (enabled) 1f else 0.45f)
            .padding(horizontal = if (tv) 27.dp else 14.dp),
        horizontalArrangement = Arrangement.spacedBy(if (tv) 15.dp else 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        FocusInk(colors) {
            leading?.invoke()
            icon?.let { MqIcon(it, tint = colors.text, size = pillIcon() * 0.9f) }
            MqText(text, role = pillText(), weight = FontWeight.SemiBold, maxLines = 1)
            trailing?.let { MqIcon(it, tint = colors.text, size = pillIcon() * 0.8f) }
        }
    }
}

/** A round minus or plus of the Style row: the touch minimum, white at 16 percent. */
@Composable
private fun StepButton(
    icon: ImageVector,
    spoken: String,
    enabled: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val tv = Marquee.form == Form.Tv
    var focused by remember { mutableStateOf(false) }
    ButtonFocused(focused)
    val colors = focusPalette(focused)
    Box(
        modifier
            .marqueeFocus(pillHeight() / 2, scale = 1.04f)
            .onFocusChanged { focused = it.isFocused }
            .size(pillHeight())
            .clip(RoundedCornerShape(Marquee.radius.pill))
            .background(if (tv && focused) colors.raised else SECONDARY)
            .semantics { contentDescription = spoken }
            .focusClickable(onClick, Role.Button, enabled)
            .alpha(if (enabled) 1f else 0.45f),
        contentAlignment = Alignment.Center,
    ) { MqIcon(icon, tint = colors.text, size = pillIcon() * 0.9f) }
}

/** A strip's name at its start ("Subtitles", "Audio"), quiet, with its icon. */
@Composable
private fun StripLabel(
    icon: ImageVector,
    text: String,
) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(if (Marquee.form == Form.Tv) 15.dp else 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(icon, tint = Marquee.colors.muted, size = pillIcon())
        MqText(text, role = pillText(), color = Marquee.colors.muted, weight = FontWeight.SemiBold, maxLines = 1)
    }
}

/** A thin rule between a strip's parts. */
@Composable
private fun StripRule() =
    Box(
        Modifier
            .padding(horizontal = 2.dp)
            .width(1.dp)
            .height(pillHeight() * 0.6f)
            .background(Color.White.copy(alpha = 0.24f)),
    )

/** Between a strip's chips, and between the strip and the bar under it. */
@Composable
private fun stripGap(): Dp = if (Marquee.form == Form.Tv) 15.dp else 8.dp

/** Between the bar and a strip over it (section 5: 16 px on the picked frame, 8 dp on a phone, 24 points on TV). */
@Composable
fun stripLift(): Dp = if (Marquee.form == Form.Tv) 24.dp else 8.dp

/**
 * The subtitle strip, right-aligned over the bar's right end (section 5): "Subtitles", Off and the language chips
 * ([stripLanguages]), More languages (the full picker), the quick shift while a subtitle can be timed (on TV with a
 * Reset chip while it is not 0, D22), Style (not for picture subtitles) and Close. Style adds a row above: Size,
 * Position and Background, applied live and kept as the profile's ([onSize], [onPosition], [onBackground]). While the
 * device's caption style is on, Background is disabled and says why (D8). Nothing here hides the controls.
 *
 * On TV [first] is the checked chip (else Off), which takes the focus when the strip opens by remote; Down from the
 * strip goes back to [opener] (section 6).
 */
@Composable
fun SubtitleStrip(
    controller: PlaybackController,
    style: Boolean,
    onStyle: (Boolean) -> Unit,
    onMore: () -> Unit,
    onClose: () -> Unit,
    onSize: (SubtitleSize) -> Unit,
    onPosition: (Int) -> Unit,
    onBackground: (SubtitleBackground) -> Unit,
    modifier: Modifier = Modifier,
    shiftWords: Boolean = true,
    first: FocusRequester? = null,
    opener: FocusRequester? = null,
    adjusting: Boolean = false,
    onAdjust: (Boolean) -> Unit = {},
) {
    val tv = Marquee.form == Form.Tv
    val rows = controller.text
    val languages = stripLanguages(rows, chipCap())
    val off = rows.none { it.selected && it.id != TRACK_OFF }
    val down = Modifier.focusProperties { opener?.let { down = it } }
    val styleFocus = remember { FocusRequester() }
    val styleChip = remember { FocusRequester() }
    val shiftFocus = remember { FocusRequester() }
    // A remote that closes the Style row (Back) lands back on Style (section 6).
    var styled by remember { mutableStateOf(false) }
    LaunchedEffect(style) {
        if (style) {
            styled = true
        } else if (styled && tv) {
            withFrameNanos { }
            runCatching { styleChip.requestFocus() }
        }
    }
    val offFirst = offIsEntry(rows, languages)
    Column(
        modifier.semantics { paneTitle = Names.SUBTITLES }.testTag(STRIP_TAG),
        horizontalAlignment = Alignment.End,
        verticalArrangement = Arrangement.spacedBy(stripGap()),
    ) {
        if (style) StyleRow(controller, onSize, onPosition, onBackground, styleFocus)
        Row(
            Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = if (tv) 16.dp else 0.dp),
            horizontalArrangement = Arrangement.spacedBy(stripGap()),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            StripLabel(Icons.Captions, Names.SUBTITLES)
            Row(
                Modifier.selectableGroup().semantics { contentDescription = Names.SUBTITLE_LANGUAGE },
                horizontalArrangement = Arrangement.spacedBy(stripGap()),
            ) {
                StripChip(
                    Bar.OFF,
                    { if (!off) controller.selectText(TRACK_OFF) },
                    down.then(if (offFirst && first != null) Modifier.focusRequester(first) else Modifier),
                    checked = off,
                )
                // Keyed, so a language arriving ahead of the focused chip leaves the focus on its language.
                languages.forEach { lang ->
                    key(lang.key) {
                        StripChip(
                            lang.name,
                            { if (!lang.chosen) controller.selectText(lang.best.id) },
                            down.then(if (lang.chosen && first != null) Modifier.focusRequester(first) else Modifier),
                            checked = lang.chosen,
                            leading = { LanguageFlag(lang.key, pillIcon()) },
                        )
                    }
                }
            }
            if (rows.size <= 1 && controller.subtitlesPending) {
                MqText(Bar.STILL_SEARCHING, role = pillText(), color = Marquee.colors.muted, maxLines = 1)
            }
            StripChip(Bar.MORE_LANGUAGES, onMore, down, ghost = true, trailing = Icons.ChevronRight)
            controller.quickShift?.let { delay ->
                StripRule()
                QuickShift(
                    delay,
                    controller::setSubtitleDelay,
                    words = shiftWords,
                    valueFocus = if (tv) shiftFocus else null,
                    adjusting = adjusting,
                    onAdjust = onAdjust,
                    valueModifier = down,
                )
                if (tv && delay != 0L) {
                    // Reset goes with its press: the focus moves to the value first, never into nothing.
                    StripChip(Bar.RESET, {
                        runCatching { shiftFocus.requestFocus() }
                        controller.setSubtitleDelay(0L)
                    }, down)
                }
            }
            if (!controller.subtitlePicture) {
                StripRule()
                StripChip(
                    Bar.STYLE,
                    { onStyle(!style) },
                    down.focusRequester(styleChip),
                    ghost = true,
                    expanded = style,
                    icon = Icons.Type,
                )
            }
            StripChip(Bar.CLOSE, onClose, down, ghost = true, icon = Icons.X, spoken = Bar.CLOSE)
        }
    }
}

/** The Style row (section 5): Size, Position and Background, on a glass pill over the strip's row. */
@Composable
private fun StyleRow(
    controller: PlaybackController,
    onSize: (SubtitleSize) -> Unit,
    onPosition: (Int) -> Unit,
    onBackground: (SubtitleBackground) -> Unit,
    first: FocusRequester,
) {
    val tv = Marquee.form == Form.Tv
    val system = rememberCaptions().system != null
    val size = controller.subtitleSize
    val position = controller.subtitlePosition
    val at = POSITIONS.indexOf(position).let { if (it < 0) 1 else it }
    // A remote opening Style lands on Size's minus (section 6).
    if (tv) {
        LaunchedEffect(Unit) {
            withFrameNanos { }
            runCatching { first.requestFocus() }
        }
    }
    Row(
        Modifier
            .horizontalScroll(rememberScrollState())
            .clip(RoundedCornerShape(Marquee.radius.pill))
            .background(Marquee.colors.glassStrong)
            .padding(if (tv) 12.dp else 6.dp),
        horizontalArrangement = Arrangement.spacedBy(stripGap()),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        StyleLabel(Bar.SIZE)
        StepButton(
            Icons.Minus,
            Names.SMALLER,
            stepSize(size, -1) != null,
            { stepSize(size, -1)?.let(onSize) },
            Modifier.focusRequester(first),
        )
        StyleValue(sizeWords(size))
        StepButton(Icons.Plus, Names.LARGER, stepSize(size, 1) != null, { stepSize(size, 1)?.let(onSize) })
        StripRule()
        StyleLabel(Bar.POSITION)
        StepButton(Icons.ChevronDown, Bar.LOWER, at > 0, { POSITIONS.getOrNull(at - 1)?.let(onPosition) })
        StyleValue(positionWords(position))
        StepButton(
            Icons.ChevronUp,
            Bar.HIGHER,
            at < POSITIONS.lastIndex,
            { POSITIONS.getOrNull(at + 1)?.let(onPosition) },
        )
        StripRule()
        StyleLabel(Bar.BACKGROUND)
        Row(
            Modifier.selectableGroup().semantics {
                contentDescription = Bar.BACKGROUND
                if (system) disabled()
            },
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            // A background from a newer core reads as Box, as it draws.
            val shown =
                controller.subtitleBackground.takeUnless { it == SubtitleBackground.Unknown } ?: SubtitleBackground.Box
            for ((background, words) in Bar.BACKGROUNDS) {
                val on = shown == background
                StripChip(
                    words,
                    { if (!on) onBackground(background) },
                    checked = on && !system,
                    enabled = !system,
                )
            }
        }
        if (system) {
            MqText(Bar.SET_BY_DEVICE, role = pillText(), color = Marquee.colors.muted, maxLines = 1)
        }
    }
}

@Composable
private fun StyleLabel(text: String) =
    MqText(
        text,
        Modifier.padding(start = 6.dp),
        role = pillText(),
        color = Marquee.colors.muted,
        weight = FontWeight.SemiBold,
        maxLines = 1,
    )

/** A stepper's value ("100%", "Default"), said as it changes. */
@Composable
private fun StyleValue(text: String) =
    MqText(
        text,
        Modifier
            .widthIn(min = if (Marquee.form == Form.Tv) 120.dp else 64.dp)
            .semantics { liveRegion = LiveRegionMode.Polite },
        role = pillText(),
        weight = FontWeight.SemiBold,
        maxLines = 1,
        textAlign = androidx.compose.ui.text.style.TextAlign.Center,
    )

/**
 * The audio strip, left-aligned over the bar's left end (section 5): "Audio", one chip per track, the one playing
 * checked, then Close. With one track or none listed it shows that one and "Choose a version" (Versions), as the web.
 */
@Composable
fun AudioStrip(
    controller: PlaybackController,
    onVersions: () -> Unit,
    onClose: () -> Unit,
    modifier: Modifier = Modifier,
    first: FocusRequester? = null,
    opener: FocusRequester? = null,
) {
    val tv = Marquee.form == Form.Tv
    val tracks = controller.audio
    val down = Modifier.focusProperties { opener?.let { down = it } }
    val checked = tracks.indexOfFirst { it.selected }.coerceAtLeast(0)
    Row(
        modifier
            .semantics { paneTitle = Names.AUDIO }
            .testTag(STRIP_TAG)
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = if (tv) 16.dp else 0.dp),
        horizontalArrangement = Arrangement.spacedBy(stripGap()),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        StripLabel(Icons.AudioLines, Names.AUDIO)
        Row(
            Modifier.selectableGroup().semantics { contentDescription = Names.AUDIO_TRACK },
            horizontalArrangement = Arrangement.spacedBy(stripGap()),
        ) {
            tracks.forEachIndexed { i, track ->
                StripChip(
                    listOfNotNull(track.name.takeIf { it != StreamChoiceCopy.AUDIO_ONE }, track.detail)
                        .joinToString(", ")
                        .ifEmpty { track.name },
                    { if (!track.selected) controller.selectAudio(track.id) },
                    down.then(if (i == checked && first != null) Modifier.focusRequester(first) else Modifier),
                    checked = track.selected,
                    icon = Icons.AudioLines,
                )
            }
        }
        if (tracks.size <= 1) {
            StripChip(
                Bar.CHOOSE_VERSION,
                onVersions,
                down.then(if (tracks.isEmpty() && first != null) Modifier.focusRequester(first) else Modifier),
                ghost = true,
                icon = Icons.Layers,
            )
        }
        StripChip(Bar.CLOSE, onClose, down, ghost = true, icon = Icons.X, spoken = Bar.CLOSE)
    }
}
