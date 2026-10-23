package app.marquee.ui.phone

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
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

    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-core") }
    private val client = CoreClient(executor.asCoroutineDispatcher())

    @After
    fun stop() {
        controller.release()
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    @Test
    fun `Choose another version shows when the core offers it, opens Versions, and a tap before does nothing`() {
        val events = mutableListOf<String>()
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhonePlayer(controller, false, false, { events += "versions" }, null, { _, _ -> }, {}, {})
            }
        }
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
        compose.waitForIdle()
        val slot = compose.onNodeWithTag(CHOOSE_TAG, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        compose.onNodeWithTag(CHOOSE_TAG, useUnmergedTree = true).performClick()
        compose.waitForIdle()
        assertEquals(emptyList(), events)
        // No clock of the app's own: a minute changes nothing until the core offers it.
        compose.mainClock.advanceTimeBy(60_000)
        compose.onAllNodesWithText(CHOOSE_ANOTHER).assertCountEquals(0)
        client.onMessage("""{"sub":7,"updates":[${RESOLVING.dropLast(1)},"chooseAnother":true}]}""")
        compose.waitUntil(10_000) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            controller.chooseAnother
        }
        compose.waitForIdle()
        // Where its slot was, so nothing moved, and said once from there.
        assertEquals(slot, compose.onNodeWithTag(CHOOSE_TAG, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot)
        val said =
            compose
                .onNode(
                    hasTestTag(CHOOSE_TAG) and SemanticsMatcher.keyIsDefined(SemanticsProperties.LiveRegion),
                    true,
                ).fetchSemanticsNode()
        assertEquals(slot, said.boundsInRoot)
        compose.onNodeWithText(CHOOSE_ANOTHER).performClick()
        compose.waitForIdle()
        assertEquals(listOf("versions"), events)
    }
}

/** A start the core is still resolving. */
private const val RESOLVING =
    """{"phase":"resolving","attempt":1,"manifest":false,"subtitlesPending":true,"trakt":"off"}"""
