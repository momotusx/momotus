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
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
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
    ) = Hero(
        Tile(id = id, kind = kind, name = name, posterShape = "poster"),
        rowId = "cinemeta:movie:top",
        reason = "#1 in Popular",
    )

    private fun show() {
        val snapshot =
            HomeSnapshot(
                HomeHeader(
                    listOf(
                        hero("tt1", "First Movie", "movie"),
                        hero("tt2", "The Series", "series"),
                        hero("tt3", "Last Movie", "movie"),
                    ),
                ),
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
                TvHome(home, state, onOpen = {}, onPlay = {}, onSetup = {}, onAddons = {})
            }
        }
        home.open()
        waitFor("the first page's Play has the focus") { focused(hasContentDescription("Play First Movie")) }
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
    fun `a hero button with the focus holds the timed pages, and they go on once it leaves`() {
        show()
        frames(20_000)
        assertEquals(true, showing("First Movie"), "the page changed under the focused Play")
        assertEquals(true, focused(hasContentDescription("Play First Movie")))
        // Down into the row: the hero is free to page.
        press(Key.DirectionDown)
        waitFor("a row tile has the focus") { focused(hasContentDescription("Row title 1", substring = true)) }
        frames(9_000)
        waitFor("the timer paged on") { showing("The Series") }
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
}
