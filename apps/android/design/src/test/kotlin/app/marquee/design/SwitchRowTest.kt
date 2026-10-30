package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals

/** A switch row (the recommended addons, Settings' addon list) as TalkBack reads it: one switch, named by its words. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class SwitchRowTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `the whole row is one switch with its state, and a press flips it`() {
        var on by mutableStateOf(true)
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                SwitchRow(on, { on = !on }) {
                    MqText("Torrentio")
                    MqText("Finds torrent streams.")
                }
            }
        }
        // The row's words merge into the one node that carries the switch role and the state.
        val row = compose.onNodeWithText("Torrentio", useUnmergedTree = false)
        row.assert(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Switch))
        row.assertIsOn()
        row.performClick()
        row.assertIsOff()
        compose.onNodeWithText("Finds torrent streams.").assertIsOff()
    }

    /**
     * Pixel walk 2026-10-30 F2: an addon's name line, rounded to its card's corners, lost the top of Torrentio's T and
     * the r of raw.githubusercontent.com. The rounded row's words draw exactly as they do with no row around them, and
     * keep the card's column.
     */
    @Test
    @Config(qualifiers = "w360dp-h800dp-mdpi")
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `a rounded row cuts none of its words and keeps them in the card's column`() {
        val words = listOf("Torrentio", "raw.githubusercontent.com")
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                val card = Modifier.fillMaxWidth().background(Marquee.colors.surface).padding(16.dp)
                Column {
                    Column(card) {
                        SwitchRow(true, {}, shape = RoundedCornerShape(Marquee.radius.m)) {
                            Column(Modifier.weight(1f)) {
                                MqText(words[0], Modifier.testTag("row-0"), TextRole.Headline, weight = FontWeight.Bold)
                                MqText(words[1], Modifier.testTag("row-1"), TextRole.Caption, Marquee.colors.dim)
                            }
                        }
                    }
                    Column(card) {
                        MqText(words[0], Modifier.testTag("alone-0"), TextRole.Headline, weight = FontWeight.Bold)
                        MqText(words[1], Modifier.testTag("alone-1"), TextRole.Caption, Marquee.colors.dim)
                    }
                }
            }
        }
        for (i in words.indices) {
            val inRow = compose.onNodeWithTag("row-$i", useUnmergedTree = true)
            val alone = compose.onNodeWithTag("alone-$i", useUnmergedTree = true)
            assertEquals(
                alone.fetchSemanticsNode().boundsInRoot.left,
                inRow.fetchSemanticsNode().boundsInRoot.left,
                "${words[i]} moved off the card's column",
            )
            val a = alone.captureToImage().toPixelMap()
            val r = inRow.captureToImage().toPixelMap()
            val cut =
                (0 until minOf(a.width, r.width)).sumOf { x ->
                    (0 until minOf(a.height, r.height)).count { y -> a[x, y] != r[x, y] }
                }
            assertEquals(0, cut, "${words[i]}: $cut pixels differ from the words drawn alone")
        }
    }
}
