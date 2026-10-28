package app.marquee.ui.phone

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeLeft
import androidx.compose.ui.test.swipeRight
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
import app.marquee.design.HeroTrailers
import app.marquee.design.MarqueeTheme
import app.marquee.design.rememberTrailerHost
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
import java.io.File
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The phone's hero with three pages (review r1, M2, L3): the timer goes through all three and on to the first
 * again, and a swipe by hand stops the timed pages for the visit.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w390dp-h844dp")
class PhoneHeroCarouselTest {
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

    private fun hero(
        id: String,
        name: String,
    ) = Hero(
        Tile(id = id, kind = "movie", name = name, posterShape = "poster"),
        rowId = "row",
        reason = "Popular Movies",
        play = PlayTarget(id, "Play"),
        playPending = false,
    )

    private val all = listOf(hero("tt1", "Page One"), hero("tt2", "Page Two"), hero("tt3", "Page Three"))

    private fun show(
        pages: List<Hero> = all,
        onPlay: (Tile) -> Unit = {},
    ) {
        val snapshot =
            HomeSnapshot(
                HomeHeader(pages),
                listOf(
                    HomeRow(
                        "row",
                        "Popular Movies",
                        "",
                        RowState.Ready,
                        (1..3).map {
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
        val home = HomeView(client, scope)
        compose.mainClock.autoAdvance = false
        compose.setContent {
            MarqueeTheme(Form.Phone) { PhoneHome(home, onOpen = {}, onPlay = onPlay, onSetup = {}, onAddons = {}) }
        }
        home.open()
        waitFor("the first page") { settledOn(pages.first().tile.name) }
    }

    /** Whether [name]'s words are on screen; the pager also composes its neighbours, off screen. */
    private fun onScreen(name: String): Boolean {
        val width =
            compose
                .onRoot()
                .fetchSemanticsNode()
                .size.width
        return compose.onAllNodesWithText(name).fetchSemanticsNodes().any {
            it.boundsInRoot.right > 0f &&
                it.boundsInRoot.left < width
        }
    }

    /** Only [name]'s page is on screen: no slide is under way. */
    private fun settledOn(name: String) =
        listOf("Page One", "Page Two", "Page Three").all {
            (it == name) ==
                onScreen(it)
        }

    private fun frames(ms: Long) {
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

    @Test
    fun `the timer goes through every page and on to the first again`() {
        show()
        for (name in listOf("Page Two", "Page Three", "Page One")) {
            frames(9_000)
            waitFor("the timer reached $name") { settledOn(name) }
        }
    }

    @Test
    fun `a swipe by hand stops the timed pages`() {
        show()
        compose.onNodeWithText("Page One").performTouchInput { swipeLeft() }
        waitFor("the swipe reached the second page") { settledOn("Page Two") }
        frames(30_000)
        assertTrue(settledOn("Page Two"), "the timer paged after a swipe by hand")
    }

    @Test
    fun `a carousel that began with one page swipes back from its first page to its last once it grew`() {
        // Review r2 N1, r3 L3: the pager leaves its first loop when the list grows.
        show(all.take(1))
        val update = CoreJson.encodeToString(HomeUpdate.serializer(), HomeUpdate.Header(HomeHeader(all)))
        client.onMessage("""{"sub":7,"updates":[$update]}""")
        waitFor("the list grew to three pages") {
            compose.onAllNodesWithContentDescription("Page 1 of 3, Page One").fetchSemanticsNodes().isNotEmpty()
        }
        waitFor("still on the first page") { settledOn("Page One") }
        compose.onNodeWithText("Page One").performTouchInput { swipeRight() }
        waitFor("the swipe back reached the last page") { settledOn("Page Three") }
    }

    @Test
    fun `a series page plays the core's episode, in the core's words`() {
        // The owner, 2026-10-26: Carrie showed Details alone.
        var played: Tile? = null
        val series =
            Hero(
                Tile(id = "tt7", kind = "series", name = "The Series", posterShape = "poster"),
                rowId = "row",
                reason = "Popular Series",
                play = PlayTarget("tt7:2:3", "Resume S2:E3", resumeMs = 600_000),
                playPending = false,
            )
        show(listOf(series, all[0]), onPlay = { played = it })
        waitFor("the series page") { settledOn("The Series") }
        // The button says it; the hero carries it as its action (the buttons are for sighted touch).
        compose.onNodeWithText("Resume S2:E3", useUnmergedTree = true).assertExists()
        val play =
            compose
                .onAllNodesWithTag(PHONE_HERO_TAG)
                .fetchSemanticsNodes()
                .flatMap { it.config.getOrElse(SemanticsActions.CustomActions) { emptyList() } }
                .single { it.label == "Resume S2:E3 The Series" }
        compose.runOnIdle { play.action() }
        assertEquals("tt7:2:3", played?.videoId, "Play opens the core's episode")
    }

    @Test
    fun `the sound switch is one name, off at launch, and keeps the session's choice when drawn again`() {
        HeroTrailers.sound = false
        var drawn by mutableStateOf(true)
        try {
            compose.setContent {
                MarqueeTheme(Form.Phone) {
                    val trailers = rememberTrailerHost()
                    if (drawn) TrailerSoundSwitch(trailers)
                }
            }
            compose.onNodeWithTag(PHONE_HERO_SOUND).assertExists()
            val switch = compose.onNodeWithContentDescription(SOUND_LABEL)
            switch.assertIsOff()
            switch.performClick()
            compose.waitForIdle()
            switch.assertIsOn()
            assertTrue(HeroTrailers.sound, "the press is the session's sound")
            drawn = false
            compose.waitForIdle()
            drawn = true
            compose.waitForIdle()
            compose.onNodeWithContentDescription(SOUND_LABEL).assertIsOn()
        } finally {
            HeroTrailers.sound = false
        }
    }

    @Test
    fun `a page whose Play is still coming keeps its place, so Details never moves when it lands`() {
        // Review r4: the core names a series' episode a moment after its page.
        val reading =
            Hero(
                Tile(id = "tt7", kind = "series", name = "The Series", posterShape = "poster"),
                rowId = "row",
                reason = "Popular Series",
                playPending = true,
            )
        show(listOf(reading))
        val buttons = {
            compose
                .onNodeWithTag(
                    PHONE_HERO_BUTTONS,
                    useUnmergedTree = true,
                ).fetchSemanticsNode()
                .boundsInRoot
        }
        val before = buttons()
        val landed = reading.copy(play = PlayTarget("tt7:1:1", "Play S1:E1"), playPending = false)
        val update = CoreJson.encodeToString(HomeUpdate.serializer(), HomeUpdate.Header(HomeHeader(listOf(landed))))
        client.onMessage("""{"sub":7,"updates":[$update]}""")
        waitFor("Play landed") {
            compose.onAllNodesWithText("Play S1:E1", useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() &&
                compose.onAllNodesWithTag(PHONE_HERO_BUTTONS, useUnmergedTree = true).fetchSemanticsNodes().size == 1
        }
        // The row is centered: the same width puts Details where it was.
        val after = buttons()
        assertEquals(before.left to before.right, after.left to after.right, "the buttons moved when Play landed")
    }

    @Test
    fun `the sound switch is drawn from the moment a trailer plays, when a kept sound starts`() {
        // Review r4, W2: drawn from `shown` (4.5 s later), a kept sound played with no switch to stop it.
        val source = File("src/main/kotlin/app/marquee/ui/phone/PhoneHome.kt").readText()
        assertTrue("if (trailer.playing && (!HeroTrailers.walk" in source, "the switch follows trailer.playing")
        assertTrue("trailer.shown && (!HeroTrailers.walk" !in source, "the switch waits for the art to fade")
    }
}
