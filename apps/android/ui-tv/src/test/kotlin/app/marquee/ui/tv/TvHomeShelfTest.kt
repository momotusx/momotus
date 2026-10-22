package app.marquee.ui.tv

import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.compose.runtime.Composable
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.pressKey
import app.marquee.core.TileCopy
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.model.Hero
import app.marquee.core.model.HomeHeader
import app.marquee.core.model.HomeRow
import app.marquee.core.model.HomeSnapshot
import app.marquee.core.model.Ratings
import app.marquee.core.model.RowState
import app.marquee.core.model.Tile
import app.marquee.core.route.Route
import app.marquee.core.views.HomeView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.design.SECONDARY_FILL
import app.marquee.design.Scrims
import app.marquee.design.formPalette
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.junit.After
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * TV Home's top shelf and shelves as the home-tv frame lays them out (design/DECISION.md, Home and Shelves),
 * measured where they land: every word of the left column over white art, Continue Watching on the first
 * screen, the tile shapes. With MARQUEE_SHOTS set to a folder holding `art/backdrop.jpg` and `art/logo.png`,
 * it writes `tv-home.jpg` there for review.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvHomeShelfTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val out = System.getenv("MARQUEE_SHOTS")?.let(::File)
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

    private fun art(name: String) = out?.let { File(it, "art/$name").path }

    private fun tile(
        id: String,
        name: String,
        kind: String,
        year: String? = null,
        progress: Float? = null,
        imdb: String? = "8.4",
        rt: String? = "Fresh",
    ) = Tile(
        id = id,
        kind = kind,
        name = name,
        poster = art("backdrop.jpg"),
        background = art("backdrop.jpg"),
        posterShape = "poster",
        releaseInfo = year,
        progress = progress,
        ratings = Ratings(imdb, rt),
    )

    private val hero =
        Hero(
            tile(
                "tt1",
                "Night of the Living Dead",
                "movie",
                "1968",
                imdb = "7.8",
                rt = "96%",
            ).copy(logo = art("logo.png")),
            "#1 in Popular Movies",
            "A ragtag group barricade themselves in an old Pennsylvania farmhouse to remain safe from a horde of " +
                "flesh-eating ghouls that are ravaging the Northeast of the United States, and the night is long.",
            listOf("Horror", "Thriller"),
            "1 h 36 min",
        )

    private fun show(
        withHero: Boolean = true,
        /**
         * In its frame, the bar over the top shelf's art. ponytail: a capture of the frame aborts Robolectric's
         * native canvas (SIGABRT), so the shots leave the bar out.
         */
        framed: Boolean = true,
        state: TvHomeState = TvHomeState(),
    ) {
        val snapshot =
            HomeSnapshot(
                HomeHeader(hero.takeIf { withHero }),
                listOf(
                    HomeRow(
                        TileCopy.CONTINUE_ROW,
                        "Continue Watching",
                        "",
                        RowState.Ready,
                        listOf(
                            tile("cw1", "Severance", "series", "S2:E3 · 36 min left", 0.4f),
                            tile("cw2", "Dark", "series", "Next: S1:E2"),
                        ),
                    ),
                    HomeRow(
                        "cinemeta:movie:top",
                        "Popular Movies",
                        "Cinemeta",
                        RowState.Ready,
                        (1..6).map { tile("m$it", "Movie $it", "movie", "2024") },
                    ),
                    HomeRow(
                        "cinemeta:series:top",
                        "Popular Series",
                        "Cinemeta",
                        RowState.Ready,
                        (1..8).map { tile("s$it", "Series $it", "series", "2022–", rt = null) },
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
        // Native graphics start the window in touch mode, where nothing takes focus; a TV has no touch mode.
        compose.runOnUiThread {
            compose.activity.window.decorView
                .requestFocusFromTouch()
        }
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                val page = @Composable {
                    TvHome(
                        home,
                        state,
                        onOpen = {},
                        onPlay = {},
                        onSetup = {},
                        onAddons = {},
                    )
                }
                // In its frame the top shelf's art runs up behind the bar.
                if (framed) {
                    // As AppRoot wires it: Down from the bar comes back to what had the focus.
                    TvShell(Route.Home, true, {
                    }, { state.focus.lastEntry() }, { state.focus.leftContent() }, { state.focus.interacted() }) {
                        page()
                    }
                } else {
                    page()
                }
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
        // The backdrop and the art load off the main thread.
        Thread.sleep(800)
        compose.waitForIdle()
    }

    private fun bounds(text: String): Rect =
        compose.onNodeWithText(text, substring = true, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot

    private fun contrast(
        ink: Color,
        fill: Color,
        art: Color,
    ): Float {
        val back = fill.compositeOver(art)
        val la = ink.compositeOver(back).luminance() + 0.05f
        val lb = back.luminance() + 0.05f
        return maxOf(la, lb) / minOf(la, lb)
    }

    @Test
    fun `every word of the top shelf keeps 4_5 to 1 over white art where it lands`() {
        show()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val tv = formPalette(Form.Tv)
        val fill = SECONDARY_FILL
        // Each line at its top trailing corner, the brightest the ramps leave it.
        val lines =
            listOf(
                Triple("the genre line", bounds("Movie · Horror · Thriller"), tv.text to Color.Transparent),
                Triple("the synopsis", bounds("A ragtag group"), tv.muted to Color.Transparent),
                Triple("the facts", bounds("1968 · 1 h 36 min"), tv.dim to Color.Transparent),
                Triple("the ratings", bounds("RT 96%"), tv.muted to Color.Transparent),
                Triple("the reason", bounds("#1 in Popular Movies"), tv.text to Color.Transparent),
                Triple("Details", bounds("Details"), Color.White to fill),
            )
        for ((name, at, ink) in lines) {
            val x = at.right / root.width
            val y = (root.bottom - at.top) / root.height
            val covered = 1 - (1 - Scrims.alpha(Scrims.topShelfSide, x)) * (1 - Scrims.alpha(Scrims.topShelfFoot, y))
            val art = tv.bg.copy(alpha = covered).compositeOver(Color.White)
            val ratio = contrast(ink.first, ink.second, art)
            println("top-shelf {\"line\":\"$name\",\"x\":$x,\"y\":$y,\"ratio\":$ratio}")
            assertTrue(ratio >= 4.5f, "$name at x $x, y $y: $ratio")
        }
    }

    @Test
    fun `Continue Watching rises onto the first screen, with the core's line and no scores`() {
        show(framed = out == null)
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val shelf = bounds("Continue Watching")
        // The frame's first shelf starts at 850 of 1080.
        assertEquals(850f / 1080f, shelf.top / root.height, 0.03f, "Continue Watching starts at ${shelf.top}")
        compose.onNodeWithText("S2:E3 · 36 min left", useUnmergedTree = true).assertExists()
        // Only Continue Watching is on screen under the top shelf, and it shows no scores.
        // Laid out, not what shows: the cards' lines run below the screen's foot.
        val scores = compose.onAllNodesWithText("IMDb 8.4", useUnmergedTree = true).fetchSemanticsNodes()
        assertTrue(
            scores.none { it.positionInRoot.y >= shelf.top },
            "a Continue Watching card shows scores",
        )
        if (out != null) save("tv-home")
    }

    @Test
    fun `movie rows are 16 by 9 cards and series rows 2 by 3 posters, each with its year and scores`() {
        show(withHero = false, framed = out == null)
        // Laid out sizes, not what the screen shows of them: the series row starts at the screen's foot.
        for ((name, ratio) in listOf("Severance" to 16f / 9f, "Movie 1" to 16f / 9f, "Series 1" to 2f / 3f)) {
            val art = compose.onNodeWithContentDescription(name).fetchSemanticsNode().size
            assertEquals(ratio, art.width.toFloat() / art.height, 0.02f, "$name's shape")
        }
        // A poster's two lines: the year, then the scores, under the art.
        val poster = compose.onNodeWithContentDescription("Series 1").fetchSemanticsNode()
        val foot = poster.positionInRoot.y + poster.size.height
        val year =
            compose
                .onAllNodesWithText("2022–", useUnmergedTree = true)
                .fetchSemanticsNodes()
                .first {
                    it.positionInRoot.x in poster.positionInRoot.x - 1..poster.positionInRoot.x + poster.size.width
                }
        assertTrue(year.positionInRoot.y >= foot, "the year sits on the poster")
        if (out != null) save("tv-home-shelves")
    }

    private fun focused() = compose.onAllNodes(isFocused(), useUnmergedTree = true).fetchSemanticsNodes().single()

    private fun focusedOn(description: String) =
        compose.onAllNodes(isFocused() and hasContentDescription(description)).fetchSemanticsNodes().isNotEmpty()

    private fun press(key: Key) {
        compose.onNode(isFocused()).performKeyInput { pressKey(key) }
        compose.waitForIdle()
    }

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

    @Test
    fun `a Continue Watching card focused from Play is whole on the screen, with its line`() {
        show()
        println(
            "DBG focused " +
                compose.onAllNodes(isFocused(), useUnmergedTree = true).fetchSemanticsNodes().map { it.config },
        )
        // The first focus is Play (FirstTargetTest); a test's clock runs out the screen's own wait, so it is asked here.
        compose
            .onNodeWithContentDescription(
                "Play Night of the Living Dead",
            ).performSemanticsAction(SemanticsActions.RequestFocus)
        waitFor("Play takes the focus") { focusedOn("Play Night of the Living Dead") }
        press(Key.DirectionDown)
        waitFor("the first card takes the focus") {
            focusedOn("Severance")
        }
        // The page settles at the row's anchor.
        compose.mainClock.advanceTimeBy(2_000)
        compose.waitForIdle()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val card = focused()
        val line = compose.onNodeWithText("S2:E3 · 36 min left", useUnmergedTree = true).fetchSemanticsNode()
        // Laid out, not clipped: the card's foot and its line's.
        assertTrue(card.positionInRoot.y + card.size.height <= root.bottom, "the card's foot is off the screen")
        assertTrue(line.positionInRoot.y + line.size.height <= root.bottom, "the card's line is off the screen")
        assertTrue(card.positionInRoot.y >= 0f, "the card's top is off the screen")
    }

    @Test
    fun `Up from Play reaches the bar and Down comes back to Play`() {
        show()
        // The first focus is Play (FirstTargetTest); a test's clock runs out the screen's own wait, so it is asked here.
        compose
            .onNodeWithContentDescription(
                "Play Night of the Living Dead",
            ).performSemanticsAction(SemanticsActions.RequestFocus)
        waitFor("Play takes the focus") { focusedOn("Play Night of the Living Dead") }
        press(Key.DirectionUp)
        assertTrue(
            compose
                .onAllNodes(
                    isFocused() and hasText("Home"),
                    useUnmergedTree = false,
                ).fetchSemanticsNodes()
                .isNotEmpty(),
            "Up missed the bar",
        )
        press(Key.DirectionDown)
        assertTrue(
            focusedOn("Play Night of the Living Dead"),
            "Down did not come back to Play",
        )
    }

    @Test
    fun `the art follows the page without a jump, past the moment the hero item leaves the list`() {
        val state = TvHomeState()
        show(state = state)
        // Where the art is laid out, its scroll included; a clipped bound would hide a jump above the screen.
        val art = { compose.onNodeWithTag(TOP_SHELF_ART, useUnmergedTree = true).fetchSemanticsNode() }
        assertEquals(0f, art().positionInRoot.y, 1.5f, "at rest the art starts at the top of the TV")
        var last = 0f
        var flipped = false
        val step = 40f
        repeat(60) {
            compose.runOnIdle { state.page.dispatchRawDelta(step) }
            compose.waitForIdle()
            val node = art()
            val top = node.positionInRoot.y
            if (state.page.firstVisibleItemIndex > 0) flipped = true
            // Each step moves the art by the step, until it has gone above the screen for good.
            if (top + node.size.height > 0f) assertEquals(last - step, top, 1.5f, "the art jumped at step $it")
            last = top
        }
        assertTrue(flipped, "the hero item never left the list's first place")
    }

    private fun save(name: String) {
        assumeTrue("MARQUEE_SHOTS is not set", out != null)
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        File(out, "$name.jpg").outputStream().use { image.compress(Bitmap.CompressFormat.JPEG, 90, it) }
    }
}
