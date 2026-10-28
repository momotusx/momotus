package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.dp
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertNotEquals

/** A profile's avatar is a circle (design/DECISION.md, Profiles), not a rounded square. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [36], qualifiers = "mdpi")
class AvatarShapeTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `the corner a rounded square would fill stays the page`() {
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                Box(Modifier.size(100.dp).background(Color.Black).testTag("avatar")) {
                    AvatarArt("letter", "amber", "A", 100.dp)
                }
            }
        }
        val pixels = compose.onNodeWithTag("avatar").captureToImage().toPixelMap()
        // (10, 10) is inside a square with 20 percent corners and outside the circle through its sides.
        assertEquals(Color.Black, pixels[10, 10])
        assertNotEquals(Color.Black, pixels[50, 20])
    }
}
