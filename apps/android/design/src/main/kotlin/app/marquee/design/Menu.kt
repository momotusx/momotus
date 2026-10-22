package app.marquee.design

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.BringIntoViewSpec
import androidx.compose.foundation.gestures.LocalBringIntoViewSpec
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties

/** One row of an [ActionMenu]; a choice of a [PullDown] has no icon unless it is the chosen one. */
data class MenuAction(
    val label: String,
    val icon: ImageVector?,
    val onClick: () -> Unit,
)

private val selectKeys = setOf(Key.DirectionCenter, Key.Enter, Key.NumPadEnter)

/**
 * The menu a press and hold (touch) or a held select (D-pad) opens on a list row: the row's name and
 * a few labeled actions. On TV focus starts on action [start]; Back closes it and focus returns to the row.
 *
 * A dialog is its own window, so it does not inherit the TV canvas density: [LocalDensity] is passed in.
 * The held select that opened it keeps sending key repeats and finally a key up into the new window; those
 * are swallowed, or the first action would fire by itself when the key is let go.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun ActionMenu(
    title: String,
    actions: List<MenuAction>,
    onDismiss: () -> Unit,
    start: Int = 0,
) {
    val tv = Marquee.form == Form.Tv
    val pad = if (tv) Marquee.space.m else Marquee.space.s
    val density = LocalDensity.current
    val first = remember { FocusRequester() }
    // True from a select key going down inside this window (not a repeat of the press that opened it).
    var ownPress by remember { mutableStateOf(false) }
    // Room for a focused row to rise and cast its shadow at either end of the scroll, as a lane has.
    val room = Marquee.tiles.pad
    val spec = remember(density, room) { RoomSpec(with(density) { room.toPx() }) }
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        CompositionLocalProvider(LocalDensity provides density, LocalBringIntoViewSpec provides spec) {
            Column(
                Modifier
                    .onPreviewKeyEvent { event ->
                        if (event.key !in selectKeys) return@onPreviewKeyEvent false
                        when (event.type) {
                            KeyEventType.KeyDown -> {
                                if (event.nativeKeyEvent.repeatCount == 0) ownPress = true
                                !ownPress
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
                    }.padding(horizontal = pad)
                    .widthIn(max = if (tv) 560.dp else 420.dp)
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(Marquee.radius.l))
                    .background(Marquee.colors.surface)
                    // A pull-down of thirty genres scrolls; focus brings its row into view with [room] to spare.
                    .verticalScroll(rememberScrollState())
                    .padding(horizontal = pad, vertical = maxOf(pad, room)),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
            ) {
                MqText(title, role = TextRole.Title3, maxLines = 2)
                actions.forEachIndexed { index, action ->
                    SecondaryButton(
                        label = action.label,
                        icon = action.icon,
                        onClick = {
                            onDismiss()
                            action.onClick()
                        },
                        modifier = Modifier.fillMaxWidth(),
                        focusRequester = if (index == start) first else null,
                    )
                }
            }
        }
    }
    // The window is attached a frame or two after the first composition. A touch screen has no focus to place.
    LaunchedEffect(first, tv) {
        if (tv) focusSoon(first, MENU_FOCUS_FRAMES)
    }
}

private const val MENU_FOCUS_FRAMES = 60

/** Scrolls a focused row into view [room] pixels clear of either end, so its rise and shadow stay whole. */
@OptIn(ExperimentalFoundationApi::class)
private class RoomSpec(
    private val room: Float,
) : BringIntoViewSpec {
    override fun calculateScrollDistance(
        offset: Float,
        size: Float,
        containerSize: Float,
    ): Float =
        when {
            offset < room -> offset - room
            offset + size + room > containerSize -> offset + size + room - containerSize
            else -> 0f
        }
}

/**
 * A pull-down (design/DECISION.md, Grids): the chosen value and a chevron on the secondary pill, opening an
 * [ActionMenu] of the choices named by [title] with the chosen one checked. A choice of more than four takes
 * it on TV in place of a segmented control.
 */
@Composable
fun <T> PullDown(
    title: String,
    choices: List<Pair<T, String>>,
    selected: T,
    modifier: Modifier = Modifier,
    /** A quieter word after a choice's name (a catalog's addon). */
    detail: (T) -> String? = { null },
    onSelect: (T) -> Unit,
) {
    var open by remember { mutableStateOf(false) }
    val chosen = choices.firstOrNull { it.first == selected }?.second ?: title
    SecondaryButton(chosen, {
        open = true
    }, modifier, icon = Icons.ChevronDown, iconAfter = true, spoken = "$title, $chosen")
    if (open) {
        ActionMenu(
            title,
            choices.map { (value, name) ->
                MenuAction(
                    listOfNotNull(name, detail(value)).joinToString(" · "),
                    Icons.Check.takeIf {
                        value ==
                            selected
                    },
                ) {
                    onSelect(value)
                }
            },
            onDismiss = { open = false },
            start = choices.indexOfFirst { it.first == selected }.coerceAtLeast(0),
        )
    }
}
