package app.marquee.ui.phone

import androidx.compose.runtime.remember
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.test.junit4.v2.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.ZapCopy
import app.marquee.core.model.Dial
import app.marquee.core.model.OtherVersion
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.EndCard
import app.marquee.player.PlaybackController
import app.marquee.player.PlayerStatus
import app.marquee.player.TrackChoice
import app.marquee.player.ZapControls
import app.marquee.player.ZapPanel
import app.marquee.player.shownRows
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertNull

/**
 * A zap play's parts (plans/zap.md 2.3): the failure card after it played, the end card, What is this? and the
 * subtitle rows, none of which names the title or offers another version until asked.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w411dp-h891dp")
class ZapPlayerPartsTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `a zap's failure card offers Zap again, Try again and Back only`() {
        var zaps = 0
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                PlayerStatus(
                    stage = PlaybackController.Stage.Failed,
                    status = null,
                    notice = null,
                    problem = "Playback stopped",
                    attempts = emptyList(),
                    onVersions = {},
                    onBack = {},
                    focus = remember { FocusRequester() },
                    offer = OtherVersion("k2", "1080p WEB"),
                    onRetry = {},
                    zapAgain = { zaps++ },
                )
            }
        }
        compose.onNodeWithText(ZapCopy.AGAIN).performClick()
        assertEquals(1, zaps)
        compose.onNodeWithText(StreamChoiceCopy.EMPTY_TRY_AGAIN).assertExists()
        compose.onNodeWithText(StreamChoiceCopy.BACK).assertExists()
        compose.onNodeWithText(StreamChoiceCopy.CHOOSE_ANOTHER).assertDoesNotExist()
        compose.onNodeWithText("Play 1080p WEB instead").assertDoesNotExist()
    }

    @Test
    fun `while the name is hidden the subtitle rows carry no release text`() {
        val rows = listOf(TrackChoice("u1", "English", release = "Severance.S02E05.1080p.WEB"))
        assertNull(shownRows(rows, revealed = false).single().release)
        assertEquals("Severance.S02E05.1080p.WEB", shownRows(rows, revealed = true).single().release)
    }

    @Test
    fun `a zap's end card offers Zap again and Back, never Play again`() {
        var zaps = 0
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                EndCard("Zap", null, {}, {}, {}, remember { FocusRequester() }, zapAgain = { zaps++ })
            }
        }
        compose.onNodeWithText(ZapCopy.AGAIN).performClick()
        assertEquals(1, zaps)
        compose.onNodeWithText("Back").assertExists()
        compose.onNodeWithText("Play again").assertDoesNotExist()
        compose.onNodeWithText("Back to Zap").assertDoesNotExist()
    }

    @Test
    fun `What is this names the title and its episode, then starts it over or opens its page`() {
        val pressed = mutableListOf<String>()
        val zap =
            ZapControls(
                name = "Severance",
                line = "S2:E5 · Trojan's Horse",
                dial = Dial.Near,
                onAgain = { pressed += "again" },
                onDial = { pressed += "dial" },
                onFromStart = { pressed += "start" },
                onOpenTitle = { pressed += "title" },
            )
        compose.setContent { MarqueeTheme(Form.Phone) { ZapPanel(zap, onClose = { pressed += "close" }) } }
        compose.onNodeWithText("Severance").assertExists()
        compose.onNodeWithText("S2:E5 · Trojan's Horse").assertExists()
        compose.onNodeWithText(ZapCopy.FROM_START).performClick()
        compose.onNodeWithText(ZapCopy.OPEN_TITLE).performClick()
        assertEquals(listOf("start", "title"), pressed)
    }
}
