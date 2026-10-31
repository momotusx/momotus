package app.marquee.android

import android.app.Application
import android.graphics.Bitmap
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asAndroidBitmap
import androidx.compose.ui.input.InputMode
import androidx.compose.ui.input.InputModeManager
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.platform.LocalInputModeManager
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.captureToImage
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.isFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.unit.dp
import app.marquee.core.LanguagePicks
import app.marquee.core.model.LanguageChoice
import app.marquee.core.model.SubtitleLayout
import app.marquee.design.Form
import app.marquee.design.Marquee
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import java.io.File
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * Settings > Subtitles and audio's second line (plans/dual-subtitles.md 5.5): Off and Spoken language, the first
 * subtitle language that cannot be picked, and Layout and Only when paused shown only while a second line is set.
 * With MQ_SHOTS set, the render tests save theirs there.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class SubtitlesAudioTest {
    @get:Rule
    val compose = createComposeRule()

    private val languages =
        listOf(
            LanguageChoice("en", "English"),
            LanguageChoice("es", "Spanish"),
            LanguageChoice("tr", "Turkish"),
        )

    private var second by mutableStateOf<String?>(null)
    private var layout by mutableStateOf(SubtitleLayout.Stacked)
    private var paused by mutableStateOf(false)
    private val picks = mutableListOf<String?>()
    private var inputModes: InputModeManager? = null

    private fun rows(form: Form) =
        compose.setContent {
            inputModes = LocalInputModeManager.current
            MarqueeTheme(form) {
                Box(
                    Modifier
                        .fillMaxSize()
                        .background(
                            Marquee.colors.bg,
                        ).verticalScroll(rememberScrollState())
                        .padding(24.dp),
                ) {
                    SecondLineRows(
                        second,
                        layout,
                        paused,
                        first = "eng",
                        load = { languages },
                        onSecond = {
                            picks += it
                            second = it
                        },
                        onLayout = { layout = it },
                        onPaused = { paused = it },
                    )
                }
            }
        }

    private fun shows(text: String) = compose.onAllNodesWithText(text).fetchSemanticsNodes().isNotEmpty()

    private fun shot(name: String) {
        val dir = System.getenv("MQ_SHOTS") ?: return
        val image = compose.onRoot().captureToImage().asAndroidBitmap()
        File(dir, "$name.png").outputStream().use { image.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w412dp-h915dp-xxhdpi")
    fun `phone, Off hides Layout and Only when paused, Spoken language shows them and each writes`() {
        rows(Form.Phone)
        compose.waitUntil(5_000) { shows("Spanish") }
        assertFalse(shows("Layout"))
        assertFalse(shows("Only when paused"))
        shot("settings-second-line-off-phone")

        compose.onNodeWithText("Spoken language").performClick()
        compose.runOnIdle { assertEquals(listOf<String?>(LanguagePicks.SPOKEN), picks) }
        compose.onNodeWithText("Layout").assertExists()
        compose.onNodeWithText("One box").performClick()
        compose.runOnIdle { assertEquals(SubtitleLayout.OneBox, layout) }
        compose.onNodeWithText("Only when paused").performClick()
        compose.runOnIdle { assertTrue(paused) }
        shot("settings-second-line-on-phone")

        // A language from the list, then Off: the rows go again.
        compose.onNodeWithText("Spanish").performClick()
        compose.onNodeWithText("Off").performClick()
        compose.runOnIdle { assertEquals(listOf(LanguagePicks.SPOKEN, "es", null), picks) }
        assertFalse(shows("Layout"))
    }

    @Test
    @Config(qualifiers = "w412dp-h915dp-xxhdpi")
    fun `the first subtitle language reads Subtitle language and cannot be picked`() {
        rows(Form.Phone)
        compose.waitUntil(5_000) { shows("English") }
        compose.onNodeWithText("Subtitle language", substring = true).assertExists()
        compose.onNode(hasText("English", substring = true)).assertIsNotEnabled()
        compose.onNode(hasText("English", substring = true)).performClick()
        compose.runOnIdle {
            assertTrue(picks.isEmpty())
            assertNull(second)
        }
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    @Config(qualifiers = "w960dp-h540dp-xhdpi")
    fun `TV, the remote moves from Off to Spoken language, picks it, and the first line's language takes focus`() {
        rows(Form.Tv)
        compose.waitUntil(5_000) { shows("Spanish") }
        // As after a remote's key press: keyboard input mode, where chips take focus as on a TV.
        compose.runOnIdle { inputModes!!.requestInputMode(InputMode.Keyboard) }
        compose.onNodeWithText("Off").requestFocus()
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.DirectionRight) }
        compose.onNode(isFocused()).assert(hasText("Spoken language"))
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.Enter) }
        compose.runOnIdle { assertEquals(listOf<String?>(LanguagePicks.SPOKEN), picks) }
        compose.onNodeWithText("Layout").assertExists()
        // The language a remote cannot pick still takes focus, so its words say why.
        compose.onNode(hasText("English", substring = true)).requestFocus()
        compose.onNode(isFocused()).assert(hasText("English", substring = true))
        compose.onNode(isFocused()).performKeyInput { pressKey(Key.Enter) }
        compose.runOnIdle { assertEquals(listOf<String?>(LanguagePicks.SPOKEN), picks) }
        shot("settings-second-line-tv")
    }
}
