package app.marquee.android

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals

/** Find addons' list chips (plans/plugins-and-marketplace.md 7.1): past four lists one row that scrolls. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class FindAddonsChoicesTest {
    @get:Rule
    val compose = createComposeRule()

    private fun show(
        form: Form,
        names: List<String>,
        onPick: (String) -> Unit = {},
    ) {
        var picked by mutableStateOf(names.first())
        compose.setContent {
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
        show(Form.Phone, names) { got = it }
        compose.onNodeWithText("F").performClick()
        compose.runOnIdle { assertEquals("F", got) }
    }
}
