package app.marquee.android

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import app.marquee.core.LinkInstall
import app.marquee.core.client.CoreJson
import app.marquee.core.session.CoreSession
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

/**
 * The pasted link's sheet belongs to the page (plans/addon-install-sheet.md): on Find addons the field sits in a
 * scrolling list, and a check must outlive the field scrolling out of it.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class LinkSheetTest {
    @get:Rule
    val compose = createComposeRule()

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val sent = CopyOnWriteArrayList<JsonObject>()

    @After
    fun stop() = scope.cancel()

    private val addon =
        """{"url":"","host":"sheet.example","name":"Sheet Test","version":"1.0.0","enabled":false,""" +
            """"provides":["subtitles"],"types":["movie","series"],"configurable":false,"builtIn":false}"""

    @Test
    fun `a check outlives its field scrolling out of the list, and its sheet shows on the page`() {
        val session = CoreSession(RuntimeEnvironment.getApplication())
        val client = session.client
        // The preview is held until the field has gone.
        var previewId: Long? = null
        client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            sent += msg
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            if (msg["op"]?.jsonPrimitive?.content == "previewAddon") {
                previewId = id
            } else {
                client.onMessage("""{"id":$id,"ok":true,"result":{}}""")
            }
        }
        val link = LinkInstall(scope)
        var fieldInList by mutableStateOf(true)
        compose.setContent {
            MarqueeTheme(Form.Phone) {
                val focus = remember { FocusRequester() }
                LinkSheet(session, link, focus)
                if (fieldInList) LinkField(session, link, focus)
            }
        }
        compose.onNode(hasSetTextAction()).performTextInput("https://sheet.example/manifest.json")
        compose.onNode(hasText("Add")).performClick()
        compose.waitUntil(10_000) { previewId != null }

        // Scrolled out of the list: the field leaves composition while the check runs.
        fieldInList = false
        compose.waitForIdle()
        client.onMessage("""{"id":$previewId,"ok":true,"result":$addon}""")
        val sheet = hasText("Install Sheet Test?")
        compose.waitUntil(10_000) { compose.onAllNodes(sheet).fetchSemanticsNodes().isNotEmpty() }
        compose.onNodeWithText("Adds subtitles for movies and series.").assertExists()
        assertEquals(0, sent.count { it["op"]?.jsonPrimitive?.content == "installAddon" })
    }
}
