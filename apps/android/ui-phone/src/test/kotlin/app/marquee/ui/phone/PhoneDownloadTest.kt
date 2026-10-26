package app.marquee.ui.phone

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.core.DownloadCopy
import app.marquee.core.DownloadItem
import app.marquee.core.GroupAct
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.model.DownloadBadge
import app.marquee.core.model.DownloadState
import app.marquee.core.model.Episode
import app.marquee.core.model.GroupDownload
import app.marquee.core.model.PlayTarget
import app.marquee.core.model.TitleDetails
import app.marquee.core.model.TitleHeader
import app.marquee.core.model.TitleSnapshot
import app.marquee.core.model.TitleState
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

/** The download control on the phone title page (plans/offline-downloads.md 3.1, 3.2): what it says and does. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w411dp-h2400dp")
class PhoneDownloadTest {
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

    private val calls = mutableListOf<String>()
    private val downloads =
        object : TitleDownloads {
            override fun item(
                item: DownloadItem,
                videoId: String,
                badge: DownloadBadge?,
            ) {
                calls += "$item $videoId ${badge?.id}"
            }

            override fun group(
                act: GroupAct,
                season: Int?,
                ids: List<String>,
            ) {
                calls += "$act $season $ids"
            }
        }

    private fun film(
        download: DownloadBadge? = null,
        offline: Boolean = false,
    ) = TitleHeader(
        state = TitleState.Ready,
        details = TitleDetails(id = "tt1", kind = "movie", name = "Sintel", source = "local"),
        inLibrary = false,
        playTarget = PlayTarget("tt1", "Play"),
        watched = false,
        download = download,
        offline = offline,
    )

    private fun show(
        header: TitleHeader,
        actions: TitleDownloads? = downloads,
        episodes: List<Episode> = emptyList(),
    ) {
        val snapshot = CoreJson.encodeToString(TitleSnapshot.serializer(), TitleSnapshot(header, episodes))
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content ==
                    "subscribe"
                ) {
                    """{"sub":7,"snapshot":$snapshot}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val view = TitleView(client, scope, header.details!!.kind, "tt1")
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PhoneTitle(view, null, { _, _ -> }, {}, {}, {}, downloads = actions)
            }
        }
        compose.waitForIdle()
        view.open()
        val deadline = System.currentTimeMillis() + 120_000
        while (compose.onAllNodesWithText("Sintel").fetchSemanticsNodes().isEmpty() &&
            compose.onAllNodes(hasContentDescription("Sintel")).fetchSemanticsNodes().isEmpty()
        ) {
            check(System.currentTimeMillis() < deadline) { "the title never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    private fun control(spoken: String) = compose.onNode(hasContentDescription(spoken) and hasClick)

    private val hasClick = SemanticsMatcher.keyIsDefined(SemanticsActions.OnClick)

    @Test
    fun `a film's control starts its download, and a kids page has none`() {
        show(film())
        control("Download").performClick()
        assertEquals(listOf("Start tt1 null"), calls)
        // No menu hint where a tap starts it.
        assertEquals(null, control("Download").fetchSemanticsNode().config[SemanticsActions.OnClick].label)
    }

    @Test
    fun `a kids page shows no download control`() {
        show(film(), actions = null)
        compose.onAllNodes(hasContentDescription("Download")).assertCountEquals(0)
    }

    /** A kids profile sees a download as a plain "Downloaded", with nothing to press (3.2). */
    @Test
    fun `a kids page shows a download as a plain badge`() {
        show(film(DownloadBadge("d1", DownloadState.Done, mobileData = false, progress = 1f)), actions = null)
        compose.onAllNodesWithText("Downloaded", useUnmergedTree = true).assertCountEquals(1)
        compose.onAllNodes(hasContentDescription("Downloaded") and hasClick).assertCountEquals(0)
    }

    @Test
    fun `a running download says its percent, and a tap opens its menu`() {
        show(film(DownloadBadge("d1", DownloadState.Running, mobileData = false, progress = 0.45f)))
        val node = control("Downloading").fetchSemanticsNode()
        assertEquals("45 percent", node.config[SemanticsProperties.StateDescription])
        assertEquals("Shows download options", node.config[SemanticsActions.OnClick].label)
        control("Downloading").performClick()
        compose.onNodeWithText("Pause download").performClick()
        assertEquals(listOf("Pause tt1 d1"), calls)
    }

    @Test
    fun `offline the page says what it shows and drops Versions and Trailer`() {
        show(film(DownloadBadge("d1", DownloadState.Done, mobileData = false, progress = 1f), offline = true))
        compose.onAllNodesWithText(DownloadCopy.TITLE_OFFLINE, useUnmergedTree = true).assertCountEquals(1)
        compose.onAllNodes(hasContentDescription("Versions")).assertCountEquals(0)
        control("Downloaded").performClick()
        compose.onNodeWithText("Delete download").performClick()
        assertEquals(listOf("Delete tt1 d1"), calls)
    }

    @Test
    fun `a series' control opens its season and series menu`() {
        show(
            TitleHeader(
                state = TitleState.Ready,
                details = TitleDetails(id = "tt1", kind = "series", name = "Sintel", source = "local"),
                inLibrary = false,
                watched = false,
                seasons = listOf(1, 2),
                selectedSeason = 1,
                seasonDownload = GroupDownload(1, 8, 0, 0),
                seriesDownload = GroupDownload(2, 16, 0, 0),
            ),
        )
        control("Download").performClick()
        compose.onNodeWithText("Download every season (2 seasons, 16 episodes)").assertExists()
        compose.onNodeWithText("Download Season 1 (8 episodes)").performClick()
        assertEquals(listOf("DownloadSeason 1 []"), calls)
    }

    private fun episode(
        n: Int,
        state: DownloadState?,
    ) = Episode(
        videoId = "tt1:1:$n",
        season = 1,
        episode = n,
        title = "Ep $n",
        watched = false,
        download = state?.let { DownloadBadge("d$n", it, mobileData = false) },
    )

    /**
     * Deleting a whole season's downloads asks first; the question counts the episodes already downloaded, not the
     * queued ones the delete also cancels (code review r2 n6). Cancel deletes nothing.
     */
    @Test
    fun `a season's delete asks first`() {
        val states = listOf(DownloadState.Done, DownloadState.Done, DownloadState.Done, DownloadState.Queued, null)
        show(
            TitleHeader(
                state = TitleState.Ready,
                details = TitleDetails(id = "tt1", kind = "series", name = "Sintel", source = "local"),
                inLibrary = false,
                watched = false,
                seasons = listOf(1),
                selectedSeason = 1,
                seasonDownload = GroupDownload(1, 5, 5, 0),
                seriesDownload = GroupDownload(1, 5, 5, 0),
            ),
            episodes = states.mapIndexed { i, state -> episode(i + 1, state) },
        )
        control("Downloaded").performClick()
        compose.onNodeWithText("Delete Season 1 downloads").performClick()
        assertEquals(emptyList<String>(), calls)
        compose.onNodeWithText("Delete 3 downloaded episodes of Sintel?").assertExists()
        compose.onNodeWithText("Cancel").performClick()
        assertEquals(emptyList<String>(), calls)
        control("Downloaded").performClick()
        compose.onNodeWithText("Delete Season 1 downloads").performClick()
        compose.onNodeWithText("Delete").performClick()
        assertEquals(listOf("DeleteSeason 1 [d1, d2, d3, d4]"), calls)
    }

    /** Each episode's control names its episode after its state: a season reads as many names as rows (M3). */
    @Test
    fun `each episode's control names its episode`() {
        val episodes =
            (1..2).map {
                Episode(
                    videoId = "tt1:1:$it",
                    season = 1,
                    episode = it,
                    title = "Ep $it",
                    watched = false,
                    download = if (it == 2) DownloadBadge("d$it", DownloadState.Done, mobileData = false) else null,
                )
            }
        show(
            TitleHeader(
                state = TitleState.Ready,
                details = TitleDetails(id = "tt1", kind = "series", name = "Sintel", source = "local"),
                inLibrary = false,
                watched = false,
                seasons = listOf(1),
                selectedSeason = 1,
                seasonDownload = GroupDownload(1, 2, 1, 0),
                seriesDownload = GroupDownload(1, 2, 1, 0),
            ),
            episodes = episodes,
        )
        control("Download, S1:E1").assertExists()
        control("Downloaded, S1:E2").assertExists()
    }
}
