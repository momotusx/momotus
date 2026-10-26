package app.marquee.ui.phone

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.test.swipeLeft
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
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
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
    )

    private fun show() {
        val snapshot =
            HomeSnapshot(
                HomeHeader(listOf(hero("tt1", "Page One"), hero("tt2", "Page Two"), hero("tt3", "Page Three"))),
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
            MarqueeTheme(Form.Phone) { PhoneHome(home, onOpen = {}, onPlay = {}, onSetup = {}, onAddons = {}) }
        }
        home.open()
        waitFor("the first page") { settledOn("Page One") }
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
}
