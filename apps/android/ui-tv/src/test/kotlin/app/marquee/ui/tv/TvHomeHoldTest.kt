package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.platform.AccessibilityManager
import androidx.compose.ui.platform.LocalAccessibilityManager
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.unit.dp
import app.marquee.core.PendingRemoval
import app.marquee.core.TileCopy
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.model.Hero
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
import org.robolectric.annotation.GraphicsMode
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** A held select on a Continue Watching tile offers "Remove from Continue Watching" (journey reviews 2026-10-20). */
@OptIn(androidx.compose.ui.test.ExperimentalTestApi::class)
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

    private fun show(
        failedRow: Boolean = false,
        accessibility: AccessibilityManager? = null,
        more: Int = 0,
        hero: Hero? = null,
    ) {
        val popular =
            if (failedRow) {
                HomeRow("cinemeta:series:top", "Popular", "Cinemeta", RowState.Failed("timed out"))
            } else {
                null
            }
        val snapshot =
            HomeSnapshot(
                HomeHeader(hero),
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
                ) +
                    (1..more).map { n ->
                        HomeRow(
                            "walk:row:$n",
                            "Row $n",
                            "Walk",
                            RowState.Ready,
                            (1..4).map { tile("tt$n$it", "Title $n.$it") },
                        )
                    },
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
                CompositionLocalProvider(
                    LocalAccessibilityManager provides (accessibility ?: LocalAccessibilityManager.current),
                ) {
                    TvHome(
                        home,
                        TvHomeState(),
                        onOpen = {},
                        onPlay = {},
                        onSetup = {},
                        onAddons = {},
                        removal = removal,
                    )
                }
            }
        }
        compose.waitForIdle()
        home.open()
        val deadline = System.currentTimeMillis() + 60_000
        // Under a hero only the first row is on the first screen (the v3 tiles are taller), and a
        // lazy list draws nothing below the screen: wait for that row then.
        val drawn = if (hero == null) "Popular" else "Continue Watching"
        while (compose.onAllNodesWithText(drawn).fetchSemanticsNodes().isEmpty()) {
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
            // The core's updates are applied on its state thread: tell Compose.
            Snapshot.sendApplyNotifications()
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

    /** The core's update for the row under Continue Watching. */
    private fun second(row: HomeRow) =
        client.onMessage(
            """{"sub":7,"updates":[{"kind":"rows","diffs":[{"op":"set","index":1,"value":""" +
                CoreJson.encodeToString(HomeRow.serializer(), row) + "}]}]}",
        )

    @Test
    fun `a failed row's Try again gives the focus to its first tile once the tiles come`() {
        show(failedRow = true)
        val tryAgain = compose.onNodeWithText("Try again")
        tryAgain.requestFocus()
        tryAgain.performClick()
        // The core turns the row to a skeleton (the focused button goes), then answers with the tiles.
        second(HomeRow("cinemeta:series:top", "Popular", "Cinemeta", RowState.Loading))
        waitFor("the row loads") { compose.onAllNodesWithText("Try again").fetchSemanticsNodes().isEmpty() }
        second(HomeRow("cinemeta:series:top", "Popular", "Cinemeta", RowState.Ready, listOf(tile("tt2", "Andor"))))
        waitFor("the row's first tile takes the focus") { focused(hasContentDescription("Andor")) }
    }

    @Test
    fun `with TalkBack on, Undo waits as long as the system asks`() {
        // Before Android 10, TalkBack makes a note with a control wait until it is answered.
        val talkBack =
            object : AccessibilityManager {
                override fun calculateRecommendedTimeoutMillis(
                    originalTimeoutMillis: Long,
                    containsIcons: Boolean,
                    containsText: Boolean,
                    containsControls: Boolean,
                ) = if (containsControls) Long.MAX_VALUE else originalTimeoutMillis
            }
        show(accessibility = talkBack)
        compose.onNodeWithContentDescription("Severance").performSemanticsAction(SemanticsActions.OnLongClick)
        compose.onNodeWithText(TileCopy.REMOVE_CONTINUE).performClick()
        waitFor("Undo shows") { compose.onAllNodesWithText("Undo").fetchSemanticsNodes().isNotEmpty() }
        // Well past the 600 ms this removal has without TalkBack.
        Thread.sleep(2_500)
        compose.waitForIdle()
        compose.runOnIdle { assertEquals(emptyList(), removed.map { it.id }) }
        compose.onNodeWithText("Undo").assertExists()
    }

    @Test
    fun `moving down and back up the rows keeps each focused row's header in view, never above the page`() {
        show(more = 6)
        compose.onNodeWithContentDescription("Severance").performSemanticsAction(SemanticsActions.RequestFocus)
        val rows = listOf("Popular" to "Andor") + (1..5).map { "Row $it" to "Title $it.1" }
        // Down brings a row up from the foot; Up brings one down from above the page to the anchor, where its
        // header has to fit under the top edge (the pill's foot in the app; the Fire TV walk, FTV4).
        val walk = rows.map { Key.DirectionDown to it } + rows.reversed().drop(1).map { Key.DirectionUp to it }
        for ((key, row) in walk) {
            val (title, first) = row
            compose.onNode(isFocused()).performKeyInput { pressKey(key) }
            waitFor("$title's first tile takes the focus") { focused(hasContentDescription(first)) }
            compose.mainClock.advanceTimeBy(1_000)
            compose.waitForIdle()
            // Unclipped: a header scrolled under the page's top edge is clipped to it, not moved.
            val header = compose.onNodeWithText(title).getUnclippedBoundsInRoot()
            assertTrue(header.top >= 0.dp, "$title's header went above the page: ${header.top}")
        }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `a hero with a long name and no logo keeps its Details label whole`() {
        // The Fire TV walk (FTV4): a two-line name in place of a logo and three lines of text pushed the
        // buttons past the card's foot, and the label under Details was cut.
        val tile =
            Tile(id = "tt9", kind = "movie", name = "Project Hail Mary and the Very Long Title", posterShape = "poster")
        val text = (1..60).joinToString(" ") { "word$it" }
        show(hero = Hero(tile, "#1 in Popular Movies on Cinemeta", text, listOf("Drama"), "2 h 36 min"))
        waitFor("the hero draws") {
            compose.onAllNodesWithContentDescription("Details").fetchSemanticsNodes().isNotEmpty()
        }
        val circle = compose.onNodeWithContentDescription("Details").fetchSemanticsNode()
        // The button's column: the ring, the 10 dp gap and the label under it.
        val button = checkNotNull(circle.layoutInfo.parentInfo)
        // In the TV canvas's units: the ring is 72 of them, whatever the test screen's density.
        val unit = circle.size.height / 72f
        val whole = button.height / unit
        val ring = 72f
        assertTrue(whole >= ring + 10 + 24, "the Details label got ${whole - ring - 10} dp of its line")
    }
}
