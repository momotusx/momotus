package app.marquee.design

import android.app.ActivityManager
import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.Dp
import coil3.ImageLoader
import coil3.compose.AsyncImage
import coil3.disk.DiskCache
import coil3.intercept.Interceptor
import coil3.memory.MemoryCache
import coil3.network.okhttp.OkHttpNetworkFetcherFactory
import coil3.request.ImageRequest
import coil3.request.ImageResult
import coil3.request.crossfade
import coil3.size.Size
import okhttp3.Dispatcher
import okhttp3.OkHttpClient
import okio.Path.Companion.toOkioPath

/** A row of posters opens many connections to one host at once; OkHttp's default is 5 per host. */
private val posterClient: OkHttpClient by lazy {
    OkHttpClient
        .Builder()
        .dispatcher(Dispatcher().apply { maxRequestsPerHost = 16 })
        .build()
}

/**
 * One image loader for the app, sized for the device. What goes wrong on weak TVs is memory and
 * decode work, so: the memory cache is a share of RAM (smaller on low-RAM devices), every request
 * has an exact pixel size, and there is no crossfade.
 */
fun createImageLoader(context: Context): ImageLoader {
    val lowRam = (context.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager).isLowRamDevice
    return ImageLoader
        .Builder(context)
        .memoryCache { MemoryCache.Builder().maxSizePercent(context, if (lowRam) 0.08 else 0.15).build() }
        .diskCache {
            DiskCache
                .Builder()
                .directory(context.cacheDir.resolve("images").toOkioPath())
                .maxSizeBytes(if (lowRam) 64L * 1024 * 1024 else 128L * 1024 * 1024)
                .build()
        }.components {
            add(HttpsUpgrade)
            add(OkHttpNetworkFetcherFactory(callFactory = { posterClient }))
        }.crossfade(false)
        .build()
}

/**
 * Posters from some addons come as `http://`, which the app's network security config refuses
 * (cleartext is allowed for loopback only). Try `https://` for them; a host without TLS shows the
 * title text instead of the picture.
 */
internal object HttpsUpgrade : Interceptor {
    override suspend fun intercept(chain: Interceptor.Chain): ImageResult {
        val data = chain.request.data
        val upgraded = (data as? String)?.let(::upgrade)
        if (upgraded == null || upgraded == data) return chain.proceed()
        return chain
            .withRequest(
                chain.request
                    .newBuilder()
                    .data(upgraded)
                    .build(),
            ).proceed()
    }

    fun upgrade(url: String): String =
        if (url.startsWith("http://") &&
            !url.startsWith("http://127.0.0.1")
        ) {
            "https://" + url.removePrefix("http://")
        } else {
            url
        }
}

/**
 * A picture at an exact size, with the title text behind it: while the image loads, or if it
 * cannot (no network, a host without TLS), the text stays readable.
 * [fallbackUrl] is tried when [url] cannot load (an episode still not made yet: the poster).
 */
@Composable
fun RemoteImage(
    url: String?,
    width: Dp,
    height: Dp,
    fallbackText: String,
    modifier: Modifier = Modifier,
    fallbackUrl: String? = null,
    /** Where the crop keeps the picture: a poster in a 16:9 slot keeps its upper part (design/DECISION.md, Shelves). */
    alignment: Alignment = Alignment.Center,
    overlay: @Composable BoxScope.() -> Unit = {},
) {
    val context = LocalContext.current
    val density = LocalDensity.current
    val widthPx = with(density) { width.roundToPx() }
    val heightPx = with(density) { height.roundToPx() }
    var tried by remember(url, fallbackUrl) { mutableIntStateOf(0) }
    val current = listOfNotNull(url, fallbackUrl).getOrNull(tried)
    val failed = current == null
    // The surface fill is what a tile shows while the picture loads; the title takes over when
    // there is no picture or it cannot load.
    Box(modifier.size(width, height).background(Marquee.colors.surface)) {
        if (failed) {
            MqText(
                fallbackText,
                role = TextRole.Callout,
                color = Marquee.colors.muted,
                modifier = Modifier.align(Alignment.Center).padding(Marquee.space.xs),
                maxLines = 4,
            )
        }
        if (current != null) {
            val request =
                remember(current, widthPx, heightPx) {
                    ImageRequest
                        .Builder(context)
                        .data(current)
                        .size(Size(widthPx, heightPx))
                        .memoryCacheKey(imageKey(current, widthPx, heightPx))
                        .build()
                }
            AsyncImage(
                model = request,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                alignment = alignment,
                onError = { tried++ },
                modifier = Modifier.size(width, height),
            )
        }
        overlay()
    }
}

/** A stop of a scrim over art: [at] along it (0 at the edge it starts from), [alpha] of the page color there. */
@Immutable
data class ScrimStop(
    val at: Float,
    val alpha: Float,
)

/**
 * The backdrops' scrims (design/DECISION.md, Backdrops and scrims), the one source for every hero and for
 * the check over white art in DesignTest: a secondary's white label on its 16 percent fill keeps
 * 4.5:1 wherever a secondary row sits on them.
 */
object Scrims {
    /**
     * TV Home's top shelf, the whole TV: Apple's layered ramps (a wash from the leading edge, the bottom always
     * ending on the page), deepened by the check (DesignTest) until every word of the left column keeps 4.5:1
     * over white art: the frame's own stops (v2.html, `.shade`) left the synopsis at 1.2:1 and Details at 2.7:1.
     */
    val topShelfSide =
        listOf(ScrimStop(0f, 0.94f), ScrimStop(0.48f, 0.82f), ScrimStop(0.64f, 0.3f), ScrimStop(0.8f, 0f))
    val topShelfFoot =
        listOf(
            ScrimStop(0f, 1f),
            ScrimStop(0.05f, 1f),
            ScrimStop(0.14f, 0.8f),
            ScrimStop(0.3f, 0.45f),
            ScrimStop(0.5f, 0.12f),
            ScrimStop(0.62f, 0f),
        )

    /** The TV title page: dark over the text column's half, and the art fading into the page below. */
    val titleSide = listOf(ScrimStop(0f, 0.94f), ScrimStop(0.5f, 0.8f), ScrimStop(0.75f, 0.35f), ScrimStop(1f, 0f))
    val titleFoot = listOf(ScrimStop(0f, 1f), ScrimStop(0.4f, 0f))

    /**
     * The phone's Home hero, the iPhone's 390 by 680 (the home-phone frame): the art fades into the black page
     * from the foot, and a light wash at the top keeps the status bar and the large title readable. The foot is
     * deepened by the check (DesignTest): on the frame's stops the ratings row's source read 3.6:1 over white art.
     */
    val phoneHomeFoot =
        listOf(
            ScrimStop(0f, 1f),
            ScrimStop(0.12f, 0.94f),
            ScrimStop(0.3f, 0.8f),
            ScrimStop(0.42f, 0.5f),
            ScrimStop(0.56f, 0.15f),
            ScrimStop(0.66f, 0f),
        )
    val phoneHomeTop = listOf(ScrimStop(0f, 0.7f), ScrimStop(0.16f, 0.45f), ScrimStop(0.26f, 0f))

    /** The phone title's art: under the status bar from the top, and toward the logo from the bottom. */
    val phoneTitleTop = listOf(ScrimStop(0f, 0.5f), ScrimStop(0.25f, 0f))
    val phoneTitleFoot = listOf(ScrimStop(0f, 0.6f), ScrimStop(0.5f, 0f))

    /** How much page color covers the art at [t] along [stops], linear between them. */
    fun alpha(
        stops: List<ScrimStop>,
        t: Float,
    ): Float {
        if (stops.isEmpty()) return 0f
        if (t <= stops.first().at) return stops.first().alpha
        if (t >= stops.last().at) return stops.last().alpha
        val i = stops.indexOfFirst { it.at >= t }
        val (a, b) = stops[i - 1] to stops[i]
        return a.alpha + (b.alpha - a.alpha) * (t - a.at) / (b.at - a.at)
    }
}

/**
 * A wide picture that fills its bounds, for hero artwork, under its [Scrims] in the page color: [side] from
 * the leading edge, [foot] from the bottom up, [top] from the top down. Coil takes the request size from the
 * layout, so one image of known place needs no size here.
 */
@Composable
fun Backdrop(
    url: String?,
    modifier: Modifier = Modifier,
    side: List<ScrimStop> = emptyList(),
    foot: List<ScrimStop> = emptyList(),
    top: List<ScrimStop> = emptyList(),
    /** Where the crop keeps the picture: a poster standing in for a backdrop keeps its upper part. */
    alignment: Alignment = Alignment.Center,
    /** The picture did not load (a cover the page gives up for its backdrop). */
    onError: (() -> Unit)? = null,
) {
    val page = Marquee.colors.bg
    val scrims =
        Modifier.drawWithCache {
            fun stops(list: List<ScrimStop>) = list.map { it.at to page.copy(alpha = it.alpha) }.toTypedArray()
            val brushes =
                listOfNotNull(
                    side.takeIf { it.isNotEmpty() }?.let { Brush.horizontalGradient(*stops(it)) },
                    foot.takeIf { it.isNotEmpty() }?.let {
                        Brush.verticalGradient(*stops(it.map { s -> ScrimStop(1f - s.at, s.alpha) }.reversed()))
                    },
                    top.takeIf { it.isNotEmpty() }?.let { Brush.verticalGradient(*stops(it)) },
                )
            onDrawWithContent {
                drawContent()
                brushes.forEach { drawRect(it) }
            }
        }
    Box(modifier.background(Marquee.colors.surface).then(scrims)) {
        if (url != null) {
            AsyncImage(
                model = url,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                alignment = alignment,
                modifier = Modifier.matchParentSize(),
                onError = { onError?.invoke() },
            )
        }
    }
}

/**
 * A title's logo, left aligned in a box of [height] and at most [maxWidth] wide; the name as text
 * (in [role]) when there is no logo or it cannot load.
 *
 * @param decorative the name is already spoken by what the logo sits in (a tile), so a screen
 * reader skips the logo.
 */
@Composable
fun Logo(
    url: String?,
    name: String,
    height: Dp,
    maxWidth: Dp,
    modifier: Modifier = Modifier,
    role: TextRole = TextRole.Hero,
    color: Color = Marquee.colors.text,
    decorative: Boolean = false,
) {
    var failed by remember(url) { mutableStateOf(url == null) }
    if (failed) {
        val spoken = if (decorative) Modifier.clearAndSetSemantics { } else Modifier
        MqText(name, modifier = modifier.then(spoken).widthIn(max = maxWidth), role = role, color = color, maxLines = 2)
    } else {
        AsyncImage(
            model = url,
            contentDescription = if (decorative) null else name,
            contentScale = ContentScale.Fit,
            alignment = Alignment.CenterStart,
            onError = { failed = true },
            modifier = modifier.height(height).widthIn(max = maxWidth),
        )
    }
}

/** The memory cache key of a sized picture: a prefetch must use the same key the tile asks with. */
fun imageKey(
    url: String,
    widthPx: Int,
    heightPx: Int,
): String = "$url@${widthPx}x$heightPx"
