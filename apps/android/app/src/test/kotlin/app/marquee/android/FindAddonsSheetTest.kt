package app.marquee.android

import android.app.Application
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.core.InstallCards
import app.marquee.core.InstallSheetCopy
import app.marquee.core.LinkInstall
import app.marquee.core.client.CoreJson
import app.marquee.core.session.CoreSession
import app.marquee.design.ARM_MS
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.JsonObject
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
import org.robolectric.annotation.GraphicsMode
import java.util.concurrent.CopyOnWriteArrayList
import kotlin.test.assertEquals

/** Find addons' card wiring (plans/addon-install-sheet.md): a card's Install only opens the sheet. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class, qualifiers = "w411dp-h1600dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class FindAddonsSheetTest {
    @get:Rule
    val compose = createComposeRule()

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val sent = CopyOnWriteArrayList<JsonObject>()

    @After
    fun stop() = scope.cancel()

    private val list =
        """{"lists":[{"id":"official","name":"Official","state":{"state":"ready"},"entries":[""" +
            """{"key":"https://a.example/manifest.json","id":"org.example","host":"a.example","name":"Example",""" +
            """"types":["movie"],"provides":["subtitles"],"configuration":"none",""" +
            """"configureUrl":"https://a.example/configure","installed":"absent"}]}]}"""

    /** The page on a session whose core answers the list above and every install. */
    private fun show() {
        val session = CoreSession(RuntimeEnvironment.getApplication())
        val client = session.client
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            sent += msg
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result =
                when (msg["op"]?.jsonPrimitive?.content) {
                    "subscribe" -> """{"sub":1,"snapshot":$list}"""
                    "installListed" -> """{"id":"org.example","name":"Example"}"""
                    else -> "{}"
                }
            client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                FindAddonsScreen(session, InstallCards(scope), LinkInstall(scope), onPasteLink = {})
            }
        }
    }

    private fun installs() = sent.count { it["op"]?.jsonPrimitive?.content == "installListed" }

    @Test
    fun `a card's Install opens the sheet and sends nothing, and the sheet's Install sends it once`() {
        show()
        val card = hasContentDescription("Install Example", substring = true)
        compose.waitUntil(10_000) { compose.onAllNodes(card).fetchSemanticsNodes().isNotEmpty() }
        compose.onNode(card).performClick()
        compose.onNodeWithText("Install Example?").assertExists()
        assertEquals(0, installs(), "a card press installs nothing")

        compose.mainClock.advanceTimeBy(ARM_MS + 100)
        compose.onNode(hasText(InstallSheetCopy.INSTALL)).performClick()
        compose.waitUntil(10_000) { installs() == 1 }
        compose.onNodeWithText("Install Example?").assertDoesNotExist()
        compose.waitForIdle()
        assertEquals(1, installs())
    }
}
