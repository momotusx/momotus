package app.marquee.design

import android.app.Application
import android.graphics.Bitmap
import android.provider.Settings
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

    @Test
    fun `the drift runs only on Welcome, unpaused, without reduced motion, in front`() {
        assertTrue(wallRuns(sheet = false, paused = false, reduced = false, resumed = true))
        assertTrue(!wallRuns(sheet = true, paused = false, reduced = false, resumed = true))
        assertTrue(!wallRuns(sheet = false, paused = true, reduced = false, resumed = true))
        assertTrue(!wallRuns(sheet = false, paused = false, reduced = true, resumed = true))
        assertTrue(!wallRuns(sheet = false, paused = false, reduced = false, resumed = false))
    }
}
