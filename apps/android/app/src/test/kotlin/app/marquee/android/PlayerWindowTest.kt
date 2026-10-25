package app.marquee.android

import android.content.res.Configuration
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.media3.common.SimpleBasePlayer
import app.marquee.player.PlaybackController
import app.marquee.player.ScreenBrightness
import app.marquee.player.brightnessToWindow
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** The brightness a swipe left in the window goes back as plans/player-gestures.md 3.5 says (tests 21 to 23). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class PlayerWindowTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val scope = CoroutineScope(SupervisorJob())
    private val idle =
        object : SimpleBasePlayer(Looper.getMainLooper()) {
            override fun getState(): State = State.Builder().build()
        }
    private val controller = PlaybackController(idle, scope, "Sintel", null, null, SystemClock.elapsedRealtime())
    private var first by mutableStateOf(true)
    private var second by mutableStateOf(false)
    private lateinit var brightness: ScreenBrightness

    @After
    fun stop() {
        controller.release()
        scope.cancel()
    }

    private fun players() {
        compose.setContent {
            if (first) brightness = rememberPlayerWindow(controller, phone = true).brightness
            if (second) rememberPlayerWindow(controller, phone = true)
        }
        compose.waitForIdle()
    }

    private fun window() = compose.activity.window

    private fun override() = window().attributes.screenBrightness

    @Test
    fun `the last player out lets the system's brightness back, one that replaced it keeps the viewer's`() {
        players()
        compose.runOnIdle { brightness.set(window(), 0.5f) }
        // The next episode comes in while this one fades out.
        second = true
        compose.waitForIdle()
        first = false
        compose.waitForIdle()
        assertEquals(0.5f, brightness.level)
        assertEquals(brightnessToWindow(0.5f), override())
        second = false
        compose.waitForIdle()
        assertEquals(WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE, override())
        assertNull(brightness.level)
    }

    @Test
    fun `a change of the system's brightness wins over the swiped one`() {
        players()
        compose.runOnIdle { brightness.set(window(), 0.5f) }
        Settings.System.putInt(compose.activity.contentResolver, Settings.System.SCREEN_BRIGHTNESS, 40)
        shadowOf(Looper.getMainLooper()).idle()
        compose.waitForIdle()
        assertEquals(WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE, override())
        assertNull(brightness.level)
    }

    @Test
    fun `the small window follows the system, and the viewer's brightness comes back after it`() {
        players()
        compose.runOnIdle { brightness.set(window(), 0.5f) }
        compose.runOnIdle { compose.activity.onPictureInPictureModeChanged(true, Configuration()) }
        assertEquals(WindowManager.LayoutParams.BRIGHTNESS_OVERRIDE_NONE, override())
        assertEquals(0.5f, brightness.level)
        compose.runOnIdle { compose.activity.onPictureInPictureModeChanged(false, Configuration()) }
        assertEquals(brightnessToWindow(0.5f), override())
    }
}
