package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.lifecycle.compose.LocalLifecycleOwner
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.client.CoreClient
import app.marquee.core.client.stateThread
import app.marquee.core.model.EmptyKind
import app.marquee.core.views.PlaybackView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
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
import org.robolectric.annotation.Config
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals

/**
 * The TV player's failure card in the shared table's order (plans/versions-android-spec.md, section 6): a hand pick
 * that did not start offers the core's next version, then another version, then Back; a start whose 90 s ran out
 * says "This didn't start" with Try again, another version, Back; never an account step.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class PlayerFailureCardTest {
    @get:Rule
    val compose = createComposeRule()

    private val executor = stateThread("test-state")
    private val client = CoreClient(executor.asCoroutineDispatcher())
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
        client.close()
        executor.shutdownNow()
    }

    /** The player on a playback view whose core answers with a failed play carrying [extra] JSON fields. */
    private fun show(extra: String) {
        client.attach { text ->
            val msg =
                app.marquee.core.client.CoreJson
                    .parseToJsonElement(text)
                    .jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    """{"sub":7,"snapshot":{"phase":"failed","attempt":1,"manifest":false,""" +
                        """"subtitlesPending":false,"trakt":"off"$extra}}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val view = PlaybackView(client, scope, JsonObject(emptyMap()))
        controller.attach(view)
        compose.setContent { MarqueeTheme(Form.Tv) { TvPlayer(controller, false, {}, null, { _, _ -> }, {}) } }
        compose.waitForIdle()
        view.open()
        val deadline = System.currentTimeMillis() + 60_000
        while (compose.onAllNodesWithText("Back").fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) {
                "the failure card never showed (snapshot ${view.snapshot}, status ${view.status}, stage ${controller.stage})"
            }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    /** The card's buttons, exactly [labels], left to right. */
    private fun buttons(vararg labels: String) {
        val gone =
            listOf(
                "Add an account",
                "Settings",
                "Try again",
                "Choose another version",
                "Choose a version",
                "Play from this device",
            )
        for (word in gone - labels.toSet()) {
            compose.onAllNodesWithText(word).fetchSemanticsNodes().let {
                assertEquals(0, it.size, word)
            }
        }
        val lefts =
            labels.map {
                compose
                    .onNodeWithText(it)
                    .fetchSemanticsNode()
                    .boundsInRoot.left
            }
        assertEquals(lefts.sorted(), lefts, "in the shared order")
    }

    @Test
    fun `a start whose 90 s ran out says This didn't start with Try again, another version, Back`() {
        show(""","empty":"notStarted"""")
        compose.onNodeWithText("This didn't start").assertExists()
        buttons("Try again", "Choose another version", "Back")
        compose.onNodeWithText("Try again").assertIsFocused()
    }

    @Test
    fun `a hand pick that did not start offers the next version instead, then another version, then Back`() {
        show(""","offer":{"key":"k2","label":"720p WEB"}""")
        buttons("Play 720p WEB instead", "Choose another version", "Back")
    }

    @Test
    fun `a search that ends empty after Play opened the player shows the shared table's actions for each state`() {
        val events = mutableListOf<String>()
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                TvPlayer(
                    controller,
                    false,
                    { events += "versions" },
                    null,
                    { _, _ -> },
                    { events += "back" },
                    onOffer = { events += "play $it" },
                    onSettings = { events += "settings $it" },
                )
            }
        }
        val fail = { kind: EmptyKind, kept: String? ->
            compose.runOnIdle {
                controller.fail(StreamChoiceCopy.emptyLine(kind), kind, kept) { events += "again" }
            }
            compose.waitForIdle()
            compose.onNodeWithText(StreamChoiceCopy.emptyLine(kind)).assertExists()
        }
        // Nothing to do but leave: Back alone, focused; never a way to a page that holds only Back. needsAccount
        // too: an account is set up in Settings only, so the card has no Settings and no account words.
        for (kind in listOf(EmptyKind.NeedsAccount, EmptyKind.KidsNothing, EmptyKind.NoResults)) {
            fail(kind, null)
            buttons("Back")
            compose.onNodeWithText("Back").assertIsFocused()
        }
        fail(EmptyKind.NoSource, null)
        buttons("Settings", "Back")
        compose.onNodeWithText("Settings").performClick()
        fail(EmptyKind.NeedsAccount, null)
        for (gone in listOf("Add an account", "account")) {
            compose.onAllNodesWithText(gone, substring = true, ignoreCase = true).assertCountEquals(0)
        }
        assertEquals(listOf("settings NoSource"), events)
        events.clear()
        fail(EmptyKind.Unreachable, "kept")
        buttons("Play from this device", "Try again", "Back")
        compose.onNodeWithText("Play from this device").performClick()
        fail(EmptyKind.Unreachable, null)
        buttons("Try again", "Back")
        compose.onNodeWithText("Try again").performClick()
        assertEquals(listOf("play kept", "again"), events)
        events.clear()
        fail(EmptyKind.NothingPlaysHere, null)
        buttons("Choose a version", "Back")
        compose.onNodeWithText("Choose a version").performClick()
        assertEquals(listOf("versions"), events)
    }

    @Test
    fun `the failure history says why each source failed, never the addon's name`() {
        show(
            ""","offer":{"key":"k2","label":"720p WEB"},"attempts":[""" +
                """{"key":"k1","via":"Torrentio","error":"The link no longer exists (404)"},""" +
                """{"key":"k3","via":"Comet","error":"The link no longer exists (404)"}]""",
        )
        compose.onNodeWithText("The link no longer exists (404) · 2 sources").assertExists()
        for (name in listOf(
            "Torrentio",
            "Comet",
        )) {
            compose.onAllNodesWithText(name, substring = true).assertCountEquals(0)
        }
    }

    @Test
    fun `back from Settings opened on the failure card the start runs again`() {
        val events = mutableListOf<String>()
        val entry = Entry()
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                CompositionLocalProvider(LocalLifecycleOwner provides entry) {
                    TvPlayer(controller, false, {}, null, { _, _ -> }, {}, onSettings = { events += "settings $it" })
                }
            }
        }
        compose.runOnIdle {
            controller.fail(StreamChoiceCopy.emptyLine(EmptyKind.NoSource), EmptyKind.NoSource) { events += "again" }
        }
        compose.onNodeWithText("Settings").performClick()
        compose.runOnIdle { entry.covered() }
        compose.runOnIdle { entry.back() }
        compose.waitForIdle()
        assertEquals(listOf("settings NoSource", "again"), events)
    }
}
