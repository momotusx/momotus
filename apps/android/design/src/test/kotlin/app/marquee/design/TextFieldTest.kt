package app.marquee.design

import android.content.Context
import android.view.View
import android.view.accessibility.AccessibilityNodeInfo
import android.view.inputmethod.InputMethodManager
import androidx.activity.OnBackPressedDispatcher
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
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
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.test.ExperimentalTestApi
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.assertCountEquals
import androidx.compose.ui.test.assertIsFocused
import androidx.compose.ui.test.hasContentDescription
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performKeyInput
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.pressKey
import androidx.compose.ui.test.requestFocus
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
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

/** A field on a TV opens the keyboard on select, never as focus passes over it (the Fire TV walk). */
@OptIn(ExperimentalTestApi::class)
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class TextFieldTest {
    @get:Rule
    val compose = createComposeRule()

    private lateinit var view: View
    private var back: OnBackPressedDispatcher? = null

    /** What TalkBack is handed for the field's node. */
    private fun nodeInfo(): AccessibilityNodeInfo {
        val id = compose.onNodeWithContentDescription("Addon link").fetchSemanticsNode().id
        var info: AccessibilityNodeInfo? = null
        compose.runOnIdle { info = view.accessibilityNodeProvider.createAccessibilityNodeInfo(id) }
        return checkNotNull(info)
    }

    private fun keyboardShown(): Boolean {
        val imm =
            RuntimeEnvironment.getApplication().getSystemService(
                Context.INPUT_METHOD_SERVICE,
            ) as InputMethodManager
        return shadowOf(imm).isSoftInputVisible
    }

    private fun field(
        form: Form,
        start: String = "",
        mask: VisualTransformation = VisualTransformation.None,
    ) {
        compose.setContent {
            view = LocalView.current
            back = LocalOnBackPressedDispatcherOwner.current?.onBackPressedDispatcher
            MarqueeTheme(form) {
                var text by remember { mutableStateOf(start) }
                Column {
                    // What sits over and under the field on a page: Down and Up move the focus through it.
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .height(48.dp)
                            .semantics { contentDescription = "Above" }
                            .focusable(),
                    )
                    MqTextField(
                        text,
                        { text = it },
                        label = "Addon link",
                        placeholder = "https://",
                        visualTransformation = mask,
                    )
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .height(48.dp)
                            .semantics { contentDescription = "Below" }
                            .focusable(),
                    )
                }
            }
        }
    }

    /** Down from Above onto the field, then select: the editor has the focus. */
    private fun selectField() {
        val above = compose.onNodeWithContentDescription("Above")
        above.requestFocus()
        above.performKeyInput { pressKey(Key.DirectionDown) }
        compose.onNodeWithContentDescription("Addon link").performKeyInput { pressKey(Key.DirectionCenter) }
        compose.waitForIdle()
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
    fun `on a TV Back in the editor makes it a button again and keeps the focus on it`() {
        // The Fire TV walk: Back closed the keyboard and the focus left for the nav pill or the first result.
        field(Form.Tv)
        selectField()
        assertTrue(keyboardShown())
        compose.onNodeWithContentDescription("Addon link").performKeyInput { pressKey(Key.Back) }
        compose.waitForIdle()
        val node = compose.onNodeWithContentDescription("Addon link")
        node.assertIsFocused()
        // The button, not the editor: it has no keyboard action.
        node.assert(SemanticsMatcher.keyNotDefined(SemanticsActions.OnImeAction))
        assertFalse(keyboardShown(), "Back left the keyboard open")
        node.performKeyInput { pressKey(Key.DirectionCenter) }
        compose.waitForIdle()
        assertTrue(keyboardShown(), "select did not open the keyboard again")
    }

    @Test
    fun `on a TV TalkBack is handed an edit box named by the field's label`() {
        field(Form.Tv, start = "kit")
        val info = nodeInfo()
        assertEquals("android.widget.EditText", info.className)
        assertTrue(info.isEditable, "TalkBack would not call it an edit box")
        assertEquals("Addon link", info.contentDescription?.toString())
        assertEquals("kit", info.text?.toString())
    }

    @Test
    fun `on a TV TalkBack is handed a secret as a password, never its text`() {
        field(Form.Tv, start = "SECRET-KIT", mask = PasswordVisualTransformation())
        val info = nodeInfo()
        assertTrue(info.isPassword)
        assertEquals("Addon link", info.contentDescription?.toString())
        assertFalse(info.text?.contains("SECRET") == true, "the secret reached TalkBack: ${info.text}")
    }

    @Test
    fun `on a TV a back event in the editor makes it a button again and keeps the focus on it`() {
        // Android 16 and later send predictive back as a back event, not KEYCODE_BACK: the BackHandler takes it.
        field(Form.Tv)
        selectField()
        assertTrue(keyboardShown())
        compose.runOnIdle { checkNotNull(back).onBackPressed() }
        compose.waitForIdle()
        val node = compose.onNodeWithContentDescription("Addon link")
        node.assertIsFocused()
        node.assert(SemanticsMatcher.keyNotDefined(SemanticsActions.OnImeAction))
        assertFalse(keyboardShown(), "Back left the keyboard open")
    }

    /**
     * The name TalkBack reads for the editor. Its node always has a child (its clip), so Compose drops its own
     * description and TalkBack names it from its children, as it does a button: the label is such a child.
     */
    private fun editorLabel(): String? {
        val label = compose.onNode(hasContentDescription("Addon link"), useUnmergedTree = true).fetchSemanticsNode()
        val editor = checkNotNull(label.parent)
        assertTrue(SemanticsProperties.EditableText in editor.config, "the label is not the editor's child")
        var spoken: CharSequence? = null
        compose.runOnIdle {
            spoken =
                view.accessibilityNodeProvider.createAccessibilityNodeInfo(label.id)?.contentDescription
        }
        return spoken?.toString()
    }

    @Test
    fun `on a phone TalkBack hears the label of an empty field`() {
        field(Form.Phone)
        val info = nodeInfo()
        assertEquals("android.widget.EditText", info.className)
        assertTrue(info.isEditable)
        assertEquals("Addon link", editorLabel())
    }

    @Test
    fun `on a TV TalkBack hears the label of an empty field's editor`() {
        field(Form.Tv)
        selectField()
        assertTrue(nodeInfo().isEditable)
        assertEquals("Addon link", editorLabel())
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

    @Test
    fun `on a TV leaving the field makes it a button again, so the next pass leaves the keyboard closed`() {
        field(Form.Tv)
        selectField()
        assertTrue(keyboardShown())
        compose.onNodeWithContentDescription("Addon link").performKeyInput { pressKey(Key.DirectionDown) }
        compose.waitForIdle()
        compose.onNodeWithContentDescription("Below").assertIsFocused()
        assertFalse(keyboardShown(), "the keyboard stayed open after the field was left")
        // Back up through it: a button again, which keeps the keyboard closed.
        compose.onNodeWithContentDescription("Below").performKeyInput { pressKey(Key.DirectionUp) }
        compose.waitForIdle()
        compose.onNodeWithContentDescription("Addon link").assertIsFocused()
        assertFalse(keyboardShown(), "the second pass opened the keyboard")
    }

    @Test
    fun `on a TV a secret shows as dots and is never read out`() {
        field(Form.Tv, start = "SECRET-KIT", mask = PasswordVisualTransformation())
        compose.onAllNodesWithText("SECRET-KIT", substring = true, useUnmergedTree = true).assertCountEquals(0)
        val node = compose.onNodeWithContentDescription("Addon link")
        node.assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.Password))
        node.assert(
            SemanticsMatcher.expectValue(SemanticsProperties.EditableText, AnnotatedString("\u2022".repeat(10))),
        )
    }

    @Test
    fun `on a TV the button reads as an edit box a screen reader can fill`() {
        field(Form.Tv)
        val node = compose.onNodeWithContentDescription("Addon link")
        node.assert(SemanticsMatcher.keyIsDefined(SemanticsProperties.EditableText))
        node.assert(SemanticsMatcher.keyNotDefined(SemanticsProperties.Role))
        node.performSemanticsAction(
            SemanticsActions.SetText,
        ) { it(AnnotatedString("https://example.org/manifest.json")) }
        node.assert(
            SemanticsMatcher.expectValue(
                SemanticsProperties.EditableText,
                AnnotatedString("https://example.org/manifest.json"),
            ),
        )
    }
}
