package app.marquee.design

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties

/** One row of an [ActionMenu]. */
data class MenuAction(
    val label: String,
    val icon: ImageVector,
    val onClick: () -> Unit,
)

/**
 * The menu a press and hold (touch) or a held select (D-pad) opens on a list row: the row's name and
 * a few labeled actions. Focus starts on the first action; Back closes it and focus returns to the row.
 */
@Composable
fun ActionMenu(
    title: String,
    actions: List<MenuAction>,
    onDismiss: () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val first = remember { FocusRequester() }
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Column(
            Modifier
                .padding(horizontal = Marquee.space.m)
                .widthIn(max = if (tv) 560.dp else 420.dp)
                .fillMaxWidth()
                .clip(RoundedCornerShape(Marquee.radius.l))
                .background(Marquee.colors.surface)
                .padding(Marquee.space.m),
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
                    focusRequester = if (index == 0) first else null,
                )
            }
        }
    }
    // The window is attached a frame or two after the first composition.
    LaunchedEffect(first) {
        repeat(MENU_FOCUS_FRAMES) {
            withFrameNanos { }
            if (runCatching { first.requestFocus() }.getOrDefault(false)) return@LaunchedEffect
        }
    }
}

private const val MENU_FOCUS_FRAMES = 60
