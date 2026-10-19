package app.marquee.design

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

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
}
