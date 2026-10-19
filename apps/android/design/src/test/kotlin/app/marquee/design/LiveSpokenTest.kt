package app.marquee.design

import android.os.Looper
import android.text.Editable
import android.text.TextWatcher
import android.view.View
import android.view.ViewGroup
import android.widget.TextView
import androidx.activity.ComponentActivity
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import java.time.Duration
import kotlin.test.assertEquals

/** The words TalkBack reads from a live line: always the newest, also when they change as the line appears. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class LiveSpokenTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun liveRegion(view: View): TextView? =
        when {
            view is TextView && view.accessibilityLiveRegion != View.ACCESSIBILITY_LIVE_REGION_NONE -> view
            view is ViewGroup -> (0 until view.childCount).firstNotNullOfOrNull { liveRegion(view.getChildAt(it)) }
            else -> null
        }

    @Test
    fun `words that change while the line appears end as the newest`() {
        var said by mutableStateOf("Adding the addon")
        compose.setContent { MarqueeTheme(Form.Phone) { LiveSpoken(said) { MqText(said) } } }
        compose.waitForIdle()
        val view = liveRegion(compose.activity.window.decorView)!!
        val written = mutableListOf<String>()
        view.addTextChangedListener(
            object : TextWatcher {
                override fun beforeTextChanged(
                    s: CharSequence?,
                    start: Int,
                    count: Int,
                    after: Int,
                ) = Unit

                override fun onTextChanged(
                    s: CharSequence?,
                    start: Int,
                    before: Int,
                    count: Int,
                ) = Unit

                override fun afterTextChanged(s: Editable?) {
                    written += s.toString()
                }
            },
        )
        // The core refused the link at once: the warning replaces the line within the first moment.
        said = "This link carries a sign-in in its settings."
        compose.waitForIdle()
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(600))
        compose.waitForIdle()
        assertEquals("This link carries a sign-in in its settings.", view.text.toString())
        // Written once: TalkBack drops a live region that changes again right after it spoke.
        assertEquals(listOf("This link carries a sign-in in its settings."), written)

        // A line already there changes twice at once (the next add, refused at once): written once again.
        written.clear()
        said = "Adding the addon"
        compose.waitForIdle()
        said = "This link carries a sign-in in its settings, maybe someone else's."
        compose.waitForIdle()
        shadowOf(Looper.getMainLooper()).idleFor(Duration.ofMillis(600))
        compose.waitForIdle()
        assertEquals(listOf("This link carries a sign-in in its settings, maybe someone else's."), written)
    }
}
