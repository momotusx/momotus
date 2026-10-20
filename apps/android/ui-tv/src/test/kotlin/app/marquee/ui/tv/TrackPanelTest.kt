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
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.hasScrollToIndexAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performScrollToIndex
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
        // The panel opens on the chosen English track: back to the top of the list.
        compose.onNode(hasScrollToIndexAction()).performScrollToIndex(0)

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
    fun `the Timing row sits over the tracks, Up from the first track reaches it, and it moves the subtitles`() {
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
        // Under the title, outside the list: there without scrolling, however long the list.
        compose.onNodeWithText("On time").assertExists()
        compose.onNode(hasScrollToIndexAction()).performScrollToIndex(0)
        val first = compose.onNodeWithContentDescription("Off")
        first.requestFocus()
        first.performKeyInput { pressKey(Key.DirectionUp) }
        compose.onNode(isFocused() and (hasText("Earlier") or hasText("Later"))).assertExists()
        compose.onNodeWithText("Later").performClick()
        compose.onNodeWithText("0.5 s later").assertExists()
        compose.runOnIdle { kotlin.test.assertEquals(listOf(500L), shifts) }
    }

    @Test
    fun `with no subtitle on there is no Timing row`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TrackPanel("Subtitles", rows, onPick = {}, timing = null) } }
        compose.onAllNodesWithText("Timing").assertCountEquals(0)
    }

    /** Off, then one track under each of twelve languages, then the file's own track, chosen (always last). */
    private val many =
        listOf(TrackChoice("off", "Off")) +
            listOf(
                "Arabic",
                "Bulgarian",
                "Chinese",
                "Croatian",
                "Czech",
                "Danish",
                "Dutch",
                "English",
                "Finnish",
                "French",
                "German",
                "Greek",
            ).map { TrackChoice("late:$it", "OpenSubtitles v3 $it", section = it) } +
            TrackChoice("0:0", "English", "SRT", section = "In the file", selected = true)

    @Test
    fun `with many language headings over the chosen track, the panel opens on it with the focus`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TrackPanel("Subtitles", many, onPick = {}) } }
        compose.waitForIdle()
        compose.onNodeWithContentDescription("English, SRT").assertIsFocused()
    }

    @Test
    fun `Left from any track goes to the Timing row`() {
        compose.setContent { MarqueeTheme(Form.Tv) { TrackPanel("Subtitles", many, onPick = {}, timing = 0L) } }
        compose.waitForIdle()
        val chosen = compose.onNodeWithContentDescription("English, SRT")
        chosen.requestFocus()
        chosen.performKeyInput { pressKey(Key.DirectionLeft) }
        compose.onNode(isFocused() and hasText("Earlier")).assertExists()
    }
}
