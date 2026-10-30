package app.marquee.android

import android.app.Application
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createComposeRule
import app.marquee.core.client.CoreJson
import app.marquee.core.session.CoreSession
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertTrue

/** Find addons > Nuvio plugins (R8). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class, qualifiers = "w411dp-h900dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class PluginDirectoryTest {
    @get:Rule
    val compose = createComposeRule()

    private val directory =
        """{"source":"nuvioplugins.com","fromSnapshot":false,"repos":[{"url":"https://r.example/manifest.json",""" +
            """"host":"r.example","name":"Yoru's Repo","description":"","providers":["Castle"],"languages":["en"],""" +
            """"types":["movie"],"installed":false}]}"""

    /**
     * Pixel walk 2026-10-30 F6: the search field was an empty grey bar. Empty, it shows what it searches, as Find
     * addons' list search does: its placeholder draws words in the field.
     */
    @Test
    fun `the empty search field says what it searches`() {
        val session = CoreSession(RuntimeEnvironment.getApplication())
        session.client.attach { text ->
            val msg = CoreJson.parseToJsonElement(text).jsonObject
            val id = msg["id"]?.jsonPrimitive?.longOrNull ?: return@attach
            val result = if (msg["op"]?.jsonPrimitive?.content == "pluginDirectory") directory else "{}"
            session.client.onMessage("""{"id":$id,"ok":true,"result":$result}""")
        }
        compose.setContent { MarqueeTheme(Form.Phone) { PluginDirectoryScreen(session) } }
        compose.waitUntil(10_000) { compose.onAllNodes(hasSetTextAction()).fetchSemanticsNodes().isNotEmpty() }
        val field = compose.onNode(hasSetTextAction()).captureToImage().toPixelMap()
        // The field is a dark fill; the placeholder's words are the only light pixels in it.
        val pixels = (0 until field.width).flatMap { x -> (0 until field.height).map { y -> field[x, y] } }
        val light = pixels.count { it.luminance() > 0.3f }
        assertTrue(light > 20, "the empty field draws no words ($light light pixels)")
    }
}
