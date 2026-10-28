package app.marquee.design

import android.view.Gravity
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.input.InputMode
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalInputModeManager
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider

/**
 * A question before an action that cannot be taken back in one press (plans/addon-install-sheet.md; design/DECISION.md,
 * Six digits and Confirmation): [title], its [lines], and two actions. Phone: a page sheet from the foot (grabber,
 * radius 10 top corners, the page dimmed), [confirm] in `keyFill` blue over a plain [cancel], both pinned low. TV: a
 * centered card, [cancel] first and focused as the white primary, [confirm] second.
 *
 * Its own window, so it is modal for touch, keys and TalkBack; Back and a tap outside are [onCancel]. Focus moves in
 * on open, and back to the control that had it when the window closes. The caller closes it in both actions.
 */
@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun ConfirmSheet(
    title: String,
    lines: List<String>,
    confirm: String,
    cancel: String,
    onConfirm: () -> Unit,
    onCancel: () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    // A dialog is its own window: it does not inherit the TV canvas density (as ActionMenu).
    val density = LocalDensity.current
    val first = remember { FocusRequester() }
    Dialog(onDismissRequest = onCancel, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        val window = (LocalView.current.parent as? DialogWindowProvider)?.window
        SideEffect { window?.setGravity(if (tv) Gravity.CENTER else Gravity.BOTTOM) }
        // The page behind keeps its own count of focused buttons; this window starts with none.
        CompositionLocalProvider(
            LocalDensity provides density,
            LocalButtonFocus provides remember { mutableIntStateOf(0) },
        ) {
            val colors = Marquee.colors
            val shape =
                if (tv) RoundedCornerShape(Marquee.radius.l) else RoundedCornerShape(topStart = 10.dp, topEnd = 10.dp)
            Column(
                Modifier
                    .then(if (tv) Modifier.widthIn(max = 760.dp) else Modifier)
                    .fillMaxWidth()
                    .clip(shape)
                    .background(colors.surface)
                    .semantics { paneTitle = title }
                    .padding(horizontal = if (tv) 48.dp else 20.dp, vertical = if (tv) 44.dp else 8.dp),
                verticalArrangement = Arrangement.spacedBy(if (tv) Marquee.space.m else Marquee.space.s),
            ) {
                if (!tv) {
                    Box(
                        Modifier
                            .align(Alignment.CenterHorizontally)
                            .size(36.dp, 5.dp)
                            .clip(RoundedCornerShape(Marquee.radius.pill))
                            .background(colors.control)
                            .clearAndSetSemantics { },
                    )
                }
                // A long name or a large font scrolls the words; the actions stay pinned under them.
                Column(
                    Modifier
                        .weight(1f, fill = false)
                        .verticalScroll(rememberScrollState())
                        .padding(top = if (tv) 0.dp else Marquee.space.s),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
                ) {
                    MqText(
                        title,
                        modifier = Modifier.semantics { heading() },
                        role = if (tv) TextRole.Title2 else TextRole.Title3,
                        weight = FontWeight.Bold,
                    )
                    for (line in lines) MqText(line, role = TextRole.Callout, color = colors.muted)
                }
                if (tv) {
                    Row(
                        Modifier.padding(top = Marquee.space.s),
                        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
                    ) {
                        PrimaryButton(cancel, onCancel, focusRequester = first)
                        SecondaryButton(confirm, onConfirm)
                    }
                } else {
                    Column(
                        Modifier.padding(top = Marquee.space.s, bottom = 12.dp),
                        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                    ) {
                        KeyButton(confirm, onConfirm, Modifier.fillMaxWidth(), focusRequester = first)
                        SecondaryButton(cancel, onCancel, Modifier.fillMaxWidth())
                    }
                }
            }
            // The window is attached a frame or two after its first composition; a touch screen has no focus to place.
            // A TV is never in touch mode, and the new window is told so, as the page's own window was.
            val modes = LocalInputModeManager.current
            LaunchedEffect(first) {
                if (tv) modes.requestInputMode(InputMode.Keyboard)
                focusSoon(first, SHEET_FOCUS_FRAMES)
            }
        }
    }
}

private const val SHEET_FOCUS_FRAMES = 60
