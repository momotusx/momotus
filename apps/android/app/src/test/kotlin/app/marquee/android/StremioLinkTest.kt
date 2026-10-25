package app.marquee.android

import android.app.Application
import android.content.Intent
import android.net.Uri
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** An addon's Install button (stremio://) opens Marquee on the link field, filled in (journey review P2-6). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class StremioLinkTest {
    @get:Rule
    val compose = createComposeRule()

    private val link = "stremio://torrentio.strem.fun/manifest.json"

    @Test
    fun `an Install button's link opens Momotus, and only a stremio link counts`() {
        val view = Intent(Intent.ACTION_VIEW, Uri.parse(link)).addCategory(Intent.CATEGORY_BROWSABLE)
        val found = RuntimeEnvironment.getApplication().packageManager.queryIntentActivities(view, 0)
        assertEquals(listOf(MainActivity::class.java.name), found.map { it.activityInfo.name })
        assertEquals(link, addonLink(view))
        assertNull(addonLink(Intent(Intent.ACTION_VIEW, Uri.parse("https://torrentio.strem.fun/manifest.json"))))
        assertNull(addonLink(Intent(Intent.ACTION_MAIN)))
        // A title link from a website is not an addon.
        assertNull(addonLink(Intent(Intent.ACTION_VIEW, Uri.parse("stremio:///detail/movie/tt1727587"))))
        // A launch from the recent apps repeats the old intent: the link was taken already.
        assertNull(addonLink(Intent(view).addFlags(Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY)))
    }

    @Test
    fun `the handed link fills the field and waits for Add`() {
        var offered by mutableStateOf<String?>(null)
        val added = mutableListOf<String>()
        val focus = FocusRequester()
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                InstallField(
                    false,
                    focus,
                    null,
                    offered = offered,
                    onTaken = { offered = null },
                ) { url, _, _ ->
                    added += url
                }
            }
        }
        compose.runOnIdle { offered = link }
        compose.onNodeWithText(link).assertExists()
        compose.runOnIdle { assertEquals(emptyList(), added, "never added by itself") }
        compose.onNodeWithText("Add").performClick()
        compose.runOnIdle { assertEquals(listOf(link), added) }
    }
}
