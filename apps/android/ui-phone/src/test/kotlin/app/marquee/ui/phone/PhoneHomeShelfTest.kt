package app.marquee.ui.phone

import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
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
import app.marquee.core.views.HomeView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
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
 * The phone's Home as the iPhone TV app lays it out (the home-phone frame; design/DECISION.md, Home and Shelves),
 * measured where it lands: the hero's words over white art, the 390 by 680 shape, Continue Watching with its line,
 * bare posters, on a screen as tall as the frame (1470). With MARQUEE_SHOTS set to a folder holding `art/backdrop.jpg` and `art/logo.png`, it writes
 * `phone-home.jpg` there for review.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "w390dp-h1470dp-xxhdpi")
class PhoneHomeShelfTest {
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
        line: String? = null,
        progress: Float? = null,
    ) = Tile(
        id = id,
        kind = kind,
        name = name,
        poster = art("backdrop.jpg"),
        background = art("backdrop.jpg"),
        posterShape = "poster",
        releaseInfo = line,
        progress = progress,
        ratings = Ratings("8.4", "Fresh"),
    )

    private fun show() {
        val hero =
            Hero(
                tile(
                    "tt1",
                    "Night of the Living Dead",
                    "movie",
                    "1968",
                ).copy(logo = art("logo.png"), ratings = Ratings("7.8", "96%")),
                "#1 in Popular Movies",
                "A ragtag group barricade themselves in an old Pennsylvania farmhouse.",
                listOf("Horror", "Thriller"),
                "1 h 36 min",
            )
        val snapshot =
            HomeSnapshot(
                HomeHeader(hero),
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
                        (1..5).map {
                            tile("m$it", "Movie $it", "movie", "2024")
                        },
                    ),
                    HomeRow(
                        "cinemeta:series:top",
                        "Popular Series",
                        "Cinemeta",
                        RowState.Ready,
                        (1..5).map {
                            tile("s$it", "Series $it", "series", "2022–")
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
        compose.setContent {
            MarqueeTheme(Form.Phone) { PhoneHome(home, onOpen = {}, onPlay = {}, onSetup = {}, onAddons = {}) }
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
    fun `the hero is the frame's 390 by 680, and every word on it keeps 4_5 to 1 over white art where it lands`() {
        show()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val hero = compose.onNodeWithTag(PHONE_HERO_TAG).fetchSemanticsNode()
        val height = hero.size.height / root.width * 390f
        assertEquals(680f, height, 4f, "the hero is $height tall at 390 wide")
        val phone = formPalette(Form.Phone)
        val fill = Color(0x29FFFFFF)
        val heroTop = hero.positionInRoot.y
        val heroHeight = hero.size.height.toFloat()
        val lines =
            listOf(
                Triple("the genre line", bounds("Movie · Horror · Thriller"), phone.text to Color.Transparent),
                Triple("the ratings", bounds("RT 96%"), phone.muted to Color.Transparent),
                Triple("the reason", bounds("#1 in Popular Movies"), phone.text to Color.Transparent),
                Triple(
                    "Details",
                    compose.onNodeWithContentDescription("Details").fetchSemanticsNode().boundsInRoot,
                    Color.White to fill,
                ),
            )
        for ((name, at, ink) in lines) {
            val y = (heroTop + heroHeight - at.top) / heroHeight
            val art = phone.bg.copy(alpha = Scrims.alpha(Scrims.phoneHomeFoot, y)).compositeOver(Color.White)
            val ratio = contrast(ink.first, ink.second, art)
            println("phone-hero {\"line\":\"$name\",\"y\":$y,\"ratio\":$ratio}")
            assertTrue(ratio >= 4.5f, "$name at y $y: $ratio")
        }
        if (out != null) save("phone-home")
    }

    @Test
    fun `Continue Watching is 16 by 9 with its line, and a catalog's posters are 99 by 149 and bare`() {
        show()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val dp = root.width / 390f
        val still = compose.onNodeWithContentDescription("Severance").fetchSemanticsNode().size
        assertEquals(239f, still.width / dp, 1f)
        assertEquals(134f, still.height / dp, 1f)
        compose.onNodeWithText("S2:E3 · 36 min left", useUnmergedTree = true).assertExists()
        val node = compose.onNodeWithContentDescription("Movie 1").fetchSemanticsNode()
        assertEquals(99f, node.size.width / dp, 1f)
        assertEquals(149f, node.size.height / dp, 1f)
        // No name, year or score under a catalog's poster (with no picture the name stands in on the art).
        val foot = node.positionInRoot.y + node.size.height
        for (text in listOf("Movie 1", "2024", "IMDb 8.4")) {
            val under =
                compose.onAllNodesWithText(text, useUnmergedTree = true).fetchSemanticsNodes().filter {
                    it.positionInRoot.y >=
                        foot
                }
            assertTrue(
                under.none { it.positionInRoot.x < node.positionInRoot.x + node.size.width },
                "$text under the poster",
            )
        }
    }

    private fun save(name: String) {
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        File(out, "$name.jpg").outputStream().use { image.compress(Bitmap.CompressFormat.JPEG, 90, it) }
    }
}
