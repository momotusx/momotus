package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.model.BlockedReason
import app.marquee.core.model.BlockedVersions
import app.marquee.core.model.Group
import app.marquee.core.model.HiddenSummary
import app.marquee.core.model.LearnedChoice
import app.marquee.core.model.LearnedNote
import app.marquee.core.model.Network
import app.marquee.core.model.Playability
import app.marquee.core.model.QualityPref
import app.marquee.core.model.RankKey
import app.marquee.core.model.ReasonLine
import app.marquee.core.model.SourceClass
import app.marquee.core.model.Speed
import app.marquee.core.model.StreamPrefs
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionStatus
import app.marquee.core.model.VersionsConditions
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.views.VersionsView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.JsonObject
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
import kotlin.test.assertTrue

/** The Versions screen on a TV with the core's stream choice (plans/m13-stream-choice.md 3.7, 3.11). */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvVersionsTest {
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

    private val gib = 1L shl 30

    private fun row(
        key: String,
        line: ReasonLine,
    ) = VersionRow(
        key = key,
        quality = "1080p",
        features = listOf("HDR10"),
        sizeBytes = 2 * gib,
        status = VersionStatus.Torrent(1204),
        via = "Torrentio",
        releaseName = "$key.2010.1080p.mkv",
        group = Group.Best,
        failoverCount = 0,
        line = line,
        sourceClass = SourceClass.Swarm,
        score = RankKey(true, true, true, true, true, 3, Playability.Native, true, 3, false, 1080, 0),
    )

    private fun streams(sizeLimitBytes: Long?) =
        StreamPrefs(
            quality = QualityPref.Balanced,
            sizeLimitBytes = sizeLimitBytes,
            minSeeders = 50,
            hdr = true,
            preferReady = true,
            learn = true,
        )

    private fun snapshot(
        recommended: VersionRow? = null,
        rows: List<VersionRow> = emptyList(),
        blocked: BlockedVersions? = null,
        note: LearnedNote? = null,
        nobodySharing: Boolean = false,
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
        blocked = blocked,
        note = note,
        recommended = recommended,
        rows = rows,
        hidden = HiddenSummary(blocked?.count ?: 0),
        done = true,
        frozen = false,
        nobodySharing = nobodySharing,
    )

    private val played = mutableListOf<String>()
    private var accounts = 0
    private val undone = mutableListOf<LearnedNote>()

    /** The screen on a view whose core answers the subscribe with [snapshot]. */
    private fun show(snapshot: VersionsSnapshot) {
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    """{"sub":7,"snapshot":${CoreJson.encodeToString(VersionsSnapshot.serializer(), snapshot)}}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val view = VersionsView(client, scope, JsonObject(emptyMap()))
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                TvVersions(
                    "Sintel",
                    view,
                    FocusMemory(),
                    onPlay = { played += it.key },
                    onAddSource = {},
                    onAddAccount = { accounts++ },
                    onUndo = { undone += it },
                )
            }
        }
        // The view opens once the screen composed and its recomposer runs: a snapshot the core's state thread
        // writes in the moment between the first composition and the recomposer starting is never heard of,
        // and the screen stays on "Opening" (seen once in about 75 gate runs, on a cold build).
        compose.waitForIdle()
        view.open()
        // Each round applies, idles and looks again: the header counts the versions once the screen drew the snapshot.
        val deadline = System.currentTimeMillis() + 120_000
        while (compose.onAllNodesWithText(" version", substring = true).fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) {
                "the screen never drew the snapshot (snapshot ${view.snapshot != null}, status ${view.status})"
            }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    @Test
    fun `a row says its line, and focus opens it to the details`() {
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), listOf(row("a", ReasonLine.Quick(1204, 2 * gib)))))
        compose.onNodeWithText("This TV: 1080p, about 42 Mbit/s on Wi-Fi").assertExists()
        compose.onNodeWithText("Starts quickly", substring = true).assertExists()
        compose.onAllNodesWithText("1,204 seeders, 2 GB", substring = true).assertCountEquals(0)
        compose.onNodeWithText("Starts quickly", substring = true).requestFocus()
        compose.onNodeWithText("1,204 seeders, 2 GB", substring = true).assertExists()
        compose.onNodeWithText("Starts quickly", substring = true).performClick()
        assertEquals(listOf("a"), played)
    }

    @Test
    fun `versions that none of plays here say why, and Show them lists them`() {
        show(
            snapshot(
                blocked = BlockedVersions(BlockedReason.NothingPlaysHere, 42, listOf(row("a", ReasonLine.Few(31)))),
            ),
        )
        compose.onNodeWithText("Found 42 versions, but none plays on this device.").assertExists()
        compose.onAllNodesWithText("Add an account").assertCountEquals(0)
        compose.onAllNodesWithText("May start slowly", substring = true).assertCountEquals(0)
        compose.onNodeWithText("Show them").performClick()
        compose.onNodeWithText("May start slowly", substring = true).assertExists()
    }

    @Test
    fun `versions that need an account offer Add an account, and Right from Show them enters the list`() {
        show(snapshot(blocked = BlockedVersions(BlockedReason.NeedsAccount, 12, listOf(row("a", ReasonLine.Few(31))))))
        compose.onNodeWithText("Add an account").performClick()
        assertEquals(1, accounts)
        compose.onNodeWithText("Show them").performClick()
        compose.onNodeWithText("Show them").requestFocus()
        compose.onNodeWithText("Show them").performKeyInput { pressKey(Key.DirectionRight) }
        // ponytail: Robolectric's narrow fonts keep the two buttons on one line, so this checks where Right ends,
        // not the rule for the wrapped buttons of a real TV (Add an account above, reaching further right).
        compose.onNodeWithContentDescription("1080p, HDR10, 2 GB, via Torrentio").assertIsFocused()
    }

    @Test
    fun `the learned note shows with Undo over the list, and Undo sends it back`() {
        val note = LearnedNote(2160, LearnedChoice.Hdr, up = false, prefs = streams(null))
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), note = note))
        compose.onNodeWithText("Undo").performClick()
        compose.waitUntil(120_000) { undone.isNotEmpty() }
        assertEquals(listOf(note), undone)
        compose.onAllNodesWithText("Undo").assertCountEquals(0)
    }

    @Test
    fun `almost nobody sharing shows once, right before Play`() {
        show(snapshot(row("best", ReasonLine.Few(1)), listOf(row("a", ReasonLine.Few(0))), nobodySharing = true))
        // Once, on the card with its details line; not again over the list.
        compose.onAllNodesWithText(StreamChoiceCopy.NOBODY_SHARING).assertCountEquals(1)
        compose.onAllNodesWithText(StreamChoiceCopy.NOBODY_SHARING_DETAILS).assertCountEquals(1)
        // The card's line comes right before Play: above it, and nothing else between them.
        val lines = compose.onAllNodesWithText(StreamChoiceCopy.NOBODY_SHARING_DETAILS).fetchSemanticsNodes()
        val play =
            compose
                .onAllNodesWithText("Play this version")
                .fetchSemanticsNodes()
                .first()
                .boundsInRoot
        val card = lines.map { it.boundsInRoot }.filter { it.bottom <= play.top }.maxByOrNull { it.bottom }
        assertTrue(card != null && play.top - card.bottom < 64, "the line sits right over Play")
    }

    @Test
    fun `almost nobody sharing shows once over the list when there is no recommended card`() {
        show(snapshot(rows = listOf(row("a", ReasonLine.Few(0)), row("b", ReasonLine.Few(1))), nobodySharing = true))
        compose.onAllNodesWithText("Play this version").assertCountEquals(0)
        compose.onAllNodesWithText(StreamChoiceCopy.NOBODY_SHARING).assertCountEquals(1)
        compose.onAllNodesWithText(StreamChoiceCopy.NOBODY_SHARING_DETAILS).assertCountEquals(1)
    }
}
