package app.marquee.android

import android.app.Application
import androidx.activity.ComponentActivity
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.core.client.CoreJson
import app.marquee.core.session.CoreSession
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** An empty library says so and gives the way on, Discover (plans/2026-10-28-ui-ux-audit.md, STATE-05). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class LibraryEmptyTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val session = CoreSession(RuntimeEnvironment.getApplication())

    @After
    fun stop() = session.client.close()

    @Test
    fun `an empty library offers Discover, and it goes there`() {
        session.client.attach { text ->
            val id = CoreJson.parseToJsonElement(text).jsonObject["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            session.client.onMessage("""{"id":$id,"ok":true,"result":{"sub":4,"snapshot":{"sort":"recent"}}}""")
        }
        var discovered = 0
        compose.setContent {
            MarqueeTheme(Form.Phone) { LibraryScreen(session, onOpen = {}, onDiscover = { discovered++ }) }
        }
        val deadline = System.currentTimeMillis() + 60_000
        while (compose.onAllNodesWithText("Discover").fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "the empty library never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
        compose.onNodeWithText("Your library is empty. Add a title from its page and it shows here.").assertExists()
        compose.onNodeWithText("Discover").performClick()
        assertEquals(1, discovered)
    }
}
