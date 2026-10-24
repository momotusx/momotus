package app.marquee.design

import android.content.pm.PackageManager
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyGridState
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performSemanticsAction
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * Left and Right inside a row leave the page where it is (the owner on the Fire TV, 2026-10-25: Discover
 * bobbed up and down on every move along a row). On a TV (the leanback feature) a page without a scroll spec
 * of its own pivots its focused child to 30 percent of its height, and it reads where the child is on every
 * frame of its scroll: the focus scale, were it laid out, would grow the tile's top past that line and the page
 * would follow it, then back for the next tile.
 */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class FocusStillTest {
    @get:Rule
    val compose = createComposeRule()

    @Before
    fun leanback() {
        shadowOf(RuntimeEnvironment.getApplication().packageManager)
            .setSystemFeature(PackageManager.FEATURE_LEANBACK, true)
    }

    @Test
    fun `Right along a lane on a page leaves the page still`() {
        val page = LazyListState()
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                val metrics = Marquee.tiles
                LazyColumn(state = page, modifier = Modifier.fillMaxSize()) {
                    items((1..LANES).toList()) { lane ->
                        LazyRow(
                            contentPadding = PaddingValues(horizontal = Marquee.safe.side),
                            horizontalArrangement = Arrangement.spacedBy(metrics.gap),
                        ) {
                            items((1..TILES).toList()) { PosterTile("T$lane.$it", null, {}, caption = "2024") }
                        }
                    }
                }
            }
        }
        val moves =
            still(start = "T3.1", presses = PRESSES) {
                page.firstVisibleItemIndex to
                    page.firstVisibleItemScrollOffset
            }
        assertTrue(focused("T3.${PRESSES + 1}"), "focus went along the lane")
        println("BENCH lane-still ${moves.joinToString(",")}")
        assertEquals(setOf(moves.first()), moves.toSet(), "the page moved on Right: $moves")
    }

    @Test
    fun `Right along a line of a grid leaves the grid still`() {
        val grid = LazyGridState()
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                val metrics = Marquee.tiles
                // Discover's and Library's grid.
                LazyVerticalGrid(
                    columns = GridCells.Adaptive(metrics.poster.width),
                    state = grid,
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(horizontal = Marquee.safe.side),
                    horizontalArrangement = Arrangement.spacedBy(metrics.gap),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
                ) {
                    items((1..LANES * TILES).toList()) { PosterTile("T$it", null, {}, caption = "2024") }
                }
            }
        }
        val columns = grid.layoutInfo.maxSpan
        val first = 2 * columns + 1
        val moves =
            still(start = "T$first", presses = columns - 1) {
                grid.firstVisibleItemIndex to
                    grid.firstVisibleItemScrollOffset
            }
        assertTrue(focused("T${first + columns - 1}"), "focus went along the line")
        println("BENCH grid-still columns=$columns ${moves.joinToString(",")}")
        assertEquals(setOf(moves.first()), moves.toSet(), "the grid moved on Right: $moves")
    }

    private fun focused(name: String) =
        compose.onAllNodes(hasContentDescription(name) and isFocused()).fetchSemanticsNodes().isNotEmpty()

    /** Focuses [start], lets the page settle, then presses Right [presses] times, reading [at] after every frame. */
    private fun still(
        start: String,
        presses: Int,
        at: () -> Pair<Int, Int>,
    ): List<Pair<Int, Int>> {
        compose.onNodeWithContentDescription(start).performSemanticsAction(SemanticsActions.RequestFocus)
        compose.mainClock.advanceTimeBy(2_000)
        compose.waitForIdle()
        compose.mainClock.autoAdvance = false
        val seen = mutableListOf(at())
        repeat(presses * FRAMES) { frame ->
            if (frame % FRAMES == 0) compose.onNode(isFocused()).performKeyInput { keyDown(Key.DirectionRight) }
            if (frame % FRAMES == 3) compose.onRoot().performKeyInput { keyUp(Key.DirectionRight) }
            compose.mainClock.advanceTimeByFrame()
            compose.waitForIdle()
            seen += at()
        }
        compose.mainClock.autoAdvance = true
        return seen
    }

    private companion object {
        const val LANES = 8
        const val TILES = 20

        /** The owner's walk: about 15 presses along a row. */
        const val PRESSES = 15

        /** 16 ms frames a press: past the focus tween and the scroll that follows it. */
        const val FRAMES = 40
    }
}
