package app.marquee.android

import android.app.Application
import android.content.Context
import android.view.inputmethod.InputMethodManager
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
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
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.unit.dp
import app.marquee.design.Form
import app.marquee.design.MarqueeTheme
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** Search on a TV: the D-pad passes the field with the keyboard closed, and select opens it (the Fire TV walk). */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36], application = Application::class)
class SearchFieldTest {
    @get:Rule
    val compose = createComposeRule()

    private fun keyboardShown(): Boolean {
        val imm =
            RuntimeEnvironment.getApplication().getSystemService(Context.INPUT_METHOD_SERVICE) as InputMethodManager
        return shadowOf(imm).isSoftInputVisible
    }

    @Test
    fun `on a TV Down onto Search leaves the keyboard closed, and select opens it`() {
        compose.setContent {
            MarqueeTheme(Form.Tv) {
                var text by remember { mutableStateOf("") }
                Column {
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .height(48.dp)
                            .semantics { contentDescription = "Nav" }
                            .focusable(),
                    )
                    SearchField(text) { text = it }
                }
            }
        }
        val nav = compose.onNodeWithContentDescription("Nav")
        nav.requestFocus()
        nav.performKeyInput { pressKey(Key.DirectionDown) }
        compose.waitForIdle()
        val field = compose.onNodeWithContentDescription("Search titles")
        field.assertIsFocused()
        assertFalse(keyboardShown(), "the keyboard opened as focus passed")
        field.performKeyInput { pressKey(Key.DirectionCenter) }
        compose.waitForIdle()
        assertTrue(keyboardShown(), "select did not open the keyboard")
    }

    @Test
    fun `on a phone the clear button empties the field and keeps the cursor in it`() {
        var text by mutableStateOf("sintel")
        compose.setContent { MarqueeTheme(Form.Phone) { SearchField(text) { text = it } } }
        compose.onNodeWithContentDescription("Clear search").performClick()
        compose.waitForIdle()
        assertEquals("", text)
        compose.onNodeWithContentDescription("Clear search").assertDoesNotExist()
        compose.onNodeWithContentDescription("Search titles").assertIsFocused()
    }

    @Test
    fun `on a TV there is no clear button, the remote deletes with its keys`() {
        compose.setContent { MarqueeTheme(Form.Tv) { SearchField("sintel") {} } }
        compose.onNodeWithContentDescription("Clear search").assertDoesNotExist()
    }
}
