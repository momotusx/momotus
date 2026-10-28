package app.marquee.android

import android.app.Application
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import app.marquee.core.model.ProfileInfo
import app.marquee.design.AvatarArt
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.design.Tokens
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The picker's pictures are circles in every state (design/DECISION.md, Profiles and Selected): a focused TV tile lifts
 * a circle, not a rounded square, and the chosen picture is a grey fill behind it, not a white edge.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], application = Application::class, qualifiers = "w1920dp-h1080dp-mdpi")
class ProfileCirclesTest {
    @get:Rule
    val compose = createComposeRule()

    private fun tile(
        form: Form,
        content: @Composable () -> Unit,
    ) {
        compose.setContent {
            MarqueeTheme(form) { Box(Modifier.background(Color.Black).testTag("tile")) { content() } }
        }
    }

    private fun pixels() = compose.onNodeWithTag("tile").captureToImage().toPixelMap()

    /** Under a rounded square's 20 percent corners and outside the circle, near the tile's top left. */
    private fun cornerStaysPage() {
        assertEquals(Color.Black, pixels()[24, 8])
    }

    @Test
    fun `a focused TV profile tile lifts a circle, so the corner stays the page`() {
        val me =
            ProfileInfo("p1", "Mia", "letter:ember", 0, hasPin = false, isOwner = true, syncing = false, kids = false)
        tile(Form.Tv) { ProfileTile(me, active = false, modifier = Modifier, onClick = {}, forceFocus = true) }
        compose.waitForIdle()
        cornerStaysPage()
    }

    @Test
    fun `Add profile is a circle, so the corner stays the page`() {
        tile(Form.Tv) { AddTile(onClick = {}) }
        compose.waitForIdle()
        cornerStaysPage()
    }

    /** A pixel inside the circle, above the plus: the tile's fill. */
    private fun fill() = pixels()[100, 20]

    @Test
    fun `a focused Add profile lifts a circle too, and turns white`() {
        tile(Form.Tv) { AddTile(onClick = {}, forceFocus = true) }
        compose.waitForIdle()
        cornerStaysPage()
        assertTrue(fill().luminance() > 0.7f, "the focused tile is ${fill()}, not white")
    }

    @Test
    fun `an Add profile tile at rest is the surface grey`() {
        tile(Form.Tv) { AddTile(onClick = {}) }
        compose.waitForIdle()
        assertTrue(fill().luminance() < 0.3f, "the tile at rest is ${fill()}")
    }

    @Test
    fun `the chosen picture sits on a grey disc, and an unchosen one on nothing`() {
        tile(Form.Phone) {
            Choice(selected = true, label = "Chosen", size = 48.dp, onClick = {}) { Box(Modifier.size(48.dp)) }
        }
        compose.waitForIdle()
        // The tile is 60 dp: 6 dp of disc around the 48 dp picture. (3, 30) is on the disc, (1, 1) is its corner.
        assertEquals(Tokens.selectedFill, pixels()[3, 30])
        assertEquals(Color.Black, pixels()[1, 1])
    }

    /** The editor's picture on TV, 64 dp in its 76 dp choice; (38, 12) is inside the circle, above the glyph. */
    private fun picture(focused: Boolean) {
        tile(Form.Tv) {
            Choice(selected = false, label = "Popcorn", size = 64.dp, onClick = {}, forceFocus = focused) { lit ->
                AvatarArt("popcorn", "ember", "M", 64.dp, lit = lit)
            }
        }
        compose.waitForIdle()
    }

    @Test
    fun `a focused picture on TV turns white, as a focused profile does`() {
        picture(focused = true)
        val fill = pixels()[38, 12]
        assertTrue(fill.luminance() > 0.7f, "the focused picture is $fill, not white")
    }

    @Test
    fun `a picture at rest on TV keeps its color`() {
        picture(focused = false)
        val fill = pixels()[38, 12]
        assertTrue(fill.luminance() < 0.5f, "the picture at rest is $fill")
    }

    @Test
    fun `an unchosen picture has no disc`() {
        tile(Form.Phone) {
            Choice(selected = false, label = "Other", size = 48.dp, onClick = {}) { Box(Modifier.size(48.dp)) }
        }
        compose.waitForIdle()
        assertEquals(Color.Black, pixels()[3, 30])
    }
}
