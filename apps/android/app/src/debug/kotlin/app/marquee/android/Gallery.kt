package app.marquee.android

import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.unit.dp
import app.marquee.core.client.Trace
import app.marquee.core.model.HomeRow
import app.marquee.core.model.HomeUpdate
import app.marquee.core.model.ListDiff
import app.marquee.core.model.Segment
import app.marquee.core.model.SegmentKind
import app.marquee.core.model.Tile
import app.marquee.design.Chip
import app.marquee.design.ChosenVersion
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MarqueeTheme
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.Palette
import app.marquee.design.PlayFilled
import app.marquee.design.PosterTile
import app.marquee.design.PrimaryButton
import app.marquee.design.RoundButton
import app.marquee.design.RowHeader
import app.marquee.design.SecondaryButton
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusLine
import app.marquee.design.StatusNote
import app.marquee.design.TextRole
import app.marquee.design.TileShape
import app.marquee.design.laneHeight
import app.marquee.player.SkipButton

/** Debug-only screen that shows every design component in every state. `intent extras: form`. */
@Composable
fun GalleryHost(form: Form) {
    MarqueeTheme(form) { Gallery() }
}

@Composable
private fun Gallery() {
    val context = androidx.compose.ui.platform.LocalContext.current
    val tiles = remember { realTiles(context) }
    val tv = Marquee.form == Form.Tv
    val safe = Marquee.safe
    Column(
        Modifier
            .then(if (tv) Modifier else Modifier.safeDrawingPadding())
            .fillMaxSize()
            .semantics { testTagsAsResourceId = true }
            .verticalScroll(rememberScrollState())
            .padding(start = safe.side, end = safe.side, top = safe.top, bottom = safe.bottom),
        verticalArrangement = Arrangement.spacedBy(if (tv) 40.dp else 24.dp),
    ) {
        MqText("Momotus design system", role = TextRole.Title1)
        MqText(
            "${if (tv) "TV canvas, points on 1920x1080" else "Phone, dp"}. Real titles and posters from Cinemeta.",
            role = TextRole.Callout,
            color = Marquee.colors.muted,
        )

        Section("Type scale") {
            for (role in TextRole.entries) {
                MqText(
                    "${role.name}: The quiet lobby of the cinema",
                    role = role,
                    maxLines = 1,
                )
            }
            // Titles outside Latin fall back to the system font without clipping.
            MqText("Title2: Ayla, Dağların Kızı · 千と千尋の神隠し · Ёлки", role = TextRole.Title2, maxLines = 1)
        }

        Section("Buttons") {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(20.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                PrimaryButton("Play", {}, icon = PlayFilled)
                PrimaryButton("Resume from 41:12", {}, icon = PlayFilled, big = true)
                SecondaryButton("Versions", {}, icon = Icons.Layers)
                SecondaryButton("Add to Library", {}, icon = Icons.BookmarkPlus)
                SecondaryButton("In Library", {}, icon = Icons.BookmarkCheck, selected = true)
                SecondaryButton("Mark as watched", {}, icon = Icons.Check)
                SecondaryButton("Watched", {}, icon = Icons.Check, selected = true)
                SecondaryButton("Unavailable", {}, enabled = false)
                RoundButton(Icons.Info, "Details", {}, showLabel = true)
                RoundButton(Icons.Film, "Trailer", {}, showLabel = true)
                RoundButton(Icons.BookmarkCheck, "In Library", {}, showLabel = true, selected = true)
            }
        }

        Section("Under the primary action: the chosen version, a wait, and why nothing plays") {
            Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
                ChosenVersion(
                    facts = listOf("1080p", "1.6 GB"),
                    status = "Torrent, 10 seeds",
                    tone = Status.Warn,
                    spoken = "1080p, 1.6 GB, Torrent, 10 seeds",
                )
                ChosenVersion(
                    facts = listOf("4K", "Dolby Vision", "18 GB"),
                    status = "Ready",
                    tone = Status.Ok,
                    spoken = "4K, Dolby Vision, 18 GB, Ready",
                )
                StatusNote("Finding versions", busy = true)
                StatusNote("No source has this yet", tone = Status.Warn)
            }
        }

        Section("Skip button (shown while a skippable part plays)") {
            SkipButton(Segment(SegmentKind.Intro, 30_000, 75_000, "TheIntroDB"), {})
        }

        Section("Chips") {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Chip("All", selected = true, onClick = {})
                Chip("Movies", selected = false, onClick = {})
                Chip("Series", selected = false, onClick = {})
            }
        }

        Section("Status") {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(32.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                StatusLine(Status.Ok, "Network")
                StatusLine(Status.Warn, "Display")
                StatusLine(Status.Bad, "Sources")
                StatusLine(Status.Idle, "Audio")
            }
        }

        Section("Tiles (the third is focused, the fourth is in progress, the last has no picture)") {
            RowHeader("Popular Movies", "Cinemeta")
            Row(
                Modifier.height(Marquee.tiles.laneHeight(TileShape.Poster)),
                horizontalArrangement = Arrangement.spacedBy(Marquee.tiles.gap),
            ) {
                tiles.take(5).forEachIndexed { index, tile ->
                    PosterTile(
                        title = tile.name,
                        imageUrl = if (index == 4) null else tile.poster,
                        onClick = {},
                        modifier = Modifier.padding(top = Marquee.tiles.pad),
                        progress = if (index == 3) 0.4f else null,
                        caption = if (index == 3) "S1:E2 · 36 min left" else tile.releaseInfo,
                        ratings =
                            app.marquee.core.TileCopy
                                .ratings(tile.ratings),
                        forceFocus = index == 2,
                    )
                }
            }
        }

        Section("Continue Watching card (a real title with sample progress)") {
            tiles.firstOrNull { it.background != null }?.let { tile ->
                Row(Modifier.height(Marquee.tiles.laneHeight(TileShape.Landscape))) {
                    PosterTile(
                        title = tile.name,
                        imageUrl = tile.background,
                        onClick = {},
                        modifier = Modifier.padding(top = Marquee.tiles.pad),
                        shape = TileShape.Landscape,
                        progress = 0.4f,
                        caption = "S1:E2 · 36 min left",
                    )
                }
            }
        }

        Section("Explained failure and loading") {
            StateMessage(
                title = "Popular Movies could not load",
                body = "Cinemeta did not answer in time. Check the network, then run the setup check.",
                icon = Icons.CircleAlert,
                actionLabel = "Run setup check",
                onAction = {},
            )
            Row(horizontalArrangement = Arrangement.spacedBy(Marquee.tiles.gap)) {
                repeat(4) {
                    Skeleton(Modifier.size(Marquee.tiles.poster.width, Marquee.tiles.poster.height))
                }
            }
        }

        Section("Icons (Lucide, always beside a label)") {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(28.dp),
                verticalArrangement = Arrangement.spacedBy(20.dp),
            ) {
                for ((name, icon) in iconList) IconCell(name, icon)
            }
        }

        Section("Palette") {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                for ((name, color) in swatches(Marquee.colors)) Swatch(name, color)
            }
        }
    }
}

@Composable
private fun Section(
    title: String,
    content: @Composable () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        MqText(title, role = TextRole.Caption, color = Marquee.colors.muted)
        content()
    }
}

@Composable
private fun IconCell(
    name: String,
    icon: ImageVector,
) {
    Column(
        Modifier.width(
            if (Marquee.form ==
                Form.Tv
            ) {
                150.dp
            } else {
                84.dp
            },
        ),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        MqIcon(icon)
        MqText(name, role = TextRole.Caption, color = Marquee.colors.muted, maxLines = 1)
    }
}

@Composable
private fun Swatch(
    name: String,
    color: Color,
) {
    val tv = Marquee.form == Form.Tv
    val shape = RoundedCornerShape(Marquee.radius.s)
    Column(Modifier.width(if (tv) 160.dp else 88.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Box(
            Modifier
                .fillMaxWidth()
                .height(if (tv) 64.dp else 40.dp)
                .border(1.dp, Marquee.colors.muted.copy(alpha = 0.4f), shape)
                .background(color, shape),
        )
        MqText(name, role = TextRole.Caption, color = Marquee.colors.muted, maxLines = 1)
    }
}

private fun swatches(p: Palette): List<Pair<String, Color>> =
    listOf(
        "bg" to p.bg,
        "surface" to p.surface,
        "raised" to p.raised,
        "line" to p.line,
        "text" to p.text,
        "muted" to p.muted,
        "accent" to p.accent,
        "ok" to p.ok,
        "warn" to p.warn,
        "bad" to p.bad,
    )

private val iconList: List<Pair<String, ImageVector>> by lazy {
    listOf(
        "search" to Icons.Search,
        "house" to Icons.House,
        "compass" to Icons.Compass,
        "library" to Icons.LibraryBig,
        "settings" to Icons.Settings,
        "play" to Icons.Play,
        "pause" to Icons.Pause,
        "skip-back" to Icons.SkipBack,
        "skip-forward" to Icons.SkipForward,
        "captions" to Icons.Captions,
        "languages" to Icons.Languages,
        "volume" to Icons.Volume2,
        "gauge" to Icons.Gauge,
        "layers" to Icons.Layers,
        "bookmark" to Icons.Bookmark,
        "check" to Icons.Check,
        "eye" to Icons.Eye,
        "alert" to Icons.CircleAlert,
        "wrench" to Icons.Wrench,
        "qr-code" to Icons.QrCode,
        "key" to Icons.KeyRound,
        "link" to Icons.Link,
        "refresh" to Icons.RefreshCw,
        "wifi-off" to Icons.WifiOff,
        "monitor" to Icons.Monitor,
        "info" to Icons.Info,
        "trash" to Icons.Trash,
        "puzzle" to Icons.Puzzle,
    )
}

/** Tiles from the recorded Home trace (debug builds ship it as an asset): real Cinemeta titles. */
private fun realTiles(context: Context): List<Tile> {
    val text =
        runCatching {
            context.assets
                .open("traces/home.jsonl")
                .bufferedReader()
                .use { it.readText() }
        }.getOrNull()
            ?: return emptyList()
    val json = app.marquee.core.client.CoreJson
    val rows = ArrayList<HomeRow>()
    for (line in Trace.parse(text)) {
        val updates = line.msg["updates"] as? kotlinx.serialization.json.JsonArray ?: continue
        for (element in updates) {
            val update = json.decodeFromJsonElement(HomeUpdate.serializer(), element)
            if (update !is HomeUpdate.Rows) continue
            for (diff in update.diffs) {
                when (diff) {
                    is ListDiff.Replace -> rows.add(diff.value)
                    is ListDiff.Insert -> rows.add(diff.value)
                    is ListDiff.PushBack -> rows.add(diff.value)
                    else -> Unit
                }
            }
        }
    }
    return rows.flatMap { it.tiles }.filter { it.poster != null }.distinctBy { it.id }
}
