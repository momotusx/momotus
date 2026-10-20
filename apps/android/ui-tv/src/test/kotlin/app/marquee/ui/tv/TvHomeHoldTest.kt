package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import app.marquee.core.PendingRemoval
import app.marquee.core.TileCopy
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.model.HomeHeader
import app.marquee.core.model.HomeRow
import app.marquee.core.model.HomeSnapshot
import app.marquee.core.model.RowState
import app.marquee.core.model.Tile
import app.marquee.core.views.HomeView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
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
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals

/** A held select on a Continue Watching tile offers "Remove from Continue Watching" (journey reviews 2026-10-20). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvHomeHoldTest {
    @get:Rule
    val compose = createComposeRule()

    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
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

    @After
    fun stop() {
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    private fun tile(
        id: String,
        name: String,
    ) = Tile(id = id, kind = "series", name = name, posterShape = "poster", releaseInfo = "S1:E2 · 36 min left")

    private val removed = mutableListOf<Tile>()

    // A short Undo window, so a test sees the removal go through.
    private val removal = PendingRemoval(scope, undoMs = 600) { removed += it }

    private val sent = java.util.concurrent.CopyOnWriteArrayList<kotlinx.serialization.json.JsonObject>()

    private fun show(failedRow: Boolean = false) {
        val popular =
            if (failedRow) {
                HomeRow("cinemeta:series:top", "Popular", "Cinemeta", RowState.Failed("timed out"))
            } else {
                null
            }
        val snapshot =
            HomeSnapshot(
                HomeHeader(),
                listOfNotNull(
                    HomeRow(
                        TileCopy.CONTINUE_ROW,
                        "Continue Watching",
                        "",
                        RowState.Ready,
                        listOf(tile("tt1", "Severance"), tile("tt3", "Dark")),
                    ),
                    popular
                        ?: HomeRow(
                            "cinemeta:series:top",
                            "Popular",
                            "Cinemeta",
                            RowState.Ready,
                            listOf(tile("tt2", "Andor")),
                        ),
                ),
            )
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            sent += msg
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
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                TvHome(home, TvHomeState(), onOpen = {}, onPlay = {}, onSetup = {}, onAddons = {}, removal = removal)
            }
        }
        compose.waitForIdle()
        home.open()
        val deadline = System.currentTimeMillis() + 60_000
        while (compose.onAllNodesWithText("Popular").fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "Home never drew its rows" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    /** Lets the screen and the main looper (the removal's timer resumes there) run until [check], or fails. */
    private fun waitFor(
        what: String,
        check: () -> Boolean,
    ) {
        val end = System.currentTimeMillis() + 60_000
        while (!check()) {
            check(System.currentTimeMillis() < end) { "waited 60 s and it never happened: $what" }
            compose.waitForIdle()
            Thread.sleep(20)
        }
    }

    private fun focused(matcher: androidx.compose.ui.test.SemanticsMatcher) =
        compose.onAllNodes(matcher and isFocused()).fetchSemanticsNodes().isNotEmpty()

    @Test
    fun `a hold on a Continue Watching tile offers Remove, and Remove names the title`() {
        show()
        // A remote has no long press to discover: the row says how.
        compose.onNodeWithText(TileCopy.HOLD_TO_REMOVE).assertExists()
        compose.onNodeWithContentDescription("Severance").performSemanticsAction(SemanticsActions.OnLongClick)
        compose.onNodeWithText(TileCopy.REMOVE_CONTINUE).performClick()
        // Undo is the one thing the viewer may want now: it has the focus at once.
        waitFor("Undo takes the focus") { focused(hasText("Undo")) }
        // The window passes: the removal goes to the core, and focus goes on to the next tile of the row.
        waitFor("the removal goes to the core") { removed.isNotEmpty() }
        compose.runOnIdle { assertEquals(listOf("tt1"), removed.map { it.id }) }
        waitFor("the next tile takes the focus") { focused(hasContentDescription("Dark")) }
    }

    @Test
    fun `Undo brings the title back with the focus, and nothing is sent`() {
        show()
        compose.onNodeWithContentDescription("Severance").performSemanticsAction(SemanticsActions.OnLongClick)
        compose.onNodeWithText(TileCopy.REMOVE_CONTINUE).performClick()
        waitFor("Undo takes the focus") { focused(hasText("Undo")) }
        compose.onNodeWithText("Undo").performClick()
        waitFor("the title comes back with the focus") { focused(hasContentDescription("Severance")) }
        Thread.sleep(1_000)
        compose.runOnIdle { assertEquals(emptyList(), removed.map { it.id }) }
    }

    @Test
    fun `a tile of another row has no hold menu`() {
        show()
        compose
            .onNodeWithContentDescription(
                "Andor",
            ).assert(SemanticsMatcher.keyNotDefined(SemanticsActions.OnLongClick))
    }

    @Test
    fun `a failed row's Try again asks that catalog again, not all of Home`() {
        show(failedRow = true)
        compose.onNodeWithText("Try again").performClick()
        compose.waitUntil(10_000) {
            sent.any {
                val command = it["command"] as? kotlinx.serialization.json.JsonObject
                it["op"]?.jsonPrimitive?.content == "command" &&
                    command?.get("type")?.jsonPrimitive?.content == "retryRow" &&
                    command["rowId"]?.jsonPrimitive?.content == "cinemeta:series:top"
            }
        }
    }
}
