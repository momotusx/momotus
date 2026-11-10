package app.marquee.player

import android.content.Context
import android.text.Layout
import android.text.SpannableStringBuilder
import android.text.Spanned
import android.text.TextPaint
import android.text.style.BackgroundColorSpan
import android.text.style.RelativeSizeSpan
import android.view.accessibility.CaptioningManager
import androidx.annotation.ColorInt
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import androidx.media3.common.Player
import androidx.media3.common.text.Cue
import androidx.media3.common.text.CueGroup
import androidx.media3.ui.CaptionStyleCompat
import androidx.media3.ui.SubtitleView
import androidx.media3.ui.compose.ContentFrame
import androidx.media3.ui.compose.SURFACE_TYPE_SURFACE_VIEW
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.model.SubtitleBackground
import app.marquee.core.model.SubtitleLayout
import app.marquee.core.model.SubtitleSize
import kotlinx.coroutines.delay
import kotlin.math.ceil
import kotlin.math.max
import kotlin.math.min
import android.graphics.Color as AndroidColor

/**
 * The picture and its subtitles. The picture goes to a SurfaceView, which is what lets the
 * platform switch a TV to an HDR mode; a TextureView would tone-map everything to SDR. The frame
 * fits the picture to the screen at its own shape (black bars where the shapes differ). The
 * subtitles follow the system caption settings (color, edge, and size), their size scaled by the
 * profile's subtitle size ([subtitleScale]).
 *
 * On the emulator's phone screen a 1080p picture shows at its native 1920 px in the middle of a
 * 2400 px screen, with or without this frame doing the sizing (checked with a hand-sized
 * PlayerSurface too): the layout is full width, so that is the emulator's video layer, which
 * does not scale up here. A real display is the thing to check it on.
 */
@Composable
fun PlayerVideo(
    player: Player,
    modifier: Modifier = Modifier,
    subtitleSize: SubtitleSize = SubtitleSize.Medium,
    /** While the controls show: their height, so the subtitles sit just above them, never under or over them. */
    subtitleLift: Dp = 0.dp,
    /** While the controls show: the foot of the controls over the subtitles (the transport), which they keep clear of. */
    subtitleCeiling: Dp = 0.dp,
    /** While the controls show: the top of the transport in the middle, which the lines go over when under it is too
     * tight ([cueSpan]); 0 for none. */
    subtitleOver: Dp = 0.dp,
    /** A control in the bottom corner (Skip, Up Next), in px: the subtitles' lines keep clear of it. */
    subtitleCorner: Rect? = null,
    /** An addon track drawn by the app ([PlaybackController.addonCues]): its cues now, read every frame; null for none. */
    addonCues: (() -> List<Cue>)? = null,
    /** The picture's width over its height, its pixel shape counted ([PlaybackController.videoRatio]); 0 before. */
    videoRatio: Float = 0f,
    /** The second subtitle line ([PlaybackController.secondCues]): its cues now, read every frame; null for none. */
    secondCues: (() -> List<Cue>)? = null,
    /** How two lines draw (plans/dual-subtitles.md 4). */
    subtitleLayout: SubtitleLayout = SubtitleLayout.Stacked,
    /** The second line shows only while paused. */
    secondOnPause: Boolean = false,
    /** While the controls show: the foot of the bar at the top, which Split's second line hangs under. */
    subtitleTop: Dp = 0.dp,
    /** The profile's position: the plain cues' foot, percent of the picture over its foot ([cueFoot]). */
    subtitlePosition: Int = DEFAULT_POSITION,
    /** The profile's background, unless the device's caption style is on ([captionStyle]). */
    subtitleBackground: SubtitleBackground = SubtitleBackground.Box,
) {
    BoxWithConstraints(modifier.background(Color.Black)) {
        ContentFrame(
            player = player,
            modifier = Modifier.fillMaxSize(),
            surfaceType = SURFACE_TYPE_SURFACE_VIEW,
            contentScale = ContentScale.Fit,
        )
        // The first line's cues: the player's (the file's own track), or an addon track's; and the second line's,
        // always an addon track's. Each is its own state, so the cues drawn are rebuilt only when one changes.
        var cues by remember(player) { mutableStateOf<List<Cue>>(player.currentCues.cues) }
        var second by remember(player) { mutableStateOf<List<Cue>>(emptyList()) }
        // The viewer's pause (a stall keeps it true): Only when paused keys on it, and the poll wakes on it.
        var playing by remember(player) { mutableStateOf(player.playWhenReady) }
        val addon = rememberUpdatedState(addonCues)
        val two = rememberUpdatedState(secondCues)
        DisposableEffect(player) {
            val listener =
                object : Player.Listener {
                    override fun onCues(cueGroup: CueGroup) {
                        if (addon.value == null) cues = cueGroup.cues
                    }

                    override fun onPlayWhenReadyChanged(
                        playWhenReady: Boolean,
                        reason: Int,
                    ) {
                        playing = playWhenReady
                    }
                }
            player.addListener(listener)
            onDispose { player.removeListener(listener) }
        }
        val lifecycle = LocalLifecycleOwner.current.lifecycle
        LaunchedEffect(player, addonCues != null, secondCues != null, playing) {
            if (addonCues == null) cues = player.currentCues.cues
            if (secondCues == null) second = emptyList()
            if (addonCues == null && secondCues == null) return@LaunchedEffect
            // Only while the screen is in front. ponytail: a look every 40 ms while it plays, near a frame's worth for a
            // subtitle, and every 250 ms while paused, for a seek or a delay step; a wait until the next cue's start
            // or end if the polling ever shows in a profile.
            lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
                while (true) {
                    val firstNow = addon.value
                    val secondNow = two.value
                    if (firstNow == null && secondNow == null) break
                    firstNow?.invoke()?.let { if (it != cues) cues = it }
                    secondNow?.invoke()?.let { if (it != second) second = it }
                    delay(if (player.isPlaying) ADDON_POLL_MS else ADDON_PAUSED_POLL_MS)
                }
            }
        }
        // The system caption size (as setUserDefaultTextSize reads it) times the profile's size.
        val captions = rememberCaptions()
        val style = captionStyle(captions.system, subtitleBackground)
        val text = SubtitleView.DEFAULT_TEXT_SIZE_FRACTION * captions.scale * subtitleScale(subtitleSize)
        val box = boxColor(style)
        val height = if (constraints.hasBoundedHeight) constraints.maxHeight.toFloat() else 0f
        val width = if (constraints.hasBoundedWidth) constraints.maxWidth.toFloat() else 0f
        val top =
            with(LocalDensity.current) {
                val bar = if (subtitleTop > 0.dp) (subtitleTop + CUE_GAP).toPx() else 0f
                if (height >
                    0f
                ) {
                    secondTop(height, width, videoRatio, bar, cues, text * height, CUE_GAP.toPx()) / height
                } else {
                    0f
                }
            }
        // Not one frame of a line the controller no longer draws: [second] empties only after the effect runs.
        val shown = if (secondCues != null && secondShows(secondOnPause, playing)) second else emptyList()
        val drawn = remember(cues, shown, subtitleLayout, top, box) { twoLines(cues, shown, subtitleLayout, top, box) }
        val frame =
            with(LocalDensity.current) {
                val gap = CUE_GAP.toPx()
                if (height <= 0f || width <= 0f) {
                    CueFrame(SubtitleView.DEFAULT_BOTTOM_PADDING_FRACTION, text)
                } else {
                    val (foot, ceiling) =
                        cueSpan(
                            foot =
                                cueFoot(
                                    height,
                                    width,
                                    videoRatio,
                                    lifted = (height - subtitleLift.toPx() - gap).takeIf { subtitleLift > 0.dp },
                                    position = subtitlePosition,
                                ),
                            ceiling = if (subtitleCeiling > 0.dp) (subtitleCeiling + CUE_CLEARANCE).toPx() else 0f,
                            over = (subtitleOver.toPx() - gap).takeIf { subtitleOver > 0.dp },
                            top = if (subtitleTop > 0.dp) (subtitleTop + CUE_CLEARANCE).toPx() else 0f,
                            twoLines = CUE_LINES * CUE_LINE * text * height,
                        )
                    cueFrame(
                        height = height,
                        width = width,
                        foot = foot,
                        ceiling = ceiling,
                        text = text,
                        corner = subtitleCorner?.let { Rect(it.left - gap, it.top - gap, it.right, it.bottom) },
                        lines = { span -> cueLines(drawn, text * height, span) },
                    )
                }
            }
        // The view keeps the whole picture area, so the cues keep their size and a picture cue (PGS) its shape.
        Subtitles(drawn, frame, style, width, height, Modifier.fillMaxSize().testTag(SUBTITLES_TAG))
    }
}

/**
 * Where cues without a line of their own sit and how big they draw, as fractions of the picture area's height; and
 * the width their lines keep to with its centre, as fractions of its width (1 and the middle: the whole width).
 */
data class CueFrame(
    val bottom: Float,
    val text: Float,
    val width: Float = 1f,
    val center: Float = 0.5f,
)

/**
 * Cues set from the foot (most subtitles) with their foot at [foot] (px from the top) and their size [text] (a
 * fraction of [height]). While the [lines] they take there would reach above [ceiling], they draw at 85%, or smaller
 * still when 85% does not clear it, down to 75%, so a subtitle meets a control only past that; the iPhone keeps the
 * same rule. Beside a control in the bottom corner ([corner], px), the lines keep left of it: centred on the screen
 * while that leaves them half its width, else centred in the room left of it, and [lines] counts the wrapping.
 */
fun cueFrame(
    height: Float,
    width: Float,
    foot: Float,
    ceiling: Float,
    text: Float,
    corner: Rect? = null,
    /** The lines the cues on screen take in a span this many px wide, at full size. */
    lines: (span: Float) -> Int = { CUE_LINES },
): CueFrame {
    // A corner control beside two lines at the foot (not wholly under or over them) narrows the lines first.
    val twoLines = CUE_LINES * CUE_LINE * text * height
    val left = corner?.takeIf { it.top < foot && it.bottom > foot - twoLines }?.left?.coerceAtLeast(0f)
    val centred = left?.let { 2 * it / width - 1 }
    val (span, center) =
        when {
            left == null || centred == null -> 1f to 0.5f
            centred >= CUE_MIN_WIDTH -> centred to 0.5f
            else -> left / width to left / width / 2
        }
    val box = lines(span * width) * CUE_LINE * text * height
    val room = foot - ceiling
    val scale =
        when {
            box <= room -> 1f
            box * CUE_SHRINK <= room -> CUE_SHRINK
            else -> max(room / box, CUE_FLOOR)
        }
    return CueFrame(1f - foot / height, text * scale, span, center)
}

/**
 * The foot of the cues set from the foot, px from the top: [position] percent of the picture's height over the
 * picture's foot (the picture fitted to the view at [ratio]; the view's own height before the ratio is known), never
 * under the screen's 5% title-safe line. With the controls up it is the higher of that and [lifted] (just over them)
 * raised by what the position adds over the default 8%, so a change shows while the strip is open (plans/player-bar.md
 * section 5). The black band under wide video never takes the lines.
 */
internal fun cueFoot(
    height: Float,
    width: Float,
    ratio: Float,
    lifted: Float?,
    position: Int = DEFAULT_POSITION,
): Float {
    val picture = if (ratio > 0f) min(height, width / ratio) else height
    val plain = min((height + picture) / 2 - picture * position / 100f, height * (1 - SAFE_LINE))
    return lifted?.let { min(plain, it - picture * max(0, position - DEFAULT_POSITION) / 100f) } ?: plain
}

/**
 * The cues' foot and ceiling, px from the top, with the controls up: the lifted [foot] under the transport in the
 * middle ([ceiling], its foot) while that room holds [twoLines] at the smallest size ([CUE_FLOOR]). When it does not
 * (a phone across with an open strip and its Style row), the lines hang over the transport instead, their foot at
 * [over] (its top, less the gap) under [top] (the title bar's foot), when that room is the larger: the lines stay
 * readable while the viewer tunes them, never under Back 10, Play and Forward 10 (Pixel walk 2026-11-09 L23).
 */
internal fun cueSpan(
    foot: Float,
    ceiling: Float,
    over: Float?,
    top: Float,
    twoLines: Float,
): Pair<Float, Float> =
    if (over != null && foot - ceiling < twoLines * CUE_FLOOR && over - top > foot - ceiling) {
        over to top
    } else {
        foot to ceiling
    }

/** The profile's default position, today's rule: the cues' foot 8% of the picture over its foot ([CUE_RISE]). */
const val DEFAULT_POSITION = 8

/** The positions the strip and Settings step through, percent of the picture (plans/player-bar.md D7). */
val POSITIONS = listOf(5, DEFAULT_POSITION, 12, 16, 20, 25)

/** "Default" for 8, else "12%". */
fun positionWords(position: Int): String =
    if (position == DEFAULT_POSITION) StreamChoiceCopy.Bar.DEFAULT else "$position%"

/**
 * The cues drawn with a second line (plans/dual-subtitles.md 4, 6.2). Stacked and One box join every cue of the second
 * line, its own placement dropped (decision 7), at [SECOND_SIZE] over the first line's foot cues, highest first, into
 * one cue that hangs from the foot and wraps as one ([SubtitleView] draws each cue on its own, so two would overlap),
 * placed across as the first line's lowest foot cue (its alignment, position and width). One box gives it the caption
 * window [box] and clears the lines' own background under it. Split keeps the first line as it is and hangs the
 * second, at full size, from [top] (a fraction of the view's height). The first line's signs stay where its file put
 * them.
 */
internal fun twoLines(
    first: List<Cue>,
    second: List<Cue>,
    layout: SubtitleLayout,
    top: Float,
    @ColorInt box: Int,
): List<Cue> {
    val words = second.mapNotNull { cue -> cue.text?.takeIf { it.isNotEmpty() } }
    if (words.isEmpty()) return first
    val text = SpannableStringBuilder()
    words.forEach { if (text.isEmpty()) text.append(it) else text.append('\n').append(it) }
    if (layout == SubtitleLayout.Split) {
        val upper =
            Cue
                .Builder()
                .setText(text)
                .setLine(top, Cue.LINE_TYPE_FRACTION)
                .setLineAnchor(Cue.ANCHOR_TYPE_START)
                .build()
        return first + upper
    }
    text.setSpan(RelativeSizeSpan(SECOND_SIZE), 0, text.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
    val (foot, signs) = first.partition { linesOverFoot(it) != null }
    foot.sortedByDescending { linesOverFoot(it) }.forEach { text.append('\n').append(it.text ?: "") }
    val joined = Cue.Builder().setText(text)
    // The block keeps the first line's place across (a left or right aligned file): the foot cue's, never the second's.
    foot.minByOrNull { linesOverFoot(it) ?: 0f }?.let {
        joined
            .setTextAlignment(it.textAlignment)
            .setPosition(it.position)
            .setPositionAnchor(it.positionAnchor)
            .setSize(it.size)
    }
    if (layout == SubtitleLayout.OneBox) {
        // Set last, so it wins over the caption style's line background that Media3 puts first.
        text.setSpan(BackgroundColorSpan(AndroidColor.TRANSPARENT), 0, text.length, Spanned.SPAN_EXCLUSIVE_EXCLUSIVE)
        joined.setWindowColor(box)
    }
    return signs + joined.build()
}

/** The second line's size, of the first's, in Stacked and One box. */
const val SECOND_SIZE = 0.85f

/** Only when paused: the second line shows while the viewer has it paused (a stall is no pause), else always. */
internal fun secondShows(
    onPause: Boolean,
    playWhenReady: Boolean,
): Boolean = !onPause || !playWhenReady

/**
 * Split's second line: its top, px from the top, the mirror of the foot ([cueFoot]): [CUE_RISE] of the picture under
 * the picture's top, never over the 5% line, under [bar] (the top bar's foot and a gap, 0 with the controls hidden),
 * and [gap] under the lowest sign the first line's file placed in the top half.
 */
internal fun secondTop(
    height: Float,
    width: Float,
    ratio: Float,
    bar: Float,
    first: List<Cue>,
    textPx: Float,
    gap: Float,
): Float {
    val picture = if (ratio > 0f) min(height, width / ratio) else height
    val sign = first.mapNotNull { signFoot(it, height, textPx) }.maxOrNull()
    return maxOf((height - picture) / 2 + picture * CUE_RISE, height * SAFE_LINE, bar, sign?.plus(gap) ?: 0f)
}

/** The top of the screen text keeps under: the 5% title-safe line (EBU R 95). */
private const val SAFE_LINE = 0.05f

/**
 * The foot, px from the top, of a text cue the file placed in the top half (a sign); null for any other. Estimated
 * from its line and its lines' count, as the foot block is ([CUE_LINE]).
 */
private fun signFoot(
    cue: Cue,
    height: Float,
    textPx: Float,
): Float? {
    if (cue.bitmap != null || linesOverFoot(cue) != null) return null
    val box = (cue.text?.lines()?.size ?: return null) * CUE_LINE * textPx
    val top =
        when {
            cue.lineType == Cue.LINE_TYPE_FRACTION -> {
                cue.line * height -
                    when (cue.lineAnchor) {
                        Cue.ANCHOR_TYPE_MIDDLE -> box / 2
                        Cue.ANCHOR_TYPE_END -> box
                        else -> 0f
                    }
            }

            cue.lineType == Cue.LINE_TYPE_NUMBER && cue.line >= 0 -> {
                cue.line * CUE_LINE * textPx
            }

            else -> {
                return null
            }
        }
    return (top + box).takeIf { top < height / 2 }
}

/**
 * One box's fill (plans/dual-subtitles.md 4 E): the user's caption window when their style sets one, else the caption
 * background, which the joined cue's window takes over from its lines.
 */
@ColorInt
internal fun boxColor(style: CaptionStyleCompat): Int =
    if (AndroidColor.alpha(style.windowColor) > 0) style.windowColor else style.backgroundColor

/** The system caption style while the device's captions are on, else null ([systemStyle]), and the text size ([captionScale]). */
internal data class Captions(
    val system: CaptionStyleCompat?,
    val scale: Float,
)

internal fun captions(context: Context) = Captions(systemStyle(context), captionScale(context))

/**
 * The style cues draw in: the device's caption style while it is on, which outranks an app look (plans/player-bar.md
 * D8); else the profile's [background]. Box is today's look (Media3's default: white on a black box); Outline and
 * Shadow draw white with a black edge and no box.
 */
internal fun captionStyle(
    system: CaptionStyleCompat?,
    background: SubtitleBackground,
): CaptionStyleCompat =
    system ?: when (background) {
        SubtitleBackground.Outline -> edged(CaptionStyleCompat.EDGE_TYPE_OUTLINE)
        SubtitleBackground.Shadow -> edged(CaptionStyleCompat.EDGE_TYPE_DROP_SHADOW)
        SubtitleBackground.Box, SubtitleBackground.Unknown -> CaptionStyleCompat.DEFAULT
    }

private fun edged(edge: Int) =
    CaptionStyleCompat(
        AndroidColor.WHITE,
        AndroidColor.TRANSPARENT,
        AndroidColor.TRANSPARENT,
        edge,
        AndroidColor.BLACK,
        null,
    )

/**
 * The system captions, followed while the player shows: a change in the system settings draws at once, not only
 * after the app restarts (the Pixel walk, 2026-11-01).
 */
@Composable
internal fun rememberCaptions(): Captions {
    val context = LocalContext.current
    var now by remember(context) { mutableStateOf(captions(context)) }
    DisposableEffect(context) {
        val stop = followCaptions(context) { now = it }
        onDispose { stop() }
    }
    return now
}

/** Hands [changed] the system captions on each change of the system caption settings, until the call it returns. */
internal fun followCaptions(
    context: Context,
    changed: (Captions) -> Unit,
): () -> Unit {
    val manager = context.getSystemService(CaptioningManager::class.java) ?: return {}
    val listener =
        object : CaptioningManager.CaptioningChangeListener() {
            override fun onEnabledChanged(enabled: Boolean) = changed(captions(context))

            override fun onUserStyleChanged(userStyle: CaptioningManager.CaptionStyle) = changed(captions(context))

            override fun onFontScaleChanged(fontScale: Float) = changed(captions(context))
        }
    manager.addCaptioningChangeListener(listener)
    return { manager.removeCaptioningChangeListener(listener) }
}

/** The system caption style, as [SubtitleView.setUserDefaultStyle] reads it, while captions are on; null while off. */
private fun systemStyle(context: Context): CaptionStyleCompat? {
    val captions = context.getSystemService(CaptioningManager::class.java)
    return if (captions?.isEnabled == true) CaptionStyleCompat.createFromCaptionStyle(captions.userStyle) else null
}

/**
 * The cues' foot over the picture's foot, a share of the picture's height: Media3's own default, which its PlayerView
 * measures on the content frame, between EBU R 95's 5% and CEA-608's 10%, on every platform
 * (plans/dual-subtitles.md 11).
 */
const val CUE_RISE = SubtitleView.DEFAULT_BOTTOM_PADDING_FRACTION

/** The lines the corner rule makes room for, and the rule's budget when the cues are not known: two. */
const val CUE_LINES = 2

/** The smallest cues draw while the controls show: past it a long subtitle reaches the transport, still readable. */
const val CUE_FLOOR = 0.75f

/**
 * The lines [cues] set from the foot take at [textPx] in [spanPx]: each of their own lines, wrapped at about half
 * the text size a character. ponytail: an average glyph width for the default typeface, not a measured layout.
 */
internal fun cueLines(
    cues: List<Cue>,
    textPx: Float,
    spanPx: Float,
): Int =
    cues.filter { linesOverFoot(it) != null }.sumOf { cue ->
        cue.text?.lines()?.sumOf { line -> max(1, ceil(line.length * CHAR_WIDTH * textPx / spanPx).toInt()) } ?: 0
    }

/** A character's width on average, of the text size. */
private const val CHAR_WIDTH = 0.5f

/**
 * A cue line's height for its text size: the default typeface's top to bottom as the cue's layout keeps it
 * (Roboto: 1.30 for the first line, 1.20 for the last). ponytail: one factor for the default typeface; a taller
 * caption typeface from the system settings runs a little over it.
 */
const val CUE_LINE = 1.25f

/** The size cues draw at while they would meet a control. */
const val CUE_SHRINK = 0.85f

/** The least width, of the screen's, cues keep centred on it beside a corner control. */
const val CUE_MIN_WIDTH = 0.5f

/** Between the cues' foot and the controls under them. */
private val CUE_GAP = 8.dp

/** Between two lines of cues and the controls over them. */
private val CUE_CLEARANCE = 4.dp

/** The profile's subtitle size as a factor on the system caption size (Medium leaves it as it is). */
fun subtitleScale(size: SubtitleSize): Float =
    when (size) {
        SubtitleSize.Small -> 0.75f
        SubtitleSize.Large -> 1.3f
        SubtitleSize.ExtraLarge -> 1.6f
        SubtitleSize.Medium, SubtitleSize.Unknown -> 1f
    }

@Composable
private fun Subtitles(
    cues: List<Cue>,
    frame: CueFrame,
    style: CaptionStyleCompat,
    width: Float,
    height: Float,
    modifier: Modifier,
) {
    AndroidView(
        factory = { context -> SubtitleView(context) },
        update = {
            it.setStyle(style)
            it.setFractionalTextSize(frame.text)
            it.setCues(fitted(cues, frame, style, width, height))
        },
        modifier = modifier,
    )
}

/** The cues as the view draws them: in the [frame] ([CueFrame.fit]), One box's block fitted to its lines ([hugged]). */
internal fun fitted(
    cues: List<Cue>,
    frame: CueFrame,
    style: CaptionStyleCompat,
    width: Float,
    height: Float,
): List<Cue> {
    // Media3's painter measures at the view's text size in the style's typeface.
    val paint =
        TextPaint().apply {
            isAntiAlias = true
            isSubpixelText = true
            textSize = frame.text * height
            typeface = style.typeface
        }
    // Fitted to its lines first: the frame's narrowing beside a corner control then keeps the smaller of the two.
    return cues.map { cue -> frame.fit(cue.hugged(paint, width)) }
}

/**
 * One box's block (a foot cue with a window of its own) sized to its widest line, so its box fits both lines
 * (plans/dual-subtitles.md 4 E): Media3 draws a cue's window across the cue's whole size, and a WebVTT line comes
 * 100% wide, so the box was a band across the picture, or across the room a corner control leaves. Its place
 * across stays: centred where it was centred, from its edge where its file put it. Unchanged while its widest line
 * fills the room. [paint] measures as Media3's painter does; [width] is the view's, px.
 */
internal fun Cue.hugged(
    paint: TextPaint,
    width: Float,
): Cue {
    val words = text
    if (!windowColorSet || words.isNullOrEmpty() || linesOverFoot(this) == null) return this
    // Media3's SubtitlePainter: lines wrap in the view's width less an eighth of the text size on each side.
    val room = width - 2 * (paint.textSize * PAINTER_PADDING + 0.5f).toInt()
    if (room <= 0f) return this
    val breaks = listOf(-1) + words.indices.filter { words[it] == '\n' } + words.length
    val widest = breaks.zipWithNext { a, b -> Layout.getDesiredWidth(words, a + 1, b, paint) }.max()
    // Two px over the widest line: Media3 rounds the room down, and the line must not wrap.
    val fits = (ceil(widest) + 2f) / room
    val now = if (size == Cue.DIMEN_UNSET) 1f else size
    return if (fits < now) buildUpon().setSize(fits).build() else this
}

/** Media3's `SubtitlePainter.INNER_PADDING_RATIO`: a cue's padding each side, of the text size. */
private const val PAINTER_PADDING = 0.125f

/**
 * How many lines over the foot a text cue set from the foot sits: 0 with no line of its own (SubRip) or at the foot
 * of an SSA or ASS screen (a line of 0.9 or more, anchored at its end: Media3 gives 0.95 to every bottom-aligned
 * dialogue line), n for a line counted up from the bottom (-1 - n: WebVTT, and what Media3 hands over for most
 * subtitles). Null for a picture cue (PGS) or a cue its file placed anywhere else, which stays where it is.
 */
private fun linesOverFoot(cue: Cue): Float? =
    when {
        cue.bitmap != null -> null
        cue.line == Cue.DIMEN_UNSET -> 0f
        cue.lineType == Cue.LINE_TYPE_NUMBER && cue.line < 0 -> -cue.line - 1
        cue.lineType == Cue.LINE_TYPE_FRACTION && cue.lineAnchor == Cue.ANCHOR_TYPE_END && cue.line >= SSA_FOOT -> 0f
        else -> null
    }

/** An SSA or ASS line this low (a fraction of the screen, its foot) is a subtitle at the foot, not a sign. */
private const val SSA_FOOT = 0.9f

/**
 * A cue set from the foot, kept to the frame's width, with its foot at the frame's and its lines growing up from it
 * (a cue stacked over another, line -2 and up, a line higher each), the controls up or not. The view's own bottom
 * padding moves only cues with no line: Media3 draws a WebVTT bottom line (every addon line) on the view's last
 * pixel, which on a TV is past the overscan-safe line and, under wide video, on the black bar.
 */
internal fun CueFrame.fit(cue: Cue): Cue {
    val over = linesOverFoot(cue) ?: return cue
    val fitted =
        cue
            .buildUpon()
            .setLine(1f - bottom - over * CUE_LINE * text, Cue.LINE_TYPE_FRACTION)
            .setLineAnchor(Cue.ANCHOR_TYPE_END)
    if (width < 1f) {
        fitted
            .setSize(if (cue.size == Cue.DIMEN_UNSET) width else minOf(cue.size, width))
            .setPosition(center)
            .setPositionAnchor(Cue.ANCHOR_TYPE_MIDDLE)
    }
    return fitted.build()
}

/** The system caption text size, 1 when captions are off in the system settings (as SubtitleView reads it). */
private fun captionScale(context: Context): Float {
    val captions = context.getSystemService(CaptioningManager::class.java) ?: return 1f
    return if (captions.isEnabled) captions.fontScale else 1f
}

/** How often an addon track's cues are looked up while it is on: playing, and paused. */
private const val ADDON_POLL_MS = 40L
private const val ADDON_PAUSED_POLL_MS = 250L

/** The subtitles' area, for tests of where they sit. */
const val SUBTITLES_TAG = "subtitles"
