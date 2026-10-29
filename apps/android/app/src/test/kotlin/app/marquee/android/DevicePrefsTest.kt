package app.marquee.android

import android.app.Application
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsOff
import androidx.compose.ui.test.assertIsOn
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.design.TrailerPreviews
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** Settings > This device: Trailer previews is a switch with its state, not a lone "On" chip. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class DevicePrefsTest {
    @get:Rule
    val compose = createComposeRule()

    @Test
    fun `Trailer previews is one switch that says its state and keeps it`() {
        val context = RuntimeEnvironment.getApplication()
        val was = TrailerPreviews.on(context)
        compose.setContent { MarqueeTheme(Form.Phone) { DevicePrefs() } }
        val row = compose.onNodeWithText("Trailer previews")
        row.assert(SemanticsMatcher.expectValue(SemanticsProperties.Role, Role.Switch))
        if (was) row.assertIsOn() else row.assertIsOff()
        row.performClick()
        if (was) row.assertIsOff() else row.assertIsOn()
        assertEquals(!was, TrailerPreviews.on(context))
    }
}
