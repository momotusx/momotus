package app.marquee.ui.phone

import androidx.activity.ComponentActivity
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.isRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import app.marquee.core.model.NextEpisode
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.PlaybackController
import app.marquee.player.SHIFT_TAG
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The phone's bar fits its width (plans/player-bar.md D17, acceptance 1): one row on the Pixel 9 Pro in landscape and
 * at 915 by 412 dp, two rows at 640 by 360, with a series (Episodes and Next), the shift on, every target 48 dp and the
 * scrubber at least 120 dp; turning the subtitle off and on moves no row. Native graphics, so text measures as drawn.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36])
class PlayerBarFitTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val play = BarPlay()
    private val controller get() = play.controller

    @After
    fun stop() = play.close()

    private fun playing() {
        compose.setContent {
            MarqueeTheme(Form.Phone) { PhonePlayer(controller, false, false, {}, {}, { _, _ -> }, {}, {}) }
        }
        play.attach { _, check -> compose.waitUntil(10_000) { check() } }
        compose.runOnIdle { play.player.open() }
        compose.waitUntil(10_000) { controller.stage == PlaybackController.Stage.Playing }
        // Paused, so the controls stay; Next after the core's snapshot, which names none.
        compose.runOnIdle {
            controller.next = NextEpisode("tt1:1:2", "The Second", 1, 2)
            controller.pause()
            play.english()
        }
        compose.waitUntil(10_000) { compose.onAllNodes(hasTestTag(SHIFT_TAG)).fetchSemanticsNodes().size == 1 }
        compose.waitForIdle()
    }

    private fun bar() = compose.onNodeWithTag(BAR_TAG).fetchSemanticsNode().boundsInRoot

    /** The bar's rows, its targets each 48 dp both ways, and the scrubber's width. */
    private fun measure(least: Int = 8): Int {
        val density =
            compose
                .onAllNodes(isRoot())
                .fetchSemanticsNodes()
                .first()
                .layoutInfo.density.density
        val bar = bar()
        val targets =
            compose
                .onAllNodes(SemanticsMatcher.keyIsDefined(SemanticsActions.OnClick))
                .fetchSemanticsNodes()
                .map { it.boundsInRoot }
                .filter { it.top >= bar.top - 1f && it.bottom <= bar.bottom + 1f }
        // Speed, Versions, Subtitles, Earlier, the value, Later, Episodes, Next (Audio once the file lists tracks); five
        // with the subtitle off.
        assertTrue(targets.size >= least, "the bar's targets: ${targets.size}")
        for (t in targets) {
            assertTrue(t.width / density >= 47.5f && t.height / density >= 47.5f, "a target under 48 dp: $t")
        }
        val scrub = compose.onNodeWithContentDescription("Position").fetchSemanticsNode().boundsInRoot
        assertTrue(scrub.width / density >= 120f, "the scrubber is ${scrub.width / density} dp")
        val subtitles =
            compose.onNodeWithContentDescription("Subtitles, ", substring = true).fetchSemanticsNode().boundsInRoot
        return if (subtitles.top >= scrub.bottom - 1f) 2 else 1
    }

    private fun rowsWithTheShiftOffAndOn(): Int {
        playing()
        val rows = measure()
        val height = bar().height
        compose.runOnIdle { controller.selectText("off") }
        compose.waitUntil(5_000) { compose.onAllNodes(hasTestTag(SHIFT_TAG)).fetchSemanticsNodes().isEmpty() }
        assertEquals(rows, measure(least = 5), "the row count moved with the subtitle")
        assertEquals(height, bar().height, 0.5f, "the bar's height moved with the subtitle")
        return rows
    }

    @Test
    @Config(qualifiers = "w891dp-h411dp-mdpi")
    fun `one row on the Pixel 9 Pro in landscape`() = assertEquals(1, rowsWithTheShiftOffAndOn())

    @Test
    @Config(qualifiers = "w915dp-h412dp-mdpi")
    fun `one row at 915 by 412 dp`() = assertEquals(1, rowsWithTheShiftOffAndOn())

    @Test
    @Config(qualifiers = "w640dp-h360dp-mdpi")
    fun `two rows at 640 by 360 dp`() = assertEquals(2, rowsWithTheShiftOffAndOn())

    @Test
    @Config(qualifiers = "w891dp-h411dp-mdpi")
    fun `the bar is at most 72 dp tall in one row`() {
        playing()
        val density =
            compose
                .onAllNodes(isRoot())
                .fetchSemanticsNodes()
                .first()
                .layoutInfo.density.density
        assertTrue(bar().height / density <= 72f, "the bar is ${bar().height / density} dp")
    }
}
