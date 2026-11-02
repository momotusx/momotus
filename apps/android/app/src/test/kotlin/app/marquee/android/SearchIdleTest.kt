package app.marquee.android

import android.app.Application
import androidx.activity.ComponentActivity
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.test.assertIsDisplayed
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
import kotlin.test.assertTrue

/**
 * An empty Search gives the user something to do (owner, 2026-11-02): Recent searches with a way to clear them,
 * then the core's shelf of titles, titled as Home titles it; the hint only where neither shows.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class SearchIdleTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val session = CoreSession(RuntimeEnvironment.getApplication())
    private val sent = mutableListOf<String>()

    @After
    fun stop() = session.client.close()

    private val shelf =
        """{"rowId":"cinemeta:movie:top","title":"Popular Movies","source":"Cinemeta","state":{"state":"ready"},""" +
            """"tiles":[{"id":"tt1","kind":"movie","name":"Heat Wave","posterShape":"poster"}]}"""

    private fun answer(snapshot: String) {
        session.client.attach { text ->
            sent += text
            val id =
                CoreJson
                    .parseToJsonElement(text)
                    .jsonObject["id"]
                    ?.jsonPrimitive
                    ?.longOrNull ?: return@attach
            session.client.onMessage("""{"id":$id,"ok":true,"result":{"sub":4,"snapshot":$snapshot}}""")
        }
        compose.setContent {
            MarqueeTheme(Form.Phone) { SearchScreen(session, onOpen = {}, onAddons = {}) }
        }
    }

    private fun waitFor(text: String) {
        val deadline = System.currentTimeMillis() + 60_000
        while (compose.onAllNodesWithText(text, useUnmergedTree = true).fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "$text never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
    }

    @Test
    fun `an empty search shows Recent, a shelf of titles and Clear, and no hint`() {
        answer("""{"query":"","rows":[],"recent":["heat"],"searchable":2,"start":$shelf}""")
        waitFor("Popular Movies")
        compose.onNodeWithText("Recent searches", ignoreCase = true).assertIsDisplayed()
        compose.onNodeWithText("heat").assertIsDisplayed()
        assertEquals(0, compose.onAllNodesWithText("Type a title", substring = true).fetchSemanticsNodes().size)
        // No addon's name reaches the page.
        assertEquals(0, compose.onAllNodesWithText("Cinemeta", substring = true).fetchSemanticsNodes().size)
        compose.onNodeWithText("Clear recent searches").performClick()
        compose.waitForIdle()
        assertTrue(sent.any { """"clearRecent"""" in it }, "Clear sent no command: $sent")
    }

    @Test
    fun `a shelf alone is enough, with no recent searches and no hint`() {
        answer("""{"query":"","rows":[],"recent":[],"searchable":2,"start":$shelf}""")
        waitFor("Popular Movies")
        assertEquals(0, compose.onAllNodesWithText("Type a title", substring = true).fetchSemanticsNodes().size)
        assertEquals(0, compose.onAllNodesWithText("Clear recent searches").fetchSemanticsNodes().size)
    }

    @Test
    fun `with nothing to start from the hint is the one line`() {
        answer("""{"query":"","rows":[],"recent":[],"searchable":2,"start":null}""")
        waitFor("Type a title. Results come from every addon with a search catalog.")
        assertEquals(0, compose.onAllNodesWithText("Clear recent searches").fetchSemanticsNodes().size)
    }
}
