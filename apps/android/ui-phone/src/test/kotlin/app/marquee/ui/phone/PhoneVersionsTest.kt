package app.marquee.ui.phone

import android.os.Handler
import android.os.Looper
import androidx.activity.OnBackPressedDispatcher
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.click
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasScrollToIndexAction
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToIndex
import androidx.compose.ui.test.performTouchInput
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import androidx.lifecycle.compose.LocalLifecycleOwner
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
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The Versions screen on a phone with the core's stream choice (plans/m13-stream-choice.md 3.7, 3.9, 3.11). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w411dp-h2400dp")
class PhoneVersionsTest {
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

    private val played = mutableListOf<String>()
    private var accounts = 0
    private var back: OnBackPressedDispatcher? = null
    private var backs = 0
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
            back = LocalOnBackPressedDispatcherOwner.current?.onBackPressedDispatcher
            MarqueeTheme(Form.Phone) {
                PhoneVersions(
                    "Sintel",
                    view,
                    onPlay = { played += it },
                    onAddSource = {},
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
    fun `one tap plays a row, its Details open a sheet with its own Play, and Back closes the sheet first`() {
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), listOf(row("a", ReasonLine.Quick(1204, 2 * gib)))))
        compose.onAllNodesWithText("This phone", substring = true).assertCountEquals(0)
        compose.onAllNodesWithText("Torrentio", substring = true).assertCountEquals(0)
        // One tap plays: no open-first step.
        compose.onNodeWithText("2 GB, shared by 1,204 people").assertIsDisplayed().performClick()
        assertEquals(listOf("a"), played)
        // The info button, the row's sibling at its trailing edge, opens the sheet; the reader has the row's Details.
        val rowNode = compose.onNodeWithContentDescription("1080p, HDR, 2 GB, shared by 1,204 people")
        val bounds = rowNode.fetchSemanticsNode().boundsInRoot
        compose.onRoot().performTouchInput { click(Offset(bounds.right + 24 * density, bounds.center.y)) }
        compose.onNodeWithTag(DETAILS_SHEET).assertIsDisplayed()
        compose.onNodeWithText("1,204 seeders, 2 GB").assertIsDisplayed()
        compose.onNode(hasText("Play this version") and hasAnyAncestor(hasTestTag(DETAILS_SHEET))).performClick()
        assertEquals(listOf("a", "a"), played)
        // Back closes the sheet; the page stays.
        compose.runOnIdle { back!!.onBackPressed() }
        compose.onAllNodesWithTag(DETAILS_SHEET).assertCountEquals(0)
        assertEquals(0, backs)
        // The reader's way in: the row's Details action.
        compose.runOnIdle {
            rowNode
                .fetchSemanticsNode()
                .config[SemanticsActions.CustomActions]
                .single { it.label == "Details" }
                .action()
        }
        compose.onNodeWithTag(DETAILS_SHEET).assertIsDisplayed()
    }

    @Test
    fun `the details sheet starts with what the row shows, then the seeders, the file and the language`() {
        val tr =
            row("a", ReasonLine.VeryFew(0)).copy(
                sizeBytes = 117L shl 20,
                group = Group.OtherLanguages,
                languages = listOf("tr"),
            )
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), listOf(tr)))
        // The row itself: its badges, its language, its size and its warning.
        compose.onNodeWithText("117 MB, may not start", useUnmergedTree = true).assertExists()
        compose.runOnIdle {
            compose
                .onNodeWithContentDescription("1080p, HDR, Turkish", substring = true)
                .fetchSemanticsNode()
                .config[SemanticsActions.CustomActions]
                .single { it.label == "Details" }
                .action()
        }
        compose.onNodeWithTag(DETAILS_SHEET).assertIsDisplayed()
        val inSheet = hasAnyAncestor(hasTestTag(DETAILS_SHEET))

        fun top(text: String) =
            compose
                .onAllNodes(hasText(text, substring = true) and inSheet, useUnmergedTree = true)
                .fetchSemanticsNodes()
                .first()
                .boundsInRoot.top
        // Nothing the row shows is dropped: the badge, the language, the size and the warning come first.
        val tops = listOf(top("1080p"), top("117 MB, may not start"), top("0 seeders"), top("a.2010"))
        assertEquals(tops.sorted(), tops, "badges, the row's line, the seeders, then the file")
        assertTrue(
            compose
                .onAllNodes(
                    hasText("Turkish") and inSheet,
                    useUnmergedTree = true,
                ).fetchSemanticsNodes()
                .isNotEmpty(),
        )
    }

    @Test
    fun `an open sheet is modal, the page under it is out of the reader's reach and the focus is in the sheet`() {
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), listOf(row("a", ReasonLine.Quick(1204, 2 * gib)))))
        val spoken = "1080p, HDR, 2 GB, shared by 1,204 people"
        compose.runOnIdle {
            compose
                .onNodeWithContentDescription(spoken)
                .fetchSemanticsNode()
                .config[SemanticsActions.CustomActions]
                .single { it.label == "Details" }
                .action()
        }
        compose.onNodeWithTag(DETAILS_SHEET).assertIsDisplayed()
        // Nothing under the sheet is in the tree a screen reader walks: not the row, not the card, not the heading.
        compose.onAllNodesWithContentDescription(spoken).assertCountEquals(0)
        compose.onAllNodesWithTag(CARD).assertCountEquals(0)
        compose.onAllNodesWithText("Versions").assertCountEquals(0)
        compose.onNode(hasText("Play this version") and hasAnyAncestor(hasTestTag(DETAILS_SHEET))).assertIsFocused()
        // Closed, the page is back for the reader, and the focus is back on the row's info button at its trailing edge.
        compose.runOnIdle { back!!.onBackPressed() }
        val rowBounds =
            compose
                .onNodeWithContentDescription(
                    spoken,
                ).assertIsDisplayed()
                .fetchSemanticsNode()
                .boundsInRoot
        val focused = compose.onNode(isFocused(), useUnmergedTree = true).fetchSemanticsNode().boundsInRoot
        assertTrue(focused.left >= rowBounds.right - 1 && focused.center.y in rowBounds.top..rowBounds.bottom)
    }

    @Test
    fun `a tapped fold keeps its place under the finger, opening and folding`() {
        // A second section below.
        val rows =
            (1..40).map { row("r$it", ReasonLine.Quick(1204, 2 * gib)) } +
                (1..40).map { row("s$it", ReasonLine.Quick(1204, gib)).copy(group = Group.SmallerFaster) }
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), rows))

        // The fold says itself with its section ("Show all 40 Other good choices versions"); the first is this one.
        fun fold(text: String) = compose.onAllNodesWithContentDescription(text, substring = true).onFirst()

        fun top(text: String) = fold(text).fetchSemanticsNode().boundsInRoot.top
        // The second section opened first gives the list room below the first one's fold; that fold held its place,
        // so the list moved, and is put back at its top.
        compose.onAllNodesWithContentDescription(StreamChoiceCopy.showAll(40), substring = true)[1].performClick()
        compose.waitForIdle()
        compose.onNode(hasScrollToIndexAction()).performScrollToIndex(0)
        compose.waitForIdle()
        val before = top(StreamChoiceCopy.showAll(40))
        fold(StreamChoiceCopy.showAll(40)).performClick()
        compose.waitForIdle()
        // Forty rows opened above it, and the toggle, now Show fewer, is where the finger was.
        assertEquals(before, top(StreamChoiceCopy.SHOW_FEWER), 1f)
        fold(StreamChoiceCopy.SHOW_FEWER).performClick()
        compose.waitForIdle()
        assertEquals(before, top(StreamChoiceCopy.showAll(40)), 1f)
    }

    @Test
    fun `a re-rank between the press and the release plays the pressed version`() {
        val a = row("a", ReasonLine.Quick(1204, gib)).copy(sizeBytes = 1 * gib)
        val b = row("b", ReasonLine.Quick(1204, gib)).copy(sizeBytes = 3 * gib)
        val best = row("best", ReasonLine.Ready("TorBox", gib))
        show(snapshot(best, listOf(a, b)))
        val pressed = compose.onNodeWithContentDescription("1080p, HDR, 1 GB", substring = true)
        val at = pressed.fetchSemanticsNode().boundsInRoot.center
        compose.onRoot().performTouchInput { down(at) }
        // The core puts b over a while the finger is down.
        val reranked = snapshot(best, listOf(b, a))
        client.onMessage(
            """{"sub":7,"updates":[${CoreJson.encodeToString(VersionsSnapshot.serializer(), reranked)}]}""",
        )
        compose.waitUntil(10_000) {
            Snapshot.sendApplyNotifications()
            shadowOf(Looper.getMainLooper()).idle()
            val top = { size: String ->
                compose
                    .onNodeWithContentDescription(
                        "1080p, HDR, $size",
                        substring = true,
                    ).fetchSemanticsNode()
                    .boundsInRoot.top
            }
            top("3 GB") < top("1 GB")
        }
        compose.onRoot().performTouchInput { up() }
        compose.waitForIdle()
        assertEquals(listOf("a"), played)
    }

    @Test
    fun `account-only versions say the core's line over dimmed rows with Settings, then Back`() {
        val rows = listOf(row("a", ReasonLine.Few(31)))
        show(snapshot(empty = EmptyVersions(EmptyKind.NeedsAccount, EmptyAction.SettingsAccounts, 12, rows)))
        compose.onNodeWithText("These play through a streaming account", useUnmergedTree = true).assertIsDisplayed()
        compose.onNodeWithText("12 versions").assertIsDisplayed()
        for (gone in listOf("No playable", "Add an account", "Show them", "Hidden (", "Choose a version")) {
            compose.onAllNodesWithText(gone, substring = true).assertCountEquals(0)
        }
        // A dimmed row says why it is dimmed, never how fast it would start.
        compose.onNodeWithText("2 GB, needs an account").assertIsDisplayed()
        compose.onAllNodesWithText("may start slowly", substring = true, ignoreCase = true).assertCountEquals(0)
        val settings =
            compose
                .onNodeWithText(StreamChoiceCopy.SETTINGS)
                .fetchSemanticsNode()
                .boundsInRoot.top
        assertTrue(
            settings <
                compose
                    .onNodeWithText("Back")
                    .fetchSemanticsNode()
                    .boundsInRoot.top,
            "Settings, then Back",
        )
        compose.onNodeWithText(StreamChoiceCopy.SETTINGS).performClick()
        compose.onNodeWithText("Back").performClick()
        assertEquals(1 to 1, accounts to backs)
    }

    @Test
    fun `none plays here says the core's line over dimmed rows that still start, with Back only`() {
        val rows = listOf(row("a", ReasonLine.Few(31)))
        show(snapshot(empty = EmptyVersions(EmptyKind.NothingPlaysHere, EmptyAction.ChooseVersion, 1, rows)))
        compose.onNodeWithText("None of these plays on this device", useUnmergedTree = true).assertIsDisplayed()
        compose.onNodeWithText("Back").assertIsDisplayed()
        for (gone in listOf("Choose a version", "Try again", StreamChoiceCopy.SETTINGS, "Play from this device")) {
            compose.onAllNodesWithText(gone).assertCountEquals(0)
        }
        // Dimmed, and still startable: a tap opens it to its own Play.
        compose.onAllNodesWithText("may start slowly", substring = true, ignoreCase = true).assertCountEquals(0)
        compose.onNodeWithText("2 GB, won't play here").performClick()
        assertEquals(listOf("a"), played)
        compose.onNodeWithText("Back").performClick()
        assertEquals(1, backs)
    }

    @Test
    fun `no source answering with one kept here plays it from this device`() {
        show(snapshot(empty = EmptyVersions(EmptyKind.Unreachable, EmptyAction.PlayFromDevice, 0, key = "kept")))
        compose.onNodeWithText("Can't reach any source", useUnmergedTree = true).assertIsDisplayed()
        compose.onNodeWithText("Try again").assertIsDisplayed()
        compose.onNodeWithText("Play from this device").performClick()
        assertEquals(listOf("kept"), played)
    }

    @Test
    fun `a version in another language than the viewer's carries its language`() {
        val turkish = row("tr", ReasonLine.Few(31)).copy(group = Group.OtherLanguages, languages = listOf("tr"))
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), listOf(turkish)))
        compose.onNodeWithText("Turkish").assertIsDisplayed()
    }

    @Test
    fun `the learned note shows with Undo, and Undo sends it back`() {
        val note = LearnedNote(1080, LearnedChoice.Size, up = true, prefs = streams(10 * gib))
        show(snapshot(row("best", ReasonLine.Ready("TorBox", gib)), note = note))
        // Its words are a live region (LiveSpoken), checked in StreamChoiceCopyTest.
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
    fun `a shortcut under the card plays in one tap`() {
        // The coherent page the core would send: a 1080p pick of 4 GB, Smaller file a 720p of 1.5 GB, Sharper a
        // 4K of 12 GB.
        val pick =
            row(
                "pick",
                ReasonLine.Ready("TorBox", 4 * gib),
            ).copy(sizeBytes = 4 * gib, status = VersionStatus.Ready("TorBox"))
        val small = row("small", ReasonLine.Quick(1204, 3 * gib / 2)).copy(quality = "720p", sizeBytes = 3 * gib / 2)
        val sharp = row("sharp", ReasonLine.Few(31)).copy(quality = "4K", sizeBytes = 12 * gib)
        show(snapshot(pick, listOf(sharp, small)).copy(smaller = small, sharper = sharp))
        compose.onNodeWithText("Sharper: 4K, 12 GB").performClick()
        compose.onNodeWithText("Smaller file: 720p, 1.5 GB").performClick()
        assertEquals(listOf("sharp", "small"), played)
    }

    @Test
    fun `back from Settings the page asks the core again, so what was added shows without a press`() {
        var subscribes = 0
        val rows = listOf(row("a", ReasonLine.Few(31)))
        val snapshot = snapshot(empty = EmptyVersions(EmptyKind.NeedsAccount, EmptyAction.SettingsAccounts, 1, rows))
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
            MarqueeTheme(Form.Phone) {
                CompositionLocalProvider(LocalLifecycleOwner provides entry) {
                    PhoneVersions("Sintel", view, {}, {}, { accounts++ }, {}, {})
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
        assertEquals(1, accounts)
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
