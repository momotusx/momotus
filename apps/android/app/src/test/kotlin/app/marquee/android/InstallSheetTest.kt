package app.marquee.android

import android.app.Application
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.input.InputMode
import androidx.compose.ui.input.InputModeManager
import androidx.compose.ui.platform.LocalInputModeManager
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import app.marquee.core.InstallSheetCopy
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals

/** The addon install sheet (plans/addon-install-sheet.md): what it says, and that only its Install installs. */
@OptIn(ExperimentalComposeUiApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class, qualifiers = "w411dp-h891dp")
@GraphicsMode(GraphicsMode.Mode.NATIVE)
class InstallSheetTest {
    @get:Rule
    val compose = createComposeRule()

    private var installs = 0
    private var cancels = 0
    private var open by mutableStateOf(false)
    private var inputModes: InputModeManager? = null

    /** The sheet as a card's Install opens it; each action closes it, as the page does. */
    private fun show(form: Form) {
        compose.setContent {
            inputModes = LocalInputModeManager.current
            MarqueeTheme(form) {
                if (open) {
                    InstallSheet(
                        "OpenSubtitles v3",
                        "opensubtitles-v3.strem.io",
                        listOf("subtitles"),
                        listOf("movie", "series"),
                        onInstall = {
                            installs++
                            open = false
                        },
                        onCancel = {
                            cancels++
                            open = false
                        },
                    )
                }
            }
        }
        compose.runOnIdle { open = true }
    }

    @Test
    fun `the sheet says the name, where it comes from, what it adds and the note`() {
        show(Form.Phone)
        compose.onNodeWithText("Install OpenSubtitles v3?").assertIsDisplayed()
        compose.onNodeWithText("From opensubtitles-v3.strem.io").assertIsDisplayed()
        compose.onNodeWithText("Adds subtitles for movies and series.").assertIsDisplayed()
        compose.onNodeWithText(InstallSheetCopy.NOTE).assertIsDisplayed()
    }

    @Test
    fun `Cancel installs nothing and closes the sheet`() {
        show(Form.Phone)
        compose.onNodeWithText(InstallSheetCopy.CANCEL).performClick()
        compose.runOnIdle {
            assertEquals(0, installs)
            assertEquals(1, cancels)
        }
        compose.onNodeWithText(InstallSheetCopy.NOTE).assertDoesNotExist()
    }

    @Test
    fun `Install installs exactly once`() {
        show(Form.Phone)
        compose.onNodeWithText(InstallSheetCopy.INSTALL).performClick()
        compose.runOnIdle {
            assertEquals(1, installs)
            assertEquals(0, cancels)
        }
        compose.onNodeWithText(InstallSheetCopy.NOTE).assertDoesNotExist()
    }

    /** As after a remote's key press: keyboard input mode, where buttons take focus as on a TV. */
    @Test
    fun `on a TV Cancel has the focus when the sheet opens`() {
        compose.setContent {
            inputModes = LocalInputModeManager.current
            MarqueeTheme(Form.Tv) {
                if (open) {
                    InstallSheet(
                        "Torrentio",
                        "torrentio.strem.fun",
                        listOf("stream"),
                        listOf("movie"),
                        onInstall = { installs++ },
                        onCancel = { cancels++ },
                    )
                }
            }
        }
        compose.runOnIdle { inputModes!!.requestInputMode(InputMode.Keyboard) }
        compose.runOnIdle { open = true }
        compose.onNodeWithText(InstallSheetCopy.CANCEL).assertIsFocused()
        compose.runOnIdle {
            assertEquals(0, installs)
            assertEquals(0, cancels)
        }
    }
}
