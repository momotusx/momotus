package app.marquee.android

import android.app.Application
import android.os.Looper
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.activity.ComponentActivity
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.time.Duration
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** A note is read out where a screen asks for it, and by default is not: a hint or a row in a list stays quiet. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class NoteSpokenTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun liveRegion(view: View): TextView? =
        when {
            view is TextView && view.accessibilityLiveRegion != View.ACCESSIBILITY_LIVE_REGION_NONE -> view
            view is ViewGroup -> (0 until view.childCount).firstNotNullOfOrNull { liveRegion(view.getChildAt(it)) }
            else -> null
        }

    private fun spoken(on: Boolean): String? {
        compose.setContent { MarqueeTheme(Form.Phone) { Note("Nothing of this type in your library.", spoken = on) } }
        compose.waitForIdle()
        val view = liveRegion(compose.activity.window.decorView) ?: return null
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(2_000))
        return view.text.toString()
    }

    @Test
    fun `where it is set the words are read out once`() {
        assertEquals("Nothing of this type in your library.", spoken(on = true))
    }

    @Test
    fun `by default there is no live region`() {
        assertNull(spoken(on = false))
    }
}
