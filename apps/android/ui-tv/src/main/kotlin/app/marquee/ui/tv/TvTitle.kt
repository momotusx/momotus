@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.tv

import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.unit.dp
import app.marquee.core.Format
import app.marquee.core.TitleCopy
import app.marquee.core.VersionsCopy
import app.marquee.core.model.Episode
import app.marquee.core.model.TitleState
import app.marquee.core.model.VersionsSnapshot
import app.marquee.core.views.TitleView
import app.marquee.design.Backdrop
import app.marquee.design.Chip
import app.marquee.design.Icons
import app.marquee.design.Logo
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.PosterTile
import app.marquee.design.PrimaryButton
import app.marquee.design.SecondaryButton
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.TextRole
import app.marquee.design.TileShape
import app.marquee.design.heightOf

private const val PLAY = "play"
private const val EPISODES = "episodes"
private const val SEASONS = "seasons"
private const val ACTIONS = "actions"
private const val RETRY = "retry"

/**
 * A title on TV: artwork behind the facts, then Play (with the version it will play under it),
 * Versions, the library and watched toggles, and for a series the seasons and the episodes of
 * the selected one. Focus starts on Play and comes back to what it was on after Versions.
 *
 * @param onPlay Play was chosen with the recommended version [key] of [videoId].
 * @param onVersions the Versions screen for [videoId] was asked for (also what Play does while no
 * version is known yet).
 * @param onEpisode an episode was chosen: its versions.
 */
@Composable
fun TvTitle(
    title: TitleView,
    versions: VersionsSnapshot?,
    focus: FocusMemory,
    onPlay: (key: String, videoId: String) -> Unit,
    onVersions: (videoId: String) -> Unit,
    onEpisode: (Episode) -> Unit,
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
        Box(
            Modifier
                .fillMaxSize()
                .background(
                    Brush.horizontalGradient(
                        0f to scrim,
                        0.55f to scrim.copy(alpha = 0.88f),
                        1f to scrim.copy(alpha = 0.3f),
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
                        Box(
                            Modifier.padding(horizontal = safe.side),
                        ) { Info(title, versions, versionsFailed, focus, onPlay, onVersions) }
                    }
                    if (header.seasons.isNotEmpty()) item(key = "seasons") { Seasons(title, focus) }
                    if (title.episodes.isNotEmpty()) item(key = "episodes") { Episodes(title, focus, onEpisode) }
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
) {
    val header = checkNotNull(title.header)
    val details = checkNotNull(header.details)
    val colors = Marquee.colors
    val target = header.playTarget
    val recommended = versions?.recommended
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        Logo(details.logo, details.name, height = 160.dp, maxWidth = 640.dp)
        MqText(TitleCopy.meta(details), role = TextRole.Callout, color = colors.muted, maxLines = 1)
        details.description?.let {
            MqText(
                it,
                // As wide as the hero's text column: the scrim is dark enough for it only that far.
                modifier = Modifier.widthIn(max = 820.dp),
                role = TextRole.Body,
                maxLines = 5,
            )
        }
        Row(
            Modifier
                .padding(top = Marquee.space.xs)
                .focusProperties { enter = { focus.rowEntry(ACTIONS) ?: FocusRequester.Default } }
                .focusGroup(),
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
        ) {
            PrimaryButton(
                label = target?.let(TitleCopy::playLabel) ?: "Play",
                icon = Icons.Play,
                enabled = target != null,
                onClick = {
                    when {
                        target != null && recommended != null -> onPlay(recommended.key, target.videoId)
                        target != null -> onVersions(target.videoId)
                    }
                },
                focusRequester = focus.requesterFor(PLAY),
                onFocusChange = focus.watch(ACTIONS, PLAY),
            )
            if (target != null) {
                SecondaryButton(
                    label = "Versions",
                    icon = Icons.Layers,
                    onClick = { onVersions(target.videoId) },
                    focusRequester = focus.requesterFor("versions"),
                    onFocusChange = focus.watch(ACTIONS, "versions"),
                )
            }
            SecondaryButton(
                label = if (header.inLibrary) "In library" else "Add to library",
                icon = if (header.inLibrary) Icons.BookmarkCheck else Icons.BookmarkPlus,
                onClick = { title.setInLibrary(!header.inLibrary) },
                focusRequester = focus.requesterFor("library"),
                onFocusChange = focus.watch(ACTIONS, "library"),
            )
            if (details.kind == "movie") {
                SecondaryButton(
                    label = if (header.watched) "Watched" else "Mark watched",
                    icon = if (header.watched) Icons.Check else Icons.Eye,
                    onClick = { title.setWatched(details.id, !header.watched) },
                    focusRequester = focus.requesterFor("watched"),
                    onFocusChange = focus.watch(ACTIONS, "watched"),
                )
            }
        }
        MqText(
            VersionsCopy.chosen(versions, versionsFailed),
            role = TextRole.Callout,
            color = colors.muted,
            maxLines = 1,
        )
    }
}

@Composable
private fun Seasons(
    title: TitleView,
    focus: FocusMemory,
) {
    val header = checkNotNull(title.header)
    LazyRow(
        modifier =
            Modifier
                .focusProperties { enter = { focus.rowEntry(SEASONS) ?: FocusRequester.Default } }
                .focusGroup(),
        contentPadding = PaddingValues(horizontal = Marquee.safe.side, vertical = Marquee.space.xs),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        items(header.seasons, key = { it }) { season ->
            val key = "season:$season"
            Chip(
                label = Format.seasonName(season),
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
) {
    val metrics = Marquee.tiles
    val laneHeight = metrics.pad * 2 + metrics.heightOf(TileShape.Landscape) + metrics.caption
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
                PosterTile(
                    title = episode.title.ifBlank { Format.episodeLabel(episode.season, episode.episode) },
                    imageUrl = episode.thumbnail,
                    onClick = { onEpisode(episode) },
                    modifier = Modifier.padding(top = metrics.pad),
                    shape = TileShape.Landscape,
                    progress = episode.progress,
                    caption = TitleCopy.episode(episode),
                    focusRequester = focus.requesterFor(key),
                    onFocusChange = focus.watch(EPISODES, key),
                    onLongClick = { title.setWatched(episode.videoId, !episode.watched) },
                    longClickLabel = if (episode.watched) "Mark as not watched" else "Mark as watched",
                )
            }
        }
        MqText(
            "Hold select on an episode to mark it watched or not watched.",
            role = TextRole.Callout,
            color = Marquee.colors.muted,
            modifier = Modifier.padding(horizontal = Marquee.safe.side),
        )
    }
}
