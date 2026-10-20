package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.editableText
import androidx.compose.ui.semantics.isEditable
import androidx.compose.ui.semantics.password
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setText
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp

/**
 * A one-line field on a raised pill whose edge turns accent with focus (design/DECISION.md v2).
 * [label] is its spoken name, [placeholder] shows while it is empty; Up and Down leave it on a remote, and
 * select opens the keyboard there ([FieldInput]).
 */
@Composable
fun MqTextField(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    placeholder: String,
    modifier: Modifier = Modifier,
    keyboardOptions: KeyboardOptions = KeyboardOptions.Default,
    keyboardActions: KeyboardActions = KeyboardActions.Default,
    visualTransformation: VisualTransformation = VisualTransformation.None,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Marquee.radius.pill)
    Box(
        modifier
            .fillMaxWidth()
            .height(if (tv) 72.dp else 48.dp)
            // The shared ring, halo and lift, driven by the text input's own focus: never color alone.
            .marqueeFocus(Marquee.radius.pill, Tokens.FOCUS_SCALE_TILE, forced = focused)
            .clip(shape)
            .background(colors.raised)
            .border(2.dp, if (focused) colors.accent else colors.control, shape)
            .padding(horizontal = if (tv) 24.dp else 16.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        FieldInput(
            value,
            onValueChange,
            label,
            placeholder,
            TextStyle(color = colors.text, fontSize = Marquee.type.callout, fontFamily = MarqueeFonts.text),
            onFocus = { focused = it },
            modifier = Modifier.fillMaxWidth(),
            keyboardOptions = keyboardOptions,
            keyboardActions = keyboardActions,
            visualTransformation = visualTransformation,
        )
    }
}

/**
 * The text inside a pill field (a [MqTextField], the Search field). On a TV it is a button until it is selected:
 * a TV's keyboard (Fire TV's) is a window over the page that takes the D-pad, so an editor that opened it as focus
 * passed would never let Down through. Leaving the editor makes it a button again, and Back keeps the focus on that
 * button. The button shows the text as
 * [visualTransformation] draws it (dots for a secret) and reads as an edit box that a screen reader can fill.
 * [onFocus] reports the focus of either, for the pill's ring.
 */
@Composable
fun FieldInput(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    placeholder: String,
    textStyle: TextStyle,
    onFocus: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    keyboardOptions: KeyboardOptions = KeyboardOptions.Default,
    keyboardActions: KeyboardActions = KeyboardActions.Default,
    visualTransformation: VisualTransformation = VisualTransformation.None,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    var editing by remember { mutableStateOf(!tv) }
    var entered by remember { mutableStateOf(false) }
    // Back left the editor: the button takes the focus once it is there.
    var backed by remember { mutableStateOf(false) }
    val input = remember { FocusRequester() }
    val button = remember { FocusRequester() }
    LaunchedEffect(editing) {
        if (!tv) return@LaunchedEffect
        if (editing) {
            runCatching { input.requestFocus() }
        } else if (backed) {
            backed = false
            runCatching { button.requestFocus() }
        }
    }
    if (!editing) {
        val shown = visualTransformation.filter(AnnotatedString(value)).text.text
        val secret = visualTransformation is PasswordVisualTransformation
        Box(modifier.fillMaxHeight(), contentAlignment = Alignment.CenterStart) {
            MqText(
                shown.ifEmpty { placeholder },
                color = if (value.isEmpty()) colors.dim else colors.text,
                maxLines = 1,
                // The button over it says it (masked, by name): the drawn text is not read twice.
                modifier = Modifier.clearAndSetSemantics { },
            )
            // The button has no child, so its label reaches TalkBack: Compose leaves out the description
            // of a node that merges children.
            Box(
                Modifier
                    .matchParentSize()
                    .onFocusChanged { onFocus(it.isFocused) }
                    .semantics {
                        contentDescription = label
                        editableText = AnnotatedString(shown)
                        isEditable = true
                        if (secret) password()
                        setText { typed ->
                            onValueChange(typed.text)
                            true
                        }
                    }.focusRequester(button)
                    .focusClickable({ editing = true }, role = null),
            )
        }
        return
    }
    BasicTextField(
        value = value,
        onValueChange = onValueChange,
        singleLine = true,
        textStyle = textStyle,
        cursorBrush = SolidColor(colors.accent),
        keyboardOptions = keyboardOptions,
        keyboardActions = keyboardActions,
        visualTransformation = visualTransformation,
        modifier =
            modifier
                .focusRequester(input)
                .onPreviewKeyEvent {
                    // On a TV, Back in the editor (a Fire TV keyboard closes on it and lets it through) makes the
                    // field a button again with the focus kept on it; unhandled, Compose would move the focus out
                    // of the page's section (FocusDirection.Exit), to the nav pill or a result.
                    if (!tv || it.key != Key.Back) return@onPreviewKeyEvent false
                    if (it.type == KeyEventType.KeyUp) {
                        backed = true
                        editing = false
                    }
                    true
                }.dpadExits()
                .onFocusChanged {
                    onFocus(it.isFocused)
                    // On a TV, leaving the editor (Up, Down, or Back closing the keyboard) makes it a button again.
                    if (it.isFocused) {
                        entered = true
                    } else if (tv && entered) {
                        entered = false
                        editing = false
                    }
                }.semantics { contentDescription = label },
        decorationBox = { inner ->
            Box(contentAlignment = Alignment.CenterStart) {
                if (value.isEmpty()) MqText(placeholder, color = colors.dim, maxLines = 1)
                inner()
            }
        },
    )
}
