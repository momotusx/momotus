package app.marquee.android

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import app.marquee.core.model.LanguageChoice
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** The subtitle and audio language pickers search every language the core knows (journey reviews 2026-10-20). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class LanguagePickerTest {
    @get:Rule
    val compose = createComposeRule()

    private val all =
        listOf(
            LanguageChoice("en", "English"),
            LanguageChoice("pl", "Polish"),
            LanguageChoice("pt-BR", "Portuguese (Brazil)"),
            LanguageChoice("he", "Hebrew"),
        )

    @Test
    @Config(qualifiers = "iw-rIL")
    fun `a Hebrew phone offers Hebrew first and saves the core's code, not Java's old one`() {
        var chosen by mutableStateOf(emptyList<String>())
        compose.setContent { MarqueeTheme(Form.Phone) { Languages(chosen, { all }, { chosen = it }) } }
        compose.onNodeWithText("Hebrew").performClick()
        compose.runOnIdle { assertEquals(listOf("he"), chosen) }
    }

    @Test
    fun `a language outside the old eight is found by name and added after the chosen ones`() {
        var chosen by mutableStateOf(listOf("en"))
        compose.setContent { MarqueeTheme(Form.Phone) { Languages(chosen, { all }, { chosen = it }) } }
        compose.onNodeWithContentDescription("Find a language").performTextInput("pol")
        compose.onNodeWithText("Polish").performClick()
        compose.runOnIdle { assertEquals(listOf("en", "pl"), chosen) }
        // A chosen one is a press away from off.
        compose.onNodeWithText("English").performClick()
        compose.runOnIdle { assertEquals(listOf("pl"), chosen) }
    }

    @Test
    fun `audio adds the base code`() {
        var chosen by mutableStateOf(emptyList<String>())
        compose.setContent { MarqueeTheme(Form.Phone) { Languages(chosen, { all }, { chosen = it }, audio = true) } }
        compose.onNodeWithContentDescription("Find a language").performTextInput("portu")
        compose.onNodeWithText("Portuguese").performClick()
        compose.runOnIdle { assertEquals(listOf("pt"), chosen) }
    }

    @Test
    fun `on a TV the focus goes with the picked language to the chosen ones`() {
        var chosen by mutableStateOf(listOf("en"))
        compose.setContent { MarqueeTheme(Form.Tv) { Languages(chosen, { all }, { chosen = it }) } }
        // On a TV the field is a button until selected: select opens it for typing.
        compose.onNodeWithContentDescription("Find a language").performClick()
        compose.onNodeWithContentDescription("Find a language").performTextInput("pol")
        compose.onNodeWithText("Polish").performClick()
        compose.waitUntil(5_000) {
            compose.onAllNodes(hasText("Polish") and isFocused()).fetchSemanticsNodes().isNotEmpty()
        }
        compose.runOnIdle { assertEquals(listOf("en", "pl"), chosen) }
    }
}
