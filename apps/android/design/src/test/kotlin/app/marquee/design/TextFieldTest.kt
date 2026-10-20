package app.marquee.design

import android.content.Context
import android.view.inputmethod.InputMethodManager
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.unit.dp
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** A field on a TV opens the keyboard on select, never as focus passes over it (the Fire TV walk). */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class TextFieldTest {
    @get:Rule
    val compose = createComposeRule()

    private fun keyboardShown(): Boolean {
        val imm =
            RuntimeEnvironment.getApplication().getSystemService(
                Context.INPUT_METHOD_SERVICE,
            ) as InputMethodManager
        return shadowOf(imm).isSoftInputVisible
    }

    private fun field(form: Form) {
        compose.setContent {
            MarqueeTheme(form) {
                var text by remember { mutableStateOf("") }
                Column {
                    // What sits over the field on a page: Down from it moves the focus into the field.
                    Box(Modifier.size(48.dp).semantics { contentDescription = "Above" }.focusable())
                    MqTextField(text, { text = it }, label = "Addon link", placeholder = "https://")
                }
            }
        }
    }

    @Test
    fun `on a TV focus leaves the keyboard closed, and select opens it`() {
        field(Form.Tv)
        val above = compose.onNodeWithContentDescription("Above")
        above.requestFocus()
        above.performKeyInput { pressKey(Key.DirectionDown) }
        compose.waitForIdle()
        val node = compose.onNodeWithContentDescription("Addon link")
        node.assertIsFocused()
        assertFalse(keyboardShown(), "the keyboard opened as focus passed")
        node.performKeyInput { pressKey(Key.DirectionCenter) }
        compose.waitForIdle()
        compose.onNodeWithContentDescription("Addon link").assertIsFocused()
        assertTrue(keyboardShown(), "select did not open the keyboard")
    }

    @Test
    fun `on a phone focus opens the keyboard as before`() {
        field(Form.Phone)
        val above = compose.onNodeWithContentDescription("Above")
        above.requestFocus()
        above.performKeyInput { pressKey(Key.DirectionDown) }
        compose.waitForIdle()
        compose.onNodeWithContentDescription("Addon link").assertIsFocused()
        assertTrue(keyboardShown())
    }
}
