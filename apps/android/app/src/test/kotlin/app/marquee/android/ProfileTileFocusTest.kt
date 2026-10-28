package app.marquee.android

import android.app.Application
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.assertIsNotFocused
import androidx.compose.ui.test.hasClickAction
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.requestFocus
import app.marquee.core.model.ProfileInfo
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * The picker's tiles take real focus on TV, the path that drives their lifted look ([ProfileCirclesTest] draws the
 * look itself with the forced flag, since native graphics, which a capture needs, do not grant focus requests).
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class, qualifiers = "w1920dp-h1080dp-mdpi")
class ProfileTileFocusTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `a profile tile takes focus`() {
        val me =
            ProfileInfo("p1", "Mia", "letter:ember", 0, hasPin = false, isOwner = true, syncing = false, kids = false)
        compose.setContent {
            MarqueeTheme(Form.Tv) { ProfileTile(me, active = false, modifier = Modifier, onClick = {}) }
        }
        compose.onNode(hasClickAction()).assertIsNotFocused().requestFocus()
        compose.onNode(hasClickAction()).assertIsFocused()
    }

    @Test
    fun `the Add profile tile takes focus`() {
        compose.setContent { MarqueeTheme(Form.Tv) { AddTile(onClick = {}) } }
        compose.onNode(hasClickAction()).assertIsNotFocused().requestFocus()
        compose.onNode(hasClickAction()).assertIsFocused()
    }
}
