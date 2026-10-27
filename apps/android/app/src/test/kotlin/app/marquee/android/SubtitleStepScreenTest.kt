package app.marquee.android

import android.app.Application
import android.graphics.Bitmap
import android.os.Handler
import android.os.Looper
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.graphics.luminance
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasAnyAncestor
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.unit.dp
import app.marquee.core.SubtitleStep
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreTransport
import app.marquee.core.client.StateThreadRule
import app.marquee.core.model.LanguageChoice
import app.marquee.core.session.CoreOps
import app.marquee.design.Form
import app.marquee.design.Marquee
import app.marquee.design.MarqueeTheme
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
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
import kotlin.coroutines.CoroutineContext
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

    @get:Rule
    val stateThread = StateThreadRule()
    private val client = CoreClient(stateThread.dispatcher)

    // The step runs on the main looper, as on viewModelScope: SubtitleStep is main-thread only, and its state
    // written from another thread reached the composition late or, now and then, not before a check gave up.
    // Its work then runs only while the test idles the looper: see [until].
    private val main =
        object : CoroutineDispatcher() {
            private val handler = Handler(Looper.getMainLooper())

            override fun dispatch(
                context: CoroutineContext,
                block: Runnable,
            ) {
                handler.post(block)
            }
        }
    private val scope = CoroutineScope(SupervisorJob() + main)
    private val sent = CopyOnWriteArrayList<String>()

    @After
    fun stop() {
        scope.cancel()
        client.close()
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

    /** Holds the language list until it is released (a slow core). */
    @Volatile private var heldList: (() -> Unit)? = null

    @Volatile private var holdList = false

    /** A step over a core that knows [languages] and takes every write. */
    private fun step(device: String): SubtitleStep {
        // As the core answers: the device's language marked.
        val list =
            languages.joinToString(",", "[", "]") {
                """{"code":"${it.code}","name":"${it.name}","device":${it.code == device}}"""
            }
        client.attach(
            CoreTransport { text ->
                sent += text
                val request = CoreJson.parseToJsonElement(text).jsonObject
                val id = request.getValue("id").jsonPrimitive.long
                val op = request.getValue("op").jsonPrimitive.content
                val result = if (op == "languages") list else PREFS
                val answer = { client.onMessage("""{"id":$id,"ok":true,"result":$result}""") }
                when {
                    holdSaves && op == "setPrefs" -> held = answer
                    holdList && op == "languages" -> heldList = answer
                    else -> answer()
                }
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

    /** Waits for [condition], idling the main looper between checks: a bare waitUntil does not, so work the
     * step posted there (a save, a list that came) would never run. */
    private fun until(condition: () -> Boolean) =
        compose.waitUntil(5_000) {
            compose.waitForIdle()
            condition()
        }

    /** The page shows [text] now. */
    private fun shows(text: String) = compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    /** The page has the list in: Continue is enabled. */
    private fun ready() = runCatching { compose.onNodeWithText("Continue").assertIsEnabled() }.isSuccess

    private fun wrote(languages: String) = sent.any { "\"setPrefs\"" in it && "\"subtitleLanguages\":$languages" in it }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w412dp-h915dp-xxhdpi")
    fun `phone, the device's language leads and is chosen, and No subtitles writes none`() {
        val step = step("tr")
        var done = 0
        page(Form.Phone) { SubtitleStepPage(step) { done++ } }
        until { shows("This device's language") && ready() }
        compose.onNodeWithText("Turkish").assertExists()
        shot("subtitle-step-phone")
        // The question and its line sit over the list: No subtitles may be further down.
        compose.onNodeWithText(SubtitleStep.NONE).performScrollTo().performClick()
        compose.onNodeWithText("Continue").performClick()
        until { done == 1 }
        assertTrue(wrote("[]"), "No subtitles writes an empty list: $sent")
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w412dp-h915dp-xxhdpi")
    fun `the chosen language is a brighter fill with no edge than the others`() {
        val step = step("tr")
        page(Form.Phone) { SubtitleStepPage(step) {} }
        until { shows("This device's language") && ready() }
        compose.onNodeWithText(SubtitleStep.NONE).performScrollTo()
        val pixels = compose.onRoot().captureToImage().toPixelMap()

        // Just inside a row's left edge, where an outline would draw, and well inside it, at its middle.
        fun at(text: String): Pair<Color, Color> {
            val row = compose.onNodeWithText(text).fetchSemanticsNode().boundsInRoot
            val y = row.center.y.toInt()
            return pixels[row.left.toInt() + 2, y] to pixels[row.left.toInt() + 20, y]
        }
        val (chosenEdge, chosenInside) = at("Turkish")
        assertEquals(chosenInside.toArgb(), chosenEdge.toArgb(), "the chosen row has no edge")
        val (otherEdge, otherInside) = at(SubtitleStep.NONE)
        assertEquals(otherInside.toArgb(), otherEdge.toArgb(), "a row not chosen has no edge")
        // White 24 percent under the chosen row, 12 under the rest, both over the ramp's black.
        assertTrue(
            chosenInside.luminance() > otherInside.luminance() + 0.01f,
            "chosen $chosenInside, other $otherInside",
        )
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, Continue has the focus, so one press keeps the device's language`() {
        val step = step("tr")
        var done = 0
        page(Form.Tv) { SubtitleStepPage(step) { done++ } }
        until { ready() }
        until { runCatching { compose.onNode(isFocused()).assert(hasText("Continue")) }.isSuccess }
        compose.onNode(isFocused()).performClick()
        until { done == 1 }
        assertTrue(wrote("""["tr"]"""), "Continue writes the device's language: $sent")
    }

    @Test
    @Config(qualifiers = "w412dp-h915dp-xxhdpi")
    fun `a profile with languages passes by, once, and shows nothing`() {
        val step = step("tr")
        var done = 0
        page(Form.Phone) { SubtitleStepGate({ false }, step) { done++ } }
        until { done == 1 }
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
        page(Form.Phone) { SubtitleStepGate({ true }, step) { done++ } }
        until { runCatching { compose.onNodeWithText(SubtitleStep.TITLE).assertExists() }.isSuccess }
        compose.onNodeWithText(SubtitleStep.SKIP).performClick()
        until { done == 1 }
        assertTrue(sent.none { "\"setPrefs\"" in it }, "Skip writes nothing: $sent")
    }

    @Test
    @Config(qualifiers = "w412dp-h915dp-xxhdpi")
    fun `Continue waits for the language list`() {
        holdList = true
        val step = step("tr")
        page(Form.Phone) { SubtitleStepPage(step) {} }
        until { heldList != null }
        compose.onNodeWithText("Continue").assertIsNotEnabled()
        heldList?.invoke()
        until { ready() }
        compose.onNodeWithText("Continue").assertIsEnabled()
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, a list that comes after 2 s still puts the focus on Continue`() {
        holdList = true
        val step = step("tr")
        var done = 0
        page(Form.Tv) { SubtitleStepPage(step) { done++ } }
        until { heldList != null }
        // Past the first focus request's frames: a disabled Continue could not take it.
        compose.mainClock.advanceTimeBy(2_000)
        heldList?.invoke()
        until { ready() }
        until { runCatching { compose.onNode(isFocused()).assert(hasText("Continue")) }.isSuccess }
        compose.onNode(isFocused()).performClick()
        until { done == 1 }
        assertTrue(wrote("""["tr"]"""), "one press keeps the device's language: $sent")
    }

    @Test
    @Config(qualifiers = "w412dp-h915dp-xxhdpi")
    fun `Skip waits while Continue saves, so the step leaves once`() {
        holdSaves = true
        val step = step("tr")
        var done = 0
        page(Form.Phone) { SubtitleStepPage(step) { done++ } }
        until { ready() }
        compose.onNodeWithText("Continue").performClick()
        until { held != null }
        compose.onNodeWithText(SubtitleStep.SKIP).performClick()
        compose.waitForIdle()
        assertEquals(0, done, "Skip went while the save ran")
        held?.invoke()
        until { done == 1 }
        compose.waitForIdle()
        assertEquals(1, done)
    }

    @Test
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `the subtitles step is framed, its question in the frame, step 4 of 4`() {
        val step = step("tr")
        page(Form.Tv) { SubtitleStepPage(step) {} }
        until { ready() }
        compose.onNode(hasText(SubtitleStep.TITLE) and hasAnyAncestor(hasTestTag(FILM_FRAME))).assertExists()
        compose.onNodeWithText("Step 4 of 4").assertExists()
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV render of the step`() {
        val step = step("tr")
        page(Form.Tv) { SubtitleStepPage(step) {} }
        until { shows("This device's language") }
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
        until { runCatching { compose.onNodeWithText("Turkish first").assertExists() }.isSuccess }
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
