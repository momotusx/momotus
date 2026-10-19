package app.marquee.ui.tv

import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.TrackChoice
import app.marquee.player.TrackPanel
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** The subtitles panel on a TV: a heading over each language, which the D-pad skips (plans/m13-subtitle-languages.md 4). */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class TrackPanelTest {
    @get:Rule
    val compose = createComposeRule()

    private val rows =
        listOf(
            TrackChoice("off", "Off"),
            TrackChoice("late:tur", "Breaking.Bad.S01E01", "SubSense", section = "Turkish"),
            TrackChoice("late:eng1", "OpenSubtitles v3 1", section = "English", selected = true),
            TrackChoice("late:eng2", "OpenSubtitles v3 2", section = "English"),
            TrackChoice("0:0", "Spanish", "SRT", section = "In the file"),
        )

    @Test
    fun `each language has one heading, and Down goes from row to row past them`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TrackPanel("Subtitles", rows, onPick = {}) } }
        compose.waitForIdle()

        fun heading(text: String) =
            compose
                .onNodeWithText(text)
                .assert(SemanticsMatcher.keyNotDefined(SemanticsActions.OnClick))
                .assert(SemanticsMatcher.keyNotDefined(SemanticsActions.RequestFocus))
        heading("Turkish")
        heading("English")
        val turkish = compose.onNodeWithContentDescription("Breaking.Bad.S01E01, SubSense")
        turkish.requestFocus()
        turkish.performKeyInput { pressKey(Key.DirectionDown) }
        // Past the English heading to its first track.
        compose.onNodeWithContentDescription("OpenSubtitles v3 1").assertIsFocused()
        compose.onNodeWithContentDescription("OpenSubtitles v3 1").performKeyInput { pressKey(Key.DirectionDown) }
        compose.onNodeWithContentDescription("OpenSubtitles v3 2").performKeyInput { pressKey(Key.DirectionDown) }
        compose.onNodeWithContentDescription("Spanish, SRT").assertIsFocused()
        // The list scrolled to it, and its heading came with it.
        heading("In the file")
    }
}
