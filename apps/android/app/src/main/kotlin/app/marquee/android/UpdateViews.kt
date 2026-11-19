package app.marquee.android

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import app.marquee.core.UpdateCopy
import app.marquee.design.Icons
import app.marquee.design.LiveSpoken
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.TextRole
import app.marquee.design.focusClickable
import app.marquee.design.marqueeFocus

/**
 * About's update row (plans/auto-update.md section 5): one action at a time, its state in the line under it. A wait
 * (checking, downloading) is a line the screen reader says, with nothing to press.
 */
@Composable
internal fun UpdateRow() {
    val updater = (LocalContext.current.applicationContext as MarqueeApp).updater ?: return
    when (val state = updater.state) {
        Updater.State.Idle -> {
            SettingsRow(Icons.RefreshCw, Tone.Grey, UpdateCopy.CHECK, "", updater::check)
        }

        Updater.State.Current -> {
            SettingsRow(Icons.RefreshCw, Tone.Grey, UpdateCopy.CHECK, UpdateCopy.CURRENT, updater::check)
        }

        Updater.State.Failed -> {
            SettingsRow(Icons.RefreshCw, Tone.Grey, UpdateCopy.CHECK, UpdateCopy.FAILED, updater::check)
        }

        Updater.State.Checking -> {
            Waiting(UpdateCopy.CHECKING)
        }

        is Updater.State.Downloading -> {
            Waiting(UpdateCopy.downloading(state.version, state.percent))
        }

        is Updater.State.Ready -> {
            if (state.silent) {
                SettingsRow(
                    Icons.Download,
                    Tone.Grey,
                    UpdateCopy.INSTALL_NOW,
                    UpdateCopy.leaving(state.version),
                    updater::install,
                )
            } else {
                SettingsRow(
                    Icons.Download,
                    Tone.Grey,
                    UpdateCopy.INSTALL,
                    UpdateCopy.ready(state.version),
                    updater::install,
                )
            }
        }
    }
}

@Composable
private fun Waiting(words: String) {
    LiveSpoken(words) { MqText(words, role = TextRole.Body, color = Marquee.colors.muted) }
}

/**
 * Home's one quiet line while an update waits for a press (plans/auto-update.md section 5): no button of its own;
 * selecting it opens Settings, About, where Install is. Shown again on every visit until it is installed.
 */
@Composable
internal fun UpdateNote(
    version: String,
    onOpen: () -> Unit,
) {
    val shape = RoundedCornerShape(Marquee.radius.card)
    Box(
        Modifier
            .padding(horizontal = Marquee.safe.side)
            .widthIn(max = 900.dp)
            .marqueeFocus(Marquee.radius.card)
            .clip(shape)
            .focusClickable(onOpen, Role.Button),
    ) {
        Callout(Icons.Download, UpdateCopy.home(version))
    }
}
