package app.marquee.ui.phone

import android.content.Context
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.WindowManager
import android.view.accessibility.AccessibilityManager
import androidx.activity.ComponentActivity
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.click
import androidx.compose.ui.test.doubleClick
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performMouseInput
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipe
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.PlaybackController
import app.marquee.player.ScreenBrightness
import app.marquee.player.brightnessToWindow
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The phone player's level swipes against the real audio service and window (plans/player-gestures.md, tests 8 to 20). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w844dp-h390dp")
class PhonePlayerSwipeTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val main =
        object : CoroutineDispatcher() {
            private val handler = Handler(Looper.getMainLooper())

            override fun dispatch(
                context: CoroutineContext,
                block: Runnable,
            ) {
                handler.post(block)
            }
        }
    private val scope = CoroutineScope(SupervisorJob() + main)
    private val player = PlayingPlayer()
    private val controller = PlaybackController(player, scope, "Sintel", null, null, SystemClock.elapsedRealtime())
    private val brightness = ScreenBrightness()
    private val audio = RuntimeEnvironment.getApplication().getSystemService(AudioManager::class.java)

    @After
    fun stop() {
        controller.release()
        scope.cancel()
    }

    private fun playing() {
        audio.setStreamVolume(AudioManager.STREAM_MUSIC, 7, 0)
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhonePlayer(controller, false, false, {}, null, { _, _ -> }, {}, {}, brightness = brightness)
            }
        }
        compose.runOnIdle { player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        compose.waitForIdle()
    }

    private fun volume() = audio.getStreamVolume(AudioManager.STREAM_MUSIC)

    private fun windowBrightness() = compose.activity.window.attributes.screenBrightness

    private fun controlsShown() =
        compose.onAllNodesWithContentDescription("Picture in picture").fetchSemanticsNodes().isNotEmpty()

    /** A vertical swipe of [dy] px from [x], [y] (fractions of the root), held for [ms]. */
    private fun drag(
        x: Float,
        y: Float,
        dy: Float,
        ms: Long = 300,
    ) = compose.onRoot().performTouchInput {
        val from = Offset(width * x, height * y)
        swipe(from, from + Offset(0f, dy), ms)
    }

    @Test
    fun `a swipe on the right half sets the media volume, and the indicator shows what the device took`() {
        playing()
        drag(0.75f, 0.45f, -120f)
        val up = volume()
        assertTrue(up > 7, "a swipe up raises the volume, was 7, now $up")
        val shown =
            compose
                .onNodeWithTag(
                    LEVEL_TAG,
                ).fetchSemanticsNode()
                .config[SemanticsProperties.ProgressBarRangeInfo]
        assertEquals(indexLevel(up, 0, audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC)), shown.current)
        drag(0.75f, 0.4f, 200f)
        assertTrue(volume() < up, "a swipe down lowers it")
        assertEquals(WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE, windowBrightness())
    }

    @Test
    fun `a swipe on the left half sets the window's brightness and leaves the volume`() {
        playing()
        drag(0.25f, 0.45f, -120f)
        assertTrue(windowBrightness() > 0f, "the window has its own brightness now")
        assertEquals(7, volume())
        // To the bottom, then half the sweep up: from the held 0, not from the floor read back (0.123).
        drag(0.25f, 0.2f, 330f)
        assertEquals(0.01f, windowBrightness())
        drag(
            0.15f,
            0.8f,
            -12f - 0.5f * 0.8f *
                compose
                    .onRoot()
                    .fetchSemanticsNode()
                    .size.height,
        )
        assertEquals(brightnessToWindow(0.5f), windowBrightness(), 0.02f)
    }

    @Test
    fun `the edges are the system's, and a wider back gesture widens its band`() {
        playing()
        drag(10f / 844f, 0.45f, -120f)
        assertEquals(WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE, windowBrightness())
        drag(0.75f, 10f / 390f, 120f)
        assertEquals(7, volume())
        // The viewer widened the back gesture: 40 px on the left now.
        compose.runOnIdle {
            ViewCompat.dispatchApplyWindowInsets(
                compose.activity.window.decorView,
                WindowInsetsCompat
                    .Builder()
                    .setInsets(WindowInsetsCompat.Type.systemGestures(), Insets.of(40, 0, 0, 0))
                    .build(),
            )
        }
        compose.waitForIdle()
        drag(30f / 844f, 0.45f, -120f)
        assertEquals(WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE, windowBrightness())
        drag(50f / 844f, 0.45f, -120f)
        assertTrue(windowBrightness() > 0f, "past the band it is a swipe")
    }

    @Test
    fun `a diagonal touch and two fingers set nothing, and a second finger ends a swipe where it is`() {
        playing()
        compose.onRoot().performTouchInput {
            val from = Offset(width * 0.75f, height * 0.5f)
            swipe(from, from + Offset(40f, -40f), 300)
        }
        assertEquals(7, volume())
        // Two fingers down before any move: never a swipe.
        compose.onRoot().performTouchInput {
            down(0, Offset(width * 0.75f, height * 0.45f))
            down(1, Offset(width * 0.85f, height * 0.45f))
            moveBy(0, Offset(0f, -120f))
            up(0)
            up(1)
        }
        assertEquals(7, volume())
        // A swipe, then a second finger: further moves change nothing.
        compose.onRoot().performTouchInput {
            down(0, Offset(width * 0.75f, height * 0.45f))
            // The first move commits it, the next one sets the level.
            moveBy(0, Offset(0f, -20f))
            moveBy(0, Offset(0f, -60f))
        }
        val held = volume()
        assertTrue(held > 7, "the swipe set the volume before the second finger, was 7, now $held")
        compose.onRoot().performTouchInput {
            down(1, Offset(width * 0.85f, height * 0.45f))
            moveBy(0, Offset(0f, -100f))
            up(1)
            up(0)
        }
        assertEquals(held, volume())
    }

    @OptIn(ExperimentalTestApi::class)
    @Test
    fun `a mouse drag sets nothing`() {
        playing()
        compose.onRoot().performMouseInput {
            moveTo(Offset(width * 0.75f, height * 0.45f))
            press()
            moveBy(Offset(0f, -60f))
            moveBy(Offset(0f, -60f))
            release()
        }
        assertEquals(7, volume())
    }

    @Test
    fun `a drag that starts on the position bar is the bar's`() {
        playing()
        if (!controlsShown()) compose.onRoot().performTouchInput { click(center) }
        compose.waitUntil(3_000) { controlsShown() }
        compose.onNodeWithContentDescription("Position").performTouchInput {
            val from = Offset(width * 0.75f, centerY)
            swipe(from, from + Offset(0f, -120f), 300)
        }
        assertEquals(7, volume())
    }

    @Test
    fun `with the subtitle list open the picture takes no swipe`() {
        playing()
        if (!controlsShown()) compose.onRoot().performTouchInput { click(center) }
        compose.waitUntil(3_000) { controlsShown() }
        compose.onNodeWithContentDescription("Subtitles", substring = true).performClick()
        compose.waitForIdle()
        drag(0.1f, 0.45f, -120f)
        assertEquals(WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE, windowBrightness())
    }

    @Test
    fun `with TalkBack's touch exploration on the picture takes no swipe`() {
        val manager =
            RuntimeEnvironment.getApplication().getSystemService(Context.ACCESSIBILITY_SERVICE) as AccessibilityManager
        shadowOf(manager).setTouchExplorationEnabled(true)
        playing()
        drag(0.75f, 0.45f, -120f)
        assertEquals(7, volume())
    }

    @Test
    fun `a swipe never toggles the controls, and keeps them up while the finger moves`() {
        playing()
        if (!controlsShown()) compose.onRoot().performTouchInput { click(center) }
        compose.waitUntil(3_000) { controlsShown() }
        // Moves for 4.5 s, past the 4 s the controls wait: they stay. Each move is its own frame, as on a device.
        compose.onRoot().performTouchInput { down(Offset(width * 0.75f, height * 0.8f)) }
        repeat(45) {
            compose.onRoot().performTouchInput { moveBy(Offset(0f, -3f)) }
            compose.mainClock.advanceTimeBy(100)
        }
        assertTrue(controlsShown(), "the controls stay while the finger moves")
        compose.onRoot().performTouchInput { up() }
        compose.mainClock.advanceTimeBy(1_000)
        assertTrue(controlsShown(), "and a swipe never hides them")
        compose.mainClock.advanceTimeBy(4_000)
        compose.waitForIdle()
        assertTrue(!controlsShown(), "they hide 4 s after the last move")
    }

    @Test
    fun `the indicator fades after the swipe ends`() {
        playing()
        compose.onRoot().performTouchInput {
            down(Offset(width * 0.75f, height * 0.45f))
            moveBy(Offset(0f, -40f))
            moveBy(Offset(0f, -40f))
        }
        // Held: it stays however long the finger rests.
        compose.mainClock.advanceTimeBy(2_000)
        compose.onNodeWithTag(LEVEL_TAG).assertExists()
        compose.onRoot().performTouchInput { up() }
        compose.mainClock.advanceTimeBy(600)
        compose.onNodeWithTag(LEVEL_TAG).assertExists()
        compose.mainClock.advanceTimeBy(1_500)
        assertEquals(0, compose.onAllNodesWithTag(LEVEL_TAG).fetchSemanticsNodes().size)
    }

    @Test
    fun `a double tap still steps forward 10 s`() {
        playing()
        compose.onRoot().performTouchInput { doubleClick(Offset(width * 0.75f, height * 0.5f)) }
        compose.runOnIdle { assertEquals(70_000L, player.currentPosition) }
    }

    @Test
    fun `a fixed volume has no swipe`() {
        // The audio service answers isVolumeFixed, which Robolectric cannot set: the seam takes it.
        assertEquals(null, MediaVolume(audio, fixed = true).start())
        assertEquals(MediaVolume(audio, fixed = false).level(), MediaVolume(audio, fixed = false).start())
    }
}
