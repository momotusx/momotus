package app.marquee.android

import android.app.Application
import androidx.activity.ComponentActivity
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
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
import kotlin.test.assertTrue

/** Each Calendar series that could not load has its own Try again, which retries that one only (audit P2). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class CalendarRetryTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val session = CoreSession(RuntimeEnvironment.getApplication())

    @After
    fun stop() = session.client.close()

    @Test
    fun `each failed series retries on its own`() {
        val sent = mutableListOf<String>()
        session.client.attach { text ->
            synchronized(sent) { sent += text }
            val id =
                CoreJson
                    .parseToJsonElement(text)
                    .jsonObject["id"]
                    ?.jsonPrimitive
                    ?.longOrNull ?: return@attach
            session.client.onMessage(
                """{"id":$id,"ok":true,"result":{"sub":4,"snapshot":{"today":"2026-10-31","days":[],"pending":0,""" +
                    """"failed":[{"itemId":"tt1","name":"Andor","reason":"timed out"},""" +
                    """{"itemId":"tt2","name":"Severance","reason":null}],""" +
                    """"noSchedule":[],"series":2}}}""",
            )
        }
        compose.setContent {
            MarqueeTheme(Form.Phone) { CalendarScreen(session, onOpen = {}, onDiscover = {}) }
        }
        val deadline = System.currentTimeMillis() + 60_000
        while (compose.onAllNodesWithContentDescription("Try again for Severance").fetchSemanticsNodes().isEmpty()) {
            check(System.currentTimeMillis() < deadline) { "the failed series never drew" }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
        compose.onNodeWithText("Could not load Andor (timed out).").assertExists()
        compose.onNodeWithText("Could not load Severance.").assertExists()
        compose.onNodeWithContentDescription("Try again for Andor").assertExists()
        compose.onNodeWithContentDescription("Try again for Severance").performClick()
        val retries = { synchronized(sent) { sent.filter { "\"retry\"" in it } } }
        while (retries().isEmpty() && System.currentTimeMillis() < deadline) {
            compose.waitForIdle()
            Thread.sleep(10)
        }
        Thread.sleep(100)
        val one = retries()
        assertTrue(one.size == 1 && "\"itemId\":\"tt2\"" in one[0], "one retry for Severance: $one")
    }
}
