package app.marquee.ui.tv

import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import androidx.activity.ComponentActivity
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.unit.dp
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.model.PlayTarget
import app.marquee.core.model.Ratings
import app.marquee.core.model.Slot
import app.marquee.core.model.Tile
import app.marquee.core.model.TitleDetails
import app.marquee.core.model.TitleHeader
import app.marquee.core.model.TitleScores
import app.marquee.core.model.TitleSnapshot
import app.marquee.core.model.TitleState
import app.marquee.core.model.TitleUpdate
import app.marquee.core.views.TitleView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.serialization.builtins.ListSerializer
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
 * The TV title page's ratings row, genre chips and rated cover (plans/title-page-ratings-genres-similar.md): the
 * numbers spoken once each; the chips reached with Up from Play, past a cut synopsis; the rated cover never a focus
 * stop, so the page's D-pad order is the same with and without it; Back from a title opened in More like this lands
 * on its tile.
 */
@OptIn(ExperimentalTestApi::class)
// Real text layout: the synopsis is cut, so it is a focus stop, and the people column has its real height.
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvTitleScoresTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

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

    private fun page(
        cover: String?,
        similar: List<Tile> = emptyList(),
    ) = TitleSnapshot(
        TitleHeader(
            state = TitleState.Ready,
            details =
                TitleDetails(
                    id = "tt0111161",
                    kind = "movie",
                    name = "The Shawshank Redemption",
                    source = "local",
                    description =
                        "A banker convicted of uxoricide forms a friendship over a quarter century. "
                            .repeat(
                                12,
                            ),
                    releaseInfo = "1994",
                    genres = listOf("Drama", "Crime"),
                    // The longest cast: the column shows four of it.
                    cast = (1..12).map { "Actor Number $it" },
                    directors = listOf("Frank Darabont", "Second Director", "Third Director"),
                    writers = listOf("Stephen King", "Frank Darabont", "Third Writer"),
                    ratings = Ratings(imdb = "9.3"),
                    imdbUrl = "https://www.imdb.com/title/tt0111161/",
                    scores =
                        TitleScores(
                            Slot.Shown("87%", "https://www.themoviedb.org/movie/278"),
                            Slot.Shown("92%", "https://trakt.tv/movies/tt0111161"),
                        ),
                    ratedPoster = cover,
                ),
            inLibrary = false,
            playTarget = PlayTarget("tt0111161", "Play"),
            watched = false,
        ),
        similar = similar,
    )

    private fun show(
        page: TitleSnapshot,
        focus: FocusMemory = FocusMemory(),
        content: (@androidx.compose.runtime.Composable (TitleView) -> Unit)? = null,
    ) {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    """{"sub":7,"snapshot":${CoreJson.encodeToString(TitleSnapshot.serializer(), page)}}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val view = TitleView(client, scope, "movie", "tt0111161")
        // Native graphics start the window in touch mode, where nothing takes focus; a TV has no touch mode.
        compose.runOnUiThread {
            compose.activity.window.decorView
                .requestFocusFromTouch()
        }
        compose.setContent {
            MarqueeTheme(Form.Tv) { content?.invoke(view) ?: TvTitle(view, null, focus, { _, _ -> }, {}, {}, {}) }
        }
        compose.waitForIdle()
        view.open()
        val deadline = System.currentTimeMillis() + 120_000
        while (compose.onAllNodesWithText("Play").fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "the title never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    /** What the focused node shows or says. */
    private fun focused(): String {
        val config = compose.onNode(isFocused()).fetchSemanticsNode().config
        return config.getOrNull(SemanticsProperties.Text)?.joinToString { it.text }?.take(20)
            ?: config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString()
            ?: "?"
    }

    /** Where focus goes from Play: Up twice, back down, Right along the actions and back to Play. */
    private fun walk(): List<String> {
        compose.onNode(hasText("Play") and hasClickAction()).requestFocus()
        compose.waitForIdle()
        val path = mutableListOf(focused())
        repeat(2) {
            compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionUp) }
            compose.waitForIdle()
            path += focused()
        }
        repeat(2) {
            compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionDown) }
            compose.waitForIdle()
        }
        repeat(3) {
            compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionRight) }
            compose.waitForIdle()
            path += focused()
        }
        repeat(3) {
            compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionLeft) }
            compose.waitForIdle()
        }
        return path
    }

    @Test
    fun `the numbers speak once each and Up from Play reaches the genre chips past the synopsis`() {
        show(page(cover = null))
        compose.onAllNodes(hasContentDescription("IMDb 9.3 out of 10")).assertCountEquals(1)
        compose.onAllNodes(hasContentDescription("TMDB 87 percent")).assertCountEquals(1)
        compose.onAllNodes(hasContentDescription("Trakt 92 percent")).assertCountEquals(1)
        // TV has no browser: no score is a link, whatever URLs the core sent.
        compose.onAllNodes(hasContentDescription(", on ", substring = true)).assertCountEquals(0)
        val path = walk()
        assertEquals("Play", path[0].take(4))
        assertEquals("A banker convicted ", path[1].take(19), "Up from Play: the cut synopsis")
        assertEquals("Drama", path[2], "Up from the synopsis: the genre chips")
    }

    @Test
    fun `the rated cover takes no focus - the D-pad order is the same with it`() {
        val shown = page(cover = null)
        show(shown)
        val without = walk()
        // A real image on disk, so the cover draws rather than hiding itself on a failed load.
        val file = File.createTempFile("cover", ".png").apply { deleteOnExit() }
        file.outputStream().use {
            Bitmap.createBitmap(20, 30, Bitmap.Config.ARGB_8888).compress(Bitmap.CompressFormat.PNG, 100, it)
        }
        val covered = shown.header.copy(details = shown.header.details!!.copy(ratedPoster = file.absolutePath))
        val update = listOf<TitleUpdate>(TitleUpdate.Header(covered))
        client.onMessage(
            """{"sub":7,"updates":${CoreJson.encodeToString(ListSerializer(TitleUpdate.serializer()), update)}}""",
        )
        val deadline = System.currentTimeMillis() + 60_000
        while (compose.onAllNodes(hasContentDescription("Cover with ratings")).fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "the cover never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
        // The same stops from Play to the people's first name; past it the narrower column wraps the names apart.
        assertEquals(without.take(4), walk().take(4))
        // The cover is never a stop, whichever way the D-pad goes.
        compose.onNode(hasText("Play") and hasClickAction()).requestFocus()
        for (key in List(6) { listOf(Key.DirectionRight, Key.DirectionDown) }.flatten() + List(6) { Key.DirectionUp }) {
            compose.onNode(isFocused()).performKeyInput { pressKey(key) }
            compose.waitForIdle()
            assertTrue(focused() != "Cover with ratings", "the cover took focus")
        }
        compose.onAllNodes(hasContentDescription("Cover with ratings")).assertCountEquals(1)
        // The card sits beside the facts and the people, never over them, and the column ends inside the safe area.
        val card = compose.onNodeWithContentDescription("Cover with ratings").getUnclippedBoundsInRoot()
        val facts = compose.onNodeWithText("Released", substring = true).getUnclippedBoundsInRoot()
        assertTrue(card.right <= facts.left, "card $card over the facts $facts")
        // 300 by 450 on the 1920 canvas, where one dp is one canvas point (TvCanvas).
        assertEquals(300.dp, card.right - card.left)
        compose.onAllNodesWithText("Actor Number", substring = true).assertCountEquals(4)
        // Two writers at most: the second is also a director, so the first one's row is the column's foot.
        compose.onAllNodesWithText("Third Writer").assertCountEquals(0)
        val last = compose.onNodeWithText("Stephen King").getUnclippedBoundsInRoot()
        assertTrue(last.bottom <= 1080.dp - 60.dp, "the people end at ${last.bottom}, past the safe bottom")
    }

    @Test
    fun `Back from a title in More like this lands on its tile`() {
        val tiles =
            listOf("tt0061512" to "Cool Hand Luke", "tt0070511" to "Papillon").map { (id, name) ->
                Tile(id = id, kind = "movie", name = name, posterShape = "poster", releaseInfo = "1967")
            }
        val focus = FocusMemory()
        var shown by mutableStateOf(true)
        // The page leaves the composition and comes back as the back stack keeps it: its saved state kept.
        show(page(cover = null, similar = tiles), focus) { view ->
            val saved = rememberSaveableStateHolder()
            if (shown) saved.SaveableStateProvider("title") { TvTitle(view, null, focus, { _, _ -> }, {}, {}, {}) }
        }
        val deadline = System.currentTimeMillis() + 60_000
        while (compose
                .onAllNodes(
                    hasContentDescription("Papillon", substring = true),
                ).fetchSemanticsNodes()
                .isEmpty()
        ) {
            check(System.currentTimeMillis() < deadline) { "More like this never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
        compose.onNode(hasContentDescription("Papillon", substring = true)).requestFocus()
        compose.waitForIdle()
        assertEquals("similar:tt0070511", focus.focusedKey)
        shown = false
        compose.waitForIdle()
        shown = true
        val back = System.currentTimeMillis() + 60_000
        while (compose
                .onAllNodes(
                    hasContentDescription("Papillon", substring = true) and isFocused(),
                ).fetchSemanticsNodes()
                .isEmpty()
        ) {
            check(System.currentTimeMillis() < back) { "focus came back to ${focused()}" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }
}
