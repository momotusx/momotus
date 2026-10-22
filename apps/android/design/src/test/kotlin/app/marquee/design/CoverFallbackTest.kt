package app.marquee.design

import android.graphics.Bitmap
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
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
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.assertEquals

/**
 * A cover RPDB does not know answers 404: the tile then shows the addon's own poster, and only when that fails
 * too does the title's name stand in.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class CoverFallbackTest {
    @get:Rule
    val compose = createComposeRule()

    private val asked = CopyOnWriteArrayList<String>()

    /** Answers every address but those in [missing], which fail as a 404 would. */
    private fun serve(vararg missing: String) {
        val picture = Bitmap.createBitmap(4, 6, Bitmap.Config.ARGB_8888).asImage()
        val factory =
            Fetcher.Factory<Uri> { data, _: Options, _: ImageLoader ->
                Fetcher {
                    asked += data.toString()
                    if (data.toString() in missing) error("HTTP 404")
                    ImageFetchResult(picture, isSampled = false, dataSource = DataSource.NETWORK) as FetchResult
                }
            }
        SingletonImageLoader.setUnsafe(
            ImageLoader.Builder(RuntimeEnvironment.getApplication()).components { add(factory) }.build(),
        )
    }

    @After
    fun reset() = SingletonImageLoader.reset()

    private fun show() =
        compose.setContent {
            MarqueeTheme(Form.Phone) { PosterTile("Inception", RPDB, {}, fallbackImageUrl = ADDON, captioned = false) }
        }

    @Test
    fun `a cover RPDB does not know gives way to the addon's poster`() {
        serve(RPDB)
        show()
        compose.waitUntil(5_000) { ADDON in asked }
        compose.waitForIdle()
        assertEquals(listOf(RPDB, ADDON), asked.toList())
        assertEquals(0, compose.onAllNodesWithText("Inception").fetchSemanticsNodes().size, "the name stood in")
    }

    @Test
    fun `when the addon's poster fails too, the name stands in`() {
        serve(RPDB, ADDON)
        show()
        compose.waitUntil(5_000) { compose.onAllNodesWithText("Inception").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(listOf(RPDB, ADDON), asked.toList())
    }

    private companion object {
        const val RPDB = "https://api.ratingposterdb.com/t0-free-rpdb/imdb/poster-default/tt0.jpg"
        const val ADDON = "https://images.metahub.space/poster/medium/tt0/img"
    }
}
