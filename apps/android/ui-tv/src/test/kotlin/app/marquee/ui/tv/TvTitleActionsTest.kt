package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
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
import app.marquee.core.model.VersionsConditions
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.views.TitleView
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
import kotlin.test.assertEquals

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
            EmptyKind.NeedsAccount to EmptyAction.SettingsAccounts,
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
}
