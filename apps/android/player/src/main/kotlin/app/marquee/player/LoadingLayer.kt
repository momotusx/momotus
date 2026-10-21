package app.marquee.player

import android.graphics.Bitmap
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.ProgressBarRangeInfo
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.progressBarRangeInfo
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import app.marquee.design.Form
import app.marquee.design.LocalReducedMotion
import app.marquee.design.Marquee
import app.marquee.design.MarqueeFonts
import app.marquee.design.MqText
import app.marquee.design.TextRole
import coil3.SingletonImageLoader
import coil3.compose.AsyncImage
import coil3.request.ImageRequest
import coil3.request.SuccessResult
import coil3.request.allowHardware
import coil3.toBitmap
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * The loading screen (plans/player-polish.md 1.2, design/DECISION.md "Loading"): until the first frame the title's
 * [backdrop] at 30 percent with a light blur over black; on a later stall no backdrop, so the paused frame shows.
 * The title's [logo] (trimmed, [rememberTrimmedLogo]) fills from a faint copy to solid as [fill] goes from 0 to 1,
 * or the [title] in display type when there is none; under it the core's [lines], small and quiet. A gentle pulse,
 * none under reduced motion. Nothing here takes focus: on a TV the remote's Back cancels.
 */
@Composable
fun LoadingLayer(
    title: String,
    logo: ImageBitmap?,
    backdrop: String?,
    fill: Float,
    lines: List<String>,
    modifier: Modifier = Modifier,
    /** Until the first frame: the black stage under the backdrop. False over a paused frame. */
    opening: Boolean = true,
) {
    val tv = Marquee.form == Form.Tv
    val reduced = LocalReducedMotion.current
    val shown by animateFloatAsState(fill.coerceIn(0f, 1f), tween(if (reduced) 0 else FILL_EASE_MS), label = "fill")
    Box(
        modifier
            .fillMaxSize()
            .then(if (opening) Modifier.background(Color.Black) else Modifier)
            .testTag(LOADING_TAG)
            .semantics {
                contentDescription = "Loading $title"
                progressBarRangeInfo = ProgressBarRangeInfo(shown, 0f..1f)
            },
        contentAlignment = Alignment.Center,
    ) {
        if (opening && backdrop != null) {
            AsyncImage(
                model = backdrop,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier =
                    Modifier
                        .fillMaxSize()
                        .graphicsLayer {
                            alpha = BACKDROP_ALPHA
                            // The blur's soft edge stays off screen.
                            scaleX = 1.06f
                            scaleY = 1.06f
                        }.blur(if (tv) 12.dp else 8.dp),
            )
        }
        BoxWithConstraints(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
            // About 40 percent of the width on TV and 70 on the phone, held by its height when stacked (the frames).
            val wide = maxWidth * (if (tv) 0.4f else 0.7f)
            val tall = maxHeight * (if (tv) 0.45f else 0.6f)
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(if (tv) 40.dp else 16.dp),
            ) {
                Pulse(enabled = !reduced) {
                    Filled(shown) {
                        if (logo != null) {
                            val k = minOf(wide.value / logo.width, tall.value / logo.height)
                            Image(
                                logo,
                                contentDescription = null,
                                modifier =
                                    Modifier.size(
                                        (logo.width * k).dp,
                                        (
                                            logo.height *
                                                k
                                        ).dp,
                                    ),
                            )
                        } else {
                            BasicText(
                                title,
                                Modifier.widthIn(max = wide),
                                style =
                                    TextStyle(
                                        color = Color.White,
                                        fontSize = if (tv) 120.sp else 56.sp,
                                        lineHeight = 1.1.em,
                                        fontFamily = MarqueeFonts.display,
                                        fontWeight = FontWeight.Bold,
                                        letterSpacing = (-0.02).em,
                                        textAlign = TextAlign.Center,
                                    ),
                                maxLines = 3,
                            )
                        }
                    }
                }
                Column(
                    Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    lines.forEach {
                        MqText(it, role = TextRole.Caption, color = Marquee.colors.muted, textAlign = TextAlign.Center)
                    }
                }
            }
        }
    }
}

/** The faint copy of [content] and over it the solid copy revealed from the left to [fill], its edge feathered. */
@Composable
private fun Filled(
    fill: Float,
    content: @Composable () -> Unit,
) {
    // The layer says the title once, in its own words: the two copies are only for the eye.
    Box(Modifier.clearAndSetSemantics { }) {
        Box(Modifier.graphicsLayer { alpha = FAINT }) { content() }
        Box(
            Modifier
                .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
                .drawWithContent {
                    drawContent()
                    val (from, to) = revealEdge(fill)
                    drawRect(
                        Brush.horizontalGradient(
                            listOf(Color.Black, Color.Transparent),
                            startX = size.width * from,
                            endX = size.width * to,
                        ),
                        blendMode = BlendMode.DstIn,
                    )
                },
        ) { content() }
    }
}

/** A slow breath of the mark while it waits: opacity and a small scale, two seconds each way. */
@Composable
private fun Pulse(
    enabled: Boolean,
    content: @Composable () -> Unit,
) {
    if (!enabled) return content()
    val breath = rememberInfiniteTransition(label = "pulse")
    val t by breath.animateFloat(0f, 1f, infiniteRepeatable(tween(PULSE_MS), RepeatMode.Reverse), label = "breath")
    Box(
        Modifier.graphicsLayer {
            alpha = 1f - 0.15f * t
            scaleX = 1f + 0.03f * t
            scaleY = 1f + 0.03f * t
        },
    ) { content() }
}

/**
 * Where the solid copy's edge runs, as shares of the mark's width: solid to the first, clear from the second, the
 * feather 6 percent wide between them; at 0 nothing shows, at 1 all of it (the frames' `p * 106 - 6`).
 */
fun revealEdge(fill: Float): Pair<Float, Float> {
    val from = fill.coerceIn(0f, 1f) * (1f + FEATHER) - FEATHER
    return from to from + FEATHER
}

/** The lines under the mark: what the player says now (the pick's "Starting ..."), then the core's words on a long wait. */
fun loadingLines(
    status: String?,
    caption: String?,
): List<String> = listOfNotNull(status, caption?.takeIf { it != status })

/**
 * The fill once the player has the URL (plans/core-protocol.md, `start`): from where the core's fill got ([base]) to
 * full as the player's buffer ahead of the playhead reaches its start threshold, as Stremio fills a link from the
 * player's own buffer.
 */
fun bufferFill(
    base: Float,
    aheadMs: Long,
    thresholdMs: Long,
): Float = base + (1f - base) * (aheadMs.toFloat() / thresholdMs).coerceIn(0f, 1f)

/** The drawn box of a logo inside its transparent margins: the rows and columns with a pixel over [alpha]; null if none. */
fun drawnBox(
    pixels: IntArray,
    width: Int,
    height: Int,
    alpha: Int = 16,
): IntRect? {
    var left = width
    var top = height
    var right = -1
    var bottom = -1
    for (y in 0 until height) {
        for (x in 0 until width) {
            if (pixels[y * width + x] ushr 24 > alpha) {
                if (x < left) left = x
                if (x > right) right = x
                if (y < top) top = y
                bottom = y
            }
        }
    }
    return if (right < 0) null else IntRect(left, top, right + 1, bottom + 1)
}

/**
 * The title's logo at [url], trimmed to its drawn box (logos come with wide transparent margins, Apple trims them
 * too); null while it loads, and for no logo or one that does not load, so the name shows instead.
 */
@Composable
fun rememberTrimmedLogo(url: String?): ImageBitmap? {
    val context = LocalContext.current
    val logo by produceState<ImageBitmap?>(null, url) {
        value = null
        if (url == null) return@produceState
        val request =
            ImageRequest
                .Builder(context)
                .data(url)
                .size(LOGO_PX)
                .allowHardware(false)
                .build()
        val result = SingletonImageLoader.get(context).execute(request) as? SuccessResult ?: return@produceState
        value = withContext(Dispatchers.Default) { trimmed(result.image.toBitmap()) }
    }
    return logo
}

private fun trimmed(bitmap: Bitmap): ImageBitmap? {
    val pixels = IntArray(bitmap.width * bitmap.height)
    bitmap.getPixels(pixels, 0, bitmap.width, 0, 0, bitmap.width, bitmap.height)
    val box = drawnBox(pixels, bitmap.width, bitmap.height) ?: return null
    return Bitmap.createBitmap(bitmap, box.left, box.top, box.width, box.height).asImageBitmap()
}

/** The loading layer, for tests. */
const val LOADING_TAG = "player-loading"

private const val BACKDROP_ALPHA = 0.3f
private const val FAINT = 0.2f
private const val FEATHER = 0.06f
private const val FILL_EASE_MS = 900
private const val PULSE_MS = 2_000
private const val LOGO_PX = 800
