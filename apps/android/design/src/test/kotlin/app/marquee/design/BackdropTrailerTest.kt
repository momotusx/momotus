package app.marquee.design

import android.graphics.Bitmap
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import coil3.ImageLoader
import coil3.SingletonImageLoader
import coil3.Uri
import coil3.asImage
import coil3.decode.DataSource
import coil3.fetch.FetchResult
import coil3.fetch.Fetcher
import coil3.fetch.ImageFetchResult
import coil3.request.Options
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.assertEquals

/**
 * A title page's cover has the small poster under it until it lands; once the art fades for the header's trailer,
 * the small poster fades with it, so the trailer shows (on 2026-10-30 the Pixel played it behind the poster).
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "mdpi")
class BackdropTrailerTest {
    @get:Rule
    val compose = createComposeRule()

    private val asked = CopyOnWriteArrayList<String>()

    @After
    fun reset() = SingletonImageLoader.reset()

    @Test
    fun `the faded art lets the trailer show through the small poster`() {
        val red = Bitmap.createBitmap(4, 6, Bitmap.Config.ARGB_8888).apply { eraseColor(Color.Red.toArgb()) }.asImage()
        val factory =
            Fetcher.Factory<Uri> { data, _: Options, _: ImageLoader ->
                Fetcher {
                    asked += data.toString()
                    ImageFetchResult(red, isSampled = false, dataSource = DataSource.NETWORK) as FetchResult
                }
            }
        SingletonImageLoader.setUnsafe(
            ImageLoader.Builder(RuntimeEnvironment.getApplication()).components { add(factory) }.build(),
        )
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                Backdrop(
                    null,
                    Modifier.size(40.dp, 60.dp).testTag("art"),
                    under = POSTER,
                    artAlpha = { 0f },
                    trailer = { Box(Modifier.matchParentSize().background(Color.Blue)) },
                )
            }
        }
        compose.waitUntil(5_000) { POSTER in asked }
        compose.waitForIdle()
        assertEquals(Color.Blue, compose.onNodeWithTag("art").captureToImage().toPixelMap()[20, 30])
    }

    private companion object {
        const val POSTER = "https://images.metahub.space/poster/small/tt0/img"
    }
}
