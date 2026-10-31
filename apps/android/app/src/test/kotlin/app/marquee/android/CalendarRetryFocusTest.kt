package app.marquee.android

import android.app.Application
import androidx.activity.ComponentActivity
import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithContentDescription
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.requestFocus
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

/**
 * On a TV, a Calendar series line that leaves after its Try again loaded it hands the focus on: to the next line's
 * Try again, else the one before, and after the last line to the list's first episode, never to nothing, as on the
 * web. Compose does it (the lines are positional, so the next one takes the place and the focus of the line that left);
 * this holds the screen to it.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class CalendarRetryFocusTest {
    @get:Rule
    val compose = createAndroidComposeRule<ComponentActivity>()

    private val session = CoreSession(RuntimeEnvironment.getApplication())

    @After
    fun stop() = session.client.close()

    private fun snapshot(failed: List<Pair<String, String>>): String {
        val lines = failed.joinToString(",") { (id, name) -> """{"itemId":"$id","name":"$name","reason":null}""" }
        return """{"today":"2026-10-31","days":[{"date":"2026-11-02","items":[{"itemId":"tt9","videoId":"tt9:1:1",""" +
            """"name":"Slow Horses","season":1,"episode":1,"title":"Failure's Contagious","date":"2026-11-02",""" +
            """"watched":false}]}],"pending":0,"failed":[$lines],"noSchedule":[],"series":3}"""
    }

    private fun waitFor(
        what: String,
        done: () -> Boolean,
    ) {
        val deadline = System.currentTimeMillis() + 60_000
        while (!done()) {
            check(System.currentTimeMillis() < deadline) { what }
            Snapshot.sendApplyNotifications()
            compose.waitForIdle()
            Thread.sleep(10)
        }
        compose.waitForIdle()
    }

    private fun shown(label: String) =
        compose.onAllNodesWithContentDescription(label).fetchSemanticsNodes().isNotEmpty()

    private val andor = "tt1" to "Andor"
    private val severance = "tt2" to "Severance"

    /** The TV Calendar with Andor and Severance failed, the remote on [focused]'s Try again. */
    private fun open(focused: String) {
        session.client.attach { text ->
            val id =
                CoreJson
                    .parseToJsonElement(text)
                    .jsonObject["id"]
                    ?.jsonPrimitive
                    ?.longOrNull ?: return@attach
            val opened = """{"sub":4,"snapshot":${snapshot(listOf(andor, severance))}}"""
            val result = if ("\"subscribe\"" in text) opened else "{}"
            session.client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        compose.setContent {
            MarqueeTheme(Form.Tv) { CalendarScreen(session, onOpen = {}, onDiscover = {}) }
        }
        waitFor("the failed series never drew") { shown("Try again for Severance") }
        compose.onNodeWithContentDescription("Try again for $focused").requestFocus().assertIsFocused()
    }

    private fun push(failed: List<Pair<String, String>>) =
        session.client.onMessage("""{"sub":4,"updates":[${snapshot(failed)}]}""")

    @Test
    fun `a line that leaves hands the focus to the next Try again, then to the first episode`() {
        open("Andor")
        // Andor loaded: its line leaves, and Severance's Try again takes the focus.
        push(listOf(severance))
        waitFor("Andor's line never left") { !shown("Try again for Andor") }
        compose.onNodeWithContentDescription("Try again for Severance").assertIsFocused()
        // The last line leaves: the list's first episode.
        push(emptyList())
        waitFor("Severance's line never left") { !shown("Try again for Severance") }
        compose.onNodeWithContentDescription("Slow Horses").assertIsFocused()
    }

    @Test
    fun `the last line leaving hands the focus to the line before it`() {
        open("Severance")
        push(listOf(andor))
        waitFor("Severance's line never left") { !shown("Try again for Severance") }
        compose.onNodeWithContentDescription("Try again for Andor").assertIsFocused()
    }
}
