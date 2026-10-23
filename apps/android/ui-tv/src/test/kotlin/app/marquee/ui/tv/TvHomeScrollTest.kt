package app.marquee.ui.tv

import android.os.Handler
import android.os.Looper
import android.view.View
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.runtime.Composer
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.CompositionTracer
import androidx.compose.runtime.InternalComposeTracingApi
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.unit.dp
import app.marquee.core.TileCopy
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.model.Hero
import app.marquee.core.model.HomeHeader
import app.marquee.core.model.HomeRow
import app.marquee.core.model.HomeSnapshot
import app.marquee.core.model.Ratings
import app.marquee.core.model.RowState
import app.marquee.core.model.Tile
import app.marquee.core.views.HomeView
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.design.Tokens
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
import org.robolectric.shadows.ShadowBuild
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext
import kotlin.math.abs
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * One D-pad move on TV Home, frame by frame (the owner, 2026-10-22: scrolling down catches and stutters
 * instead of gliding): how the page moves in each frame, and which tiles compose inside a frame, where the
 * viewer waits on them, rather than ahead of it in the prefetch's idle time.
 */
@OptIn(InternalComposeTracingApi::class, ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TvHomeScrollTest {
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
    private var state = TvHomeState()
    private var view: View? = null

    private var tilesInFrame = 0
    private var tilesAhead = 0
    private var homeBodies = 0

    /** Counts composable bodies as they run (the compiler's trace markers), by where they run. */
    private val tracer =
        object : CompositionTracer {
            override fun isTraceInProgress() = true

            override fun traceEventEnd() {}

            override fun traceEventStart(
                key: Int,
                dirty1: Int,
                dirty2: Int,
                info: String,
            ) {
                if (info.startsWith("app.marquee.ui.tv.TvHome ")) homeBodies++
                if (!info.startsWith("app.marquee.ui.tv.TvTile ")) return
                if (Thread.currentThread().stackTrace.any { "PrefetchScheduler" in it.className }) {
                    tilesAhead++
                } else {
                    tilesInFrame++
                }
            }
        }

    @After
    fun stop() {
        Composer.setTracer(null)
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    private fun tile(
        id: String,
        name: String,
    ) = Tile(
        id = id,
        kind = "movie",
        name = name,
        posterShape = "poster",
        releaseInfo = "2024",
        ratings = Ratings(imdb = "7.4"),
    )

    private fun show(
        hero: Boolean = false,
        start: String = "Watching 1",
    ) {
        val rows =
            listOf(
                HomeRow(
                    TileCopy.CONTINUE_ROW,
                    "Continue Watching",
                    "",
                    RowState.Ready,
                    (1..10).map { tile("c$it", "Watching $it") },
                ),
            ) +
                (1..ROWS).map { n ->
                    HomeRow(
                        "walk:row:$n",
                        "Row $n",
                        "Walk",
                        RowState.Ready,
                        (1..20).map { tile("$n$it", "Title $n.$it") },
                    )
                }
        val top =
            if (hero) {
                Hero(
                    tile("h1", "Sintel"),
                    "#1 in Popular Movies",
                    "A girl and a dragon.",
                    listOf("Drama"),
                    "1 h",
                )
            } else {
                null
            }
        val snapshot = HomeSnapshot(HomeHeader(top), rows)
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                if (msg["op"]?.jsonPrimitive?.content == "subscribe") {
                    """{"sub":7,"snapshot":${CoreJson.encodeToString(HomeSnapshot.serializer(), snapshot)}}"""
                } else {
                    "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        val home = HomeView(client, scope)
        // Compose turns the lists' prefetch off when it sees Robolectric's fingerprint; the device's prefetch
        // is part of what this measures.
        ShadowBuild.setFingerprint("marquee-test")
        compose.setContent {
            view = LocalView.current
            MarqueeTheme(Form.Tv) {
                // Under the shell's pill, as in the app, which tells Home the room its top shelf's art reaches up into.
                Box(Modifier.padding(top = PILL.dp)) {
                    CompositionLocalProvider(TvNavRoom provides PILL.dp) {
                        TvHome(home, state, onOpen = {}, onPlay = {}, onSetup = {}, onAddons = {})
                    }
                }
            }
        }
        compose.waitForIdle()
        // Robolectric leaves the window GONE, and the prefetch waits for a window that shows.
        val attach =
            View::class.java
                .getDeclaredField("mAttachInfo")
                .apply { isAccessible = true }
                .get(view)
        attach.javaClass
            .getDeclaredField(
                "mWindowVisibility",
            ).apply { isAccessible = true }
            .setInt(attach, View.VISIBLE)
        home.open()
        val deadline = System.currentTimeMillis() + 60_000
        while (compose.onAllNodesWithText("Continue Watching").fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "Home never drew its rows" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
        compose.onNodeWithContentDescription(start).performSemanticsAction(SemanticsActions.RequestFocus)
        compose.mainClock.advanceTimeBy(2_000)
        compose.waitForIdle()
        Composer.setTracer(tracer)
    }

    /** What one press did: the page's move in each frame, in points, and the tiles composed. */
    private data class Move(
        val frames: List<Float>,
        val tilesInFrame: Int,
        val tilesAhead: Int,
        val homeBodies: Int,
        /** Where the header of the row focus went to came to rest. */
        val rest: Float = Float.NaN,
        /** The lane that comes into view under the focused one had its tiles composed before the press. */
        val ready: Boolean = false,
    ) {
        val total get() = frames.sum()
        val moving get() = frames.indexOfFirst { abs(it) > 0.5f }..frames.indexOfLast { abs(it) > 0.5f }

        /**
         * Frames that stood still before the move was 95 percent done: the page stopped and went on (two
         * scrolls taking turns). The last few points come in whole pixels, so the tail is left out.
         */
        val stalls: Int get() {
            val body = frames.runningFold(0f, Float::plus).drop(1).indexOfFirst { abs(it) >= abs(total) * 0.95f }
            return (moving.first..body).count { abs(frames[it]) <= 0.5f }
        }

        /** The last frame's share of the move: a page that stops at speed ends on a big one. */
        val landing get() = abs(frames[moving.last]) / abs(total)
    }

    private fun headerTop(title: String) =
        compose
            .onNodeWithText(title, useUnmergedTree = true)
            .getUnclippedBoundsInRoot()
            .top.value

    /** Where the page's items sit now, by key, in px. */
    private fun items(list: LazyListState) = list.layoutInfo.visibleItemsInfo.associate { it.key to it.offset }

    /** How far the page moved between two looks, in points (the TV canvas is 1920 points wide). */
    private fun moved(
        before: Map<Any, Int>,
        after: Map<Any, Int>,
    ): Float {
        val both = before.keys.intersect(after.keys)
        check(both.isNotEmpty()) { "the page jumped: no item in view in both frames" }
        val px = both.map { before.getValue(it) - after.getValue(it) }.average().toFloat()
        return px * TV_WIDTH / view!!.width
    }

    /**
     * Presses [key] for a remote's 50 ms, [times] times [gap] frames apart (a held key repeats every 112 ms past
     * the shell's throttle), and runs 16 ms frames one by one until [FRAMES] after the last press, following the
     * page.
     */
    private fun press(
        key: Key,
        times: Int = 1,
        gap: Int = FRAMES,
        list: LazyListState = state.page,
    ): Move {
        tilesInFrame = 0
        tilesAhead = 0
        homeBodies = 0
        compose.mainClock.autoAdvance = false
        var last = items(list)
        val frames = mutableListOf<Float>()
        repeat((times - 1) * gap + FRAMES) { frame ->
            if (frame % gap == 0 && frame / gap < times) compose.onNode(isFocused()).performKeyInput { keyDown(key) }
            if (frame % gap == 3 && frame / gap < times) compose.onRoot().performKeyInput { keyUp(key) }
            compose.mainClock.advanceTimeByFrame()
            compose.waitForIdle()
            val now = items(list)
            frames += moved(last, now)
            last = now
        }
        compose.mainClock.autoAdvance = true
        compose.waitForIdle()
        return Move(frames, tilesInFrame, tilesAhead, homeBodies)
    }

    private fun focused(name: String) =
        compose.onAllNodes(hasContentDescription(name) and isFocused()).fetchSemanticsNodes().isNotEmpty()

    /** Whether the first tile of Row [row] is composed now. */
    private fun composed(row: Int) = state.focus.requester("tile:walk:row:$row:${row}1") != null

    /** Down from Continue Watching to Row [to], one press a row. */
    private fun downTo(to: Int): List<Move> =
        (1..to).map { row ->
            // The lane under the one focus goes to comes into view on this move.
            val ready = composed(row + 1)
            press(Key.DirectionDown).copy(rest = headerTop("Row $row"), ready = ready).also {
                assertTrue(focused("Title $row.1"), "focus went to Row $row")
            }
        }

    @Test
    fun `a Down glides the row to the anchor and slows to a stop there`() {
        show()
        val moves = downTo(SETTLED + STEPS).drop(SETTLED)
        moves.forEachIndexed { i, move ->
            println(
                "BENCH glide step=${i + 1} rest=${move.rest} moving=${move.moving.count()} landing=${"%.3f".format(
                    move.landing,
                )} " +
                    move.frames.take(16).joinToString(",") { "%.0f".format(it) },
            )
            val row = SETTLED + i + 1
            // Before: three moving frames, the last a third of the move, and the row stopped at the screen's foot.
            assertTrue(move.moving.count() >= 8, "Row $row moved in ${move.moving.count()} frames: ${move.frames}")
            assertTrue(move.landing < 0.05f, "Row $row stopped at speed: ${move.frames}")
            // At the anchor: its header just under the content's top, under the pill.
            val header = move.rest
            val anchor = PILL + Tokens.space.m.value
            assertTrue(abs(header - anchor) < ANCHOR_SLACK, "Row $row's header rests at $header, the anchor is $anchor")
        }
    }

    @Test
    fun `a Down finds the lane it brings into view composed, and recomposes nothing on screen`() = lanesReady()

    @Test
    fun `on a low-memory TV, with fewer lanes ahead, a Down still finds its lane composed`() {
        state = TvHomeState(lowMemory = true)
        lanesReady()
    }

    @Test
    fun `from the hero's Play, every Down finds its lane composed`() = heroLanesReady(low = false)

    @Test
    fun `from the hero's Play on a low-memory TV, every Down finds its lane composed`() = heroLanesReady(low = true)

    /** From the hero's Play down five lanes: the lane each Down brings into view was composed before it. */
    private fun heroLanesReady(low: Boolean) {
        state = TvHomeState(lowMemory = low)
        show(hero = true, start = "Play Sintel")
        val ready =
            (1..5).map { row ->
                composed(row).also { press(Key.DirectionDown) }
            }
        println("BENCH hero-ready low=$low $ready")
        assertTrue(ready.all { it }, "a lane coming into view from the hero was not composed (low memory $low): $ready")
    }

    private fun lanesReady() {
        show()
        // Every Down from Home's first screen, then two Ups and a Down again (the prefetch follows the direction).
        val last = SETTLED + STEPS
        val downs = downTo(last)
        repeat(2) { press(Key.DirectionUp) }
        // Back on Row last - 2: the Down goes to Row last - 1 and brings Row last into view.
        val ready = composed(last)
        val moves = downs + press(Key.DirectionDown).copy(ready = ready)
        moves.forEachIndexed { i, move ->
            println(
                "BENCH compose step=${i + 1} ready=${move.ready} tilesInFrame=${move.tilesInFrame} " +
                    "tilesAhead=${move.tilesAhead} home=${move.homeBodies}",
            )
        }
        moves.forEachIndexed { i, move ->
            // Before: the lane under the one focus went to was composed only once the page began to move (the
            // first Down from Home's top, and the first after an Up), and a TV's idle time between the first
            // frames of the move is too short for 8 tiles.
            assertTrue(move.ready, "the lane coming into view on step ${i + 1} was not composed before the press")
            // Nothing composes inside a frame of the move (Robolectric gives the prefetch all the idle time it asks).
            assertEquals(0, move.tilesInFrame, "tiles composed inside a frame on step ${i + 1}")
            // The screen itself never recomposes on a move.
            assertEquals(0, move.homeBodies, "TvHome recomposed on step ${i + 1}")
        }
    }

    @Test
    fun `a held Down keeps the page moving from row to row and lands on the last`() {
        show()
        downTo(SETTLED)
        val move = press(Key.DirectionDown, times = 4, gap = HELD_GAP)
        val speeds = move.frames.map(::abs)
        // How much faster a frame goes than the one before, against the move's top speed: each repeat
        // starts Compose's scroll toward a farther row (the spring it runs is not ours to shape).
        val jolt = speeds.zipWithNext { a, b -> (b - a) / speeds.max() }.max()
        println(
            "BENCH held moving=${move.moving.count()} stalls=${move.stalls} jolt=${"%.3f".format(jolt)} " +
                move.frames.take(50).joinToString(",") { "%.0f".format(it) },
        )
        // Before: each repeat's move stopped dead before the next began.
        assertEquals(0, move.stalls, "the page stood still between repeats: ${move.frames}")
        assertTrue(focused("Title ${SETTLED + 4}.1"), "focus went down four rows")
        val anchor = PILL + Tokens.space.m.value
        val header = headerTop("Row ${SETTLED + 4}")
        assertTrue(abs(header - anchor) < ANCHOR_SLACK, "the last row rests at $header, the anchor is $anchor")
    }

    @Test
    fun `Right along a lane glides each tile in with the next one peeking past it`() {
        show()
        downTo(SETTLED)
        val lane = state.rowState("walk:row:$SETTLED")
        val moves =
            (2..12).map { tile ->
                press(Key.DirectionRight, list = lane).also {
                    assertTrue(focused("Title $SETTLED.$tile"), "focus went to tile $tile")
                }
            }
        moves.forEachIndexed { i, move ->
            println(
                "BENCH lane step=${i + 1} total=${"%.0f".format(move.total)} moving=${move.moving.count()} " +
                    move.frames.take(16).joinToString(",") { "%.0f".format(it) },
            )
        }
        val next = lane.layoutInfo.visibleItemsInfo.first { it.key == "tile:walk:row:$SETTLED:${SETTLED}13" }
        // How much of the tile after the focused one shows, in points.
        val peek = (lane.layoutInfo.viewportEndOffset - next.offset) * TV_WIDTH / view!!.width
        println("BENCH lane peek=${"%.0f".format(peek)}")
        // Before: the lane stopped as soon as the focused tile showed, at the edge and at speed, and the next one
        // did not show. Now it peeks: the inset and peek clear of the end, less the gap and the focus scale.
        assertTrue(peek >= Tokens.tvTiles.peek.value - Tokens.tvTiles.gap.value, "the next tile shows $peek points")
        val glides = moves.filter { abs(it.total) > 1f }
        assertTrue(glides.isNotEmpty(), "the lane never moved")
        glides.forEach { move ->
            assertTrue(move.moving.count() >= 6, "the lane moved in ${move.moving.count()} frames: ${move.frames}")
            assertTrue(move.landing < 0.1f, "the lane stopped at speed: ${move.frames}")
        }
    }

    @Test
    fun `Right along a lane leaves the page still`() {
        show()
        downTo(SETTLED)
        // The owner's walk on the Fire TV (2026-10-25): about 15 presses along a row, the page bobbing on each.
        val moves =
            (2..16).map { tile ->
                press(Key.DirectionRight).also {
                    assertTrue(focused("Title $SETTLED.$tile"), "focus went to tile $tile")
                }
            }
        println(
            "BENCH lane-still " + moves.joinToString(",") { "%.1f".format(it.frames.sumOf { f -> abs(f).toDouble() }) },
        )
        // Home's page has its own anchor, which leaves a row in view alone; a page on the TV's default pivot moved
        // with the focus tween on every press (FocusStillTest).
        moves.forEachIndexed { i, move ->
            assertTrue(move.frames.all { abs(it) < 0.5f }, "the page moved on Right ${i + 1}: ${move.frames}")
        }
    }

    @Test
    fun `Up into Continue Watching, then to the hero's Play, each move the page once`() {
        show(hero = true)
        downTo(2)
        press(Key.DirectionUp)
        // Into Continue Watching under the top shelf: the anchor brings the card in whole, with its lines.
        val toCard = press(Key.DirectionUp)
        assertTrue(focused("Watching 1"), "focus went back to Continue Watching")
        // On to Play: the page's own scroll to its top, the anchor holding while the hero has focus.
        val toPlay = press(Key.DirectionUp)
        for ((name, move) in listOf("card" to toCard, "play" to toPlay)) {
            println(
                "BENCH up-$name stalls=${move.stalls} moving=${move.moving.count()} " +
                    move.frames.take(30).joinToString(",") { "%.0f".format(it) },
            )
            // Before: the anchor brought the row down and stopped, then the page went on up to the hero.
            assertEquals(0, move.stalls, "the page stopped and went on into the $name: ${move.frames}")
        }
        assertTrue(focused("Play Sintel"), "focus went up to the hero's Play")
        assertEquals(0, state.page.firstVisibleItemIndex)
        assertEquals(0, state.page.firstVisibleItemScrollOffset)
    }

    @Test
    fun `the first Down from the hero's Play glides Continue Watching in and slows to a stop`() {
        show(hero = true, start = "Play Sintel")
        // The page's biggest move: from its top, past the top shelf, to the first card.
        val move = press(Key.DirectionDown)
        println(
            "BENCH hero-down total=${"%.0f".format(move.total)} stalls=${move.stalls} " +
                "landing=${"%.3f".format(
                    move.landing,
                )} " + move.frames.take(20).joinToString(",") { "%.0f".format(it) },
        )
        assertTrue(focused("Watching 1"), "focus went down to Continue Watching")
        assertEquals(0, move.stalls, "the page stopped and went on: ${move.frames}")
        assertTrue(move.landing < 0.05f, "the page stopped at speed: ${move.frames}")
        val anchor = PILL + Tokens.space.m.value
        val header = headerTop("Continue Watching")
        assertTrue(abs(header - anchor) < ANCHOR_SLACK, "Continue Watching rests at $header, the anchor is $anchor")
    }

    @Test
    fun `with the lanes composed ahead, the top shelf's art follows the page without a jump`() {
        show(hero = true, start = "Play Sintel")
        // Where the art is laid out, its scroll included (TvHomeShelfTest's walk, here with the cache window and
        // the prefetch on: a lane composed ahead must never count as one in the list's layout).
        val art = { compose.onNodeWithTag(TOP_SHELF_ART, useUnmergedTree = true).fetchSemanticsNode() }
        val step = 20f
        var last = art().positionInRoot.y
        var left = false
        repeat(80) {
            compose.runOnIdle { state.page.dispatchRawDelta(step) }
            compose.waitForIdle()
            val node = art()
            val top = node.positionInRoot.y
            if (state.page.firstVisibleItemIndex > 0) left = true
            // Each step moves the art by the step while any of it is on the screen: it never drops away early.
            if (last + node.size.height > step) assertEquals(last - step, top, 1.5f, "the art jumped at step $it")
            last = top
        }
        assertTrue(left, "the hero item never left the list's first place")
    }

    private companion object {
        const val ROWS = 7
        const val FRAMES = 40

        /** Moves before the measured ones: the page leaves its top. */
        const val SETTLED = 2
        const val STEPS = 3

        /** A held key's repeat after the shell's throttle (DpadThrottle, 112 ms), in frames. */
        const val HELD_GAP = 7

        /** The shell's pill and the gap over it, in points. */
        const val PILL = 114f

        const val TV_WIDTH = 1920f

        /** The most tiles a 1920 point lane shows. */
        const val LANE_TILES = 8

        /** The page anchors the focused tile where it is laid out, not its scaled top (FocusStillTest): whole points. */
        const val ANCHOR_SLACK = 2f
    }
}
