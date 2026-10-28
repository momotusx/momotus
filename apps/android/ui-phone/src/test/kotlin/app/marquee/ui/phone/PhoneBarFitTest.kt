package app.marquee.ui.phone

import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.marquee.core.route.Route
import app.marquee.design.Form
import app.marquee.design.Marquee
import app.marquee.design.MarqueeFonts
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertTrue

/**
 * The phone's tab bar on a 360 dp screen at a large text size: all five places fit the capsule, each touch target keeps
 * 48 dp, and a label is shown whole (smaller, down to its floor) or not at all, never cut (design/DECISION.md,
 * Navigation; ui-ux MOB-10). Native graphics: legacy ones measure text as one pixel a letter, which fits any label.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
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
            val tabWidth = tab.right - tab.left
            assertTrue(tabWidth >= 48.dp && tab.bottom - tab.top >= 48.dp, "tab $i is $tab")
            if (shown == 5) {
                val label = labels[i].getBoundsInRoot()
                // A cut label fills all the room it was given; a whole one leaves some.
                val labelWidth = label.right - label.left
                assertTrue(labelWidth < tabWidth - 4.dp, "label $i is $labelWidth in a tab of $tabWidth")
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

    /**
     * The label size the bar settles at is the largest step that fits its widest word, at each text size: a step
     * bigger would not fit, and none shown means not even the floor fits.
     */
    @Test
    fun `the labels settle at the largest step that fits, at every text size`() {
        var scale by mutableStateOf(1.0f)
        var measurer: TextMeasurer? = null
        var base: Density? = null
        var caption = 0f
        compose.setContent {
            val density = LocalDensity.current
            base = density
            measurer = rememberTextMeasurer()
            CompositionLocalProvider(LocalDensity provides Density(density.density, scale)) {
                MarqueeTheme(Form.Phone) {
                    caption = Marquee.type.caption.value
                    PhoneShell(selected = Route.Home, showTabs = true, onSelect = {}) {}
                }
            }
        }
        val words = listOf("Home", "Discover", "Library", "Search", "Settings")
        for (text in listOf(1.0f, 1.15f, 1.3f, 1.4f, 1.5f, 1.7f, 2.0f)) {
            scale = text
            compose.waitForIdle()
            val at = Density(base!!.density, text)
            val tab = tabs[0].getBoundsInRoot()
            val room = with(at) { (tab.right - tab.left - 4.dp).toPx() }
            fun widest(sp: Float) =
                words.maxOf {
                    measurer!!
                        .measure(
                            it,
                            TextStyle(
                                fontSize = sp.sp,
                                fontFamily = MarqueeFonts.text,
                                fontWeight = FontWeight.SemiBold,
                            ),
                            softWrap = false,
                            maxLines = 1,
                            density = at,
                        ).size.width
                }
            val shown = labels.fetchSemanticsNodes()
            if (shown.isEmpty()) {
                assertTrue(widest(10f) > room, "at $text times icons only, but 10 sp fits: ${widest(10f)} in $room")
            } else {
                val size = shown[0].config[LabelSizeKey]
                assertTrue(widest(size) <= room, "at $text times $size sp is cut: ${widest(size)} in $room")
                if (size < caption) assertTrue(widest(size / 0.92f) > room, "at $text times a step above $size sp fits")
            }
        }
    }
}
