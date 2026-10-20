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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.media3.common.Player
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
    /** While the controls show: the foot of the controls at the top (the title bar), which the subtitles keep clear of. */
    subtitleCeiling: Dp = 0.dp,
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
        val frame =
            with(LocalDensity.current) {
                when {
                    height <= 0f -> {
                        CueFrame(SubtitleView.DEFAULT_BOTTOM_PADDING_FRACTION, text)
                    }

                    subtitleLift > 0.dp -> {
                        cueFrame(
                            height,
                            height - (subtitleLift + CUE_GAP).toPx(),
                            (subtitleCeiling + CUE_CLEARANCE).toPx(),
                            text,
                        )
                    }

                    else -> {
                        cueFrame(height, height * (1 - SubtitleView.DEFAULT_BOTTOM_PADDING_FRACTION), 0f, text)
                    }
                }
            }
        // The view keeps the whole picture area, so the cues keep their size and a picture cue (PGS) its shape.
        Subtitles(player, frame, Modifier.fillMaxSize().testTag(SUBTITLES_TAG))
    }
}

/** Where cues without a position of their own sit and how big they draw, both as fractions of the picture area's height. */
data class CueFrame(
    val bottom: Float,
    val text: Float,
)

/**
 * Cues without a position of their own (most subtitles) with their foot at [foot] (px from the top) and their size
 * [text] (a fraction of [height]). While two lines there would reach above [ceiling], they draw at 85%, or smaller
 * still when 85% does not clear it, so a subtitle and a control never overlap; the iPhone keeps the same rule.
 */
fun cueFrame(
    height: Float,
    foot: Float,
    ceiling: Float,
    text: Float,
): CueFrame {
    val box = CUE_LINES * CUE_LINE * text * height
    val room = foot - ceiling
    val scale =
        when {
            box <= room -> 1f
            box * CUE_SHRINK <= room -> CUE_SHRINK
            else -> max(room, 0f) / box
        }
    return CueFrame(bottom = 1f - foot / height, text = text * scale)
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
    var view by remember { mutableStateOf<SubtitleView?>(null) }
    AndroidView(
        factory = { context ->
            SubtitleView(context).also {
                it.setUserDefaultStyle()
                view = it
            }
        },
        update = {
            it.setFractionalTextSize(frame.text)
            it.setBottomPaddingFraction(frame.bottom)
        },
        modifier = modifier,
    )
    DisposableEffect(player, view) {
        val target = view
        val listener =
            object : Player.Listener {
                override fun onCues(cueGroup: CueGroup) {
                    target?.setCues(cueGroup.cues)
                }
            }
        target?.setCues(player.currentCues.cues)
        player.addListener(listener)
        onDispose { player.removeListener(listener) }
    }
}

/** The system caption text size, 1 when captions are off in the system settings (as SubtitleView reads it). */
private fun captionScale(context: Context): Float {
    val captions = context.getSystemService(CaptioningManager::class.java) ?: return 1f
    return if (captions.isEnabled) captions.fontScale else 1f
}

/** The subtitles' area, for tests of where they sit. */
const val SUBTITLES_TAG = "subtitles"
