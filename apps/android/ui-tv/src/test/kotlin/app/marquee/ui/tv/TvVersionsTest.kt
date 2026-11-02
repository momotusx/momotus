package app.marquee.ui.tv

import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertContentDescriptionEquals
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotSelected
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasScrollToKeyAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performScrollToKey
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.text.TextLayoutResult
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import androidx.lifecycle.compose.LocalLifecycleOwner
import app.marquee.core.PluginCopy
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.VersionsCopy
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.client.StateThreadRule
import app.marquee.core.model.EmptyAction
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.EmptyVersions
import app.marquee.core.model.Group
import app.marquee.core.model.HiddenSummary
import app.marquee.core.model.KindChip
import app.marquee.core.model.LearnedChoice
import app.marquee.core.model.LearnedNote
import app.marquee.core.model.Network
import app.marquee.core.model.Playability
import app.marquee.core.model.QualityPref
import app.marquee.core.model.RankKey
import app.marquee.core.model.ReasonLine
import app.marquee.core.model.RowOrigin
import app.marquee.core.model.SectionQuality
import app.marquee.core.model.SourceClass
import app.marquee.core.model.SourceKind
import app.marquee.core.model.SourceRows
import app.marquee.core.model.Speed
import app.marquee.core.model.StreamPrefs
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionSection
import app.marquee.core.model.VersionStatus
import app.marquee.core.model.VersionsConditions
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.views.VersionsView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.junit.After
import org.junit.Assume.assumeTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
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

    @get:Rule
    val stateThread = StateThreadRule()
    private val client = CoreClient(stateThread.dispatcher)
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
    }

    private val gib = 1L shl 30

    private fun row(
        key: String,
        line: ReasonLine,
        kind: SourceKind = SourceKind.Torrent,
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
        kind = kind,
        score = RankKey(true, true, true, true, true, true, 3, 0, Playability.Native, true, 2, false, 1080, 3, 0),
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

    /** How many lines [text] took. */
    private fun lines(text: String): Int {
        val layouts = mutableListOf<TextLayoutResult>()
        compose
            .onNodeWithText(text, useUnmergedTree = true)
            .fetchSemanticsNode()
            .config[SemanticsActions.GetTextLayoutResult]
            .action
            ?.invoke(layouts)
        return layouts.single().lineCount
    }

    private fun press(key: Key) {
        compose.onNode(isFocused()).performKeyInput { pressKey(key) }
        compose.waitForIdle()
    }

    private val played = mutableListOf<String>()
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

    /** What the screen sent the core, in order: written on the state thread, read on the test thread. */
    private val sent = CopyOnWriteArrayList<String>()

    /** The screen on a view whose core answers the subscribe with [snapshot]. */
    private fun show(
        snapshot: VersionsSnapshot,
        held: Boolean = false,
        heading: String = "Sintel",
    ) {
        client.attach { text ->
            sent += text
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
                    heading,
                    view,
                    FocusMemory(),
                    onPlay = { played += it },
                    onAddSource = { sources++ },
                    onBack = { backs++ },
                    onUndo = { undone += it },
                    held = held,
                )
            }
        }
        // The view opens once the screen composed and its recomposer runs: a snapshot the core's state thread
        // writes in the moment between the first composition and the recomposer starting is never heard of,
        // and the screen stays on "Opening" (seen once in about 75 gate runs, on a cold build).
        compose.waitForIdle()
        view.open()
        // Each round applies, idles and looks again: the header leaves "Finding versions" once the screen drew the snapshot.
        val deadline = System.currentTimeMillis() + 120_000
        while (view.snapshot == null ||
            compose.onAllNodesWithText(StreamChoiceCopy.FINDING).fetchSemanticsNodes().isNotEmpty()
        ) {
            check(System.currentTimeMillis() < deadline) {
                "the screen never drew the snapshot (snapshot ${view.snapshot != null}, status ${view.status})"
            }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    @Test
    fun `a row is its badges and one quiet line, its details kept in place for focus, with no addon name`() {
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), listOf(row("a", ReasonLine.Quick(1204, 2 * gib)))))
        compose.onAllNodesWithText("This TV", substring = true).assertCountEquals(0)
        compose.onAllNodesWithText("Torrentio", substring = true).assertCountEquals(0)
        compose.onAllNodesWithText("Via", substring = true, ignoreCase = true).assertCountEquals(0)
        // No column table.
        compose.onAllNodesWithText("Quality", ignoreCase = true).assertCountEquals(0)
        compose.onNodeWithText("2 GB, shared by 1,204 people").assertExists()
        val row = compose.onNodeWithContentDescription("1080p, HDR, 2 GB, shared by 1,204 people")
        val before = row.fetchSemanticsNode().size
        // The details line is there before focus (unseen), so focus only fades it in: the row keeps its height.
        compose.onNodeWithText("Shared by 1,204 people, 2 GB", substring = true, useUnmergedTree = true).assertExists()
        row.requestFocus()
        compose.waitForIdle()
        assertEquals(before, row.fetchSemanticsNode().size, "a row is one height, focused or not")
        row.performClick()
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
        compose.onAllNodesWithText("may start slowly", substring = true, ignoreCase = true).assertCountEquals(0)
        assertEquals(1, lines("2 GB, won't play here"), "one line at the TV width")
        compose.onNodeWithText("2 GB, won't play here").performClick()
        assertEquals(listOf("a"), played)
    }

    @Test
    fun `versions that need an account say one quiet line over dimmed rows, Back alone and focused, no account step`() {
        val rows = listOf(row("a", ReasonLine.Few(31)))
        show(snapshot(empty = EmptyVersions(EmptyKind.NeedsAccount, EmptyAction.None, 12, rows)))
        compose.onNodeWithText("None of these plays on this device", useUnmergedTree = true).assertExists()
        actions("Back")
        compose.onNodeWithText("Back").assertIsFocused()
        // Accounts are set up in Settings only (the owner, 2026-10-23): no button, no service, no account words.
        for (gone in listOf("Add an account", "Show them", "Hidden (", "Settings", "account")) {
            compose.onAllNodesWithText(gone, substring = true, ignoreCase = true).assertCountEquals(0)
        }
        // A dimmed row says why it is dimmed, never how fast it would start, on one line at the TV width.
        compose.onNodeWithText("2 GB, won't play here").assertExists()
        assertEquals(1, lines("2 GB, won't play here"))
        compose.onAllNodesWithText("may start slowly", substring = true, ignoreCase = true).assertCountEquals(0)
        compose.onNodeWithText("Back").performClick()
        assertEquals(0 to 1, sources to backs)
    }

    @Test
    fun `no addon gives Settings for the sources, then Back`() {
        show(snapshot(empty = EmptyVersions(EmptyKind.NoSource, EmptyAction.SettingsAddons, 0)))
        compose.onNodeWithText("Add a source in Settings", useUnmergedTree = true).assertExists()
        // The line says it: no "0 versions" beside it.
        compose.onAllNodesWithText("0 versions").assertCountEquals(0)
        actions(StreamChoiceCopy.SETTINGS, "Back")
        compose.onNodeWithText(StreamChoiceCopy.SETTINGS).assertIsFocused()
        compose.onNodeWithText(StreamChoiceCopy.SETTINGS).performClick()
        assertEquals(1, sources)
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
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), listOf(turkish, row("a", ReasonLine.Few(31)))))
        compose.onNodeWithText("Turkish").assertExists()
        compose.onNodeWithContentDescription("1080p, HDR, Turkish, 2 GB").assertExists()
        // The tag sits beside the row's badges, on their line.
        val tag = compose.onNodeWithText("Turkish", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        val badge =
            compose.onAllNodesWithText("1080p", useUnmergedTree = true).fetchSemanticsNodes().map { it.boundsInRoot }
        assertTrue(badge.any { kotlin.math.abs(it.center.y - tag.center.y) < 2f && it.right < tag.left })
    }

    @Test
    fun `only plugin rows give one line for the card, focus starts on the first of them, and Left keeps it there`() {
        val found =
            row("plugin:r/castle:h1", ReasonLine.Quick())
                .copy(
                    status = VersionStatus.Direct,
                    sizeBytes = null,
                    origin = RowOrigin.Plugin,
                    sourceName = "Castle",
                    host = "cdn.example.com",
                )
        show(snapshot().copy(plugins = SourceRows(listOf(found), pending = false)))
        compose.onNodeWithText(PluginCopy.CHOOSE).assertExists()
        compose.onNodeWithText("1 version").assertExists()
        compose.onAllNodesWithText("Play this version").assertCountEquals(0)
        val castle = hasContentDescription("1080p", substring = true)
        compose.waitUntil(5_000) { compose.onAllNodes(castle and isFocused()).fetchSemanticsNodes().isNotEmpty() }
        press(Key.DirectionLeft)
        compose.onNode(castle).assertIsFocused()
    }

    @Test
    fun `an open section's heading holds Show fewer, Left from any of its rows reaches it, Left again Play`() {
        // Told apart by their sizes: 1 GB to 8 GB.
        val rows = (1..8).map { row("r$it", ReasonLine.Few(30 + it)).copy(sizeBytes = it * gib) }
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), rows))
        compose.onNode(hasScrollToKeyAction()).performScrollToKey("fold:group:best")
        compose.onNodeWithContentDescription("Show all 8 Other good choices versions").requestFocus()
        compose.onNodeWithContentDescription("Show all 8 Other good choices versions").performKeyInput {
            pressKey(Key.DirectionCenter)
        }
        compose.waitForIdle()
        println(
            "open-dbg " +
                compose
                    .onAllNodes(
                        androidx.compose.ui.test
                            .hasContentDescription("1080p", substring = true),
                    ).fetchSemanticsNodes()
                    .map {
                        "${it.config.getOrElseNullable(
                            androidx.compose.ui.semantics.SemanticsProperties.ContentDescription,
                        ) { null }} ${it.config.getOrElseNullable(
                            androidx.compose.ui.semantics.SemanticsProperties.Focused,
                        ) { null }}"
                    },
        )
        // Focus goes to the 4th row, the first that was hidden.
        compose.onNodeWithContentDescription("1080p, HDR, 4 GB").assertIsFocused()
        press(Key.DirectionLeft)
        compose.onNode(isFocused()).assertContentDescriptionEquals("Show fewer Other good choices versions")
        press(Key.DirectionLeft)
        compose.onNodeWithText("Play this version").assertIsFocused()
        // Back into the list on the row it left, Left to the heading, OK folds: two presses from inside the section.
        press(Key.DirectionRight)
        compose.onNodeWithContentDescription("1080p, HDR, 4 GB").assertIsFocused()
        press(Key.DirectionLeft)
        press(Key.DirectionCenter)
        compose.onNodeWithContentDescription("Show all 8 Other good choices versions").assertIsFocused()
        compose.onAllNodesWithContentDescription("Show fewer Other good choices versions").assertCountEquals(0)
    }

    @Test
    fun `from the 50th row of a 100-row section Left scrolls back to the heading, and OK folds it`() {
        val rows = (1..100).map { row("r$it", ReasonLine.Few(30 + it)).copy(sizeBytes = it * gib) }
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), rows))
        compose.onNode(hasScrollToKeyAction()).performScrollToKey("fold:group:best")
        compose.onNodeWithContentDescription("Show all 100 Other good choices versions").requestFocus()
        compose.onNodeWithContentDescription("Show all 100 Other good choices versions").performKeyInput {
            pressKey(Key.DirectionCenter)
        }
        compose.waitForIdle()
        compose.onNode(hasScrollToKeyAction()).performScrollToKey("row:r50")
        compose.onNodeWithContentDescription("1080p, HDR, 50 GB").requestFocus()
        compose.onNodeWithContentDescription("1080p, HDR, 50 GB").assertIsFocused()
        // The heading is out of composition this far down: Left brings it back and lands on it, never on Play.
        compose.onAllNodesWithContentDescription("Show fewer Other good choices versions").assertCountEquals(0)
        press(Key.DirectionLeft)
        compose.waitUntil(5_000) {
            compose
                .onAllNodes(
                    isFocused() and hasContentDescription("Show fewer Other good choices versions"),
                ).fetchSemanticsNodes()
                .isNotEmpty()
        }
        press(Key.DirectionCenter)
        compose.onNodeWithContentDescription("Show all 100 Other good choices versions").assertIsFocused()
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `a dimmed row keeps its words at 4_5 to 1, never under an alpha`() {
        val rows = listOf(row("a", ReasonLine.Few(31)))
        show(snapshot(empty = EmptyVersions(EmptyKind.NothingPlaysHere, EmptyAction.ChooseVersion, 1, rows)))
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        val dp = compose.density.density
        for (text in listOf("1080p", "2 GB, won't play here")) {
            val at = compose.onNodeWithText(text, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
            val y = at.center.y.toInt()
            // Just left of the words: the badge's fill, or the gap after the line's icon.
            val ground = Color(image.getPixel((at.left - 3 * dp).toInt(), y))
            var best = 0f
            for (x in at.left.toInt() until at.right.toInt()) {
                for (py in at.top.toInt() until at.bottom.toInt()) {
                    val la = Color(image.getPixel(x, py)).luminance() + 0.05f
                    val lb = ground.luminance() + 0.05f
                    best = maxOf(best, maxOf(la, lb) / minOf(la, lb))
                }
            }
            println("dimmed-row {\"text\":\"$text\",\"ratio\":$best}")
            assertTrue(best >= 4.5f, "$text: $best")
        }
    }

    private val out = System.getenv("MARQUEE_SHOTS")?.let(::File)

    private fun save(name: String) {
        assumeTrue("MARQUEE_SHOTS is not set", out != null)
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        File(out, "$name.jpg").outputStream().use { image.compress(Bitmap.CompressFormat.JPEG, 90, it) }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `shot - versions that need an account, their rows dimmed`() {
        assumeTrue("MARQUEE_SHOTS is not set", out != null)
        val rows =
            listOf(
                row("a", ReasonLine.Few(31)),
                row("b", ReasonLine.Quick(1204, 2 * gib)).copy(sizeBytes = 3 * gib),
                row("tr", ReasonLine.Few(12)).copy(group = Group.OtherLanguages, languages = listOf("tr")),
            )
        show(snapshot(empty = EmptyVersions(EmptyKind.NeedsAccount, EmptyAction.None, 3, rows)))
        compose.mainClock.advanceTimeBy(2_000)
        save("tv-versions-needs-account")
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `shot - no source answering, with one kept here`() {
        assumeTrue("MARQUEE_SHOTS is not set", out != null)
        show(snapshot(empty = EmptyVersions(EmptyKind.Unreachable, EmptyAction.PlayFromDevice, 0, key = "kept")))
        compose.mainClock.advanceTimeBy(2_000)
        save("tv-versions-unreachable")
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `the loading card's skeletons stand out from the card they sit on`() {
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                TvVersions("Sintel", null, FocusMemory(), {}, {}, {}, {})
            }
        }
        compose.waitForIdle()
        val line = compose.onNodeWithTag(CARD_PLACEHOLDER, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        val dp = compose.density.density
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        // The block fills the card's parts; the card's ground is its padding to the left.
        val y = (line.top + 20 * dp).toInt()
        val block = Color(image.getPixel((line.left + 40 * dp).toInt(), y))
        val ground = Color(image.getPixel((line.left - 8 * dp).toInt(), y))
        val la = block.luminance() + 0.05f
        val lb = ground.luminance() + 0.05f
        val ratio = maxOf(la, lb) / minOf(la, lb)
        println(
            "skeleton {\"ratio\":$ratio,\"block\":\"$block\",\"ground\":\"$ground\",\"line\":\"$line\",\"dp\":$dp,\"w\":${image.width}}",
        )
        assertTrue(ratio > 1.05f, "the skeleton melts into its card: $ratio")
        if (out != null) save("tv-versions-loading")
    }

    @Test
    fun `the table's frame holds the first row's place while versions are found, so nothing moves`() {
        var current by mutableStateOf<VersionsView?>(null)
        compose.setContent {
            MarqueeTheme(Form.Tv) { TvVersions("Sintel", current, FocusMemory(), {}, {}, {}, {}) }
        }
        compose.waitForIdle()
        val frame = compose.onNodeWithTag("row-skeleton-0", useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        // The card's Play takes the focus, so the first row sits as it would, unlifted.
        val snapshot = snapshot(row("best", ReasonLine.Ready("TorBox", gib)), listOf(row("a", ReasonLine.Few(31))))
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
        current = view
        compose.waitForIdle()
        view.open()
        compose.waitUntil(60_000) {
            Snapshot.sendApplyNotifications()
            compose.onAllNodesWithContentDescription("1080p, HDR, 2 GB").fetchSemanticsNodes().isNotEmpty()
        }
        val first = compose.onNodeWithContentDescription("1080p, HDR, 2 GB").fetchSemanticsNode().boundsInRoot
        assertEquals(frame.top, first.top, "the first row lands where its frame was")
        assertEquals(frame.height, first.height, "a frame row is a row's height")
        compose.onAllNodesWithTag("row-skeleton-0", useUnmergedTree = true).assertCountEquals(0)
    }

    /**
     * A coherent page: the pick a 1080p of 4 GB (the core keeps it out of the list), Smaller file a 720p of 1.5 GB,
     * Sharper a 4K of 12 GB, and one more 1080p, each in the core's section for its quality.
     */
    private fun coherent(): VersionsSnapshot {
        val pick =
            row(
                "pick",
                ReasonLine.Ready("TorBox", 4 * gib),
            ).copy(
                sizeBytes = 4 * gib,
                status = VersionStatus.Ready("TorBox"),
                releaseName = "Sintel.2010.1080p.BluRay.mkv",
            )
        val small =
            row(
                "small",
                ReasonLine.Quick(1204, 3 * gib / 2),
            ).copy(quality = "720p", sizeBytes = 3 * gib / 2, releaseName = "Sintel.2010.720p.WEB.mkv")
        val sharp =
            row(
                "sharp",
                ReasonLine.Few(31),
            ).copy(quality = "4K", sizeBytes = 12 * gib, releaseName = "Sintel.2010.2160p.BluRay.mkv")
        val other =
            row(
                "other",
                ReasonLine.Few(31),
            ).copy(sizeBytes = 3 * gib, releaseName = "Sintel.2010.1080p.WEB.mkv")
        return snapshot(pick, listOf(sharp, other, small)).copy(
            smaller = small,
            sharper = sharp,
            sections =
                listOf(
                    VersionSection(SectionQuality.Uhd, listOf("sharp")),
                    VersionSection(SectionQuality.Fhd, listOf("other")),
                    VersionSection(SectionQuality.Hd, listOf("small")),
                ),
        )
    }

    @Test
    fun `the card reads the pick, the list leaves it out, and the shortcuts are the core's, each one press`() {
        show(coherent())
        // The card says the pick's own facts, as one node: the kicker, its badges, the whole why sentence; no file
        // name and no sources line.
        val pick = coherent().recommended!!
        compose.onNodeWithContentDescription(VersionsCopy.cardSpoken("Best for this TV", pick)).assertExists()
        assertTrue(VersionsCopy.cardSpoken("Best for this TV", pick).startsWith("Best for this TV. 1080p, HDR. "))
        compose.onAllNodesWithText("Sintel.2010.1080p.BluRay.mkv", substring = true).assertCountEquals(0)
        compose.onAllNodesWithText("One source", substring = true).assertCountEquals(0)
        // The core keeps the pick out of the sections, so no row repeats it.
        compose.onAllNodesWithContentDescription(VersionsCopy.rowSpoken(pick)).assertCountEquals(0)
        // The core's smaller and sharper, in its order, with their own facts.
        compose.onNodeWithText("Smaller file: 720p, 1.5 GB").assertExists()
        compose.onNodeWithText("Sharper: 4K, 12 GB").assertExists()
        // A row's line has its size first, a warning after it.
        compose.onNodeWithText("12 GB, may start slowly").assertExists()
        // The core's sections, in its order: 4K, 1080p, 720p and below.
        // Each heading is one heading node: "1080p, 1 version".
        val top = { t: String ->
            compose
                .onNodeWithContentDescription(t)
                .fetchSemanticsNode()
                .boundsInRoot.top
        }
        val tops = listOf(top("4K, 1 version"), top("1080p, 1 version"), top("720p and below, 1 version"))
        val heading = SemanticsMatcher.keyIsDefined(SemanticsProperties.Heading)
        compose.onNode(hasContentDescription("1080p, 1 version") and heading).assertExists()
        assertEquals(tops.sorted(), tops)
        // Down from Play reaches Smaller file, whose version is also the 720p row; OK plays it.
        compose.waitUntil(5_000) {
            compose.onAllNodes(isFocused() and hasText("Play this version")).fetchSemanticsNodes().isNotEmpty()
        }
        press(Key.DirectionDown)
        compose.onNodeWithText("Smaller file: 720p, 1.5 GB").assertIsFocused()
        // The 720p row, the last on the page, is also in the list.
        compose.onNode(hasScrollToKeyAction()).performScrollToKey("row:${coherent().smaller!!.key}")
        compose.onNodeWithContentDescription(VersionsCopy.rowSpoken(coherent().smaller!!)).assertExists()
        press(Key.DirectionCenter)
        assertEquals(listOf("small"), played)
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `shot - the card with its shortcuts and the core's sections`() {
        assumeTrue("MARQUEE_SHOTS is not set", out != null)
        show(coherent())
        compose.mainClock.advanceTimeBy(2_000)
        save("tv-versions-shortcuts")
    }

    @Test
    fun `over the player the rows hold their order through a re-rank, and a new one goes after them`() {
        val a = row("a", ReasonLine.Few(31)).copy(sizeBytes = 1 * gib)
        val b = row("b", ReasonLine.Few(31)).copy(sizeBytes = 2 * gib)
        val c = row("c", ReasonLine.Few(31)).copy(sizeBytes = 3 * gib)
        val best = row("best", ReasonLine.Ready("TorBox", gib)).copy(quality = "4K")
        show(snapshot(best, listOf(a, b)), held = true)
        // The core re-ranks b over a and finds c, ranked first.
        val reranked = snapshot(best, listOf(c, b, a))
        client.onMessage(
            """{"sub":7,"updates":[${CoreJson.encodeToString(VersionsSnapshot.serializer(), reranked)}]}""",
        )
        compose.waitUntil(10_000) {
            Snapshot.sendApplyNotifications()
            compose.onAllNodesWithContentDescription("1080p, HDR, 3 GB").fetchSemanticsNodes().isNotEmpty()
        }
        val top = { size: String ->
            compose
                .onNodeWithContentDescription("1080p, HDR, $size")
                .fetchSemanticsNode()
                .boundsInRoot.top
        }
        val tops = listOf(top("1 GB"), top("2 GB"), top("3 GB"))
        assertEquals(tops.sorted(), tops, "a, then b, then the new c")
    }

    @Test
    fun `the coherent page keeps the core's promises, so a fixture cannot drift from the core`() {
        val page = coherent()
        val pick = page.recommended!!
        val height = { quality: String -> mapOf("4K" to 2160, "1080p" to 1080, "720p" to 720).getValue(quality) }
        // The pick is the card, never a row; the shortcuts are rows of the list.
        assertTrue(page.rows.none { it.key == pick.key })
        assertTrue(page.rows.any { it.key == page.smaller!!.key } && page.rows.any { it.key == page.sharper!!.key })
        // Smaller file is under half the pick's size; Sharper is sharper than the pick.
        assertTrue(page.smaller!!.sizeBytes!! < pick.sizeBytes!! / 2)
        assertTrue(height(page.sharper!!.quality) > height(pick.quality))
        // Every section key is a row, every row is in one section, each in the section of its quality.
        val keys = page.sections.flatMap { it.keys }
        assertEquals(page.rows.map { it.key }.sorted(), keys.sorted())
        val quality = mapOf(SectionQuality.Uhd to "4K", SectionQuality.Fhd to "1080p", SectionQuality.Hd to "720p")
        for (section in page.sections) {
            for (key in section.keys) assertEquals(quality[section.quality], page.rows.single { it.key == key }.quality)
        }
    }

    @Test
    fun `the card's placeholder is the card's height, so nothing below moves when the pick lands`() {
        var current by mutableStateOf<VersionsView?>(null)
        compose.setContent {
            MarqueeTheme(Form.Tv) { TvVersions("Sintel", current, FocusMemory(), {}, {}, {}, {}) }
        }
        compose.waitForIdle()
        val held = compose.onNodeWithTag(CARD_PLACEHOLDER, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        val snapshot = coherent()
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
        current = view
        compose.waitForIdle()
        view.open()
        compose.waitUntil(60_000) {
            Snapshot.sendApplyNotifications()
            compose.onAllNodesWithTag(CARD, useUnmergedTree = true).fetchSemanticsNodes().isNotEmpty() &&
                compose.onAllNodesWithTag(CARD_PLACEHOLDER, useUnmergedTree = true).fetchSemanticsNodes().isEmpty()
        }
        val card = compose.onNodeWithTag(CARD, useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        assertEquals(held.top, card.top)
        assertEquals(held.height, card.height, "the placeholder is the card's height")
    }

    @Test
    fun `a 400-row section composes only what shows, and a held Down over 40 rows keeps every row one height`() {
        // Told apart by their sizes: 1 GB to 400 GB.
        val rows = (1..400).map { row("r$it", ReasonLine.Few(30)).copy(sizeBytes = it * gib) }
        val started = System.nanoTime()
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), rows))
        compose.onNode(hasScrollToKeyAction()).performScrollToKey("fold:group:best")
        compose.onNodeWithContentDescription("Show all 400 Other good choices versions").requestFocus()
        compose.onNodeWithContentDescription("Show all 400 Other good choices versions").performKeyInput {
            pressKey(Key.DirectionCenter)
        }
        compose.waitForIdle()
        val ms = (System.nanoTime() - started) / 1_000_000
        val composed =
            compose
                .onAllNodes(
                    hasContentDescription("1080p, HDR, ", substring = true),
                ).fetchSemanticsNodes()
                .size
        println("versions-400 {\"rows\":400,\"composed\":$composed,\"firstCompositionMs\":$ms}")
        assertTrue(composed < 20, "only what shows is composed: $composed")
        compose.onNodeWithContentDescription("1080p, HDR, 4 GB").assertIsFocused()
        val height =
            compose
                .onNode(isFocused())
                .fetchSemanticsNode()
                .size.height
        repeat(40) {
            press(Key.DirectionDown)
            assertEquals(
                height,
                compose
                    .onNode(isFocused())
                    .fetchSemanticsNode()
                    .size.height,
                "row ${it + 5}",
            )
        }
        compose.onNodeWithContentDescription("1080p, HDR, 44 GB").assertIsFocused()
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `a long release name takes the details line's two kept lines, and every row stays one height`() {
        val long =
            "The.Long.Title.Of.A.Film.2010.Directors.Cut.Remastered.2160p.UHD.BluRay.REMUX.HDR10.HEVC.Atmos-GROUP"
        val short = row("a", ReasonLine.Few(30)).copy(sizeBytes = 1 * gib)
        val longer = row("b", ReasonLine.Few(30)).copy(sizeBytes = 3 * gib, releaseName = long)
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), listOf(short, longer)))
        val results = mutableListOf<TextLayoutResult>()
        compose
            .onNodeWithText(VersionsCopy.details(longer), useUnmergedTree = true)
            .fetchSemanticsNode()
            .config[SemanticsActions.GetTextLayoutResult]
            .action
            ?.invoke(results)
        assertEquals(2, results.single().lineCount, "the release name goes on to a second line")
        val height = { size: String ->
            compose
                .onNodeWithContentDescription("1080p, HDR, $size")
                .fetchSemanticsNode()
                .size.height
        }
        assertEquals(height("1 GB"), height("3 GB"))
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `a version whose quality is not stated has no quality badge, and its row is one height with the rest`() {
        val known = row("a", ReasonLine.Few(30)).copy(sizeBytes = 1 * gib)
        val unstated =
            row(
                "b",
                ReasonLine.Few(30),
            ).copy(sizeBytes = 5 * gib, quality = "Quality not stated", features = emptyList())
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), listOf(known, unstated)))
        // Its section says it; no badge names nothing.
        compose.onAllNodesWithText("Unknown", useUnmergedTree = true).assertCountEquals(0)
        compose.onAllNodesWithText("Quality not stated", useUnmergedTree = true).assertCountEquals(0)
        val height = { spoken: String ->
            compose
                .onNodeWithContentDescription(spoken)
                .fetchSemanticsNode()
                .size.height
        }
        assertEquals(height("1080p, HDR, 1 GB"), height("5 GB"), "the badge line keeps its height with no badge")
    }

    /** [coherent] as the core counts three kinds (plans/plugins-and-marketplace.md 4.2), every kind listed. */
    private fun kinds(selected: SourceKind? = null) =
        coherent().copy(
            kinds =
                listOf(
                    KindChip(SourceKind.Torrent, null, 2),
                    KindChip(SourceKind.Direct, null, 1),
                    KindChip(SourceKind.Debrid, "TorBox", 1),
                ),
            selectedKind = selected,
        )

    private fun update(snapshot: VersionsSnapshot) {
        client.onMessage(
            """{"sub":7,"updates":[${CoreJson.encodeToString(VersionsSnapshot.serializer(), snapshot)}]}""",
        )
    }

    @Test
    fun `one kind of version draws no kind chips`() {
        show(coherent().copy(kinds = listOf(KindChip(SourceKind.Torrent, null, 4))))
        compose.onAllNodesWithText(StreamChoiceCopy.KIND_ALL).assertCountEquals(0)
    }

    @Test
    fun `Up from Play reaches the kind chips, OK asks the core for a kind and keeps the focus, Down goes back`() {
        show(kinds())
        compose.onNodeWithContentDescription("All").assertIsSelected()
        compose.onNodeWithContentDescription("Torrents, 2 versions").assertIsNotSelected()
        compose.onNodeWithContentDescription("Direct links, 1 version").assertExists()
        // The debrid chip says the service the core names.
        compose.onNodeWithContentDescription("TorBox, 1 version").assertExists()
        compose.waitUntil(5_000) {
            compose.onAllNodes(isFocused() and hasText("Play this version")).fetchSemanticsNodes().isNotEmpty()
        }
        press(Key.DirectionUp)
        compose.onNodeWithContentDescription("All").assertIsFocused()
        press(Key.DirectionRight)
        compose.onNodeWithContentDescription("Torrents, 2 versions").assertIsFocused()
        press(Key.DirectionCenter)
        compose.onNodeWithContentDescription("Torrents, 2 versions").assertIsFocused()
        compose.waitUntil(5_000) { sent.any { "\"setKind\"" in it && "\"torrent\"" in it } }
        press(Key.DirectionDown)
        compose.onNode(hasText("Play this version")).assertIsFocused()
    }

    @Test
    fun `under a kind the page is the core's pass for it, and the chips stay when one kind is left`() {
        // The core's torrent pass: its card, its rows and sections, no shortcuts.
        val pass = coherent()
        val card = pass.rows.first { it.key == "other" }
        show(
            kinds(SourceKind.Torrent).copy(
                recommended = card,
                rows = pass.rows.filter { it.key == "small" },
                smaller = null,
                sharper = null,
                sections = listOf(VersionSection(SectionQuality.Hd, listOf("small"))),
            ),
        )
        compose.onNodeWithContentDescription("Torrents, 2 versions").assertIsSelected()
        compose.onNodeWithContentDescription(VersionsCopy.cardSpoken("Best torrent", card)).assertExists()
        compose.onAllNodesWithText("Smaller file", substring = true).assertCountEquals(0)
        // Shown once, the chips stay while the page is open.
        update(coherent().copy(kinds = listOf(KindChip(SourceKind.Torrent, null, 4))))
        val all = VersionsCopy.cardSpoken("Best for this TV", coherent().recommended!!)
        compose.waitUntil(10_000) {
            Snapshot.sendApplyNotifications()
            compose.onAllNodesWithContentDescription(all).fetchSemanticsNodes().isNotEmpty()
        }
        compose.onNodeWithContentDescription("All").assertExists()
    }

    private fun bounds(text: String) =
        compose
            .onNode(hasText(text))
            .fetchSemanticsNode()
            .boundsInRoot

    @Test
    // Real text measures: the heading's width is what is pinned.
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `chips that arrive late move neither Play nor the card, and the heading keeps its width`() {
        val long = "Sintel, S1:E2 The Gatekeeper of the Dragon's Lair Beyond the Northern Mountains and the Sea"
        show(coherent(), heading = long)
        val play = bounds("Play this version")
        val heading = bounds(long)
        val title = bounds("Versions")
        update(kinds())
        compose.waitUntil(10_000) {
            Snapshot.sendApplyNotifications()
            compose.onAllNodesWithContentDescription("All").fetchSemanticsNodes().isNotEmpty()
        }
        assertEquals(play, bounds("Play this version"))
        assertEquals(heading, bounds(long))
        assertEquals(title, bounds("Versions"))
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `a long heading leaves the chip row All and the chip that is on, wholly in view`() {
        val long = "Sintel, S1:E2 The Gatekeeper of the Dragon's Lair Beyond the Northern Mountains and the Sea"
        show(kinds(selected = SourceKind.Debrid), heading = long)
        val track = compose.onNodeWithContentDescription(StreamChoiceCopy.KIND_LABEL).fetchSemanticsNode().boundsInRoot
        for (chip in listOf("All", "TorBox, 1 version")) {
            val box = compose.onNodeWithContentDescription(chip).fetchSemanticsNode().boundsInRoot
            assertTrue(
                box.left >= track.left - 0.5f && box.right <= track.right + 0.5f,
                "$chip $box out of the track $track",
            )
        }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `shot - the kind chips`() {
        assumeTrue("MARQUEE_SHOTS is not set", out != null)
        show(kinds())
        compose.mainClock.advanceTimeBy(2_000)
        save("tv-versions-kinds")
    }

    @Test
    fun `back from Settings the page asks the core again, so what was added shows without a press`() {
        var subscribes = 0
        val snapshot = snapshot(empty = EmptyVersions(EmptyKind.NoSource, EmptyAction.SettingsAddons, 0, emptyList()))
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    subscribes++
                    """{"sub":7,"snapshot":${CoreJson.encodeToString(VersionsSnapshot.serializer(), snapshot)}}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val view = VersionsView(client, scope, JsonObject(emptyMap()))
        val entry = Entry()
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                CompositionLocalProvider(LocalLifecycleOwner provides entry) {
                    TvVersions("Sintel", view, FocusMemory(), {}, { sources++ }, {}, {})
                }
            }
        }
        compose.waitForIdle()
        view.open()
        compose.waitUntil(60_000) {
            Snapshot.sendApplyNotifications()
            compose.onAllNodesWithText(StreamChoiceCopy.SETTINGS).fetchSemanticsNodes().isNotEmpty()
        }
        assertEquals(1, subscribes)
        compose.onNodeWithText(StreamChoiceCopy.SETTINGS).performClick()
        assertEquals(1, sources)
        // Settings covers the page, then the viewer comes back.
        compose.runOnIdle { entry.covered() }
        compose.runOnIdle { entry.back() }
        compose.waitUntil(10_000) {
            shadowOf(Looper.getMainLooper()).idle()
            subscribes == 2
        }
        // Once: coming back again without Settings asks nothing.
        compose.runOnIdle { entry.covered() }
        compose.runOnIdle { entry.back() }
        compose.waitForIdle()
        Thread.sleep(200)
        assertEquals(2, subscribes)
    }
}

/** A navigation entry's lifecycle: covered by Settings it stops, back on top it resumes, and it stays composed. */
internal class Entry : LifecycleOwner {
    private val registry = LifecycleRegistry.createUnsafe(this).apply { currentState = Lifecycle.State.RESUMED }
    override val lifecycle: Lifecycle get() = registry

    fun covered() {
        registry.currentState = Lifecycle.State.CREATED
    }

    fun back() {
        registry.currentState = Lifecycle.State.RESUMED
    }
}
