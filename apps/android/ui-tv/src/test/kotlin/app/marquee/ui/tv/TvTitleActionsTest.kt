package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.client.stateThread
import app.marquee.core.model.EmptyAction
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.EmptyVersions
import app.marquee.core.model.HiddenSummary
import app.marquee.core.model.Network
import app.marquee.core.model.PlayTarget
import app.marquee.core.model.Speed
import app.marquee.core.model.TitleDetails
import app.marquee.core.model.TitleHeader
import app.marquee.core.model.TitleSnapshot
import app.marquee.core.model.TitleState
import app.marquee.core.model.Trailer
import app.marquee.core.model.VersionsConditions
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.views.TitleView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.FULL_TRAILER_TAG
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
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals
import kotlin.test.assertNull

/**
 * The TV title page's button when nothing can play (plans/play-every-version.md, the shared table): each new
 * action does what it says when pressed, not only in TitleCopy.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvTitleActionsTest {
    @get:Rule
    val compose = createComposeRule()

    private val executor = stateThread("test-state")
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

    private val title =
        TitleSnapshot(
            TitleHeader(
                state = TitleState.Ready,
                details = TitleDetails(id = "tt1", kind = "movie", name = "Sintel", source = "local"),
                inLibrary = false,
                playTarget = PlayTarget("tt1", "Play"),
                watched = false,
            ),
        )

    private fun versions(
        kind: EmptyKind,
        action: EmptyAction,
        key: String? = null,
    ) = VersionsSnapshot(
        conditions =
            VersionsConditions(
                screenClass = 1080,
                hdr = false,
                network = Network.Wifi,
                http = Speed(42.2f, 3),
                p2p = Speed(10f, 0),
                minSeeders = 50,
                engine = false,
                account = true,
            ),
        empty = EmptyVersions(kind, action, 0, key = key),
        hidden = HiddenSummary(0),
        done = true,
        frozen = false,
        nobodySharing = false,
    )

    private var shown by mutableStateOf<VersionsSnapshot?>(null)
    private val calls = mutableListOf<String>()

    /** The title page on a view whose core answers with [title], and [first] for its versions. */
    private fun show(
        first: VersionsSnapshot,
        page: TitleSnapshot = title,
    ) {
        shown = first
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
        val view = TitleView(client, scope, "movie", "tt1")
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                TvTitle(
                    view,
                    shown,
                    FocusMemory(),
                    onPlay = { key, id -> calls += "play $key $id" },
                    onVersions = { calls += "versions $it" },
                    onEpisode = {},
                    onEpisodeVersions = {},
                )
            }
        }
        compose.waitForIdle()
        view.open()
        val deadline = System.currentTimeMillis() + 120_000
        while (compose.onAllNodes(hasContentDescription("Add to Library")).fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "the title never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    @Test
    fun `Play is the one primary - it plays the kept version, opens Versions, or is inert`() {
        show(versions(EmptyKind.Unreachable, EmptyAction.PlayFromDevice, key = "kept"))
        // An owner's page offers Add to Library (the core's librarySettable).
        compose.onAllNodes(hasContentDescription("Add to Library")).assertCountEquals(1)
        val play = { compose.onNodeWithText("Play").performClick() }
        play()
        assertEquals(listOf("play kept tt1"), calls)
        // Still searching: the player opens at once, with no version yet; its pick starts when it arrives.
        shown = versions(EmptyKind.Unreachable, EmptyAction.TryAgain).copy(empty = null, done = false)
        compose.waitForIdle()
        calls.clear()
        play()
        assertEquals(listOf("play null tt1"), calls)
        // No Settings or account step on the play surface: Play opens Versions, which holds them.
        for ((kind, action) in listOf(
            EmptyKind.NoSource to EmptyAction.SettingsAddons,
            EmptyKind.Unreachable to EmptyAction.TryAgain,
            EmptyKind.NothingPlaysHere to EmptyAction.ChooseVersion,
        )) {
            shown = versions(kind, action)
            compose.waitForIdle()
            for (gone in listOf("Settings", "Try again", "Play from this device", "Choose a version")) {
                compose.onAllNodesWithText(gone).assertCountEquals(0)
            }
            calls.clear()
            play()
            assertEquals(listOf("versions tt1"), calls, "$kind")
        }
        // A state whose only way on is Back: Play keeps its place, inert, with the state's line under it.
        shown = versions(EmptyKind.NoResults, EmptyAction.None)
        compose.waitForIdle()
        compose.onNodeWithText("No source has this yet", substring = true, useUnmergedTree = true).assertExists()
        compose.onNodeWithText("Play").assertIsNotEnabled()
        calls.clear()
        play()
        assertEquals(emptyList(), calls)
        // Versions that need an account: the quiet line, Play inert, and no account step anywhere on the page
        // (accounts are set up in Settings only).
        shown = versions(EmptyKind.NeedsAccount, EmptyAction.None)
        compose.waitForIdle()
        compose
            .onNodeWithText(
                "None of these plays on this device",
                substring = true,
                useUnmergedTree = true,
            ).assertExists()
        for (gone in listOf("Add an account", "Settings", "streaming account")) {
            compose.onAllNodesWithText(gone, substring = true, useUnmergedTree = true).assertCountEquals(0)
        }
        compose.onNodeWithText("Play").assertIsNotEnabled()
        calls.clear()
        play()
        assertEquals(emptyList(), calls)
    }

    @Test
    fun `a page that opens on a Back-only state starts focus on Versions, never on the inert Play`() {
        show(versions(EmptyKind.KidsNothing, EmptyAction.None))
        compose.waitUntil(5_000) {
            compose.onAllNodes(isFocused() and hasContentDescription("Versions")).fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithText("Play").assertIsNotEnabled()
    }

    @Test
    fun `with no video to play, focus starts on Add to Library and Play does nothing`() {
        show(versions(EmptyKind.NoResults, EmptyAction.None), title.copy(header = title.header.copy(playTarget = null)))
        compose.waitUntil(5_000) {
            compose
                .onAllNodes(
                    isFocused() and hasContentDescription("Add to Library"),
                ).fetchSemanticsNodes()
                .isNotEmpty()
        }
        compose.onNodeWithText("Play").assertIsNotEnabled()
    }

    @Test
    fun `Trailer plays in the app full screen, never in another app, and Back gives the focus back to it`() {
        val trailer = Trailer("Sintel", "eRsGyueVLvQ", "https://www.youtube.com/watch?v=eRsGyueVLvQ")
        show(
            versions(EmptyKind.Unreachable, EmptyAction.PlayFromDevice, key = "kept"),
            title.copy(header = title.header.copy(details = title.header.details!!.copy(trailers = listOf(trailer)))),
        )
        // By the remote, as a TV is used: focus to Trailer, select.
        val trailerButton = compose.onNode(hasContentDescription("Trailer").and(hasClickAction()))
        compose.runOnIdle { trailerButton.fetchSemanticsNode() }
        trailerButton.requestFocus()
        trailerButton.assertIsFocused()
        trailerButton.performKeyInput { pressKey(Key.DirectionCenter) }
        compose.waitUntil(5_000) { compose.onAllNodesWithTag(FULL_TRAILER_TAG).fetchSemanticsNodes().isNotEmpty() }
        assertNull(shadowOf(RuntimeEnvironment.getApplication()).nextStartedActivity)
        // The remote's Back closes it, and the focus comes back to Trailer.
        // The dialog's bar has the focus (the page under it keeps its own, on Play).
        compose.waitUntil(5_000) {
            compose.onAllNodes(hasContentDescription("Position").and(isFocused())).fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNode(hasContentDescription("Position").and(isFocused())).performKeyInput { pressKey(Key.Back) }
        compose.waitUntil(5_000) { compose.onAllNodesWithTag(FULL_TRAILER_TAG).fetchSemanticsNodes().isEmpty() }
        compose.waitUntil(5_000) {
            compose.onAllNodes(hasContentDescription("Trailer").and(isFocused())).fetchSemanticsNodes().isNotEmpty()
        }
    }

    @Test
    fun `a kids profile's page shows no Add to Library`() {
        val kids = title.copy(header = title.header.copy(librarySettable = false))
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    """{"sub":7,"snapshot":${CoreJson.encodeToString(TitleSnapshot.serializer(), kids)}}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val view = TitleView(client, scope, "movie", "tt1")
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                TvTitle(
                    view,
                    versions(EmptyKind.Unreachable, EmptyAction.TryAgain).copy(empty = null, done = false),
                    FocusMemory(),
                    onPlay = { _, _ -> },
                    onVersions = {},
                    onEpisode = {},
                    onEpisodeVersions = {},
                )
            }
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
        compose.onAllNodes(hasContentDescription("Add to Library")).assertCountEquals(0)
        compose.onAllNodes(hasContentDescription("In Library")).assertCountEquals(0)
    }
}
