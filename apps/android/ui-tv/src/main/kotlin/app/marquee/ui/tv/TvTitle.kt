@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.tv

import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
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
import app.marquee.design.EpisodeCard
import app.marquee.design.FactsTable
import app.marquee.design.Icons
import app.marquee.design.KeepInView
import app.marquee.design.Logo
import app.marquee.design.Marquee
import app.marquee.design.MenuAction
import app.marquee.design.MqText
import app.marquee.design.PlayFilled
import app.marquee.design.PosterTile
import app.marquee.design.PrimaryButton
import app.marquee.design.RatingLine
import app.marquee.design.RoundButton
import app.marquee.design.SecondaryButton
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusNote
import app.marquee.design.TextRole
import app.marquee.design.TileShape
import app.marquee.design.episodeLaneHeight
import app.marquee.design.laneHeight
import app.marquee.design.openTrailer

private const val PLAY = "play"
private const val EPISODES = "episodes"
private const val SEASONS = "seasons"
private const val ACTIONS = "actions"
private const val RETRY = "retry"

/**
 * A title on TV: artwork behind the facts, then the primary action (Play, with the version it will
 * play under it; "Add a source" when nothing can play), Versions, the library and watched toggles,
 * the details, and for a series the seasons and the episodes of the selected one. Focus starts on
 * the primary action and comes back to what it was on after Versions.
 *
 * @param onPlay Play was chosen with the recommended version [key] of [videoId].
 * @param onVersions the Versions screen for [videoId] was asked for (also what Play does while no
 * version is known yet).
 * @param onAddSource no addon gave a version: go where one is added.
 * @param onEpisode an episode was chosen: it plays.
 * @param onEpisodeVersions "Choose a version" for an episode, from its hold-select menu.
 */
@Composable
fun TvTitle(
    title: TitleView,
    versions: VersionsSnapshot?,
    focus: FocusMemory,
    onPlay: (key: String, videoId: String) -> Unit,
    onVersions: (videoId: String) -> Unit,
    onAddSource: () -> Unit,
    onEpisode: (Episode) -> Unit,
    onEpisodeVersions: (Episode) -> Unit,
    modifier: Modifier = Modifier,
    versionsFailed: Boolean = false,
) {
    val safe = Marquee.safe
    val header = title.header
    val details = header?.details
    val scrim = Marquee.colors.bg
    val failed = header?.state is TitleState.Failed
    // Focus goes to Play once the page has loaded (or to Try again when it could not), not before:
    // the buttons do not exist while the skeleton shows.
    LaunchedEffect(focus, details != null, failed) {
        if (failed) {
            focus.focusWhenReady(RETRY, waitMs = 10_000)
        } else if (details != null) {
            val wanted = focus.focusedKey?.takeIf { focus.requester(it) != null } ?: PLAY
            focus.focusWhenReady(wanted, waitMs = 10_000)
        }
    }

    Box(
        modifier.fillMaxSize().onPreviewKeyEvent { event ->
            if (event.type == KeyEventType.KeyDown) focus.interacted()
            false
        },
    ) {
        Backdrop(details?.background ?: details?.poster, Modifier.fillMaxSize())
        // The text column sits on the left half: the art stays clear on the right, and fades into the page below.
        Box(
            Modifier
                .fillMaxSize()
                .background(
                    Brush.horizontalGradient(
                        0f to scrim.copy(alpha = 0.94f),
                        0.5f to scrim.copy(alpha = 0.8f),
                        0.75f to scrim.copy(alpha = 0.35f),
                        1f to Color.Transparent,
                    ),
                ),
        )
        Box(Modifier.fillMaxSize().background(Brush.verticalGradient(0.6f to Color.Transparent, 1f to scrim)))

        val state = header?.state
        when {
            state is TitleState.Failed -> {
                StateMessage(
                    title = "This title could not load",
                    body = state.reason,
                    icon = Icons.CircleAlert,
                    actionLabel = "Try again",
                    onAction = title::retry,
                    actionFocus = focus.requesterFor(RETRY),
                    modifier = Modifier.padding(start = safe.side, top = safe.top),
                )
            }

            header == null || details == null -> {
                Column(
                    Modifier.padding(start = safe.side, top = safe.top).widthIn(max = 900.dp),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
                ) {
                    Skeleton(Modifier.size(520.dp, 120.dp))
                    Skeleton(Modifier.size(700.dp, 28.dp))
                    Skeleton(Modifier.size(900.dp, 100.dp))
                }
            }

            else -> {
                LazyColumn(
                    modifier = Modifier.fillMaxSize(),
                    // The lanes run to the screen edge (their own padding is the side margin), so the
                    // focus ring and halo of the first tile are not clipped.
                    contentPadding = PaddingValues(top = safe.top, bottom = safe.bottom),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
                ) {
                    item(key = "info") {
                        // The facts sit at the right, over the art, on strong glass.
                        Row(
                            Modifier.padding(horizontal = safe.side),
                            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xl),
                        ) {
                            Box(Modifier.weight(1f)) {
                                Info(title, versions, versionsFailed, focus, onPlay, onVersions, onAddSource)
                            }
                            FactsTable(
                                TitleCopy.facts(details, TitleCopy.seasonCount(header.seasons)),
                                Modifier.width(560.dp).padding(top = Marquee.space.xl),
                            )
                        }
                    }
                    if (header.seasons.isNotEmpty()) item(key = "seasons") { Seasons(title, focus) }
                    if (title.episodes.isNotEmpty()) {
                        item(key = "episodes") { Episodes(title, focus, onEpisode, onEpisodeVersions) }
                    }
                }
            }
        }
    }
}

@Composable
private fun Info(
    title: TitleView,
    versions: VersionsSnapshot?,
    versionsFailed: Boolean,
    focus: FocusMemory,
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
    val context = LocalContext.current
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        Logo(details.logo, details.name, height = 160.dp, maxWidth = 480.dp)
        RatingLine(
            details.rating,
            TitleCopy.meta(details, TitleCopy.seasonCount(header.seasons)),
            Modifier.widthIn(max = 760.dp),
        )
        details.description?.let {
            MqText(
                it,
                // Half the screen: the scrim is dark enough for text only that far.
                modifier = Modifier.widthIn(max = 640.dp),
                role = TextRole.Body,
                maxLines = 5,
            )
        }
        Row(
            Modifier
                .padding(top = Marquee.space.xs)
                .focusProperties { enter = { focus.rowEntry(ACTIONS) ?: FocusRequester.Default } }
                .focusGroup(),
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.m),
            verticalAlignment = Alignment.Top,
        ) {
            // One button for every state, so its node (and the focus on it) survives the addons answering:
            // Play turning into "Add a source" must not drop focus onto the page underneath.
            PrimaryButton(
                label = primary.label,
                icon =
                    when (primary) {
                        is PrimaryAction.Play -> PlayFilled
                        PrimaryAction.ChooseVersion -> Icons.Layers
                        PrimaryAction.AddSource -> Icons.Plus
                    },
                enabled = primary !is PrimaryAction.Play || target != null,
                onClick = {
                    when {
                        primary == PrimaryAction.AddSource -> onAddSource()
                        target == null -> Unit
                        primary == PrimaryAction.ChooseVersion || recommended == null -> onVersions(target.videoId)
                        else -> onPlay(recommended.key, target.videoId)
                    }
                },
                focusRequester = focus.requesterFor(PLAY),
                onFocusChange = focus.watch(ACTIONS, PLAY),
            )
            // Always there once there is something to play, so focus on it is never pulled out from under the viewer.
            if (target != null) {
                RoundButton(
                    icon = Icons.Layers,
                    label = "Versions",
                    onClick = { onVersions(target.videoId) },
                    showLabel = true,
                    focusRequester = focus.requesterFor("versions"),
                    onFocusChange = focus.watch(ACTIONS, "versions"),
                )
            }
            details.trailers.firstOrNull()?.let { trailer ->
                RoundButton(
                    icon = Icons.Film,
                    label = "Trailer",
                    onClick = { openTrailer(context, trailer.url) },
                    showLabel = true,
                    focusRequester = focus.requesterFor("trailer"),
                    onFocusChange = focus.watch(ACTIONS, "trailer"),
                )
            }
            RoundButton(
                icon = if (header.inLibrary) Icons.BookmarkCheck else Icons.BookmarkPlus,
                label = if (header.inLibrary) "In Library" else "Add to Library",
                onClick = { title.setInLibrary(!header.inLibrary) },
                selected = header.inLibrary,
                showLabel = true,
                focusRequester = focus.requesterFor("library"),
                onFocusChange = focus.watch(ACTIONS, "library"),
            )
            if (details.kind == "movie") {
                RoundButton(
                    icon = Icons.Check,
                    label = if (header.watched) "Watched" else "Mark as watched",
                    onClick = { title.setWatched(details.id, !header.watched) },
                    selected = header.watched,
                    showLabel = true,
                    focusRequester = focus.requesterFor("watched"),
                    onFocusChange = focus.watch(ACTIONS, "watched"),
                )
            }
        }
        if (target == null) {
            StatusNote("Nothing to play yet", Modifier.widthIn(max = 640.dp))
        } else {
            VersionStatus(VersionsCopy.line(versions, versionsFailed), Modifier.widthIn(max = 760.dp))
        }
    }
}

/** The line under the primary action: a wait, the chosen version, or why nothing plays. */
@Composable
private fun VersionStatus(
    line: VersionLine,
    modifier: Modifier,
) {
    when (line) {
        VersionLine.Finding -> {
            StatusNote("Finding versions", modifier, busy = true)
        }

        is VersionLine.Chosen -> {
            ChosenVersion(
                line.facts,
                line.status,
                if (line.good) Status.Ok else Status.Warn,
                line.via,
                line.spoken,
                modifier,
            )
        }

        is VersionLine.Problem -> {
            StatusNote(line.text, modifier, tone = Status.Warn)
        }
    }
}

@Composable
private fun Seasons(
    title: TitleView,
    focus: FocusMemory,
) {
    val header = checkNotNull(title.header)
    val compact = TitleCopy.compactSeasons(header.seasons)
    val order = TitleCopy.seasonOrder(header.seasons)
    val list = rememberLazyListState()
    KeepInView(list, order.indexOf(header.selectedSeason).let { if (it < 0 || !compact) it else it + 1 })
    LazyRow(
        state = list,
        modifier =
            Modifier
                // First entry lands on the selected season, which may be far along the row.
                .focusProperties {
                    enter = {
                        focus.rowEntry(SEASONS)
                            ?: focus.requester("season:${header.selectedSeason}")
                            ?: FocusRequester.Default
                    }
                }.focusGroup(),
        contentPadding = PaddingValues(horizontal = Marquee.safe.side, vertical = Marquee.space.xs),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        if (compact) item(key = "label") { MqText("Season", role = TextRole.Callout, color = Marquee.colors.muted) }
        items(order, key = { it }) { season ->
            val key = "season:$season"
            Chip(
                label = TitleCopy.seasonLabel(season, compact),
                selected = season == header.selectedSeason,
                onClick = { title.selectSeason(season) },
                focusRequester = focus.requesterFor(key),
                onFocusChange = focus.watch(SEASONS, key),
            )
        }
    }
}

@Composable
private fun Episodes(
    title: TitleView,
    focus: FocusMemory,
    onEpisode: (Episode) -> Unit,
    onVersions: (Episode) -> Unit,
) {
    val metrics = Marquee.tiles
    val laneHeight = metrics.episodeLaneHeight()
    val runtime = title.header?.details?.runtime
    var menuFor by remember { mutableStateOf<Episode?>(null) }
    menuFor?.let { episode ->
        ActionMenu(
            title = TitleCopy.episode(episode).removePrefix("Watched, "),
            actions =
                listOf(
                    MenuAction("Choose a version", Icons.Layers) { onVersions(episode) },
                    MenuAction(
                        if (episode.watched) "Mark as not watched" else "Mark as watched",
                        if (episode.watched) Icons.EyeOff else Icons.Check,
                    ) { title.setWatched(episode.videoId, !episode.watched) },
                ),
            onDismiss = { menuFor = null },
        )
    }
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        LazyRow(
            modifier =
                Modifier
                    .height(laneHeight)
                    .focusProperties { enter = { focus.rowEntry(EPISODES) ?: FocusRequester.Default } }
                    .focusGroup(),
            contentPadding = PaddingValues(horizontal = Marquee.safe.side),
            horizontalArrangement = Arrangement.spacedBy(metrics.gap),
        ) {
            items(title.episodes, key = { it.videoId }) { episode ->
                val key = "episode:${episode.videoId}"
                EpisodeCard(
                    number = "E${episode.episode}",
                    title = episode.title.ifBlank { Format.episodeLabel(episode.season, episode.episode) },
                    caption = TitleCopy.episodeCaption(episode, runtime),
                    overview = episode.overview,
                    imageUrl = episode.thumbnail,
                    watched = episode.watched,
                    progress = episode.progress,
                    spoken = TitleCopy.episode(episode),
                    onClick = { onEpisode(episode) },
                    modifier = Modifier.padding(top = metrics.pad),
                    focusRequester = focus.requesterFor(key),
                    onFocusChange = focus.watch(EPISODES, key),
                    onLongClick = { menuFor = episode },
                    longClickLabel = "More options",
                )
            }
        }
        MqText(
            "Hold select on an episode to choose a version or mark it watched.",
            role = TextRole.Callout,
            color = Marquee.colors.muted,
            modifier = Modifier.padding(horizontal = Marquee.safe.side),
        )
    }
}
