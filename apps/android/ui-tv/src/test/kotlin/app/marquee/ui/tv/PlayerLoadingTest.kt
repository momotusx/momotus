package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotFocused
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.views.PlaybackView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.CHOOSE_ANOTHER
import app.marquee.player.CHOOSE_TAG
import app.marquee.player.PlaybackController
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The TV player's loading screen and failure card with no account step (plans/versions-android-spec.md, section 6):
 * "Choose another version" under the stage line once the core offers it (its one clock: 20 s into the start), in a
 * slot reserved from the start, never taking focus by itself; and a card that never offers "Add an account".
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class PlayerLoadingTest {
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

    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-core") }
    private val client = CoreClient(executor.asCoroutineDispatcher())

    @After
    fun stop() {
        controller.release()
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    /** The core's playback view, still resolving, as a scripted core answers its subscribe. */
    private fun resolving() {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content ==
                    "subscribe"
                ) {
                    """{"sub":7,"snapshot":$RESOLVING}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val view = PlaybackView(client, scope, JsonObject(emptyMap()))
        compose.runOnIdle { controller.attach(view.also { it.open() }) }
        // The subscription is open before the core says more.
        compose.waitUntil(10_000) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            view.snapshot != null
        }
    }

    /** The core offers another version: its 20 s into the start have passed. */
    private fun offered() {
        client.onMessage("""{"sub":7,"updates":[${RESOLVING.dropLast(1)},"chooseAnother":true}]}""")
        compose.waitUntil(10_000) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            controller.chooseAnother
        }
        compose.waitForIdle()
    }

    private fun show(): MutableList<String> {
        val events = mutableListOf<String>()
        compose.setContent {
            MarqueeTheme(Form.Tv) { TvPlayer(controller, false, { events += "versions" }, null, { _, _ -> }, {}) }
        }
        compose.waitForIdle()
        return events
    }

    @Test
    fun `Choose another version waits for the core in its slot, then the first key focuses it and opens Versions`() {
        val events = show()
        resolving()
        val slot = compose.onNodeWithTag(CHOOSE_TAG, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        compose.onAllNodesWithText(CHOOSE_ANOTHER).assertCountEquals(0)
        compose
            .onAllNodes(
                hasTestTag(CHOOSE_TAG) and SemanticsMatcher.keyIsDefined(SemanticsProperties.LiveRegion),
                true,
            ).assertCountEquals(0)
        // Before the core offers it no D-pad key reaches it, nor a move that ends on it while it is hidden; the app
        // keeps no clock of its own, so a minute of waiting changes nothing.
        for (key in listOf(Key.DirectionDown, Key.DirectionUp, Key.DirectionLeft, Key.DirectionRight, Key.Tab)) {
            compose.onRoot().performKeyInput { pressKey(key) }
        }
        compose.mainClock.advanceTimeBy(60_000)
        compose.onAllNodesWithText(CHOOSE_ANOTHER).assertCountEquals(0)
        offered()
        compose.onNodeWithText(CHOOSE_ANOTHER).assertExists()
        // It never takes focus by itself, nor took it while hidden; it sits where its slot was, so nothing moved.
        compose.onNodeWithText(CHOOSE_ANOTHER).assertIsNotFocused()
        assertEquals(slot, compose.onNodeWithTag(CHOOSE_TAG, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot)
        // Said once, politely, from the slot it fills.
        val said =
            compose
                .onNode(
                    hasTestTag(CHOOSE_TAG) and SemanticsMatcher.keyIsDefined(SemanticsProperties.LiveRegion),
                    true,
                ).fetchSemanticsNode()
        assertEquals(slot, said.boundsInRoot)
        assertTrue(said.boundsInRoot.width > 0 && said.boundsInRoot.height > 0)
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionDown) }
        compose.waitForIdle()
        compose.onNodeWithText(CHOOSE_ANOTHER).assertIsFocused()
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionCenter) }
        compose.waitForIdle()
        assertEquals(listOf("versions"), events)
    }

    @Test
    fun `a pick with nothing to play offers a version, never an account`() {
        controller.fail("Momotus found 3 versions.")
        show()
        compose.onAllNodesWithText("Add an account").assertCountEquals(0)
        compose.onNodeWithText("Choose a version").assertExists()
    }
}

/** A start the core is still resolving, for [PlayerLoadingTest.resolving]. */
private const val RESOLVING =
    """{"phase":"resolving","attempt":1,"manifest":false,"subtitlesPending":true,"trakt":"off"}"""
