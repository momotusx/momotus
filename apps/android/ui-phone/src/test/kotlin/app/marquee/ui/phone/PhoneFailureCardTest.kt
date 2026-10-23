package app.marquee.ui.phone

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.lifecycle.compose.LocalLifecycleOwner
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.model.EmptyKind
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.PlaybackController
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals

/** The phone player's failure card, back from the Settings it opened. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w411dp-h891dp")
class PhoneFailureCardTest {
    @get:Rule
    val compose = createComposeRule()

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
    private val controller =
        PlaybackController(PlayingPlayer(), scope, "Sintel", null, null, SystemClock.elapsedRealtime())

    @After
    fun stop() {
        controller.release()
        scope.cancel()
    }

    @Test
    fun `back from Settings opened on the failure card the start runs again`() {
        val events = mutableListOf<String>()
        val entry = Entry()
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                CompositionLocalProvider(LocalLifecycleOwner provides entry) {
                    PhonePlayer(controller, false, false, {}, null, { _, _ -> }, {}, {}, onSettings = {
                        events += "settings $it"
                    })
                }
            }
        }
        compose.runOnIdle {
            controller.fail(StreamChoiceCopy.emptyLine(EmptyKind.NoSource), EmptyKind.NoSource) { events += "again" }
        }
        compose.onNodeWithText("Settings").performClick()
        // Settings covers the player, which stays composed, then the viewer comes back.
        compose.runOnIdle { entry.covered() }
        compose.runOnIdle { entry.back() }
        compose.waitForIdle()
        assertEquals(listOf("settings NoSource", "again"), events)
        // Once: resuming again without Settings starts nothing.
        compose.runOnIdle { entry.covered() }
        compose.runOnIdle { entry.back() }
        compose.waitForIdle()
        assertEquals(listOf("settings NoSource", "again"), events)
    }
}
