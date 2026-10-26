package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.model.Hero
import app.marquee.core.model.HomeHeader
import app.marquee.core.model.HomeRow
import app.marquee.core.model.HomeSnapshot
import app.marquee.core.model.HomeUpdate
import app.marquee.core.model.PlayTarget
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
import kotlinx.coroutines.launch
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

/**
 * The TV hero with three pages, a movie, a series and a movie (review r1, H1, H2, M2): Right and Left page with the
 * focus on the same button, or on Details where a page has no Play; a hero button with focus holds the timed
 * pages; Back aims at the page shown, not the first page's Play.
 */
@OptIn(androidx.compose.ui.test.ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvHeroCarouselTest {
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
    private lateinit var home: HomeView

    /** A scope of the composition, with its frame clock, as the app's Back handler has. */
    private lateinit var ui: CoroutineScope
    private val state = TvHomeState()

    @After
    fun stop() {
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    private fun hero(
        id: String,
        name: String,
        kind: String,
        play: PlayTarget? = if (kind == "movie") PlayTarget(id, "Play") else null,
        pending: Boolean = false,
    ) = Hero(
        Tile(id = id, kind = kind, name = name, posterShape = "poster"),
        rowId = "cinemeta:movie:top",
        reason = "#1 in Popular",
        play = play,
        playPending = pending,
    )

    private fun show(
        pages: List<Hero> =
            listOf(
                hero("tt1", "First Movie", "movie"),
                hero("tt2", "The Series", "series"),
                hero("tt3", "Last Movie", "movie"),
            ),
        start: SemanticsMatcher = hasContentDescription("Play First Movie"),
        onPlay: (Tile) -> Unit = {},
    ) {
        val snapshot =
            HomeSnapshot(
                HomeHeader(pages),
                listOf(
                    HomeRow(
                        "cinemeta:movie:top",
                        "Popular",
                        "",
                        RowState.Ready,
                        (1..4).map {
                            Tile(
                                id = "tt9$it",
                                kind = "movie",
                                name = "Row title $it",
                                posterShape = "poster",
                            )
                        },
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
        home = HomeView(client, scope)
        // The hero's clock runs on frames: time moves only when a test says.
        compose.mainClock.autoAdvance = false
        compose.setContent {
            ui = rememberCoroutineScope()
            MarqueeTheme(Form.Tv) {
                TvHome(home, state, onOpen = {}, onPlay = onPlay, onSetup = {}, onAddons = {})
            }
        }
        home.open()
        waitFor("the start focus") { focused(start) }
    }

    private fun frames(ms: Long) {
        // In steps, so the frame-driven clock sees time pass.
        repeat((ms / 100).toInt()) {
            compose.mainClock.advanceTimeBy(100)
            Snapshot.sendApplyNotifications()
        }
        compose.waitForIdle()
    }

    private fun waitFor(
        what: String,
        check: () -> Boolean,
    ) {
        val end = System.currentTimeMillis() + 60_000
        while (!check()) {
            check(System.currentTimeMillis() < end) { "waited 60 s and it never happened: $what" }
            Snapshot.sendApplyNotifications()
            compose.mainClock.advanceTimeBy(50)
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    private fun focused(matcher: SemanticsMatcher) =
        compose.onAllNodes(matcher and isFocused()).fetchSemanticsNodes().isNotEmpty()

    private fun press(key: Key) {
        compose.onNode(isFocused()).performKeyInput { pressKey(key) }
        frames(1_000)
    }

    private fun showing(name: String) = compose.onAllNodesWithText(name).fetchSemanticsNodes().isNotEmpty()

    @Test
    fun `Right on Details pages on with the focus kept, and Left from Play onto a series lands on Details`() {
        show()
        press(Key.DirectionRight)
        waitFor("Details has the focus") { focused(hasText("Details")) }
        press(Key.DirectionRight)
        waitFor("the series page") { showing("The Series") && !showing("First Movie") }
        assertEquals(true, focused(hasText("Details")), "Details kept the focus on the series page")
        press(Key.DirectionRight)
        waitFor("the last page") { showing("Last Movie") }
        // On the last page, Left from Details is a plain move to Play; Left from Play pages back to the series,
        // which has no Play: the focus goes to its Details, never into nothing.
        press(Key.DirectionLeft)
        waitFor("Play has the focus") { focused(hasContentDescription("Play Last Movie")) }
        press(Key.DirectionLeft)
        waitFor("back on the series page") { showing("The Series") && !showing("Last Movie") }
        waitFor("Details took the focus") { focused(hasText("Details")) }
    }

    @Test
    fun `the timed pages run with the focus on Play, and the focus goes with the page to the same button`() {
        // The owner, 2026-10-26: the TV hero slides while focus rests on it.
        show(
            listOf(
                hero("tt1", "First Movie", "movie"),
                hero("tt4", "Second Movie", "movie"),
                hero("tt2", "The Series", "series"),
            ),
        )
        frames(9_000)
        waitFor("the timer paged with the focus on Play") { showing("Second Movie") && !showing("First Movie") }
        waitFor("the new page's Play has the focus") { focused(hasContentDescription("Play Second Movie")) }
        // Onto a page with no Play: its Details takes the focus, never nothing.
        frames(9_000)
        waitFor("the series page") { showing("The Series") && !showing("Second Movie") }
        waitFor("Details took the focus") { focused(hasText("Details")) }
    }

    @Test
    fun `a series page with the core's Play takes the focus on Play, in its words, and plays its episode`() {
        // The owner, 2026-10-26: Carrie showed Details alone. The core names the episode; the page says it.
        var played: Tile? = null
        show(
            listOf(
                hero("tt1", "First Movie", "movie"),
                hero("tt2", "The Series", "series", PlayTarget("tt2:1:1", "Play S1:E1")),
            ),
            onPlay = { played = it },
        )
        frames(9_000)
        waitFor("the series page") { showing("The Series") && !showing("First Movie") }
        waitFor("its Play has the focus") { focused(hasContentDescription("Play S1:E1 The Series")) }
        assertEquals(true, showing("Play S1:E1"), "the button reads the core's label")
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionCenter) }
        waitFor("Play was chosen") { played != null }
        assertEquals("tt2:1:1", played?.videoId, "Play opens the core's episode")
    }

    @Test
    fun `a key press holds the timed pages for 5 s, then they go on`() {
        show()
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionRight) }
        waitFor("Details has the focus") { focused(hasText("Details")) }
        // Past a whole dwell: without the hold the page would have gone at 8 s; with it, not before 5 + 8.
        frames(10_000)
        assertEquals(
            true,
            showing("First Movie") && !showing("The Series"),
            "the page changed within 5 s of a key plus its dwell",
        )
        frames(5_000)
        waitFor("the timer went on after the hold") { showing("The Series") }
    }

    @Test
    fun `a remembered Play on a page without one lands on Details at once`() {
        // Review r2 N2, r3 L3: a restore asking for Play on a series page falls back to Details, not the 15 s wait.
        show(
            listOf(hero("tt2", "The Series", "series"), hero("tt1", "First Movie", "movie")),
            start = hasText("Details"),
        )
        press(Key.DirectionDown)
        waitFor("a row tile has the focus") { focused(hasContentDescription("Row title 1", substring = true)) }
        ui.launch { restoreFocus(home, state, "hero:play") }
        frames(1_000)
        assertEquals(true, focused(hasText("Details")), "the restore did not land on Details within a second")
    }

    @Test
    fun `Back from a row aims at the page shown, a series page's Details, not the first page's Play`() {
        show()
        press(Key.DirectionRight)
        press(Key.DirectionRight)
        waitFor("the series page") { showing("The Series") && !showing("First Movie") }
        press(Key.DirectionDown)
        waitFor("a row tile has the focus") { focused(hasContentDescription("Row title 1", substring = true)) }
        ui.launch { focusFirstRow(home, state) }
        waitFor("Back put the focus on the series page's Details") { focused(hasText("Details")) }
        assertEquals(true, state.focus.onFirstRow.value, "Back landed on the start row, so the next Back leaves")
    }

    @Test
    fun `a page whose Play is still coming keeps its place, so Details never moves when it lands`() {
        // Review r4: the core names a series' episode a moment after its page.
        val reading = hero("tt2", "The Series", "series", pending = true)
        show(listOf(reading), start = hasText("Details"))
        assertEquals(1, compose.onAllNodesWithTag(TV_PLAY_SLOT).fetchSemanticsNodes().size, "Play's place is kept")
        val before =
            compose
                .onNodeWithText("Details")
                .fetchSemanticsNode()
                .boundsInRoot.left
        val landed = reading.copy(play = PlayTarget("tt2:1:1", "Play S1:E1"), playPending = false)
        val update = CoreJson.encodeToString(HomeUpdate.serializer(), HomeUpdate.Header(HomeHeader(listOf(landed))))
        client.onMessage("""{"sub":7,"updates":[$update]}""")
        waitFor("Play landed") { compose.onAllNodesWithText("Play S1:E1").fetchSemanticsNodes().isNotEmpty() }
        assertEquals(0, compose.onAllNodesWithTag(TV_PLAY_SLOT).fetchSemanticsNodes().size)
        val after =
            compose
                .onNodeWithText("Details")
                .fetchSemanticsNode()
                .boundsInRoot.left
        assertEquals(before, after, 1f, "Details moved when Play landed")
    }

    @Test
    fun `Play's place is as wide as its longest words, so Details stays put whichever label lands`() {
        // The lead after review r4: "Resume S12:E10" must not push Details either.
        val page = hero("tt1", "First Movie", "movie")
        show(listOf(page))

        fun details() =
            compose
                .onNodeWithText("Details")
                .fetchSemanticsNode()
                .boundsInRoot.left
        val lefts =
            listOf("Play", "Play S1:E1", "Resume S12:E10").map { label ->
                val next = page.copy(play = PlayTarget("tt1", label))
                val update =
                    CoreJson.encodeToString(HomeUpdate.serializer(), HomeUpdate.Header(HomeHeader(listOf(next))))
                client.onMessage("""{"sub":7,"updates":[$update]}""")
                waitFor("the label $label") { compose.onAllNodesWithText(label).fetchSemanticsNodes().isNotEmpty() }
                details()
            }
        assertEquals(1, lefts.distinct().size, "Details moved with Play's words: $lefts")
    }
}
