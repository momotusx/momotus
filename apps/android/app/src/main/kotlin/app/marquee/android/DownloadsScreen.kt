package app.marquee.android

import android.os.StatFs
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.DownloadCopy
import app.marquee.core.DownloadItem
import app.marquee.core.Format
import app.marquee.core.client.CoreLog
import app.marquee.core.model.DownloadRow
import app.marquee.core.model.DownloadState
import app.marquee.core.model.DownloadStorage
import app.marquee.core.session.CoreSession
import app.marquee.core.session.DownloadAction
import app.marquee.core.session.DownloadTarget
import app.marquee.core.views.DownloadsView
import app.marquee.core.views.ViewStatus
import app.marquee.design.Chip
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.RemoteImage
import app.marquee.design.SecondaryButton
import app.marquee.design.Spinner
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusNote
import app.marquee.design.TextRole
import app.marquee.design.focusClickable
import app.marquee.ui.phone.DownloadButton
import app.marquee.ui.phone.DownloadMenu
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

class DownloadsHolder(
    session: CoreSession,
) : ViewModel() {
    val view: DownloadsView = session.downloads()

    override fun onCleared() = view.close()
}

@Composable
internal fun downloadsView(session: CoreSession): DownloadsView =
    viewModel<DownloadsHolder>(
        key = "downloads",
        factory = viewModelFactory { initializer { DownloadsHolder(session) } },
    ).view

/** What a Downloads page does with a row besides its ops: play it, or pick another version for it. */
class DownloadsNav(
    val onPlay: (DownloadRow) -> Unit,
    val onVersion: (DownloadRow) -> Unit,
    val onSeries: (itemId: String) -> Unit,
)

/**
 * This device's downloads (plans/offline-downloads.md 3.4): the storage line, what is under way with Pause all, then
 * the kept titles, newest first; a series opens its page. A kids profile sees its allowed titles with Play only.
 */
@Composable
fun DownloadsScreen(
    session: CoreSession,
    actions: DownloadActions,
    nav: DownloadsNav,
    kids: Boolean,
) {
    val view = downloadsView(session)
    val snapshot = view.current()
    var menu by remember { mutableStateOf<DownloadRow?>(null) }
    menu?.let { row -> RowMenu(row, actions, nav) { menu = null } }
    val onControl: ((DownloadRow) -> Unit)? = if (kids) null else ({ menu = it })
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = pagePadding(),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        item(key = "head") { BrowseHeader("Downloads") }
        if (snapshot == null) {
            // Never a blank page: opening, or why it did not (code review r1 minor 5).
            item(key = "state") { ViewState(view.status) }
            return@LazyColumn
        }
        if (!kids) item(key = "storage") { Storage(snapshot.storage) }
        actions.problem?.let { problem -> item(key = "problem") { StatusNote(problem, tone = Status.Warn) } }
        if (snapshot.downloading.isEmpty() && snapshot.titles.isEmpty()) {
            item(key = "empty") {
                StateMessage(
                    DownloadCopy.EMPTY_TITLE,
                    DownloadCopy.EMPTY_BODY,
                    Modifier.padding(horizontal = Marquee.safe.side),
                    icon = Icons.ArrowDownToLine,
                )
            }
            return@LazyColumn
        }
        if (snapshot.downloading.isNotEmpty()) {
            item(key = "downloading") {
                Row(
                    Modifier.fillMaxWidth().padding(end = Marquee.safe.side, top = Marquee.space.s),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.weight(1f)) { Kicker("Downloading") }
                    if (!kids) {
                        val paused = snapshot.downloading.all { it.state == DownloadState.Paused }
                        SecondaryButton(
                            if (paused) "Resume all" else "Pause all",
                            {
                                actions.act(
                                    if (paused) DownloadAction.Resume else DownloadAction.Pause,
                                    DownloadTarget.All,
                                )
                            },
                        )
                    }
                }
            }
            items(snapshot.downloading, key = { "run:${it.id}" }) { row ->
                ListRow(
                    row.still ?: row.poster,
                    wide = row.still != null,
                    title = rowName(row, " · "),
                    line = DownloadCopy.progressLine(row),
                    onClick = { nav.onPlay(row) },
                    control =
                        onControl?.let { open ->
                            {
                                DownloadButton(
                                    DownloadCopy.control(DownloadCopy.badgeOf(row)),
                                    { open(row) },
                                    subject = rowName(row),
                                )
                            }
                        },
                )
            }
        }
        if (snapshot.titles.isNotEmpty()) item(key = "kept") { Kicker("Downloaded") }
        items(snapshot.titles, key = { "title:${it.itemId}" }) { title ->
            val film = title.kind == "movie"
            val row = title.rows.firstOrNull()
            ListRow(
                title.poster,
                wide = false,
                title = title.name,
                line = DownloadCopy.titleLine(title),
                extra = row?.takeIf { film }?.let(DownloadCopy::subtitlesLine),
                onClick = { if (film && row != null) nav.onPlay(row) else nav.onSeries(title.itemId) },
                control =
                    when {
                        !film -> {
                            { Chevron() }
                        }

                        row != null && onControl != null -> {
                            {
                                DownloadButton(
                                    DownloadCopy.control(DownloadCopy.badgeOf(row)),
                                    { onControl(row) },
                                    subject = title.name,
                                )
                            }
                        }

                        else -> {
                            null
                        }
                    },
            )
        }
    }
}

/** One series' downloaded episodes by season, with Delete all (3.4). [onGone] once nothing of it is left. */
@Composable
fun DownloadedSeriesScreen(
    session: CoreSession,
    itemId: String,
    actions: DownloadActions,
    nav: DownloadsNav,
    kids: Boolean,
    onGone: () -> Unit,
) {
    val snapshot = downloadsView(session).current()
    var menu by remember { mutableStateOf<DownloadRow?>(null) }
    var confirm by remember { mutableStateOf(false) }
    menu?.let { row -> RowMenu(row, actions, nav) { menu = null } }
    val title = snapshot?.titles?.firstOrNull { it.itemId == itemId }
    val rows = (title?.rows.orEmpty() + snapshot?.downloading.orEmpty().filter { it.itemId == itemId })
    if (snapshot != null && rows.isEmpty()) {
        LaunchedEffect(Unit) { onGone() }
        return
    }
    val name = title?.name ?: rows.firstOrNull()?.name.orEmpty()
    val seasons = rows.sortedWith(compareBy({ it.season ?: 0 }, { it.episode ?: 0 })).groupBy { it.season ?: 0 }
    LazyColumn(
        Modifier.fillMaxSize(),
        contentPadding = pagePadding(),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        item(key = "head") { BrowseHeader(name) }
        if (!kids && rows.isNotEmpty()) {
            item(key = "delete") {
                Column(
                    Modifier.padding(horizontal = Marquee.safe.side),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                ) {
                    if (!confirm) {
                        SecondaryButton("Delete all", { confirm = true }, icon = Icons.Trash)
                    } else {
                        // DECISION's Confirmation pattern, inline: no destructive red.
                        MqText(DownloadCopy.deleteSeries(rows.map { it.state }, name), role = TextRole.Callout)
                        Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                            SecondaryButton("Delete", {
                                confirm = false
                                actions.act(DownloadAction.Remove, DownloadTarget.Title(itemId))
                            })
                            SecondaryButton("Cancel", { confirm = false })
                        }
                    }
                }
            }
        }
        for ((season, episodes) in seasons) {
            item(key = "season:$season") { Kicker(if (season == 0) "Specials" else "Season $season") }
            items(episodes, key = { it.id }) { row ->
                ListRow(
                    row.still ?: row.poster,
                    wide = row.still != null,
                    title = DownloadCopy.episodeName(row),
                    extra = DownloadCopy.subtitlesLine(row),
                    line =
                        if (row.state ==
                            DownloadState.Done
                        ) {
                            DownloadCopy.episodeLine(row)
                        } else {
                            DownloadCopy.progressLine(row)
                        },
                    onClick = { nav.onPlay(row) },
                    control =
                        if (kids) {
                            null
                        } else {
                            {
                                DownloadButton(
                                    DownloadCopy.control(DownloadCopy.badgeOf(row)),
                                    { menu = row },
                                    subject = DownloadCopy.episodeName(row),
                                )
                            }
                        },
                )
            }
        }
    }
}

/** "Severance S1:E4": a row's name, its menu's title and what its control says after its state (code review r2 n2). */
private fun rowName(
    row: DownloadRow,
    separator: String = " ",
): String =
    listOfNotNull(row.name, row.season?.let { s -> row.episode?.let { Format.episodeLabel(s, it) } })
        .joinToString(separator)

/** A row's menu: its state's items; Play and another version go where they lead. */
@Composable
private fun RowMenu(
    row: DownloadRow,
    actions: DownloadActions,
    nav: DownloadsNav,
    onDismiss: () -> Unit,
) {
    val badge = DownloadCopy.badgeOf(row)
    DownloadMenu(
        rowName(row),
        DownloadCopy.control(badge),
        onItem = { item ->
            when (item) {
                DownloadItem.Play -> nav.onPlay(row)
                DownloadItem.Another, DownloadItem.Choose -> nav.onVersion(row)
                else -> actions.item(item, badge)
            }
        },
        onDismiss = onDismiss,
    )
}

/** A Settings list row (V2 rollout section 8): the art, two lines, and the control or a chevron at the end. */
@Composable
private fun ListRow(
    art: String?,
    wide: Boolean,
    title: String,
    line: String,
    onClick: () -> Unit,
    control: (@Composable () -> Unit)?,
    extra: String? = null,
) {
    val colors = Marquee.colors
    val shape = RoundedCornerShape(Marquee.radius.l)
    Row(
        Modifier
            .padding(horizontal = Marquee.safe.side)
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surface)
            .focusClickable(onClick, Role.Button)
            .padding(12.dp),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.clip(RoundedCornerShape(Marquee.radius.s))) {
            if (wide) {
                RemoteImage(art, 96.dp, 54.dp, fallbackText = title)
            } else {
                RemoteImage(art, 48.dp, 72.dp, fallbackText = title)
            }
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            MqText(title, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 2)
            MqText(line, role = TextRole.Caption, color = colors.muted, maxLines = 2)
            extra?.let { MqText(it, role = TextRole.Caption, color = colors.muted, maxLines = 2) }
        }
        control?.invoke()
    }
}

@Composable
private fun Chevron() = MqIcon(Icons.ChevronRight, tint = Marquee.colors.muted)

/**
 * "Downloads use 12.4 GB · 48 GB free on this device" over a thin bar: downloads white, other used space 30 percent
 * white, free as the track; while a download waits for space, how much more it needs.
 */
@Composable
private fun Storage(storage: DownloadStorage) {
    val context = LocalContext.current
    val total = remember { runCatching { StatFs(context.filesDir.path).totalBytes }.getOrNull() }
    Column(
        Modifier.padding(horizontal = Marquee.safe.side),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        MqText(DownloadCopy.storage(storage), role = TextRole.Callout, color = Marquee.colors.muted)
        val free = storage.freeBytes
        if (total != null && total > 0 && free != null) {
            val other = (total - free - storage.downloadsBytes).coerceAtLeast(0)
            Row(
                Modifier
                    .fillMaxWidth()
                    .height(4.dp)
                    .clip(RoundedCornerShape(2.dp))
                    .background(Color.White.copy(alpha = 0.1f))
                    .semantics(mergeDescendants = true) {},
            ) {
                listOf(storage.downloadsBytes to Color.White, other to Color.White.copy(alpha = 0.3f))
                    .filter { it.first > 0 }
                    .forEach { (bytes, color) ->
                        Box(Modifier.weight(bytes.toFloat() / total).fillMaxHeight().background(color))
                    }
                val used = (storage.downloadsBytes + other).toFloat() / total
                if (used < 1f) Box(Modifier.weight(1f - used))
            }
        }
    }
}

/** Settings > Downloads (3.7): Wi-Fi only, Smart downloads, the storage line and Delete all. */
@Composable
internal fun DownloadsSettings(session: CoreSession) {
    val snapshot = downloadsView(session).current() ?: return
    val scope = rememberCoroutineScope()
    var confirm by remember { mutableStateOf(false) }

    fun send(op: suspend () -> Unit) {
        scope.launch {
            try {
                op()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                CoreLog.w("downloads setting: ${CoreLog.describe(e)}")
            }
        }
    }
    Section(0.dp, groupRadius()) {
        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
            val settings = snapshot.settings
            Chip(
                "Download on Wi-Fi only",
                selected = settings.wifiOnly,
                onClick = { send { session.ops.setDownloadSettings(wifiOnly = !settings.wifiOnly) } },
                switch = true,
            )
            Chip(
                "Smart downloads",
                selected = settings.smart,
                onClick = { send { session.ops.setDownloadSettings(smart = !settings.smart) } },
                switch = true,
            )
            MqText(DownloadCopy.SMART_LINE, role = TextRole.Caption, color = Marquee.colors.muted)
            MqText(DownloadCopy.storage(snapshot.storage), role = TextRole.Callout, color = Marquee.colors.muted)
            if (snapshot.storage.downloadsBytes > 0) {
                if (!confirm) {
                    SecondaryButton("Delete all downloads", { confirm = true }, icon = Icons.Trash)
                } else {
                    MqText(DownloadCopy.deleteAll(snapshot.storage.downloadsBytes), role = TextRole.Callout)
                    Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                        SecondaryButton("Delete", {
                            confirm = false
                            send { session.ops.downloadAct(DownloadAction.Remove, DownloadTarget.All) }
                        })
                        SecondaryButton("Cancel", { confirm = false })
                    }
                }
            }
        }
    }
}

/**
 * The shell's line on every phone page while offline (3.5): said once as it appears, and, when this profile has
 * downloads and the page is not theirs, one press from them.
 */
@Composable
internal fun OfflineBanner(onDownloads: (() -> Unit)?) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = Marquee.safe.side, vertical = Marquee.space.xs),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            Modifier.weight(1f).semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite },
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            MqIcon(Icons.WifiOff, tint = Marquee.colors.muted)
            MqText(
                if (onDownloads != null) DownloadCopy.OFFLINE else DownloadCopy.OFFLINE_ONLY,
                role = TextRole.Callout,
            )
        }
        if (onDownloads != null) {
            SecondaryButton(DownloadCopy.OFFLINE_ACTION, onDownloads, icon = Icons.ArrowDownToLine)
        }
    }
}

/** A Downloads page before its view answers: a spinner, or why it did not open (code review r1 minor 5). */
@Composable
private fun ViewState(status: ViewStatus) {
    if (status is ViewStatus.Failed) {
        StateMessage(
            "Downloads could not open",
            "Try again in a moment.",
            Modifier.padding(horizontal = Marquee.safe.side),
            icon = Icons.ArrowDownToLine,
        )
    } else {
        Box(Modifier.fillMaxWidth().padding(Marquee.space.l), contentAlignment = Alignment.Center) { Spinner() }
    }
}
