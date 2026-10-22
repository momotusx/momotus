package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.core.PlayPick
import app.marquee.core.model.BlockedReason
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

/**
 * An episode started from its title's list, where every version found plays only through a streaming service
 * account: the player's card offers Add an account, as the title page and Versions do.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class PlayerAccountTest {
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

    private fun failed(blocked: BlockedReason?): MutableList<String> {
        val events = mutableListOf<String>()
        controller.cannotPlay(PlayPick.None("Marquee found 3 versions.", blocked))
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                TvPlayer(
                    controller,
                    false,
                    { events += "versions" },
                    null,
                    { _, _ -> },
                    {},
                    onAddAccount = { events += "account" },
                )
            }
        }
        return events
    }

    @Test
    fun `versions that need an account offer Add an account first, then Choose a version`() {
        val events = failed(BlockedReason.NeedsAccount)
        compose.onNodeWithText("Add an account").performClick()
        compose.onNodeWithText("Choose a version").performClick()
        assertEquals(listOf("account", "versions"), events)
    }

    @Test
    fun `another reason keeps Choose a version, where Show them is`() {
        failed(BlockedReason.NothingPlaysHere)
        compose.onAllNodesWithText("Add an account").assertCountEquals(0)
        compose.onNodeWithText("Choose a version").assertExists()
    }
}
