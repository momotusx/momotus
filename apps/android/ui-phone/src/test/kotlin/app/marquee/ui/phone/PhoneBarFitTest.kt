package app.marquee.ui.phone

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import app.marquee.core.route.Route
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertTrue

/**
 * The phone's tab bar on a 360 dp screen at a large text size: all five places fit the capsule, each touch target keeps
 * 48 dp, and a label is shown whole (smaller, down to its floor) or not at all, never cut (design/DECISION.md,
 * Navigation; ui-ux MOB-10).
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w360dp-h640dp-mdpi")
class PhoneBarFitTest {
    @get:Rule
    val compose = createComposeRule()

    private fun show(fontScale: Float) {
        compose.setContent {
            val density = LocalDensity.current
            CompositionLocalProvider(LocalDensity provides Density(density.density, fontScale)) {
                MarqueeTheme(Form.Phone) { PhoneShell(selected = Route.Home, showTabs = true, onSelect = {}) {} }
            }
        }
        compose.waitForIdle()
    }

    private val tabs get() = compose.onAllNodes(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Tab))

    private val labels get() = compose.onAllNodes(hasTestTag(TAB_LABEL_TAG), useUnmergedTree = true)

    /** Five tabs inside the capsule, 48 dp each way, and every label shown whole inside its own tab. */
    private fun assertFits(labelsShown: Boolean?) {
        val root = compose.onRoot().getBoundsInRoot()
        tabs.assertCountEquals(5)
        val shown = labels.fetchSemanticsNodes().size
        if (labelsShown != null) assertTrue(shown == if (labelsShown) 5 else 0, "$shown of 5 labels are shown")
        assertTrue(shown == 0 || shown == 5, "$shown of 5 labels are shown: all or none")
        for (i in 0 until 5) {
            val tab = tabs[i].getBoundsInRoot()
            assertTrue(tab.left >= 21.dp - 0.5.dp && tab.right <= root.right - 21.dp + 0.5.dp, "tab $i is at $tab")
            assertTrue(tab.width >= 48.dp && tab.height >= 48.dp, "tab $i is $tab")
            if (shown == 5) {
                val label = labels[i].getBoundsInRoot()
                // A cut label fills all the room it was given; a whole one leaves some.
                assertTrue(label.width < tab.width - 4.dp, "label $i is ${label.width} in a tab of ${tab.width}")
                assertTrue(label.left >= tab.left && label.right <= tab.right, "label $i is at $label, tab $tab")
            }
        }
    }

    @Test
    fun `at 1_3 times the text size every place shows its whole label`() {
        show(fontScale = 1.3f)
        assertFits(labelsShown = true)
    }

    @Test
    fun `at 1_0 times the text size every place shows its whole label`() {
        show(fontScale = 1.0f)
        assertFits(labelsShown = true)
    }

    @Test
    fun `at twice the text size no label is cut, and the places keep their size`() {
        show(fontScale = 2.0f)
        assertFits(labelsShown = null)
    }
}
