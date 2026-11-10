package app.marquee.design

import android.app.Application
import android.graphics.Bitmap
import android.provider.Settings
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
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
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * First run's poster wall (plans/first-screens.md 3.1): four columns on the phone and nine on TV, each column six
 * posters dealt from the list; empty tiles and no image request for an empty list; the drift runs on Welcome only and
 * never under a sheet, while paused, with the system's animations off or with the walk's reduceMotion extra; and the
 * wall says nothing to assistive tech.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class FirstRunWallTest {
    @get:Rule
    val compose = createComposeRule()

    private val asked = AtomicInteger()

    @After
    fun reset() {
        SingletonImageLoader.reset()
        launchReducedMotion = false
        animators(1f)
    }

    private fun animators(scale: Float) =
        Settings.Global.putFloat(
            RuntimeEnvironment.getApplication().contentResolver,
            Settings.Global.ANIMATOR_DURATION_SCALE,
            scale,
        )

    /** Every poster is a white picture; each request is counted. */
    private fun serve() {
        val image =
            Bitmap
                .createBitmap(20, 30, Bitmap.Config.ARGB_8888)
                .apply { eraseColor(android.graphics.Color.WHITE) }
                .asImage()
        val factory =
            Fetcher.Factory<Uri> { _, _: Options, _: ImageLoader ->
                Fetcher {
                    asked.incrementAndGet()
                    ImageFetchResult(image, isSampled = false, dataSource = DataSource.NETWORK) as FetchResult
                }
            }
        SingletonImageLoader.setUnsafe(
            ImageLoader.Builder(RuntimeEnvironment.getApplication()).components { add(factory) }.build(),
        )
    }

    private val posters = List(30) { "https://art.example/p$it.jpg" }

    /** The drift after [ms] of frames, on a wall drawn with [sheet] and [paused]. */
    private fun drifted(
        form: Form = Form.Phone,
        sheet: Boolean = false,
        paused: Boolean = false,
        ms: Long = 2_000,
    ): Long {
        val drift = WallDrift().apply { this.paused = paused }
        compose.mainClock.autoAdvance = false
        compose.setContent { MarqueeTheme(form) { PosterWall(posters, drift, sheet) } }
        compose.mainClock.advanceTimeBy(ms)
        return drift.ms
    }

    @Test
    fun `phone has four columns and TV nine, six posters each dealt four apart`() {
        assertEquals(4, wallColumns(Form.Phone))
        assertEquals(9, wallColumns(Form.Tv))
        assertEquals((4..9).map { "https://art.example/p$it.jpg" }, column(posters, 1))
        // A short list repeats rather than leaving holes.
        assertEquals(listOf("a", "b", "a", "b", "a", "b"), column(listOf("a", "b"), 0))
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `an empty list draws empty tiles and asks for no image`() {
        serve()
        compose.setContent { MarqueeTheme(Form.Phone) { PosterWall(emptyList(), WallDrift(), sheet = true) } }
        compose.waitForIdle()
        assertEquals(emptyList(), column(emptyList(), 3))
        compose.onNodeWithTag(WALL).assertExists()
        assertEquals(0, asked.get())
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `posters are asked for, and the wall says nothing and takes no focus`() {
        serve()
        compose.setContent { MarqueeTheme(Form.Phone) { PosterWall(posters, WallDrift(), sheet = true) } }
        compose.waitUntil(5_000) { asked.get() > 0 }
        val wall = compose.onNodeWithTag(WALL).fetchSemanticsNode()
        assertTrue(wall.children.isEmpty(), "the wall exposes ${wall.children}")
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `the wall drifts on Welcome`() {
        assertTrue(drifted() > 1_000)
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `the wall holds still under a sheet`() {
        assertEquals(0L, drifted(sheet = true))
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `Pause motion holds it still on TV too`() {
        assertEquals(0L, drifted(Form.Tv, paused = true))
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `the system's animations off hold it still`() {
        animators(0f)
        assertEquals(0L, drifted())
    }

    @Test
    @Config(qualifiers = "w390dp-h844dp-xxhdpi")
    fun `the walk's reduceMotion extra holds it still`() {
        launchReducedMotion = true
        assertEquals(0L, drifted())
    }

    /** The share of the wall's pixels that are plain background (no tile drawn there), on a still wall. */
    private fun blackShare(form: Form): Float {
        compose.setContent {
            MarqueeTheme(form) { PosterWall(emptyList(), WallDrift().apply { paused = true }, sheet = false) }
        }
        val map = compose.onNodeWithTag(WALL).captureToImage().toPixelMap()
        var black = 0
        for (x in 0 until map.width step 4) {
            for (y in 0 until map.height step 4) {
                val p = map[x, y]
                if (p.red + p.green + p.blue < 0.02f) black++
            }
        }
        return black / (((map.width + 3) / 4) * ((map.height + 3) / 4)).toFloat()
    }

    /**
     * Pixel walk 2026-11-09 L7, L31: every column is its list twice, about twice the wall's height. Measured inside the
     * wall's height the tiles past its foot got no room, so the drift slid empty space into view: most of the phone
     * and every other TV column stayed black. At first open only the 14 dp gaps between tiles show the background.
     */
    @Test
    @Config(qualifiers = "w412dp-h915dp-xxhdpi")
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `the phone wall fills the screen at first open`() {
        val share = blackShare(Form.Phone)
        assertTrue(share < 0.25f, "${(share * 100).toInt()} percent of the phone wall is empty")
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `the TV wall fills the screen at first open`() {
        val share = blackShare(Form.Tv)
        assertTrue(share < 0.25f, "${(share * 100).toInt()} percent of the TV wall is empty")
    }

    /**
     * Pixel walk 2026-11-09 L8: the walk line counts the tiles on screen, not every tile of the wall, so a wall that
     * draws only part of the screen cannot read 24/24.
     */
    @Test
    fun `the walk line counts the tiles on screen with their poster`() {
        val phone = WallGeometry(412f, 915f, 4)
        val shown = phone.visible(6_000)
        assertTrue(shown.size in 6..20, "${shown.size} tiles on the phone")
        assertTrue(shown.size < 4 * 12)
        val tv = WallGeometry(960f, 540f, 9).visible(6_000)
        assertTrue(tv.size in 10..40, "${tv.size} tiles on TV")
        assertEquals(0 to shown.size, phone.filled(6_000, posters, emptySet()))
        assertEquals(shown.size to shown.size, phone.filled(6_000, posters, posters.toSet()))
        val one = column(posters, shown[0].first)[shown[0].second % 6]
        val (some, of) = phone.filled(6_000, posters, setOf(one))
        assertTrue(some in 1 until of, "$some of $of with one poster drawn")
    }

    @Test
    fun `the drift runs only on Welcome, unpaused, without reduced motion, in front`() {
        assertTrue(wallRuns(sheet = false, paused = false, reduced = false, resumed = true))
        assertTrue(!wallRuns(sheet = true, paused = false, reduced = false, resumed = true))
        assertTrue(!wallRuns(sheet = false, paused = true, reduced = false, resumed = true))
        assertTrue(!wallRuns(sheet = false, paused = false, reduced = true, resumed = true))
        assertTrue(!wallRuns(sheet = false, paused = false, reduced = false, resumed = false))
    }
}
