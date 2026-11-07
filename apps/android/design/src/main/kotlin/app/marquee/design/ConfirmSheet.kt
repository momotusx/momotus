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
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.input.InputMode
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
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
import kotlinx.coroutines.delay

/**
 * A question before an action that cannot be taken back in one press (plans/addon-install-sheet.md; design/DECISION.md,
 * Six digits and Confirmation): [title], its [lines], and two actions. Phone: a page sheet from the foot (grabber,
 * radius 10 top corners, the page dimmed, at most 640 dp wide), [confirm] in `keyFill` blue over a plain [cancel], both
 * pinned low. TV: a centered card, [cancel] first and focused as the white primary, [confirm] second.
 *
 * Its own window, so it is modal for touch, keys and TalkBack; Back and a tap outside are [onCancel]. Focus moves in on
 * open and goes back to [opener] once the window is gone. The press that opened it never answers it: the buttons take
 * a press only [ARM_MS] after the sheet shows, a select key held down or let go from before is swallowed, and only the
 * first answer counts. The caller closes it in both actions.
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
    /** The control that opened the sheet: it takes the focus back when the sheet closes. */
    opener: FocusRequester? = null,
    /** A line shown only behind [detailsLabel] (where an addon comes from: a raw host is never on screen at once). */
    details: String? = null,
    detailsLabel: String = "Details",
) {
    val tv = Marquee.form == Form.Tv
    // A dialog is its own window: it does not inherit the TV canvas density (as ActionMenu).
    val density = LocalDensity.current
    val page = LocalView.current
    val first = remember { FocusRequester() }
    var armed by remember { mutableStateOf(false) }
    var answered by remember { mutableStateOf(false) }
    val answer = { action: () -> Unit, press: Boolean ->
        if (!answered && (armed || !press)) {
            answered = true
            action()
        }
    }
    // After the dialog window is gone (posted: it goes in this same frame), so the page's window holds the focus again.
    DisposableEffect(opener) { onDispose { opener?.let { o -> page.post { runCatching { o.requestFocus() } } } } }
    Dialog(
        onDismissRequest = { answer(onCancel, false) },
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        // Set in the sheet's first composition, before its first frame is drawn.
        val window = (LocalView.current.parent as? DialogWindowProvider)?.window
        SideEffect { window?.setGravity(if (tv) Gravity.CENTER else Gravity.BOTTOM or Gravity.CENTER_HORIZONTAL) }
        // The page behind keeps its own count of focused buttons; this window starts with none.
        CompositionLocalProvider(
            LocalDensity provides density,
            LocalButtonFocus provides remember { mutableIntStateOf(0) },
        ) {
            val colors = Marquee.colors
            val shape =
                if (tv) RoundedCornerShape(TV_RADIUS) else RoundedCornerShape(topStart = 10.dp, topEnd = 10.dp)
            // True from a select key going down in this window, not a repeat of a press from before.
            var ownPress by remember { mutableStateOf(false) }
            Column(
                Modifier
                    .onPreviewKeyEvent { e ->
                        if (e.key !in SELECT_KEYS) return@onPreviewKeyEvent false
                        when (e.type) {
                            KeyEventType.KeyDown -> {
                                val repeat = e.nativeKeyEvent.repeatCount > 0
                                if (!repeat) ownPress = true
                                repeat
                            }

                            KeyEventType.KeyUp -> {
                                val foreign = !ownPress
                                ownPress = false
                                foreign
                            }

                            else -> {
                                false
                            }
                        }
                    }.widthIn(max = if (tv) 760.dp else 640.dp)
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
                    if (details != null) {
                        var open by remember { mutableStateOf(false) }
                        SecondaryButton(detailsLabel, { open = !open }, selected = open)
                        if (open) MqText(details, role = TextRole.Callout, color = colors.muted)
                    }
                }
                if (tv) {
                    Row(
                        Modifier.padding(top = Marquee.space.s),
                        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
                    ) {
                        PrimaryButton(cancel, { answer(onCancel, true) }, focusRequester = first)
                        SecondaryButton(confirm, { answer(onConfirm, true) })
                    }
                } else {
                    Column(
                        Modifier.padding(top = Marquee.space.s, bottom = 12.dp),
                        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                    ) {
                        KeyButton(confirm, { answer(onConfirm, true) }, Modifier.fillMaxWidth(), focusRequester = first)
                        SecondaryButton(cancel, { answer(onCancel, true) }, Modifier.fillMaxWidth())
                    }
                }
            }
            // The window is attached a frame or two after its first composition. A TV is never in touch mode, and the
            // new window is told so, as the page's own window was; a touch screen has no focus to place.
            val modes = LocalInputModeManager.current
            LaunchedEffect(first) {
                if (tv) modes.requestInputMode(InputMode.Keyboard)
                if (modes.inputMode == InputMode.Keyboard) focusSoon(first, SHEET_FOCUS_FRAMES)
            }
        }
    }
    LaunchedEffect(Unit) {
        delay(ARM_MS)
        armed = true
    }
}

/** How long after the sheet shows its buttons take a press (the web's 400 ms). */
const val ARM_MS = 400L

private const val SHEET_FOCUS_FRAMES = 60

/** DECISION.md, Radii: a TV card. */
private val TV_RADIUS = 24.dp

private val SELECT_KEYS = setOf(Key.DirectionCenter, Key.Enter, Key.NumPadEnter)
