package app.marquee.ui.tv

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.hasScrollToKeyAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performScrollToKey
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

    @Test
    fun `the Timing row moves this play's subtitles, and Down from the last track reaches it`() {
        val shifts = mutableListOf<Long>()
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                var delay by remember { mutableLongStateOf(0L) }
                TrackPanel(
                    "Subtitles",
                    rows,
                    onPick = {},
                    timing = delay,
                    onShift = {
                        shifts += it
                        delay += it
                    },
                )
            }
        }
        // The row sits under the list, past the panel's height here: scroll it into view to read it.
        compose.onNode(hasScrollToKeyAction()).performScrollToKey("timing")
        compose.onNodeWithText("On time").assertExists()
        val last = compose.onNodeWithContentDescription("Spanish, SRT")
        last.requestFocus()
        last.performKeyInput { pressKey(Key.DirectionDown) }
        compose.onNode(isFocused() and (hasText("Earlier") or hasText("Later"))).assertExists()
        compose.onNodeWithText("Later").performClick()
        compose.onNodeWithText("0.5 s later").assertExists()
        compose.runOnIdle { kotlin.test.assertEquals(listOf(500L), shifts) }
    }
}
