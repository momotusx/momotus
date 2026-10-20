package app.marquee.player

import android.content.Context
import android.view.accessibility.CaptioningManager
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
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
import androidx.media3.common.Player
import androidx.media3.common.text.Cue
import androidx.media3.common.text.CueGroup
import androidx.media3.ui.SubtitleView
import androidx.media3.ui.compose.ContentFrame
import androidx.media3.ui.compose.SURFACE_TYPE_SURFACE_VIEW
import app.marquee.core.model.SubtitleSize
import kotlin.math.ceil
import kotlin.math.max

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
    /** A control in the bottom corner (Skip, Up Next), in px: the subtitles' lines keep clear of it. */
    subtitleCorner: Rect? = null,
) {
    BoxWithConstraints(modifier.background(Color.Black)) {
        ContentFrame(
            player = player,
            modifier = Modifier.fillMaxSize(),
            surfaceType = SURFACE_TYPE_SURFACE_VIEW,
            contentScale = ContentScale.Fit,
        )
        // The cues on screen: the frame is set for the lines they take.
        var cues by remember(player) { mutableStateOf(player.currentCues.cues) }
        DisposableEffect(player) {
            val listener =
                object : Player.Listener {
                    override fun onCues(cueGroup: CueGroup) {
                        cues = cueGroup.cues
                    }
                }
            player.addListener(listener)
            onDispose { player.removeListener(listener) }
        }
        // The system caption size (as setUserDefaultTextSize reads it) times the profile's size.
        val text =
            SubtitleView.DEFAULT_TEXT_SIZE_FRACTION * captionScale(LocalContext.current) * subtitleScale(subtitleSize)
        val height = if (constraints.hasBoundedHeight) constraints.maxHeight.toFloat() else 0f
        val width = if (constraints.hasBoundedWidth) constraints.maxWidth.toFloat() else 0f
        val frame =
            with(LocalDensity.current) {
                val gap = CUE_GAP.toPx()
                if (height <= 0f || width <= 0f) {
                    CueFrame(SubtitleView.DEFAULT_BOTTOM_PADDING_FRACTION, text)
                } else {
                    cueFrame(
                        height = height,
                        width = width,
                        foot =
                            if (subtitleLift > 0.dp) {
                                height - subtitleLift.toPx() - gap
                            } else {
                                height * (1 - SubtitleView.DEFAULT_BOTTOM_PADDING_FRACTION)
                            },
                        ceiling = if (subtitleCeiling > 0.dp) (subtitleCeiling + CUE_CLEARANCE).toPx() else 0f,
                        text = text,
                        corner = subtitleCorner?.let { Rect(it.left - gap, it.top - gap, it.right, it.bottom) },
                        lines = { span -> cueLines(cues, text * height, span) },
                    )
                }
            }
        // The view keeps the whole picture area, so the cues keep their size and a picture cue (PGS) its shape.
        Subtitles(cues, frame, lifted = subtitleLift > 0.dp, Modifier.fillMaxSize().testTag(SUBTITLES_TAG))
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
    lifted: Boolean,
    modifier: Modifier,
) {
    AndroidView(
        factory = { context -> SubtitleView(context).also { it.setUserDefaultStyle() } },
        update = {
            it.setFractionalTextSize(frame.text)
            it.setCues(cues.map { cue -> frame.fit(cue, lifted) })
        },
        modifier = modifier,
    )
}

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
 * A cue set from the foot, kept to the frame's width; while [lifted], with its foot at the frame's (a cue stacked
 * over another, line -2 and up, a line higher each), as the view's own bottom padding moves only cues with no line.
 */
internal fun CueFrame.fit(
    cue: Cue,
    lifted: Boolean,
): Cue {
    val over = linesOverFoot(cue) ?: return cue
    if (width >= 1f && !lifted) return cue
    val fitted = cue.buildUpon()
    if (width < 1f) {
        fitted
            .setSize(if (cue.size == Cue.DIMEN_UNSET) width else minOf(cue.size, width))
            .setPosition(center)
            .setPositionAnchor(Cue.ANCHOR_TYPE_MIDDLE)
    }
    if (lifted) {
        fitted.setLine(1f - bottom - over * CUE_LINE * text, Cue.LINE_TYPE_FRACTION).setLineAnchor(Cue.ANCHOR_TYPE_END)
    }
    return fitted.build()
}

/** The system caption text size, 1 when captions are off in the system settings (as SubtitleView reads it). */
private fun captionScale(context: Context): Float {
    val captions = context.getSystemService(CaptioningManager::class.java) ?: return 1f
    return if (captions.isEnabled) captions.fontScale else 1f
}

/** The subtitles' area, for tests of where they sit. */
const val SUBTITLES_TAG = "subtitles"
