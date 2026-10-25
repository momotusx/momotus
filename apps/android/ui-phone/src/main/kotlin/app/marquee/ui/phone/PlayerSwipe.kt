package app.marquee.ui.phone

import android.media.AudioManager
import android.os.Build
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.AwaitPointerEventScope
import androidx.compose.ui.input.pointer.PointerId
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.toSize
import app.marquee.core.client.CoreLog
import app.marquee.design.Icons
import app.marquee.design.MqIcon
import kotlin.math.abs
import kotlin.math.roundToInt

/** Which level a vertical swipe on the picture sets: the left half the brightness, the right half the volume. */
internal enum class SwipeSide { Brightness, Volume }

/** The bands along the window's edges, in px, where a touch is the system's (its back gesture, its bars). */
internal data class EdgeBands(
    val left: Float,
    val top: Float,
    val right: Float,
    val bottom: Float,
)

/** The side a swipe starting at [start] sets, by the start point alone; null in an edge band. */
internal fun swipeSide(
    start: Offset,
    size: Size,
    bands: EdgeBands,
): SwipeSide? =
    when {
        start.x < bands.left || start.x > size.width - bands.right -> null
        start.y < bands.top || start.y > size.height - bands.bottom -> null
        start.x < size.width / 2 -> SwipeSide.Brightness
        else -> SwipeSide.Volume
    }

/**
 * Whether a touch that has travelled ([dx], [dy]) is a swipe: true once it has gone [threshold] up or down at
 * least twice as far as across (VLC's 2:1), false once it has gone [threshold] across first, null while undecided.
 */
internal fun commits(
    dx: Float,
    dy: Float,
    threshold: Float,
): Boolean? =
    when {
        abs(dy) >= threshold && abs(dy) >= 2 * abs(dx) -> true
        abs(dx) >= threshold -> false
        else -> null
    }

/** The level after a drag of [dy] from the commit point: 80 percent of [height] sweeps the whole range. */
internal fun level(
    start: Float,
    dy: Float,
    height: Float,
): Float = (start - dy / (SWEEP * height)).coerceIn(0f, 1f)

internal fun volumeIndex(
    level: Float,
    min: Int,
    max: Int,
): Int = (min + level * (max - min)).roundToInt()

internal fun indexLevel(
    index: Int,
    min: Int,
    max: Int,
): Float = if (max <= min) 0f else ((index - min).toFloat() / (max - min)).coerceIn(0f, 1f)

/** A swipe just reached either end: the one moment for a haptic tick. */
internal fun reachedEnd(
    before: Float,
    after: Float,
): Boolean = before != after && (after == 0f || after == 1f)

/** The media volume as the right-hand swipe sets it, with no system panel over ours. */
internal class MediaVolume(
    private val audio: AudioManager,
) {
    private val min = if (Build.VERSION.SDK_INT >= 28) audio.getStreamMinVolume(STREAM) else 0
    private val max = audio.getStreamMaxVolume(STREAM)

    /** Where a swipe starts; none on a device whose volume is fixed. */
    fun start(): Float? = if (audio.isVolumeFixed) null else level()

    fun level(): Float = indexLevel(audio.getStreamVolume(STREAM), min, max)

    /** Sets [to] and answers the level the device took, which a safe-headphone limit can hold lower. */
    fun set(to: Float): Float {
        audio.setStreamVolume(STREAM, volumeIndex(to, min, max), 0)
        return level()
    }

    private companion object {
        const val STREAM = AudioManager.STREAM_MUSIC
    }
}

/**
 * Vertical swipes on the picture (plans/player-gestures.md 3.1 and 3.2). A touch that starts in an edge band or from a
 * mouse is left alone; one that turns sideways first, meets a second finger or has its moves taken by something else
 * before it commits stays a tap. Once committed it takes every move, so no tap follows, and a second finger ends it
 * where it is.
 *
 * It goes on a layer of its own just over the picture, never on the player's Box beside its tap detector: the
 * controls drawn above that layer are hit first and never pass a touch to it, while on the Box the down arrives
 * already consumed (`detectTapGestures` and the video's interop view take it), so a swipe could not tell the picture
 * from a control.
 *
 * @param onStart the level the swipe starts from, or null when this side has none (a fixed volume).
 * @param onLevel each new level, from the start plus the drag.
 */
internal fun Modifier.levelSwipes(
    enabled: Boolean,
    bands: () -> EdgeBands,
    onStart: (SwipeSide) -> Float?,
    onLevel: (SwipeSide, Float) -> Unit,
    onEnd: () -> Unit,
): Modifier =
    pointerInput(enabled) {
        if (!enabled) return@pointerInput
        awaitEachGesture {
            val down = awaitFirstDown(requireUnconsumed = true)
            // Once per process, debug builds only (CoreLog's debug lines): what a walk's injected touch is, and the
            // bands it meets.
            if (!touchLogged) {
                touchLogged = true
                CoreLog.d("picture touch ${down.type}, bands ${bands()}")
            }
            if (down.type == PointerType.Mouse) return@awaitEachGesture
            val side = swipeSide(down.position, size.toSize(), bands()) ?: return@awaitEachGesture
            val from = awaitCommit(down.id, down.position, COMMIT.toPx()) ?: return@awaitEachGesture
            val start = onStart(side) ?: return@awaitEachGesture
            // Ended however it ends, a cancel included (the swipes turned off under the finger).
            try {
                while (true) {
                    val event = awaitPointerEvent()
                    val change = event.changes.firstOrNull { it.id == down.id }
                    if (change == null || !change.pressed || event.changes.count { it.pressed } > 1) break
                    change.consume()
                    onLevel(side, level(start, change.position.y - from.y, size.height.toFloat()))
                }
            } finally {
                onEnd()
            }
        }
    }

/** Watches the moves of [id] until they commit (answers where, the move consumed) or the touch stays a tap (null). */
private suspend fun AwaitPointerEventScope.awaitCommit(
    id: PointerId,
    down: Offset,
    threshold: Float,
): Offset? {
    while (true) {
        val event = awaitPointerEvent()
        val change = event.changes.firstOrNull { it.id == id } ?: return null
        if (event.changes.size > 1 || !change.pressed || change.isConsumed) return null
        val travel = change.position - down
        when (commits(travel.x, travel.y, threshold)) {
            true -> {
                change.consume()
                return change.position
            }

            false -> {
                return null
            }

            null -> {
                Unit
            }
        }
    }
}

/**
 * The answer to a swipe: a 44 by 168 capsule at 40 percent black (the double-tap hint's platter), the side's icon and
 * a 4 by 112 bar, white 92 percent over a white 20 percent track (design/DECISION.md, Progress). Only this recomposes
 * as the level moves; it never shows under TalkBack, so its range is there for the tests.
 */
@Composable
internal fun LevelIndicator(
    side: SwipeSide,
    level: () -> Float,
    modifier: Modifier = Modifier,
) {
    val now = level()
    val icon =
        when {
            side == SwipeSide.Brightness -> Icons.Sun
            now == 0f -> Icons.VolumeX
            now < 0.5f -> Icons.Volume1
            else -> Icons.Volume2
        }
    Column(
        modifier
            .size(44.dp, 168.dp)
            .clip(CircleShape)
            .background(Color.Black.copy(alpha = 0.4f))
            .padding(vertical = 12.dp)
            .testTag(LEVEL_TAG)
            .semantics { progressBarRangeInfo = ProgressBarRangeInfo(now, 0f..1f) },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        MqIcon(icon, tint = Color.White, size = 20.dp)
        Box(
            Modifier.size(4.dp, 112.dp).drawBehind {
                val corner = CornerRadius(size.width / 2)
                drawRoundRect(Color.White.copy(alpha = 0.2f), cornerRadius = corner)
                val filled = size.height * level()
                drawRoundRect(
                    Color.White.copy(alpha = 0.92f),
                    topLeft = Offset(0f, size.height - filled),
                    size = Size(size.width, filled),
                    cornerRadius = corner,
                )
            },
        )
    }
}

/** The first touch on the picture has been logged (the walk reads it once). */
private var touchLogged = false

/** The level indicator, for tests. */
internal const val LEVEL_TAG = "player-level"

/** The share of the picture's height a swipe travels for the whole range (VLC's factor 1.25). */
private const val SWEEP = 0.8f

/** How far a touch goes up or down before it is a swipe. */
private val COMMIT = 12.dp
