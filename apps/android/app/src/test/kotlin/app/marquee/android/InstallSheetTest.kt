package app.marquee.android

import android.app.Application
import android.os.SystemClock
import android.view.MotionEvent
import androidx.compose.foundation.layout.Column
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.input.InputMode
import androidx.compose.ui.input.InputModeManager
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.platform.LocalInputModeManager
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.getUnclippedBoundsInRoot
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.test.platform.app.InstrumentationRegistry
import app.marquee.core.InstallSheetCopy
import app.marquee.design.ARM_MS
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import app.marquee.design.SecondaryButton
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import org.robolectric.shadows.ShadowDialog
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The addon install sheet (plans/addon-install-sheet.md): what it says, and that only its Install installs. */
@OptIn(ExperimentalComposeUiApi::class, ExperimentalTestApi::class)
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
    private val opener = FocusRequester()

    /**
     * A button that opens the sheet, as a card's Install does. With [closes] each answer closes the sheet, as the page
     * does; without it the sheet stays, so a second press reaches it.
     */
    private fun show(
        form: Form,
        closes: Boolean = true,
        keyboard: Boolean = false,
    ) {
        compose.setContent {
            inputModes = LocalInputModeManager.current
            MarqueeTheme(form) {
                Column {
                    SecondaryButton("Open", { open = true }, focusRequester = opener)
                    if (open) {
                        InstallSheet(
                            "OpenSubtitles v3",
                            "opensubtitles-v3.strem.io",
                            listOf("subtitles"),
                            listOf("movie", "series"),
                            onInstall = {
                                installs++
                                if (closes) open = false
                            },
                            onCancel = {
                                cancels++
                                if (closes) open = false
                            },
                            opener = opener,
                        )
                    }
                }
            }
        }
        if (keyboard) {
            // A TV display is never in touch mode, so a new window (the sheet's) starts out of it too.
            InstrumentationRegistry.getInstrumentation().setInTouchMode(false)
            compose.runOnIdle { inputModes!!.requestInputMode(InputMode.Keyboard) }
            compose.onNodeWithText("Open").requestFocus()
        }
        compose.runOnIdle { open = true }
        compose.waitForIdle()
    }

    /** Past the moment the sheet takes no press. */
    private fun armed() {
        compose.mainClock.advanceTimeBy(ARM_MS + 100)
        compose.waitForIdle()
    }

    private fun counts(
        installed: Int,
        cancelled: Int,
    ) = compose.runOnIdle {
        assertEquals(installed, installs, "installs")
        assertEquals(cancelled, cancels, "cancels")
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
        armed()
        compose.onNodeWithText(InstallSheetCopy.CANCEL).performClick()
        counts(installed = 0, cancelled = 1)
        compose.onNodeWithText(InstallSheetCopy.NOTE).assertDoesNotExist()
    }

    @Test
    fun `Install installs exactly once, a double tap too`() {
        show(Form.Phone, closes = false)
        armed()
        val install = compose.onNodeWithText(InstallSheetCopy.INSTALL)
        install.performClick()
        install.performClick()
        counts(installed = 1, cancelled = 0)
    }

    @Test
    fun `the press that opened the sheet never answers it`() {
        compose.mainClock.autoAdvance = false
        show(Form.Phone)
        repeat(3) {
            compose.mainClock.advanceTimeByFrame()
            compose.waitForIdle()
        }
        compose.onNodeWithText(InstallSheetCopy.INSTALL).performClick()
        compose.onNodeWithText(InstallSheetCopy.CANCEL).performClick()
        counts(installed = 0, cancelled = 0)
        compose.mainClock.autoAdvance = true
        armed()
        compose.onNodeWithText(InstallSheetCopy.INSTALL).performClick()
        counts(installed = 1, cancelled = 0)
    }

    @Test
    fun `Back cancels`() {
        show(Form.Phone)
        compose.runOnUiThread { ShadowDialog.getLatestDialog().onBackPressed() }
        counts(installed = 0, cancelled = 1)
        compose.onNodeWithText(InstallSheetCopy.NOTE).assertDoesNotExist()
    }

    @Test
    fun `a tap outside the sheet cancels`() {
        show(Form.Phone)
        compose.runOnUiThread {
            val dialog = ShadowDialog.getLatestDialog()
            val at = SystemClock.uptimeMillis()
            for (action in listOf(MotionEvent.ACTION_DOWN, MotionEvent.ACTION_UP)) {
                val event = MotionEvent.obtain(at, at, action, -50f, -50f, 0)
                dialog.onTouchEvent(event)
                event.recycle()
            }
        }
        counts(installed = 0, cancelled = 1)
    }

    @Test
    fun `on a TV Cancel has the focus when the sheet opens, and Install comes second`() {
        show(Form.Tv, keyboard = true)
        compose.onNodeWithText(InstallSheetCopy.CANCEL).assertIsFocused()
        val cancel = compose.onNodeWithText(InstallSheetCopy.CANCEL).getUnclippedBoundsInRoot()
        val install = compose.onNodeWithText(InstallSheetCopy.INSTALL).getUnclippedBoundsInRoot()
        assertTrue(cancel.right <= install.left, "$cancel before $install")
        assertEquals(cancel.top, install.top)
        counts(installed = 0, cancelled = 0)
    }

    @Test
    fun `the focus goes back to the control that opened the sheet`() {
        show(Form.Tv, keyboard = true)
        armed()
        // As a remote does: select on the focused Cancel (a tap would put both windows in touch mode).
        compose.onNodeWithText(InstallSheetCopy.CANCEL).performKeyInput { pressKey(Key.Enter) }
        counts(installed = 0, cancelled = 1)
        compose.onNodeWithText("Open").assertIsFocused()
    }
}
