package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicText
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.unit.dp
import app.marquee.core.TileCopy
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.client.StateThreadRule
import app.marquee.core.model.Hero
import app.marquee.core.model.HomeHeader
import app.marquee.core.model.HomeRow
import app.marquee.core.model.HomeSnapshot
import app.marquee.core.model.PlayTarget
import app.marquee.core.model.RowState
import app.marquee.core.model.Tile
import app.marquee.core.views.HomeView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
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
import kotlin.test.assertTrue

/**
 * An urgent account alert on TV Home (the owner, 2026-11-02): first, above the hero and on the first screen when Home
 * opens with it; one that comes while Home shows moves neither the focus nor the button that has it.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvHomeAlertTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    @get:Rule
    val stateThread = StateThreadRule()
    private val client = CoreClient(stateThread.dispatcher)
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
    private var alert by mutableStateOf<String?>(null)
    private val homeState = TvHomeState()

    @After
    fun stop() {
        scope.cancel()
        client.close()
    }

    private fun tile(
        id: String,
        name: String,
    ) = Tile(id = id, kind = "movie", name = name, posterShape = "poster")

    private fun show() {
        val hero =
            Hero(
                tile("tt1", "Night of the Living Dead"),
                "",
                "#1 in Popular Movies",
                "",
                listOf("Horror"),
                "1 h 36 min",
                play = PlayTarget("tt1", "Play"),
                playPending = false,
            )
        val snapshot =
            HomeSnapshot(
                HomeHeader(listOf(hero)),
                listOf(
                    HomeRow(
                        TileCopy.CONTINUE_ROW,
                        "Continue Watching",
                        "",
                        RowState.Ready,
                        listOf(tile("cw1", "Severance")),
                    ),
                    HomeRow(
                        "cinemeta:movie:top",
                        "Popular Movies",
                        "Cinemeta",
                        RowState.Ready,
                        (1..6).map { tile("m$it", "Movie $it") },
                    ),
                ),
            )
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    """{"sub":7,"snapshot":${CoreJson.encodeToString(HomeSnapshot.serializer(), snapshot)}}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val home = HomeView(client, scope)
        compose.runOnUiThread {
            compose.activity.window.decorView
                .requestFocusFromTouch()
        }
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                TvHome(
                    home,
                    homeState,
                    onOpen = {},
                    onPlay = {},
                    onSetup = {},
                    onAddons = {},
                    alert = alert?.let { words -> { BasicText(words, Modifier.padding(24.dp)) } },
                )
            }
        }
        compose.waitForIdle()
        home.open()
        val deadline = System.currentTimeMillis() + 60_000
        while (compose.onAllNodesWithText("Continue Watching").fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "Home never drew its rows" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    private fun focusedOn(description: String) =
        compose.onAllNodes(isFocused() and hasContentDescription(description)).fetchSemanticsNodes().isNotEmpty()

    private fun waitFor(
        what: String,
        check: () -> Boolean,
    ) {
        val deadline = System.currentTimeMillis() + 10_000
        while (!check()) {
            check(System.currentTimeMillis() < deadline) { "never: $what" }
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    private val play = "Play Night of the Living Dead"

    @Test
    fun `an alert Home opens with comes first, above the hero, on the first screen`() {
        alert = "A new device joined: QA device B."
        show()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val said = compose.onNodeWithText("A new device joined: QA device B.").fetchSemanticsNode().boundsInRoot
        val button = compose.onNodeWithContentDescription(play).fetchSemanticsNode().boundsInRoot
        assertTrue(said.top >= 0f && said.bottom <= root.bottom, "the alert is on the first screen: $said")
        assertTrue(said.bottom <= button.top, "the alert sits above the hero's Play")
    }

    @Test
    fun `an alert that comes later shows at once and moves neither the focus nor Play`() {
        show()
        compose.onNodeWithContentDescription(play).performSemanticsAction(SemanticsActions.RequestFocus)
        waitFor("Play takes the focus") { focusedOn(play) }
        // Home settled first: its own first scroll and the hero's art are done.
        compose.mainClock.advanceTimeBy(2_000)
        compose.waitForIdle()
        val before = compose.onNodeWithContentDescription(play).fetchSemanticsNode().boundsInRoot
        alert = "A new device joined: QA device B."
        compose.mainClock.advanceTimeBy(1_000)
        compose.waitForIdle()
        assertTrue(focusedOn(play), "the focus stays on Play")
        assertEquals(before, compose.onNodeWithContentDescription(play).fetchSemanticsNode().boundsInRoot)
        // Seen at once, over the hero's art above Play, not above the screen's top (QA r2, Q2 row 33).
        val said = compose.onNodeWithText("A new device joined: QA device B.").fetchSemanticsNode().boundsInRoot
        assertTrue(said.top >= 0f && said.bottom <= before.top, "the alert is on the screen, above Play: $said")
    }
}
