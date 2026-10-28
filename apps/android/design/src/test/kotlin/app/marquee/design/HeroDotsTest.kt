package app.marquee.design

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.assertHeightIsAtLeast
import androidx.compose.ui.test.assertWidthIsAtLeast
import androidx.compose.ui.test.click
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performTouchInput
import androidx.compose.ui.unit.dp
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** The phone hero's page dots: one control of at least 48 by 48 dp (Material), as iOS's page control. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class HeroDotsTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `a tap goes to the dot nearest it, the dots in the row's middle`() {
        // Phone: 7 dp dots, the active one a 24 dp pill, 9 dp apart: middles at 12, 36.5 and 52.5.
        assertEquals(0, nearestDot(0f, 56f, 3, 0, 7f, 24f, 9f))
        assertEquals(1, nearestDot(30f, 56f, 3, 0, 7f, 24f, 9f))
        assertEquals(2, nearestDot(60f, 56f, 3, 0, 7f, 24f, 9f))
        // Two dots (40 dp) in a 48 dp row start 4 dp in.
        assertEquals(1, nearestDot(36f, 48f, 2, 0, 7f, 24f, 9f))
        assertEquals(0, nearestDot(15f, 48f, 2, 0, 7f, 24f, 9f))
    }

    @Test
    fun `the dots are one 48 dp control with the page shown and Previous and Next as its actions`() {
        var active by mutableIntStateOf(0)
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                HeroDots(2, active, { 0f }, label = { "Page ${it + 1} of 2" }, onPick = { active = it })
            }
        }
        // One node, not a 16 dp wide target per dot.
        assertEquals(1, compose.onAllNodesWithContentDescription("Page", substring = true).fetchSemanticsNodes().size)
        val dots = compose.onNodeWithContentDescription("Page 1 of 2")
        dots.assertWidthIsAtLeast(48.dp).assertHeightIsAtLeast(48.dp)
        val actions = dots.fetchSemanticsNode().config[SemanticsActions.CustomActions]
        assertEquals(listOf("Next page"), actions.map { it.label })
        actions.single().action()
        compose.waitForIdle()
        assertEquals(1, active)
        val back = compose.onNodeWithContentDescription("Page 2 of 2")
        val backActions = back.fetchSemanticsNode().config[SemanticsActions.CustomActions]
        assertEquals(listOf("Previous page"), backActions.map { it.label })
        // A tap at the control's left edge, off the dots themselves, goes to the first page.
        back.performTouchInput { click(centerLeft) }
        compose.waitForIdle()
        assertEquals(0, active)
    }
}
