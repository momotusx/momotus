package app.marquee.android

import android.Manifest
import android.app.Application
import android.content.Context
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertTextContains
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.lifecycle.viewmodel.compose.viewModel
import app.marquee.core.DownloadCopy
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreTransport
import app.marquee.core.model.DownloadActivity
import app.marquee.core.model.DownloadRow
import app.marquee.core.model.DownloadSettings
import app.marquee.core.model.DownloadState
import app.marquee.core.model.DownloadStorage
import app.marquee.core.model.DownloadTitle
import app.marquee.core.model.DownloadsSnapshot
import app.marquee.core.route.Route
import app.marquee.core.session.CoreSession
import app.marquee.core.session.DownloadAction
import app.marquee.core.session.DownloadTarget
import app.marquee.core.views.DownloadsView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The Downloads page, a series' downloads, Home's offline banner and the notification ask (plans/offline-downloads.md
 * 3.3 to 3.5) over a core that answers with one `downloads` snapshot and records every op (code review r2 n2, n7).
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class, qualifiers = "w411dp-h2400dp")
class DownloadsScreenTest {
    @get:Rule
    val compose = createComposeRule()

    private val app = RuntimeEnvironment.getApplication()
    private val session = CoreSession(app)
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private val actions = DownloadActions(session, { error("no add here") }, scope)
    private val ops = CopyOnWriteArrayList<String>()
    private val played = mutableListOf<String>()
    private val series = mutableListOf<String>()
    private val nav = DownloadsNav(onPlay = { played += it.id }, onVersion = {}, onSeries = { series += it })

    @After
    fun stop() {
        scope.cancel()
        session.scope.cancel()
        session.client.close()
    }

    private fun row(
        id: String,
        state: DownloadState,
        kind: String = "series",
        itemId: String = "tt2",
        name: String = "Severance",
        episode: Int? = null,
        episodeTitle: String? = null,
    ) = DownloadRow(
        id = id,
        kind = kind,
        itemId = itemId,
        videoId = "$itemId:1:${episode ?: 0}",
        season = episode?.let { 1 },
        episode = episode,
        name = name,
        episodeTitle = episodeTitle,
        state = state,
        mobileData = false,
        bytesDone = if (state == DownloadState.Done) 1_000 else 500,
        bytesTotal = 1_000,
    )

    private val running = row("d1", DownloadState.Running, episode = 4)
    private val film = row("d2", DownloadState.Done, kind = "movie", itemId = "tt1", name = "Sintel")
    private val pilot = row("d3", DownloadState.Done, episode = 3, episodeTitle = "Pilot")

    private fun snapshot(
        downloading: List<DownloadRow> = listOf(running),
        titles: List<DownloadTitle> =
            listOf(
                DownloadTitle("tt1", "movie", "Sintel", count = 1, bytes = 1_000, rows = listOf(film)),
                DownloadTitle("tt2", "series", "Severance", count = 1, bytes = 1_000, rows = listOf(pilot)),
            ),
    ) = DownloadsSnapshot(
        locked = false,
        storage = DownloadStorage(downloadsBytes = 2_000, freeBytes = 48_000_000_000),
        settings = DownloadSettings(wifiOnly = true, smart = true),
        active = DownloadActivity(1, 0, 500, 1_000, waiting = 0, mobileData = false, title = "Severance"),
        downloading = downloading,
        titles = titles,
    )

    /** The subscribe's answer, when a test sends it itself ([holdSubscribe]). */
    @Volatile private var held: (() -> Unit)? = null

    @Volatile private var holdSubscribe = false

    /** The core: the `downloads` view answers [snapshot], every op is recorded and goes through. */
    private fun core(snapshot: DownloadsSnapshot) {
        val json = CoreJson.encodeToString(DownloadsSnapshot.serializer(), snapshot)
        session.client.attach(
            CoreTransport { text ->
                val request = CoreJson.parseToJsonElement(text).jsonObject
                val id = request.getValue("id").jsonPrimitive.long
                val op = request.getValue("op").jsonPrimitive.content
                ops += "$op ${request["ids"] ?: request["itemId"] ?: request["all"] ?: ""}"
                val result = if (op == "subscribe") """{"sub":7,"snapshot":$json}""" else "{}"
                val answer = { session.client.onMessage("""{"id":$id,"ok":true,"result":$result}""") }
                if (op == "subscribe" && holdSubscribe) held = answer else answer()
            },
        )
    }

    private fun until(
        what: String,
        ready: () -> Boolean,
    ) {
        val deadline = System.currentTimeMillis() + 60_000
        while (!ready()) {
            check(System.currentTimeMillis() < deadline) { "never: $what" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    private fun shows(text: String) =
        compose.onAllNodesWithText(text, useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty()

    private val hasClick = SemanticsMatcher.keyIsDefined(SemanticsActions.OnClick)

    private fun control(spoken: String) = compose.onNode(hasContentDescription(spoken) and hasClick)

    private fun controls(spoken: String) =
        compose.onAllNodes(hasContentDescription(spoken, substring = true) and hasClick)

    @Composable
    private fun seriesPage(onGone: () -> Unit) =
        DownloadedSeriesScreen(session, "tt2", actions, nav, kids = false, onGone)

    private fun downloads(kids: Boolean = false) {
        compose.setContent { MarqueeTheme(Form.Phone) { DownloadsScreen(session, actions, nav, kids) } }
        until("the page drew") { shows("Sintel") }
    }

    /**
     * The core answers while the window draws for the first time: that write reaches no recomposer yet, and a page
     * that read only the view's state stayed empty until the next change. The page reads the latest snapshot once it
     * is attached, so the rows show.
     */
    @Test
    fun `a snapshot that comes during the first draw shows`() {
        holdSubscribe = true
        core(snapshot())
        compose.setContent {
            MarqueeTheme(Form.Phone) { DownloadsScreen(session, actions, nav, kids = false) }
            // Still inside the first composition: the core answers and the view takes the snapshot before it ends.
            val view = viewModel<DownloadsHolder>(key = "downloads").view
            if (holdSubscribe) answerNow(view)
        }
        until("the rows showed") { shows("Sintel") }
    }

    /** Sends the held subscribe answer and waits until [view] holds the snapshot. */
    private fun answerNow(view: DownloadsView) {
        holdSubscribe = false
        val deadline = System.currentTimeMillis() + 10_000
        while (held == null) check(System.currentTimeMillis() < deadline) { "no subscribe" }
        held?.invoke()
        while (view.latest.value == null) check(System.currentTimeMillis() < deadline) { "no snapshot" }
    }

    /** Ten rows never read "Downloaded" ten times: each control says its row after its state (code review r2 n2). */
    @Test
    fun `each row's control names its row, and a series row opens its page`() {
        core(snapshot())
        downloads()
        assertTrue(shows(DownloadCopy.storage(snapshot().storage)))
        control("Downloading, Severance S1:E4").assertExists()
        control("Downloaded, Sintel").performClick()
        compose.onNodeWithText("Delete download").performClick()
        until("the delete went") { ops.any { it.startsWith("downloadRemove") } }
        assertEquals("downloadRemove [\"d2\"]", ops.last { it.startsWith("downloadRemove") })
        // A series' row has no control of its own: it opens the series' page.
        controls("Downloaded, Severance").assertCountEquals(0)
        compose.onAllNodesWithText("Severance", useUnmergedTree = true).onFirst().performClick()
        assertEquals(listOf("tt2"), series)
    }

    @Test
    fun `a kids profile sees the titles with no control and no storage line`() {
        core(snapshot())
        downloads(kids = true)
        controls("Downloaded").assertCountEquals(0)
        controls("Downloading").assertCountEquals(0)
        assertTrue(!shows(DownloadCopy.storage(snapshot().storage)))
        compose.onAllNodesWithText("Pause all").assertCountEquals(0)
    }

    @Test
    fun `nothing downloaded says so`() {
        core(snapshot(downloading = emptyList(), titles = emptyList()))
        compose.setContent { MarqueeTheme(Form.Phone) { DownloadsScreen(session, actions, nav, kids = false) } }
        until("the empty page drew") { shows(DownloadCopy.EMPTY_TITLE) }
    }

    @Test
    fun `an op that did not go through says so on the page`() {
        core(snapshot())
        downloads()
        // The core refuses the next op.
        session.client.attach(
            CoreTransport { text ->
                val id =
                    CoreJson
                        .parseToJsonElement(text)
                        .jsonObject
                        .getValue("id")
                        .jsonPrimitive.long
                session.client.onMessage("""{"id":$id,"ok":false,"error":"no"}""")
            },
        )
        actions.act(DownloadAction.Pause, DownloadTarget.All)
        until("the line showed") { shows(DownloadCopy.ACT_FAILED) }
        actions.clearProblem()
        until("the line went") { !shows(DownloadCopy.ACT_FAILED) }
    }

    /** A series' page: its episodes by season, each control named by its episode; Delete all asks first (r2 n6). */
    @Test
    fun `a series page names each episode and counts only what is downloaded`() {
        core(snapshot())
        var gone = false
        compose.setContent {
            MarqueeTheme(Form.Phone) { seriesPage { gone = true } }
        }
        until("the series drew") { shows("E3 Pilot") }
        control("Downloaded, E3 Pilot").assertExists()
        control("Downloading, E4").assertExists()
        compose.onNodeWithText("Delete all").performClick()
        // The running episode is cancelled too, but only the downloaded one is counted.
        compose.onNodeWithText(DownloadCopy.deleteSeries(listOf(DownloadState.Done), "Severance")).assertExists()
        compose.onNodeWithText("Delete").performClick()
        until("the delete went") { ops.any { it.startsWith("downloadRemove") } }
        assertEquals("downloadRemove \"tt2\"", ops.last { it.startsWith("downloadRemove") })
        assertTrue(!gone)
    }

    @Test
    fun `a series page with nothing left goes`() {
        core(snapshot(downloading = emptyList(), titles = emptyList()))
        var gone = false
        compose.setContent {
            MarqueeTheme(Form.Phone) { seriesPage { gone = true } }
        }
        until("the page went") { gone }
    }

    @Test
    fun `offline with downloads, the banner says they still play, is announced, and goes to them`() {
        var went = false
        compose.setContent { MarqueeTheme(Form.Phone) { OfflineBanner(onDownloads = { went = true }) } }
        compose
            .onNode(SemanticsMatcher.expectValue(SemanticsProperties.LiveRegion, LiveRegionMode.Polite))
            .assertTextContains(DownloadCopy.OFFLINE)
        compose.onNodeWithText(DownloadCopy.OFFLINE_ACTION).performClick()
        assertTrue(went)
    }

    /** Offline, every page but the player has the banner; the way to the downloads only where it leads somewhere. */
    @Test
    fun `the offline banner shows on every page but the player, leading to downloads only where it can`() {
        assertEquals(null, offlineBanner(offline = false, anyDone = true, top = Route.Home))
        assertEquals(null, offlineBanner(offline = true, anyDone = true, top = Route.Player("movie", "tt1", "tt1", "Sintel")))
        assertEquals(true, offlineBanner(offline = true, anyDone = true, top = Route.Search))
        assertEquals(false, offlineBanner(offline = true, anyDone = false, top = Route.Home))
        assertEquals(false, offlineBanner(offline = true, anyDone = true, top = Route.Downloads))
        assertEquals(false, offlineBanner(offline = true, anyDone = true, top = Route.DownloadedSeries("tt2")))
    }

    /** Nothing downloaded, or on the Downloads pages: the plain words, no way to the downloads (3.5). */
    @Test
    fun `offline with no way to downloads, the banner only says offline`() {
        compose.setContent { MarqueeTheme(Form.Phone) { OfflineBanner(onDownloads = null) } }
        compose.onNodeWithText(DownloadCopy.OFFLINE_ONLY).assertExists()
        compose.onAllNodesWithText(DownloadCopy.OFFLINE_ACTION).assertCountEquals(0)
    }

    /** The first download asks for notifications once: not before a download, not again after an answer (3.3). */
    @Test
    fun `the notification ask comes once, after the first download`() {
        core(snapshot())
        var shown by mutableStateOf(true)
        compose.setContent { MarqueeTheme(Form.Phone) { if (shown) NotificationAsk(actions) } }
        compose.onAllNodesWithText(DownloadCopy.NOTIFICATION_ASK).assertCountEquals(0)
        actions.act(DownloadAction.Resume, DownloadTarget.All)
        compose.onNodeWithText(DownloadCopy.NOTIFICATION_ASK).assertExists()
        compose.onNodeWithText("Continue").performClick()
        compose.onAllNodesWithText(DownloadCopy.NOTIFICATION_ASK).assertCountEquals(0)
        assertTrue(app.getSharedPreferences("downloads", Context.MODE_PRIVATE).getBoolean("notificationsAsked", false))
        // A page built again remembers the answer.
        shown = false
        compose.waitForIdle()
        shown = true
        actions.act(DownloadAction.Resume, DownloadTarget.All)
        compose.onAllNodesWithText(DownloadCopy.NOTIFICATION_ASK).assertCountEquals(0)
    }

    @Test
    fun `notifications already allowed are never asked for`() {
        core(snapshot())
        shadowOf(app).grantPermissions(Manifest.permission.POST_NOTIFICATIONS)
        compose.setContent { MarqueeTheme(Form.Phone) { NotificationAsk(actions) } }
        actions.act(DownloadAction.Resume, DownloadTarget.All)
        compose.onAllNodesWithText(DownloadCopy.NOTIFICATION_ASK).assertCountEquals(0)
    }
}
