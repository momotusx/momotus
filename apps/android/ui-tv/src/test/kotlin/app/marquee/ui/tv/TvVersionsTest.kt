package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.isFocused
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
import app.marquee.core.model.EmptyAction
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.EmptyVersions
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
        score = RankKey(true, true, true, true, true, 3, 0, Playability.Native, true, 2, false, 1080, 3, 0),
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
        empty: EmptyVersions? = null,
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
        empty = empty,
        note = note,
        recommended = recommended,
        rows = rows,
        hidden = HiddenSummary(0),
        done = true,
        frozen = false,
        nobodySharing = nobodySharing,
    )

    private fun press(key: Key) {
        compose.onNode(isFocused()).performKeyInput { pressKey(key) }
        compose.waitForIdle()
    }

    private val played = mutableListOf<String>()
    private var accounts = 0
    private var sources = 0
    private var backs = 0

    /** The empty state's buttons: exactly [labels], top to bottom. */
    private fun actions(vararg labels: String) {
        val all = listOf("Play from this device", "Try again", StreamChoiceCopy.SETTINGS, "Back", "Choose a version")
        for (gone in all - labels.toSet()) compose.onAllNodesWithText(gone).assertCountEquals(0)
        val tops =
            labels.map {
                compose
                    .onNodeWithText(it)
                    .fetchSemanticsNode()
                    .boundsInRoot.top
            }
        assertEquals(tops.sorted(), tops, "in the shared order")
    }

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
                    onPlay = { played += it },
                    onAddSource = { sources++ },
                    onAddAccount = { accounts++ },
                    onBack = { backs++ },
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
    fun `a row says its line, and focus opens it to the details, with no device line and no addon name`() {
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), listOf(row("a", ReasonLine.Quick(1204, 2 * gib)))))
        compose.onAllNodesWithText("This TV", substring = true).assertCountEquals(0)
        compose.onAllNodesWithText("Torrentio", substring = true).assertCountEquals(0)
        compose.onAllNodesWithText("Via", substring = true, ignoreCase = true).assertCountEquals(0)
        compose.onNodeWithText("Starts quickly", substring = true).assertExists()
        compose.onAllNodesWithText("1,204 seeders, 2 GB", substring = true).assertCountEquals(0)
        compose.onNodeWithText("Starts quickly", substring = true).requestFocus()
        compose.onNodeWithText("1,204 seeders, 2 GB", substring = true).assertExists()
        compose.onNodeWithText("Starts quickly", substring = true).performClick()
        assertEquals(listOf("a"), played)
    }

    @Test
    fun `none plays here says the core's line over the rows, dimmed and startable, with Back only, focused`() {
        val rows = listOf(row("a", ReasonLine.Few(31)))
        show(snapshot(empty = EmptyVersions(EmptyKind.NothingPlaysHere, EmptyAction.ChooseVersion, 42, rows)))
        compose.onNodeWithText("None of these plays on this device", useUnmergedTree = true).assertExists()
        actions("Back")
        compose.onNodeWithText("Back").assertIsFocused()
        for (gone in listOf("Add an account", "Show them", "Hidden (")) {
            compose.onAllNodesWithText(gone, substring = true).assertCountEquals(0)
        }
        compose.onNodeWithText("Back").performClick()
        assertEquals(1, backs)
        compose.onNodeWithText("May start slowly", substring = true).performClick()
        assertEquals(listOf("a"), played)
    }

    @Test
    fun `versions that need an account give Settings, focused, then Back, and Right enters the list`() {
        val rows = listOf(row("a", ReasonLine.Few(31)))
        show(snapshot(empty = EmptyVersions(EmptyKind.NeedsAccount, EmptyAction.SettingsAccounts, 12, rows)))
        compose.onNodeWithText("These play through a streaming account", useUnmergedTree = true).assertExists()
        actions(StreamChoiceCopy.SETTINGS, "Back")
        compose.onNodeWithText("May start slowly", substring = true).assertExists()
        compose.onNodeWithText(StreamChoiceCopy.SETTINGS).assertIsFocused()
        compose.onNodeWithText(StreamChoiceCopy.SETTINGS).performKeyInput { pressKey(Key.DirectionRight) }
        compose.onNodeWithContentDescription("1080p, HDR10, 2 GB").assertIsFocused()
        compose.onNodeWithText(StreamChoiceCopy.SETTINGS).performClick()
        assertEquals(1 to 0, accounts to sources)
    }

    @Test
    fun `no addon gives Settings for the sources, then Back`() {
        show(snapshot(empty = EmptyVersions(EmptyKind.NoSource, EmptyAction.SettingsAddons, 0)))
        compose.onNodeWithText("Add a source in Settings", useUnmergedTree = true).assertExists()
        actions(StreamChoiceCopy.SETTINGS, "Back")
        compose.onNodeWithText(StreamChoiceCopy.SETTINGS).assertIsFocused()
        compose.onNodeWithText(StreamChoiceCopy.SETTINGS).performClick()
        assertEquals(0 to 1, accounts to sources)
    }

    @Test
    fun `no source answering with one kept here offers Play from this device first, then Try again and Back`() {
        show(snapshot(empty = EmptyVersions(EmptyKind.Unreachable, EmptyAction.PlayFromDevice, 0, key = "kept")))
        compose.onNodeWithText("Can't reach any source", useUnmergedTree = true).assertExists()
        actions("Play from this device", "Try again", "Back")
        compose.onNodeWithText("Play from this device").assertIsFocused()
        compose.onNodeWithText("Play from this device").performClick()
        assertEquals(listOf("kept"), played)
    }

    @Test
    fun `no room says the one storage line with Back only`() {
        show(snapshot(empty = EmptyVersions(EmptyKind.NoRoom, EmptyAction.None, 0)))
        compose
            .onNodeWithText(
                "This device needs a little free space to play this",
                useUnmergedTree = true,
            ).assertExists()
        actions("Back")
        compose.onNodeWithText("Back").assertIsFocused()
    }

    @Test
    fun `the learned note shows with Undo under the list, and Undo sends it back`() {
        val note = LearnedNote(2160, LearnedChoice.Hdr, up = false, prefs = streams(null))
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), note = note))
        compose.onNodeWithText("Undo").performClick()
        compose.waitUntil(120_000) { undone.isNotEmpty() }
        assertEquals(listOf(note), undone)
        compose.onAllNodesWithText("Undo").assertCountEquals(0)
    }

    @Test
    fun `almost nobody sharing never shows its own bar, the why line says it`() {
        show(snapshot(row("best", ReasonLine.Few(1)), listOf(row("a", ReasonLine.Few(0))), nobodySharing = true))
        compose.onAllNodesWithText(StreamChoiceCopy.NOBODY_SHARING).assertCountEquals(0)
        compose.onAllNodesWithText(StreamChoiceCopy.NOBODY_SHARING_DETAILS).assertCountEquals(0)
    }

    @Test
    fun `a version in another language than the viewer's carries its language`() {
        val turkish = row("tr", ReasonLine.Few(31)).copy(group = Group.OtherLanguages, languages = listOf("tr"))
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), listOf(turkish)))
        compose.onNodeWithText("Turkish").assertExists()
        compose.onNodeWithContentDescription("1080p, HDR10, Turkish, 2 GB").assertExists()
    }

    @Test
    fun `an open section's heading holds Show fewer, Left from any of its rows reaches it, Left again Play`() {
        // Told apart by their sizes: 1 GB to 8 GB.
        val rows = (1..8).map { row("r$it", ReasonLine.Few(30 + it)).copy(sizeBytes = it * gib) }
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), rows))
        compose.onNodeWithText("Show all 8").requestFocus()
        compose.onNodeWithText("Show all 8").performKeyInput { pressKey(Key.DirectionCenter) }
        compose.waitForIdle()
        // Focus goes to the 4th row, the first that was hidden.
        compose.onNodeWithContentDescription("1080p, HDR10, 4 GB").assertIsFocused()
        press(Key.DirectionLeft)
        compose.onNode(isFocused()).assertTextEquals("Show fewer")
        press(Key.DirectionLeft)
        compose.onNodeWithText("Play this version").assertIsFocused()
        // Back into the list on the row it left, Left to the heading, OK folds: two presses from inside the section.
        press(Key.DirectionRight)
        compose.onNodeWithContentDescription("1080p, HDR10, 4 GB").assertIsFocused()
        press(Key.DirectionLeft)
        press(Key.DirectionCenter)
        compose.onNodeWithText("Show all 8").assertIsFocused()
        compose.onAllNodesWithText("Show fewer").assertCountEquals(0)
    }
}
