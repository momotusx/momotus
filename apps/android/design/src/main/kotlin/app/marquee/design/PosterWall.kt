package app.marquee.design

import android.os.Build
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.currentStateAsState
import coil3.compose.AsyncImage
import coil3.request.ImageRequest
import coil3.size.Size
import kotlin.math.cos
import kotlin.math.sin

/**
 * The wall's drift, kept for the app run (plans/first-screens.md 3.1): going back to Welcome resumes where it stopped.
 * [paused] is Welcome's "Pause motion"; [ms] the drift's time so far, advanced only while the wall moves.
 */
@Stable
class WallDrift {
    var paused by mutableStateOf(false)
    var ms by mutableLongStateOf(0L)
        internal set
}

/**
 * First run's poster wall (round2.html combo A): columns of 2:3 posters (phone 4, TV 9) turned -9 degrees, each its
 * list twice, odd columns drifting up and even ones down, 160 lower, 70 to 98 s a loop. It drifts only on Welcome
 * ([sheet] false), never with reduced motion, while [WallDrift.paused] or while the app is not resumed. Under a
 * sheet it holds still under a 62 percent dim and an 8 dp blur, or a 72 percent dim where blur is missing (below
 * API 31). A tile with no poster yet is the empty rounded rectangle; posters fade in by opacity (none with reduced
 * motion). Decorative: no semantics, never focusable. [onPoster] hears each poster that lands, for the walk's log.
 */
@Composable
fun PosterWall(
    posters: List<String>,
    drift: WallDrift,
    sheet: Boolean,
    modifier: Modifier = Modifier,
    onPoster: ((String) -> Unit)? = null,
) {
    val reduced = LocalReducedMotion.current
    val resumed by LocalLifecycleOwner.current.lifecycle.currentStateAsState()
    val runs = wallRuns(sheet, drift.paused, reduced, resumed.isAtLeast(Lifecycle.State.RESUMED))
    if (runs) {
        LaunchedEffect(drift) {
            var last = -1L
            while (true) {
                withFrameNanos { now ->
                    if (last >= 0) drift.ms += (now - last) / 1_000_000
                    last = now
                }
            }
        }
    }
    val blur = sheet && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
    val columns = wallColumns(Marquee.form)
    BoxWithConstraints(
        modifier
            .testTag(WALL)
            .fillMaxSize()
            .background(Tokens.dark.bgPhone)
            .clipToBounds()
            .clearAndSetSemantics { },
        contentAlignment = Alignment.Center,
    ) {
        val wall = WallGeometry(maxWidth.value, maxHeight.value, columns)
        Row(
            Modifier
                .requiredSize(wall.width.dp, wall.height.dp)
                .graphicsLayer { rotationZ = WallGeometry.DEGREES }
                .then(if (blur) Modifier.blur(Tokens.wallBlur) else Modifier),
            horizontalArrangement = Arrangement.spacedBy(WallGeometry.GAP.dp),
        ) {
            for (c in 0 until columns) {
                // The column is its list twice, far taller than the wall: measured at its own height, or the tiles
                // past the wall's foot get no room and the drift slides empty space into view.
                Column(
                    Modifier
                        .weight(1f)
                        .wrapContentHeight(Alignment.Top, unbounded = true)
                        .graphicsLayer { translationY = wall.offset(c, drift.ms).dp.toPx() },
                    verticalArrangement = Arrangement.spacedBy(WallGeometry.GAP.dp),
                ) {
                    val list = column(posters, c)
                    for (i in 0 until PER_COLUMN * 2) {
                        WallTile(list.getOrNull(i % PER_COLUMN), wall.tile.dp, reduced, onPoster)
                    }
                }
            }
        }
        if (sheet) {
            Box(Modifier.matchParentSize().background(if (blur) Tokens.wallDim else Tokens.wallDimNoBlur))
        }
    }
}

/**
 * The wall's geometry in dp on a [w] by [h] screen (round2.html:171 to :176): a box 28 percent wider than the screen
 * on each side and 18 percent taller, turned -9 degrees about the screen's centre, of [columns] columns of 2:3 tiles
 * 14 apart; each column holds its list twice, odd columns run down and start 160 higher, 70, 84 or 98 s a loop.
 */
class WallGeometry(
    val w: Float,
    val h: Float,
    val columns: Int,
) {
    val width = w * 1.56f
    val height = h * 1.36f
    val tile = (width - GAP * (columns - 1)) / columns
    private val step = tile * 1.5f + GAP

    /** One list's height: the drift's loop. */
    val loop = step * PER_COLUMN

    /** Column [c]'s offset after [ms] of drift. */
    fun offset(
        c: Int,
        ms: Long,
    ): Float {
        val period = (70 + (c % 3) * 14) * 1000L
        val t = (ms % period) / period.toFloat()
        return if (c % 2 == 1) -loop + t * loop - LIFT else -t * loop
    }

    /** The tiles whose centres are on screen after [ms], as (column, row): what the walk counts as visible. */
    fun visible(ms: Long): List<Pair<Int, Int>> {
        val angle = Math.toRadians(DEGREES.toDouble())
        val (ca, sa) = cos(angle) to sin(angle)
        val left = -(width - w) / 2
        val top = -(height - h) / 2
        return (0 until columns).flatMap { c ->
            val dx = left + c * (tile + GAP) + tile / 2 - w / 2
            (0 until PER_COLUMN * 2).mapNotNull { r ->
                val dy = top + offset(c, ms) + r * step + tile * 0.75f - h / 2
                val x = w / 2 + dx * ca - dy * sa
                val y = h / 2 + dx * sa + dy * ca
                (c to r).takeIf { x in 0.0..w.toDouble() && y in 0.0..h.toDouble() }
            }
        }
    }

    /** The walk line's `<filled>/<visible>` after [ms]: visible tiles whose poster has [landed], over visible tiles. */
    fun filled(
        ms: Long,
        posters: List<String>,
        landed: Set<String>,
    ): Pair<Int, Int> {
        val shown = visible(ms)
        val filled = shown.count { (c, r) -> column(posters, c).getOrNull(r % PER_COLUMN) in landed }
        return filled to shown.size
    }

    companion object {
        const val GAP = 14f
        const val LIFT = 160f
        const val DEGREES = -9f
    }
}

/** The wall's columns: four on the phone, nine on TV (R1 may drop TV to six). */
fun wallColumns(form: Form): Int = if (form == Form.Tv) TV_COLUMNS else PHONE_COLUMNS

/** Whether the wall drifts: on Welcome only, unpaused, without reduced motion, while the app is in front. */
fun wallRuns(
    sheet: Boolean,
    paused: Boolean,
    reduced: Boolean,
    resumed: Boolean,
): Boolean = !sheet && !paused && !reduced && resumed

/** Column [c]'s six posters: the list turned by four per column, as the design deals them; none for an empty list. */
fun column(
    posters: List<String>,
    c: Int,
): List<String> {
    if (posters.isEmpty()) return emptyList()
    val from = (c * 4) % posters.size
    return (posters.drop(from) + posters.take(from)).let { turned -> List(PER_COLUMN) { turned[it % turned.size] } }
}

@Composable
private fun WallTile(
    url: String?,
    width: androidx.compose.ui.unit.Dp,
    reduced: Boolean,
    onPoster: ((String) -> Unit)?,
) {
    val shape = RoundedCornerShape(12.dp)
    Box(
        Modifier
            .fillMaxWidth()
            .aspectRatio(2f / 3f)
            .clip(shape)
            .background(Tokens.dark.surfacePhone),
    ) {
        if (url == null) return@Box
        var landed by remember(url) { mutableStateOf(false) }
        val alpha by animateFloatAsState(
            if (landed) 1f else 0f,
            tween(if (reduced) 0 else Tokens.PANEL_MS),
            label = "poster",
        )
        val context = LocalContext.current
        val px = with(LocalDensity.current) { width.roundToPx() }
        val request =
            remember(url, px) {
                ImageRequest
                    .Builder(context)
                    .data(url)
                    .size(Size(px, px * 3 / 2))
                    .memoryCacheKey(imageKey(url, px, px * 3 / 2))
                    .build()
            }
        AsyncImage(
            model = request,
            contentDescription = null,
            contentScale = ContentScale.Crop,
            onSuccess = {
                if (!landed) onPoster?.invoke(url)
                landed = true
            },
            modifier = Modifier.matchParentSize().graphicsLayer { this.alpha = alpha },
        )
    }
}

private const val PHONE_COLUMNS = 4
private const val TV_COLUMNS = 9

/** Posters per column; the column holds them twice, so one loop drifts by one list. */
private const val PER_COLUMN = 6

/** The wall, for tests. */
const val WALL = "poster-wall"
