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
    /** While the controls show: the foot of the title bar's controls, which the subtitles keep clear of. */
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
                    )
                }
            }
        // The view keeps the whole picture area, so the cues keep their size and a picture cue (PGS) its shape.
        Subtitles(player, frame, Modifier.fillMaxSize().testTag(SUBTITLES_TAG))
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
 * Cues without a line of their own (most subtitles) with their foot at [foot] (px from the top) and their size
 * [text] (a fraction of [height]). While two lines there would reach above [ceiling], they draw at 85%, or smaller
 * still when 85% does not clear it, so a subtitle and a control never overlap; the iPhone keeps the same rule.
 * Beside a control in the bottom corner ([corner], px), the lines keep left of it: centred on the screen while that
 * leaves them half its width, else centred in the room left of it.
 */
fun cueFrame(
    height: Float,
    width: Float,
    foot: Float,
    ceiling: Float,
    text: Float,
    corner: Rect? = null,
): CueFrame {
    val box = CUE_LINES * CUE_LINE * text * height
    val room = foot - ceiling
    val scale =
        when {
            box <= room -> 1f
            box * CUE_SHRINK <= room -> CUE_SHRINK
            else -> max(room, 0f) / box
        }
    val bottom = 1f - foot / height
    val drawn = text * scale
    // No corner control, or one wholly under or over the two lines: the whole width.
    if (corner == null || corner.top >= foot || corner.bottom <= foot - box * scale) return CueFrame(bottom, drawn)
    val centred = 2 * corner.left / width - 1
    if (centred >= CUE_MIN_WIDTH) return CueFrame(bottom, drawn, centred)
    val left = max(corner.left, 0f) / width
    return CueFrame(bottom, drawn, left, left / 2)
}

/** The lines the rule makes room for: two, the most a subtitle usually has. */
const val CUE_LINES = 2

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
    player: Player,
    frame: CueFrame,
    modifier: Modifier,
) {
    var cues by remember(player) { mutableStateOf(player.currentCues.cues) }
    AndroidView(
        factory = { context -> SubtitleView(context).also { it.setUserDefaultStyle() } },
        update = {
            it.setFractionalTextSize(frame.text)
            it.setBottomPaddingFraction(frame.bottom)
            it.setCues(cues.map(frame::fit))
        },
        modifier = modifier,
    )
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
}

/**
 * A cue the frame lifts (text with no line of its own) kept to the frame's width; a picture cue (PGS) or a cue
 * its file put on a line of its own stays as it is.
 */
private fun CueFrame.fit(cue: Cue): Cue =
    if (width >= 1f || cue.bitmap != null || cue.line != Cue.DIMEN_UNSET) {
        cue
    } else {
        cue
            .buildUpon()
            .setSize(if (cue.size == Cue.DIMEN_UNSET) width else minOf(cue.size, width))
            .setPosition(center)
            .setPositionAnchor(Cue.ANCHOR_TYPE_MIDDLE)
            .build()
    }

/** The system caption text size, 1 when captions are off in the system settings (as SubtitleView reads it). */
private fun captionScale(context: Context): Float {
    val captions = context.getSystemService(CaptioningManager::class.java) ?: return 1f
    return if (captions.isEnabled) captions.fontScale else 1f
}

/** The subtitles' area, for tests of where they sit. */
const val SUBTITLES_TAG = "subtitles"
