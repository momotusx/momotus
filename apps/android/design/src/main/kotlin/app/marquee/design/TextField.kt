package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp

/**
 * A one-line field on a raised pill whose edge turns accent with focus (design/DECISION.md v2).
 * [label] is its spoken name, [placeholder] shows while it is empty; Up and Down leave it on a remote.
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
        BasicTextField(
            value = value,
            onValueChange = onValueChange,
            singleLine = true,
            textStyle = TextStyle(color = colors.text, fontSize = Marquee.type.callout, fontFamily = MarqueeFonts.text),
            cursorBrush = SolidColor(colors.accent),
            keyboardOptions = keyboardOptions,
            keyboardActions = keyboardActions,
            visualTransformation = visualTransformation,
            modifier =
                Modifier
                    .fillMaxWidth()
                    .dpadExits()
                    .onFocusChanged { focused = it.isFocused }
                    .semantics { contentDescription = label },
            decorationBox = { inner ->
                if (value.isEmpty()) MqText(placeholder, color = colors.dim, maxLines = 1)
                inner()
            },
        )
    }
}
