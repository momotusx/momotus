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
 * motion). Decorative: no semantics, never focusable. [onPoster] hears each tile's first poster, for the walk's log.
 */
@Composable
fun PosterWall(
    posters: List<String>,
    drift: WallDrift,
    sheet: Boolean,
    modifier: Modifier = Modifier,
    onPoster: (() -> Unit)? = null,
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
        val width = maxWidth * 1.56f
        val gap = 14.dp
        val tile = (width - gap * (columns - 1)) / columns
        val loop = with(LocalDensity.current) { ((tile * 1.5f + gap) * PER_COLUMN).toPx() }
        val lower = with(LocalDensity.current) { 160.dp.toPx() }
        Row(
            Modifier
                .requiredSize(width, maxHeight * 1.36f)
                .graphicsLayer { rotationZ = -9f }
                .then(if (blur) Modifier.blur(Tokens.wallBlur) else Modifier),
            horizontalArrangement = Arrangement.spacedBy(gap),
        ) {
            for (c in 0 until columns) {
                val seconds = 70 + (c % 3) * 14
                val down = c % 2 == 1
                Column(
                    Modifier.weight(1f).graphicsLayer {
                        val t = (drift.ms % (seconds * 1000L)) / (seconds * 1000f)
                        translationY = if (down) -loop + t * loop - lower else -t * loop
                    },
                    verticalArrangement = Arrangement.spacedBy(gap),
                ) {
                    val list = column(posters, c)
                    for (i in 0 until PER_COLUMN * 2) {
                        WallTile(list.getOrNull(i % PER_COLUMN), tile, reduced, onPoster.takeIf { i < PER_COLUMN })
                    }
                }
            }
        }
        if (sheet) {
            Box(Modifier.matchParentSize().background(if (blur) Tokens.wallDim else Tokens.wallDimNoBlur))
        }
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
    onPoster: (() -> Unit)?,
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
                if (!landed) onPoster?.invoke()
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
