package app.marquee.android

import android.app.Application
import android.graphics.Bitmap
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.unit.dp
import app.marquee.core.SubtitleStep
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreTransport
import app.marquee.core.model.LanguageChoice
import app.marquee.core.session.CoreOps
import app.marquee.design.Form
import app.marquee.design.Marquee
import app.marquee.design.MarqueeTheme
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.junit.After
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executors
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * The first run's subtitles step and Settings' Subtitle language row (plans/m13-subtitle-languages.md 7), at
 * phone and TV sizes. With MQ_SHOTS set, the render tests save theirs there (native graphics, where the
 * window takes no focus: the TV's first focus is checked apart).
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class SubtitleStepScreenTest {
    @get:Rule
    val compose = createComposeRule()

    private val executor = Executors.newSingleThreadExecutor { Thread(it, "test-state") }
    private val client = CoreClient(executor.asCoroutineDispatcher())
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val sent = CopyOnWriteArrayList<String>()

    @After
    fun stop() {
        scope.cancel()
        client.close()
        executor.shutdownNow()
    }

    private val languages =
        listOf(
            LanguageChoice("de", "German"),
            LanguageChoice("en", "English"),
            LanguageChoice("pl", "Polish"),
            LanguageChoice("tr", "Turkish"),
        )

    /** Holds the answer to the next `setPrefs` until it is released (a slow save). */
    @Volatile private var held: (() -> Unit)? = null

    @Volatile private var holdSaves = false

    /** A step over a core that knows [languages] and takes every write. */
    private fun step(device: String): SubtitleStep {
        val list = languages.joinToString(",", "[", "]") { """{"code":"${it.code}","name":"${it.name}"}""" }
        client.attach(
            CoreTransport { text ->
                sent += text
                val request = CoreJson.parseToJsonElement(text).jsonObject
                val id = request.getValue("id").jsonPrimitive.long
                val op = request.getValue("op").jsonPrimitive.content
                val result = if (op == "languages") list else PREFS
                val answer = { client.onMessage("""{"id":$id,"ok":true,"result":$result}""") }
                if (holdSaves && op == "setPrefs") held = answer else answer()
            },
        )
        return SubtitleStep(CoreOps(client), scope, device).also { it.load() }
    }

    private fun page(
        form: Form,
        content: @androidx.compose.runtime.Composable () -> Unit,
    ) = compose.setContent {
        MarqueeTheme(form) { Box(Modifier.fillMaxSize().background(Marquee.colors.bg)) { content() } }
    }

    private fun shot(name: String) {
        val dir = System.getenv("MQ_SHOTS") ?: return
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        File(dir, "$name.png").outputStream().use { image.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }

    private fun wrote(languages: String) = sent.any { "\"setPrefs\"" in it && "\"subtitleLanguages\":$languages" in it }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w412dp-h915dp-xxhdpi")
    fun `phone, the device's language leads and is chosen, and No subtitles writes none`() {
        val step = step("tr")
        var done = 0
        page(Form.Phone) { SubtitleStepPage(step, "tr") { done++ } }
        compose.waitUntil(5_000) { step.all.isNotEmpty() }
        compose.onNodeWithText("This device's language").assertExists()
        compose.onNodeWithText("Turkish").assertExists()
        shot("subtitle-step-phone")
        compose.onNodeWithText(SubtitleStep.NONE).performClick()
        compose.onNodeWithText("Continue").performClick()
        compose.waitUntil(5_000) { done == 1 }
        assertTrue(wrote("[]"), "No subtitles writes an empty list: $sent")
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, Continue has the focus, so one press keeps the device's language`() {
        val step = step("tr")
        var done = 0
        page(Form.Tv) { SubtitleStepPage(step, "tr") { done++ } }
        compose.waitUntil(5_000) { step.all.isNotEmpty() }
        compose.waitUntil(5_000) { runCatching { compose.onNode(isFocused()).assert(hasText("Continue")) }.isSuccess }
        compose.onNode(isFocused()).performClick()
        compose.waitUntil(5_000) { done == 1 }
        assertTrue(wrote("""["tr"]"""), "Continue writes the device's language: $sent")
    }

    @Test
    @Config(qualifiers = "w412dp-h915dp-xxhdpi")
    fun `a profile with languages passes by, once, and shows nothing`() {
        val step = step("tr")
        var done = 0
        page(Form.Phone) { SubtitleStepGate({ false }, step, "tr") { done++ } }
        compose.waitUntil(5_000) { done == 1 }
        // A second leave would arrive at Home twice.
        compose.mainClock.advanceTimeBy(1_000)
        compose.onNodeWithText(SubtitleStep.TITLE).assertDoesNotExist()
        assertEquals(1, done)
    }

    @Test
    @Config(qualifiers = "w412dp-h915dp-xxhdpi")
    fun `Skip leaves at once and writes nothing`() {
        val step = step("tr")
        var done = 0
        page(Form.Phone) { SubtitleStepGate({ true }, step, "tr") { done++ } }
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithText(SubtitleStep.TITLE).assertExists() }.isSuccess }
        compose.onNodeWithText(SubtitleStep.SKIP).performClick()
        compose.waitUntil(5_000) { done == 1 }
        assertTrue(sent.none { "\"setPrefs\"" in it }, "Skip writes nothing: $sent")
    }

    @Test
    @Config(qualifiers = "w412dp-h915dp-xxhdpi")
    fun `Skip waits while Continue saves, so the step leaves once`() {
        holdSaves = true
        val step = step("tr")
        var done = 0
        page(Form.Phone) { SubtitleStepPage(step, "tr") { done++ } }
        compose.waitUntil(5_000) { step.all.isNotEmpty() }
        compose.onNodeWithText("Continue").performClick()
        compose.waitUntil(5_000) { held != null }
        compose.onNodeWithText(SubtitleStep.SKIP).performClick()
        compose.waitForIdle()
        assertEquals(0, done, "Skip went while the save ran")
        held?.invoke()
        compose.waitUntil(5_000) { done == 1 }
        compose.waitForIdle()
        assertEquals(1, done)
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV render of the step`() {
        val step = step("tr")
        page(Form.Tv) { SubtitleStepPage(step, "tr") {} }
        compose.waitUntil(5_000) { step.all.isNotEmpty() }
        compose.onNodeWithText("This device's language").assertExists()
        shot("subtitle-step-tv")
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w412dp-h915dp-xxhdpi")
    fun `phone, Settings names the first subtitle language over the ordered picker`() =
        settings(Form.Phone, "settings-subtitle-language-phone")

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, Settings names the first subtitle language over the ordered picker`() =
        settings(Form.Tv, "settings-subtitle-language-tv")

    private fun settings(
        form: Form,
        name: String,
    ) {
        page(form) { Column(Modifier.padding(24.dp)) { SubtitleLanguage(listOf("tr", "eng"), { languages }, {}) } }
        compose.waitUntil(5_000) { runCatching { compose.onNodeWithText("Turkish first").assertExists() }.isSuccess }
        compose.onNodeWithText("Subtitle language").assertExists()
        shot(name)
    }

    private companion object {
        const val ZERO = """{"ready":0,"height":0,"hdr":0,"size":0,"picks":0,"epoch":0,"atMs":0}"""
        const val PREFS =
            """{"streams":{"quality":"balanced","minSeeders":50,"hdr":true,"preferReady":true,"learn":true},""" +
                """"autoplayNext":true,"homeRows":{},"learned":{"uhd":$ZERO,"fhd":$ZERO,"hd":$ZERO,"sd":$ZERO},"learnedEpoch":0}"""
    }
}
