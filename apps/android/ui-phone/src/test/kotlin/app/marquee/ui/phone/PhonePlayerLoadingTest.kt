package app.marquee.ui.phone

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.CHOOSE_AFTER_MS
import app.marquee.player.CHOOSE_ANOTHER
import app.marquee.player.CHOOSE_TAG
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

/** The phone player's loading screen: "Choose another version" after 20 s, a tap before it does nothing. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class PhonePlayerLoadingTest {
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
    fun `Choose another version shows after 20 s and opens Versions, a tap on its slot before does nothing`() {
        val events = mutableListOf<String>()
        compose.mainClock.autoAdvance = false
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhonePlayer(controller, false, false, { events += "versions" }, null, { _, _ -> }, {}, {})
            }
        }
        compose.mainClock.advanceTimeBy(1_000)
        compose.onNodeWithTag(CHOOSE_TAG, useUnmergedTree = true).performClick()
        compose.mainClock.advanceTimeBy(100)
        assertEquals(emptyList(), events)
        compose.onAllNodesWithText(CHOOSE_ANOTHER).assertCountEquals(0)
        compose.mainClock.advanceTimeBy(CHOOSE_AFTER_MS)
        compose.onNodeWithText(CHOOSE_ANOTHER).performClick()
        compose.mainClock.advanceTimeBy(100)
        assertEquals(listOf("versions"), events)
    }
}
