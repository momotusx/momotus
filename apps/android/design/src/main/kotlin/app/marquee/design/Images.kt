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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
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
                onError = { tried++ },
                modifier = Modifier.size(width, height),
            )
        }
        overlay()
    }
}

/**
 * A wide picture that fills its bounds, for hero artwork. Coil takes the request size from the
 * layout, so one image of known place needs no size here.
 */
@Composable
fun Backdrop(
    url: String?,
    modifier: Modifier = Modifier,
) {
    Box(modifier.background(Marquee.colors.surface)) {
        if (url != null) {
            AsyncImage(
                model = url,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.matchParentSize(),
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
