package app.marquee.ui.phone

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import app.marquee.core.DownloadControl
import app.marquee.core.DownloadGlyph
import app.marquee.core.DownloadItem
import app.marquee.core.GroupAct
import app.marquee.core.model.DownloadBadge
import app.marquee.design.ActionMenu
import app.marquee.design.Icons
import app.marquee.design.MenuAction
import app.marquee.design.RoundButton

/**
 * What a title page's download controls do (plans/offline-downloads.md 3.2); a page without one (a kids profile, a
 * TV) shows no control and no menu items.
 */
interface TitleDownloads {
    /** [item] for the video [videoId], whose download is [badge] (null: none yet). */
    fun item(
        item: DownloadItem,
        videoId: String,
        badge: DownloadBadge?,
    )

    /** A series menu's item; [ids] are the selected season's downloads, for its delete. */
    fun group(
        act: GroupAct,
        season: Int?,
        ids: List<String>,
    )
}

val DownloadGlyph.icon: ImageVector
    get() =
        when (this) {
            DownloadGlyph.Download -> Icons.ArrowDownToLine
            DownloadGlyph.Stop -> Icons.Square
            DownloadGlyph.Pause -> Icons.Pause
            DownloadGlyph.WifiOff -> Icons.WifiOff
            DownloadGlyph.Clock -> Icons.Clock
            DownloadGlyph.Check -> Icons.CircleCheck
            DownloadGlyph.Alert -> Icons.CircleAlert
        }

val DownloadItem.icon: ImageVector
    get() =
        when (this) {
            DownloadItem.Start, DownloadItem.Again -> Icons.ArrowDownToLine
            DownloadItem.Choose, DownloadItem.Another -> Icons.Layers
            DownloadItem.Pause -> Icons.Pause
            DownloadItem.Resume, DownloadItem.Play -> Icons.Play
            DownloadItem.Cancel -> Icons.X
            DownloadItem.Delete, DownloadItem.Remove -> Icons.Trash
            DownloadItem.Retry -> Icons.RotateCw
            DownloadItem.MobileData -> Icons.Smartphone
        }

/**
 * The control (3.1) as a round button: its glyph in a ring while a download is under way, its state's word under it
 * with [showLabel]. A tap starts a download or opens its menu ([onClick]); a press and hold offers [onLongClick].
 */
@Composable
fun DownloadButton(
    control: DownloadControl,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    showLabel: Boolean = false,
    onLongClick: (() -> Unit)? = null,
) {
    RoundButton(
        control.glyph.icon,
        control.label,
        onClick,
        modifier,
        showLabel = showLabel,
        spoken = control.spoken,
        ring = control.ring,
        value = control.value,
        clickLabel = control.hint,
        onLongClick = onLongClick,
        longClickLabel = onLongClick?.let { "More options" },
    )
}

/** A download's menu: the reason in full under [title], then its items. */
@Composable
fun DownloadMenu(
    title: String,
    control: DownloadControl,
    onItem: (DownloadItem) -> Unit,
    onDismiss: () -> Unit,
    items: List<DownloadItem> = control.items,
) {
    ActionMenu(
        title = title,
        actions = items.map { MenuAction(it.label, it.icon) { onItem(it) } },
        onDismiss = onDismiss,
        detail = control.detail,
    )
}
