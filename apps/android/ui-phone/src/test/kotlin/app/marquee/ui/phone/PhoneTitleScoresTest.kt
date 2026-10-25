package app.marquee.ui.phone

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertHasClickAction
import androidx.compose.ui.test.assertHasNoClickAction
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.Density
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
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * The phone title page's ratings row, genre chips and More like this (plans/title-page-ratings-genres-similar.md):
 * IMDb, TMDB and Trakt each after its mark and spoken once, the row reserving a pending slot so nothing moves when
 * the numbers land; genres as chips that ask for Discover, plain words on a kids profile; More like this opens its
 * titles; titleVisible goes to the core while the page shows.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w411dp-h2400dp")
class PhoneTitleScoresTest {
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

    private fun header(
        scores: TitleScores,
        linked: Boolean = true,
    ) = TitleHeader(
        state = TitleState.Ready,
        details =
            TitleDetails(
                id = "tt0111161",
                kind = "movie",
                name = "The Shawshank Redemption",
                source = "local",
                releaseInfo = "1994",
                runtime = "142 min",
                genres = listOf("Drama", "Crime"),
                cast = listOf("Tim Robbins", "Morgan Freeman"),
                writers = listOf("Frank Darabont"),
                ratings = Ratings(imdb = "9.3"),
                imdbUrl = "https://www.imdb.com/title/tt0111161/".takeIf { linked },
                scores = scores,
            ),
        inLibrary = false,
        playTarget = PlayTarget("tt0111161", "Play"),
        watched = false,
        genresLinked = linked,
    )

    private val similar =
        listOf(
            Tile(
                id = "tt0061512",
                kind = "movie",
                name = "Cool Hand Luke",
                poster = "https://images.metahub.space/poster/medium/tt0061512/img",
                posterShape = "poster",
                ratings = Ratings(),
            ),
        )

    /** Opens the page on a fake core that answers with [snapshot]. */
    private fun open(snapshot: TitleSnapshot): TitleView {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    """{"sub":7,"snapshot":${CoreJson.encodeToString(TitleSnapshot.serializer(), snapshot)}}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        return TitleView(client, scope, "movie", "tt0111161")
    }

    /** The label of the click a screen reader offers on the chip that shows [text]. */
    private fun clickLabel(text: String): String? =
        compose
            .onNodeWithText(text)
            .fetchSemanticsNode()
            .config[SemanticsActions.OnClick]
            .label

    private fun waitFor(text: String) {
        val deadline = System.currentTimeMillis() + 120_000
        while (compose.onAllNodesWithText(text).fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "never drew $text" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    @Test
    fun `the ratings row ends the meta line and nothing moves when TMDB and Trakt land`() {
        val view = open(TitleSnapshot(header(TitleScores(Slot.Pending, Slot.Pending)), similar = emptyList()))
        val genres = mutableListOf<Pair<String, String>>()
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneTitle(view, null, { _, _ -> }, {}, {}, {}, onGenre = { k, g -> genres += k to g })
            }
        }
        compose.waitForIdle()
        view.open()
        waitFor("Play")
        // Nothing related: no row at all.
        compose.onAllNodesWithText("More like this").assertCountEquals(0)
        // Pending: the slots hold their place and say nothing.
        compose.onAllNodes(hasContentDescription("TMDB", substring = true)).assertCountEquals(0)
        val year = compose.onAllNodesWithText("1994", substring = true)[0].getUnclippedBoundsInRoot()
        val imdb = compose.onNodeWithContentDescription("IMDb 9.3 out of 10, on IMDb").getUnclippedBoundsInRoot()
        val play = compose.onNodeWithText("Play").getUnclippedBoundsInRoot()
        val landed =
            listOf<TitleUpdate>(
                TitleUpdate.Header(
                    header(
                        TitleScores(
                            Slot.Shown("87%", "https://www.themoviedb.org/movie/278"),
                            Slot.Shown("92%", "https://trakt.tv/movies/tt0111161"),
                        ),
                    ),
                ),
            )
        client.onMessage(
            """{"sub":7,"updates":${CoreJson.encodeToString(ListSerializer(TitleUpdate.serializer()), landed)}}""",
        )
        val deadline = System.currentTimeMillis() + 60_000
        while (compose
                .onAllNodes(
                    hasContentDescription("Trakt 92 percent, on Trakt"),
                ).fetchSemanticsNodes()
                .isEmpty()
        ) {
            check(System.currentTimeMillis() < deadline) { "the scores never landed" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
        // Each score is a link to the title's page on its service, its row as tall as before.
        for (name in listOf("IMDb 9.3 out of 10, on IMDb", "TMDB 87 percent, on TMDB", "Trakt 92 percent, on Trakt")) {
            compose.onNodeWithContentDescription(name).assertHasClickAction()
        }
        // A badge opens the title's page on its service, in the browser.
        compose.onNodeWithContentDescription("TMDB 87 percent, on TMDB").performClick()
        val opened =
            org.robolectric.Shadows
                .shadowOf(
                    org.robolectric.RuntimeEnvironment.getApplication(),
                ).nextStartedActivity
        assertEquals(android.content.Intent.ACTION_VIEW, opened?.action)
        assertEquals("https://www.themoviedb.org/movie/278", opened?.dataString)
        assertEquals(year, compose.onAllNodesWithText("1994", substring = true)[0].getUnclippedBoundsInRoot())
        assertEquals(
            imdb,
            compose.onNodeWithContentDescription("IMDb 9.3 out of 10, on IMDb").getUnclippedBoundsInRoot(),
        )
        assertEquals(play, compose.onNodeWithText("Play").getUnclippedBoundsInRoot())
        // The genres are a row of chips: one asks for Discover on the kind and that genre.
        compose.onNodeWithText("Drama").performClick()
        assertEquals(listOf("movie" to "Drama"), genres)
        // A screen reader says what the chip opens.
        assertEquals("Opens Discover", clickLabel("Drama"))
    }

    @Test
    fun `a name opens Search for that person`() {
        val view = open(TitleSnapshot(header(TitleScores(Slot.None, Slot.None))))
        val people = mutableListOf<String>()
        compose.setContent {
            MarqueeTheme(Form.Phone) { PhoneTitle(view, null, { _, _ -> }, {}, {}, {}, onPerson = { people += it }) }
        }
        compose.waitForIdle()
        view.open()
        waitFor("Play")
        waitFor("Frank Darabont")
        compose.onNodeWithText("Morgan Freeman").performClick()
        compose.onNodeWithText("Frank Darabont").performClick()
        assertEquals(listOf("Morgan Freeman", "Frank Darabont"), people)
        assertEquals("Searches for their titles", clickLabel("Morgan Freeman"))
    }

    @Test
    fun `a kids profile's genres are words, not links`() {
        val view = open(TitleSnapshot(header(TitleScores(Slot.None, Slot.None), linked = false)))
        compose.setContent { MarqueeTheme(Form.Phone) { PhoneTitle(view, null, { _, _ -> }, {}, {}, {}) } }
        compose.waitForIdle()
        view.open()
        waitFor("Drama, Crime")
        compose.onNodeWithText("Drama, Crime").assertHasNoClickAction()
        // No number will come: no slot is held for TMDB or Trakt.
        compose.onAllNodes(hasContentDescription("percent", substring = true)).assertCountEquals(0)
        // No way out to the open web: IMDb's score is plain (the core sends no URL on a kids profile).
        compose.onNodeWithContentDescription("IMDb 9.3 out of 10").assertHasNoClickAction()
        // A kids profile's Search finds only its list: the names are words, not links to it.
        compose.onNodeWithText("Tim Robbins", substring = true).assertHasNoClickAction()
    }

    @Test
    fun `More like this opens its titles`() {
        // A title Trakt lists twice shows once: a repeated key would crash the row.
        val view = open(TitleSnapshot(header(TitleScores(Slot.None, Slot.None)), similar = similar + similar))
        val opened = mutableListOf<String>()
        compose.setContent {
            MarqueeTheme(Form.Phone) { PhoneTitle(view, null, { _, _ -> }, {}, {}, {}, onOpen = { opened += it.id }) }
        }
        compose.waitForIdle()
        view.open()
        waitFor("Play")
        waitFor("More like this")
        compose.onNodeWithContentDescription("Cool Hand Luke", substring = true).performClick()
        assertEquals(listOf("tt0061512"), opened)
    }

    // Measured, with real text layout: a series' meta at a 1.3 text size on a 360 dp phone keeps the year whole and
    // puts the ratings under it, inside the screen.
    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w360dp-h800dp")
    fun `on a narrow phone at a large text size the ratings wrap under the facts, which keep the year`() {
        val landed =
            TitleScores(
                Slot.Shown("87%", "https://www.themoviedb.org/tv/1396"),
                Slot.Shown("92%", "https://trakt.tv/shows/tt0903747"),
            )
        val base = header(landed)
        val series =
            base.copy(
                details = base.details!!.copy(kind = "series", releaseInfo = "2011–2019", runtime = null),
                seasons = (1..8).toList(),
            )
        val view = open(TitleSnapshot(series))
        compose.setContent {
            val density = LocalDensity.current.density
            CompositionLocalProvider(LocalDensity provides Density(density, fontScale = 1.3f)) {
                MarqueeTheme(Form.Phone) { PhoneTitle(view, null, { _, _ -> }, {}, {}, {}) }
            }
        }
        compose.waitForIdle()
        view.open()
        waitFor("Play")
        val facts = compose.onAllNodesWithText("2011–2019", substring = true)[0]
        val layout = mutableListOf<TextLayoutResult>()
        facts.performSemanticsAction(SemanticsActions.GetTextLayoutResult) { it(layout) }
        val text = layout.single()
        assertFalse(text.isLineEllipsized(0), "the meta line was cut: ${text.layoutInput.text}")
        assertTrue(
            text.layoutInput.text.text
                .startsWith("2011–2019"),
            "the year first: ${text.layoutInput.text}",
        )
        val line = facts.getUnclippedBoundsInRoot()
        val trakt = compose.onNodeWithContentDescription("Trakt 92 percent, on Trakt").getUnclippedBoundsInRoot()
        assertTrue(trakt.top >= line.bottom, "the ratings ($trakt) wrap under the facts ($line)")
        assertTrue(trakt.right <= 360.dp, "the ratings end at ${trakt.right}, past the screen")
    }
}
