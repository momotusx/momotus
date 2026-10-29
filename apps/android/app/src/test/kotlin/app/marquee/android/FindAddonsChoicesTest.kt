package app.marquee.android

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.input.InputMode
import androidx.compose.ui.input.InputModeManager
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.platform.LocalInputModeManager
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.unit.toSize
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** Find addons' list chips (plans/plugins-and-marketplace.md 7.1): past four lists one row that scrolls. */
@OptIn(ExperimentalTestApi::class, ExperimentalComposeUiApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class FindAddonsChoicesTest {
    @get:Rule
    val compose = createComposeRule()

    private fun show(
        form: Form,
        names: List<String>,
        selected: String = names.first(),
        onPick: (String) -> Unit = {},
    ) {
        var picked by mutableStateOf(selected)
        compose.setContent {
            inputModes = LocalInputModeManager.current
            MarqueeTheme(form) {
                Choices("List", names.map { it to it }, picked, null, {}) {
                    picked = it
                    onPick(it)
                }
            }
        }
    }

    private val six =
        listOf("Official 5", "Community 96", "stremio-addons.net 703", "Directory 40", "Another list 12", "Last one 3")

    private fun oneRow(form: Form) {
        show(form, six)
        val tops = six.map { compose.onNodeWithText(it).getUnclippedBoundsInRoot().top }
        assertEquals(1, tops.toSet().size, "$form: $tops")
    }

    @Test
    fun `six lists stay on one row on a phone`() = oneRow(Form.Phone)

    @Test
    fun `six lists stay on one row on a TV`() = oneRow(Form.Tv)

    @Test
    fun `a chip on the scrolled row still picks its list`() {
        val names = listOf("A", "B", "C", "D", "E", "F")
        var got = ""
        show(Form.Phone, names, onPick = { got = it })
        compose.onNodeWithText("F").performClick()
        compose.runOnIdle { assertEquals("F", got) }
    }

    private var inputModes: InputModeManager? = null

    /** As after a remote's key press: keyboard input mode, where chips take focus as on a TV. */
    private fun keyboard() {
        compose.runOnIdle { inputModes!!.requestInputMode(InputMode.Keyboard) }
    }

    /** Lists whose chips run past the 1920 dp TV canvas. */
    private val long = (1..6).map { "A list with a long name, number $it" }

    /**
     * TV: the last chip of a row that scrolls takes focus whole on screen, and Right there goes nowhere. In a 16:9
     * window, measured in pixels, unclipped: a chip's dp are the TV canvas's, the root's the window's.
     */
    @Test
    @Config(qualifiers = "w960dp-h540dp")
    fun `on a TV the last chip comes into view with focus and the row ends there`() {
        show(Form.Tv, long)
        keyboard()
        val range = compose.onNode(SemanticsMatcher.keyIsDefined(SemanticsProperties.HorizontalScrollAxisRange))
        assertTrue(range.fetchSemanticsNode().config[SemanticsProperties.HorizontalScrollAxisRange].maxValue() > 0f)
        val first = compose.onNodeWithText(long.first())
        first.requestFocus()
        // As a remote does: Right to the last chip, one press at a time.
        repeat(long.size - 1) {
            compose.onRoot().performKeyInput { pressKey(Key.DirectionRight) }
            compose.waitForIdle()
        }
        val last = compose.onNodeWithText(long.last())
        last.assertIsFocused()
        val root = compose.onRoot().fetchSemanticsNode().boundsInRoot
        val bounds = last.fetchSemanticsNode().let { Rect(it.positionInRoot, it.size.toSize()) }
        assertTrue(bounds.left >= root.left && bounds.right <= root.right, "$bounds in $root")
        compose.onRoot().performKeyInput { pressKey(Key.DirectionRight) }
        compose.waitForIdle()
        last.assertIsFocused()
    }

    @Test
    fun `on a TV Left on the first chip stays on it`() {
        show(Form.Tv, six)
        keyboard()
        val first = compose.onNodeWithText(six.first())
        first.requestFocus()
        first.performKeyInput { pressKey(Key.DirectionLeft) }
        compose.waitForIdle()
        first.assertIsFocused()
    }
}
