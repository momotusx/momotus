package app.marquee.ui.phone

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.marquee.core.Format
import app.marquee.core.PrimaryAction
import app.marquee.core.TitleCopy
import app.marquee.core.VersionLine
import app.marquee.core.VersionsCopy
import app.marquee.core.model.Episode
import app.marquee.core.model.TitleState
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.views.TitleView
import app.marquee.design.ActionMenu
import app.marquee.design.Backdrop
import app.marquee.design.Chip
import app.marquee.design.ChosenVersion
import app.marquee.design.Icons
import app.marquee.design.LightStatusBarIcons
import app.marquee.design.Logo
import app.marquee.design.Marquee
import app.marquee.design.MenuAction
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.PrimaryButton
import app.marquee.design.ProgressBar
import app.marquee.design.RemoteImage
import app.marquee.design.SecondaryButton
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusNote
import app.marquee.design.TextRole
import app.marquee.design.Tokens
import app.marquee.design.focusClickable

/** How many lines of the synopsis show before "More". */
private const val SYNOPSIS_LINES = 3

/** The fade from the artwork into the page, over the bottom of the hero. */
private val HERO_FADE = 96.dp

/**
 * A title on a phone, top to bottom: artwork with the logo, the facts, the synopsis (three lines and
 * "More"), a full-width primary action with the version it will play under it, the library and
 * watched toggles, for a series the seasons and the episodes of the selected one (tap plays, press
 * and hold opens more), and last the details.
 *
 * @param onAddSource no addon gave a version: go where one is added.
 * @param onEpisode an episode was tapped: it plays.
 * @param onEpisodeVersions "Choose a version" for an episode, from its press-and-hold menu.
 */
@Composable
fun PhoneTitle(
    title: TitleView,
    versions: VersionsSnapshot?,
    onPlay: (key: String, videoId: String) -> Unit,
    onVersions: (videoId: String) -> Unit,
    onAddSource: () -> Unit,
    onEpisode: (Episode) -> Unit,
    onEpisodeVersions: (Episode) -> Unit,
    modifier: Modifier = Modifier,
    versionsFailed: Boolean = false,
) {
    val header = title.header
    val details = header?.details
    val state = header?.state
    val side = Marquee.safe.side
    when {
        state is TitleState.Failed -> {
            StateMessage(
                title = "This title could not load",
                body = state.reason,
                icon = Icons.CircleAlert,
                actionLabel = "Try again",
                onAction = title::retry,
                modifier =
                    modifier
                        .windowInsetsPadding(
                            WindowInsets.statusBars,
                        ).padding(horizontal = side, vertical = Marquee.space.xl),
            )
        }

        header == null || details == null -> {
            Column(
                modifier.fillMaxSize().padding(horizontal = side),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
            ) {
                Skeleton(Modifier.fillMaxWidth().aspectRatio(HERO_RATIO))
                Skeleton(Modifier.size(200.dp, 24.dp))
                Skeleton(Modifier.size(300.dp, 80.dp))
            }
        }

        else -> {
            val hasArt = details.background != null || details.poster != null
            var menuFor by remember { mutableStateOf<Episode?>(null) }
            if (hasArt) LightStatusBarIcons()
            menuFor?.let { episode ->
                ActionMenu(
                    title = TitleCopy.episode(episode).removePrefix("Watched, "),
                    actions =
                        listOf(
                            MenuAction("Choose a version", Icons.Layers) { onEpisodeVersions(episode) },
                            MenuAction(
                                if (episode.watched) "Mark as not watched" else "Mark as watched",
                                if (episode.watched) Icons.EyeOff else Icons.Check,
                            ) { title.setWatched(episode.videoId, !episode.watched) },
                        ),
                    onDismiss = { menuFor = null },
                )
            }
            LazyColumn(
                modifier = modifier.fillMaxSize(),
                contentPadding = PaddingValues(bottom = Marquee.space.xl),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
            ) {
                item(
                    key = "art",
                ) { Hero(details.background ?: details.poster, details.logo, details.name, hasArt, side) }
                item(key = "info") { Info(title, versions, versionsFailed, onPlay, onVersions, onAddSource) }
                if (header.seasons.isNotEmpty()) {
                    item(key = "seasons") {
                        val compact = TitleCopy.compactSeasons(header.seasons)
                        LazyRow(
                            contentPadding = PaddingValues(horizontal = side),
                            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            if (compact) {
                                item(
                                    key = "label",
                                ) { MqText("Season", role = TextRole.Callout, color = Marquee.colors.muted) }
                            }
                            items(TitleCopy.seasonOrder(header.seasons), key = { it }) { season ->
                                Chip(
                                    label = TitleCopy.seasonLabel(season, compact),
                                    selected = season == header.selectedSeason,
                                    onClick = { title.selectSeason(season) },
                                )
                            }
                        }
                    }
                }
                items(title.episodes, key = { it.videoId }) { episode ->
                    EpisodeRow(
                        episode,
                        details.runtime,
                        onClick = { onEpisode(episode) },
                        onMore = { menuFor = episode },
                    )
                }
                TitleCopy.facts(details).takeIf { it.isNotEmpty() }?.let { facts ->
                    item(key = "details") { Details(facts) }
                }
            }
        }
    }
}

private const val HERO_RATIO = 1.45f

/**
 * The artwork behind the logo, running under the status bar. A dark scrim holds the logo and the
 * bar icons; the last [HERO_FADE] fades into the page color, so there is no hard edge in either theme.
 * Without artwork there is no band: the logo (or the name) starts the page.
 */
@Composable
private fun Hero(
    art: String?,
    logo: String?,
    name: String,
    hasArt: Boolean,
    side: Dp,
) {
    if (!hasArt) {
        Logo(
            logo,
            name,
            height = 72.dp,
            maxWidth = 280.dp,
            role = TextRole.Title1,
            modifier =
                Modifier
                    .windowInsetsPadding(
                        WindowInsets.statusBars,
                    ).padding(horizontal = side, vertical = Marquee.space.s),
        )
        return
    }
    val page = Marquee.colors.bg
    Box(Modifier.fillMaxWidth().aspectRatio(HERO_RATIO)) {
        Backdrop(art, Modifier.matchParentSize())
        // Dark in both themes: the logo and the status bar icons are light.
        Box(
            Modifier
                .matchParentSize()
                .background(
                    Brush.verticalGradient(
                        0f to Tokens.dark.bg.copy(alpha = 0.5f),
                        0.25f to Color.Transparent,
                        0.5f to Color.Transparent,
                        1f to Tokens.dark.bg.copy(alpha = 0.6f),
                    ),
                ),
        )
        Box(
            Modifier
                .align(Alignment.BottomStart)
                .fillMaxWidth()
                .height(HERO_FADE)
                .background(Brush.verticalGradient(0f to Color.Transparent, 1f to page)),
        )
        Logo(
            logo,
            name,
            height = 72.dp,
            maxWidth = 280.dp,
            role = TextRole.Title1,
            color = Tokens.dark.text,
            modifier =
                Modifier
                    .align(
                        Alignment.BottomStart,
                    ).padding(start = side, bottom = HERO_FADE - Marquee.space.s),
        )
    }
}

@Composable
private fun Info(
    title: TitleView,
    versions: VersionsSnapshot?,
    versionsFailed: Boolean,
    onPlay: (String, String) -> Unit,
    onVersions: (String) -> Unit,
    onAddSource: () -> Unit,
) {
    val header = checkNotNull(title.header)
    val details = checkNotNull(header.details)
    val colors = Marquee.colors
    val target = header.playTarget
    val recommended = versions?.recommended
    val primary = TitleCopy.primary(target, versions, details.runtime)
    Column(
        Modifier.padding(horizontal = Marquee.safe.side),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        MqText(
            TitleCopy.meta(details, TitleCopy.seasonCount(header.seasons)),
            role = TextRole.Callout,
            color = colors.muted,
        )
        details.description?.let { Synopsis(it) }
        when (primary) {
            is PrimaryAction.Play -> {
                PrimaryButton(
                    label = primary.label,
                    icon = Icons.Play,
                    modifier = Modifier.fillMaxWidth(),
                    enabled = target != null,
                    onClick = {
                        when {
                            target != null && recommended != null -> onPlay(recommended.key, target.videoId)
                            target != null -> onVersions(target.videoId)
                        }
                    },
                )
            }

            PrimaryAction.ChooseVersion -> {
                PrimaryButton(
                    label = "Choose a version",
                    icon = Icons.Layers,
                    modifier = Modifier.fillMaxWidth(),
                    onClick = { target?.let { onVersions(it.videoId) } },
                )
            }

            PrimaryAction.AddSource -> {
                PrimaryButton(
                    label = "Add a source",
                    icon = Icons.Plus,
                    modifier = Modifier.fillMaxWidth(),
                    onClick = onAddSource,
                )
            }
        }
        if (target == null) {
            StatusNote("Nothing to play yet")
        } else {
            VersionStatus(VersionsCopy.line(versions, versionsFailed))
        }
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            // Versions is the primary action itself when nothing is ready, and has nothing to list when no addon answered.
            if (target != null && primary is PrimaryAction.Play) {
                SecondaryButton("Versions", icon = Icons.Layers, onClick = { onVersions(target.videoId) })
            }
            SecondaryButton(
                label = if (header.inLibrary) "In Library" else "Add to Library",
                icon = if (header.inLibrary) Icons.BookmarkCheck else Icons.BookmarkPlus,
                selected = header.inLibrary,
                onClick = { title.setInLibrary(!header.inLibrary) },
            )
            if (details.kind == "movie") {
                SecondaryButton(
                    label = if (header.watched) "Watched" else "Mark as watched",
                    icon = Icons.Check,
                    selected = header.watched,
                    onClick = { title.setWatched(details.id, !header.watched) },
                )
            }
        }
    }
}

/** The line under the primary action: a wait, the chosen version, or why nothing plays. */
@Composable
private fun VersionStatus(line: VersionLine) {
    when (line) {
        VersionLine.Finding -> {
            StatusNote("Finding versions", busy = true)
        }

        is VersionLine.Chosen -> {
            ChosenVersion(line.facts, line.status, if (line.good) Status.Ok else Status.Warn, line.via, line.spoken)
        }

        is VersionLine.Problem -> {
            StatusNote(line.text, tone = Status.Warn)
        }
    }
}

/** Three lines of the synopsis, and "More" when there is more to read. */
@Composable
private fun Synopsis(text: String) {
    var expanded by rememberSaveable(text) { mutableStateOf(false) }
    var cut by remember(text) { mutableStateOf(false) }
    Column {
        MqText(
            text,
            role = TextRole.Body,
            maxLines = if (expanded) Int.MAX_VALUE else SYNOPSIS_LINES,
            onTextLayout = { if (!expanded) cut = it.hasVisualOverflow },
        )
        if (cut || expanded) {
            Box(
                Modifier
                    .heightIn(min = 48.dp)
                    .focusClickable({ expanded = !expanded }, Role.Button),
                contentAlignment = Alignment.CenterStart,
            ) {
                MqText(
                    if (expanded) "Less" else "More",
                    role = TextRole.Callout,
                    weight = FontWeight.SemiBold,
                    color = Marquee.colors.accent,
                )
            }
        }
    }
}

/** Cast, directors, runtime and where the details came from: a label over its value. */
@Composable
private fun Details(facts: List<Pair<String, String>>) {
    Column(
        Modifier.padding(horizontal = Marquee.safe.side),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        for ((label, value) in facts) {
            Column {
                MqText(label, role = TextRole.Caption, color = Marquee.colors.muted)
                MqText(value, role = TextRole.Callout)
            }
        }
    }
}

@Composable
private fun EpisodeRow(
    episode: Episode,
    runtime: String?,
    onClick: () -> Unit,
    onMore: () -> Unit,
) {
    val colors = Marquee.colors
    val spoken = TitleCopy.episode(episode)
    Row(
        Modifier
            .fillMaxWidth()
            .semantics { contentDescription = spoken }
            .focusClickable(
                onClick,
                Role.Button,
                onLongClick = onMore,
                longClickLabel = "More options",
            ).padding(horizontal = Marquee.safe.side, vertical = Marquee.space.xs),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        Box(Modifier.clip(RoundedCornerShape(Marquee.radius.s))) {
            RemoteImage(
                episode.thumbnail,
                128.dp,
                72.dp,
                fallbackText = Format.episodeLabel(episode.season, episode.episode),
            ) {
                episode.progress?.let { ProgressBar(it, Modifier.align(Alignment.BottomStart)) }
            }
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            MqText(
                episode.title.ifBlank {
                    Format.episodeLabel(episode.season, episode.episode)
                },
                role = TextRole.Headline,
                maxLines = 2,
            )
            MqText(
                listOfNotNull(
                    Format.episodeLabel(episode.season, episode.episode),
                    TitleCopy.episodeNote(episode, runtime),
                ).joinToString(" · "),
                role = TextRole.Callout,
                color = colors.muted,
                maxLines = 1,
            )
            episode.overview?.let { MqText(it, role = TextRole.Caption, color = colors.muted, maxLines = 2) }
        }
        if (episode.watched) MqIcon(Icons.Check, tint = colors.muted)
    }
}
