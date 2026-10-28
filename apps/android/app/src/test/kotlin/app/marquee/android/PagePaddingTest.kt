package app.marquee.android

import android.app.Application
import androidx.activity.ComponentActivity
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.getBoundsInRoot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.core.graphics.Insets
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import app.marquee.core.route.Route
import app.marquee.design.Form
import app.marquee.design.Marquee
import app.marquee.design.MarqueeTheme
import app.marquee.ui.phone.PhoneShell
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** A page under the phone's back link pads for the status bar once, not once for the bar and once for the link. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class PagePaddingTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private fun firstLineTop(
        link: Boolean,
        statusPx: Int,
    ): Pair<Dp, Dp> {
        var gap = 0.dp
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                gap = Marquee.space.m
                PhoneShell(Route.Home, showTabs = false, onSelect = {}, onBack = if (link) ({}) else null) {
                    Box(Modifier.fillMaxSize().padding(pagePadding(framed = false))) {
                        Box(Modifier.size(10.dp).testTag("first"))
                    }
                }
            }
        }
        compose.runOnIdle {
            ViewCompat.dispatchApplyWindowInsets(
                compose.activity.window.decorView,
                WindowInsetsCompat
                    .Builder()
                    .setInsets(WindowInsetsCompat.Type.statusBars(), Insets.of(0, statusPx, 0, 0))
                    .build(),
            )
        }
        compose.waitForIdle()
        return gap to compose.onNodeWithTag("first").getBoundsInRoot().top
    }

    @Test
    fun `under the back link the first line starts at the status bar, the link's row and the gap`() {
        val (gap, top) = firstLineTop(link = true, statusPx = 60)
        val status = with(compose.density) { 60.toDp() }
        assertEquals((status + 48.dp + gap).value, top.value, 0.5f)
    }

    @Test
    fun `without the link the first line starts at the status bar and the gap`() {
        val (gap, top) = firstLineTop(link = false, statusPx = 60)
        val status = with(compose.density) { 60.toDp() }
        assertEquals((status + gap).value, top.value, 0.5f)
    }
}
