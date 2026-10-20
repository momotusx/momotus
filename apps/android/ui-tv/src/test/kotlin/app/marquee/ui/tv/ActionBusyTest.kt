package app.marquee.ui.tv

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onAllNodesWithTag
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.MarqueeTheme
import app.marquee.player.ActionButton
import app.marquee.player.BUSY_TAG
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/** Subtitles with tracks in while the core still lists: the count, a small turning mark, and it is read out. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class ActionBusyTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `a busy action shows the mark and says so, and the mark goes when it is not`() {
        var busy by mutableStateOf(true)
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                ActionButton(Icons.Captions, "Subtitles", "Off, 87 found", {}, busy = busy)
            }
        }
        compose.onAllNodesWithTag(BUSY_TAG, useUnmergedTree = true).assertCountEquals(1)
        compose.onAllNodesWithContentDescription("Subtitles, Off, 87 found, still searching").assertCountEquals(1)
        busy = false
        compose.onAllNodesWithTag(BUSY_TAG, useUnmergedTree = true).assertCountEquals(0)
        compose.onAllNodesWithContentDescription("Subtitles, Off, 87 found").assertCountEquals(1)
    }
}
