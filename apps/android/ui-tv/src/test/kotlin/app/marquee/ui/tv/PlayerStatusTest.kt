package app.marquee.ui.tv

import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.core.model.OtherVersion
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.player.PlaybackController
import app.marquee.player.PlayerStatus
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** The failure card on a TV: the core's next version is the first way on, with the focus. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], qualifiers = "w960dp-h540dp")
class PlayerStatusTest {
    @get:Rule
    val compose = createComposeRule()

    private fun show(
        offer: OtherVersion?,
        retry: Boolean = false,
        tryingAgain: Boolean = false,
    ): MutableList<String> {
        val events = mutableListOf<String>()
        val focus = FocusRequester()
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                PlayerStatus(
                    stage = PlaybackController.Stage.Failed,
                    status = null,
                    notice = null,
                    problem = "This version could not be played",
                    attempts = emptyList(),
                    onVersions = { events += "versions" },
                    onBack = { events += "back" },
                    focus = focus,
                    offer = offer,
                    onOffer = { events += "offer:$it" },
                    onRetry = if (retry) ({ events += "retry" }) else null,
                    tryingAgain = tryingAgain,
                )
            }
        }
        compose.runOnIdle { focus.requestFocus() }
        return events
    }

    @Test
    fun `a failed pick offers the next version first, focused, and Choose a version beside it`() {
        val events = show(OtherVersion("k3", "720p BluRay"))
        compose.onNodeWithText("Play 720p BluRay").assertIsFocused()
        compose.onNodeWithText("Play 720p BluRay").performClick()
        compose.onNodeWithText("Choose a version").performClick()
        assertEquals(listOf("offer:k3", "versions"), events)
    }

    @Test
    fun `without an offer Choose a version is the way on`() {
        show(null)
        compose.onAllNodesWithText("Play", substring = true).assertCountEquals(0)
        compose.onAllNodesWithText("Try again").assertCountEquals(0)
        compose.onNodeWithText("Choose a version").assertIsFocused()
    }

    /** The buttons' texts from the left. */
    private fun order() =
        listOf("Play 720p BluRay", "Try again", "Choose a version", "Back")
            .filter { compose.onAllNodesWithText(it).fetchSemanticsNodes().isNotEmpty() }
            .sortedBy {
                compose
                    .onNodeWithText(it)
                    .fetchSemanticsNode()
                    .boundsInRoot.left
            }

    @Test
    fun `a failed play offers Try again right after the offered version, and it sends retry`() {
        val events = show(OtherVersion("k3", "720p BluRay"), retry = true)
        assertEquals(listOf("Play 720p BluRay", "Try again", "Choose a version", "Back"), order())
        compose.onNodeWithText("Play 720p BluRay").assertIsFocused()
        compose.onNodeWithText("Try again").performClick()
        assertEquals(listOf("retry"), events)
    }

    @Test
    fun `a Try again that failed again at once says Trying again on the card`() {
        show(null, retry = true, tryingAgain = true)
        compose.onAllNodesWithText("Trying again").assertCountEquals(1)
        compose.onNodeWithText("Try again").assertIsFocused()
    }

    @Test
    fun `with no offer Try again comes first and takes the focus`() {
        val events = show(null, retry = true)
        assertEquals(listOf("Try again", "Choose a version", "Back"), order())
        compose.onNodeWithText("Try again").assertIsFocused()
        compose.onNodeWithText("Try again").performClick()
        assertEquals(listOf("retry"), events)
    }
}
