package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotFocused
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.pressKey
import app.marquee.core.TitleCopy
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.model.Episode
import app.marquee.core.model.PlayTarget
import app.marquee.core.model.Tile
import app.marquee.core.model.TitleDetails
import app.marquee.core.model.TitleHeader
import app.marquee.core.model.TitleSnapshot
import app.marquee.core.model.TitleState
import app.marquee.core.model.Trailer
import app.marquee.core.views.TitleView
import app.marquee.design.Form
import app.marquee.design.LocalButtonFocus
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

/**
 * The TV title page's two hold-only things, made visible: the episodes' two actions as a strip under the lane that
 * Down from a card enters, and Remove from Continue Watching while the title is in it.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h2400dp")
class TvTitleStripTest {
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

    private val first = Episode("tt1:1:1", 1, 1, "Pilot", watched = false)
    private val second = Episode("tt1:1:2", 1, 2, "The Second", watched = false)

    private fun page(
        continuing: Boolean,
        movie: Boolean = false,
        resumeMs: Long? = null,
    ) = TitleSnapshot(
        TitleHeader(
            state = TitleState.Ready,
            details =
                TitleDetails(
                    id = "tt1",
                    kind = if (movie) "movie" else "series",
                    name = "Sintel",
                    source = "local",
                    trailers = listOf(Trailer("Trailer", "abc", "https://example.test/t")),
                ),
            inLibrary = false,
            playTarget = PlayTarget("tt1:1:1", if (resumeMs != null) "Resume S1:E1" else "Play", resumeMs = resumeMs),
            watched = false,
            seasons = if (movie) emptyList() else listOf(1),
            selectedSeason = if (movie) null else 1,
            continuing = continuing,
        ),
        episodes = if (movie) emptyList() else listOf(first, second),
        similar = listOf(Tile(id = "tt9", kind = "series", name = "Another", posterShape = "poster")),
    )

    private val calls = mutableListOf<String>()

    /** The page's count of focused buttons, which a white primary yields to. */
    private val buttons = mutableIntStateOf(0)

    private fun show(
        continuing: Boolean = false,
        movie: Boolean = false,
        resumeMs: Long? = null,
    ) {
        val snapshot = CoreJson.encodeToString(TitleSnapshot.serializer(), page(continuing, movie, resumeMs))
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    """{"sub":7,"snapshot":$snapshot}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val view = TitleView(client, scope, "series", "tt1")
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                CompositionLocalProvider(LocalButtonFocus provides buttons) {
                    TvTitle(
                        view,
                        null,
                        FocusMemory(),
                        onPlay = { _, _ -> },
                        onVersions = {},
                        onEpisode = {},
                        onEpisodeVersions = { calls += "versions ${it.videoId}" },
                        onOpen = { calls += "open ${it.id}" },
                        onUncontinue = { calls += "uncontinue" },
                        onStartOver = { _, videoId -> calls += "again $videoId" },
                    )
                }
            }
        }
        compose.waitForIdle()
        view.open()
        val deadline = System.currentTimeMillis() + 120_000
        val drawn = if (movie) hasContentDescription("Versions") else hasContentDescription(TitleCopy.episode(first))
        while (compose.onAllNodes(drawn).fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "the title never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    private fun card(episode: Episode) = compose.onNode(hasContentDescription(TitleCopy.episode(episode)))

    private fun press(key: Key) = compose.onNode(isFocused()).performKeyInput { pressKey(key) }

    @Test
    fun `Down from an episode enters the strip, Right still moves along the episodes, Down again leaves it`() {
        show()
        card(first).performSemanticsAction(SemanticsActions.RequestFocus)
        press(Key.DirectionRight)
        card(second).assertIsFocused()
        press(Key.DirectionDown)
        // The pills belong to the episode focus stood on.
        val pill = compose.onNode(hasContentDescription("Choose a version for ${TitleCopy.episode(second)}"))
        pill.assertIsFocused()
        pill.performClick()
        assertEquals(listOf("versions ${second.videoId}"), calls)
        press(Key.DirectionDown)
        pill.assertIsNotFocused()
    }

    @Test
    fun `a focused strip pill does not count as a focused button, so Play stays white`() {
        show()
        card(first).performSemanticsAction(SemanticsActions.RequestFocus)
        press(Key.DirectionDown)
        compose.onNode(hasContentDescription("Choose a version for ${TitleCopy.episode(first)}")).assertIsFocused()
        compose.runOnIdle { assertEquals(0, buttons.intValue) }
    }

    @Test
    fun `Remove from Continue Watching acts on one press, its Undo comes after`() {
        show(continuing = true)
        // A round button's word is drawn for the eye and spoken as its description.
        compose.onNode(hasContentDescription("Remove from Continue Watching")).performClick()
        compose.runOnIdle { assertEquals(listOf("uncontinue"), calls) }
    }

    @Test
    fun `Start over shows part way in and plays the target from its beginning`() {
        show(resumeMs = 60_000)
        compose.onNode(hasContentDescription("Start over")).performClick()
        compose.runOnIdle { assertEquals(listOf("again tt1:1:1"), calls) }
    }

    @Test
    fun `nothing played yet has no Start over`() {
        show()
        compose.onAllNodes(hasContentDescription("Start over")).assertCountEquals(0)
    }

    @Test
    fun `a title not in Continue Watching has no remove button`() {
        show(continuing = false)
        compose.onAllNodes(hasContentDescription("Remove from Continue Watching")).assertCountEquals(0)
    }

    @Test
    fun `the actions run in DECISION's order, Versions after Watched`() {
        show(movie = true)
        val lefts =
            listOf("Trailer", "Add to Library", "Mark as watched", "Versions").map {
                compose.onNode(hasContentDescription(it)).getBoundsInRoot().left
            }
        assertEquals(lefts.sorted(), lefts)
    }
}
