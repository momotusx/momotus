package app.marquee.ui.phone

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
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
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.PrimaryButton
import app.marquee.design.ProgressBar
import app.marquee.design.RemoteImage
import app.marquee.design.SecondaryButton
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.TextRole
import app.marquee.design.Tokens
import app.marquee.design.focusClickable

/**
 * A title on a phone: artwork with the logo, the facts, a full-width Play with the version it will
 * play under it, Versions, the library and watched toggles, and for a series the seasons and the
 * episodes of the selected one (press and hold an episode to mark it watched).
 */
@Composable
fun PhoneTitle(
    title: TitleView,
    versions: VersionsSnapshot?,
    onPlay: (key: String, videoId: String) -> Unit,
    onVersions: (videoId: String) -> Unit,
    onEpisode: (Episode) -> Unit,
    modifier: Modifier = Modifier,
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
                modifier = modifier.padding(horizontal = side, vertical = Marquee.space.xl),
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
            LazyColumn(
                modifier = modifier.fillMaxSize(),
                contentPadding = PaddingValues(bottom = Marquee.space.xl),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
            ) {
                item(key = "art") {
                    Box(Modifier.fillMaxWidth().aspectRatio(HERO_RATIO)) {
                        Backdrop(details.background ?: details.poster, Modifier.matchParentSize())
                        Box(
                            Modifier
                                .matchParentSize()
                                .background(Brush.verticalGradient(0.4f to Color.Transparent, 1f to Marquee.colors.bg)),
                        )
                        Logo(
                            details.logo,
                            details.name,
                            height = 72.dp,
                            maxWidth = 260.dp,
                            role = TextRole.Title1,
                            color = Tokens.dark.text,
                            modifier =
                                Modifier
                                    .align(
                                        Alignment.BottomStart,
                                    ).padding(horizontal = side, vertical = Marquee.space.s),
                        )
                    }
                }
                item(key = "info") { Info(title, versions, onPlay, onVersions) }
                if (header.seasons.isNotEmpty()) {
                    item(key = "seasons") {
                        LazyRow(
                            contentPadding = PaddingValues(horizontal = side),
                            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                        ) {
                            items(header.seasons, key = { it }) { season ->
                                Chip(
                                    label = Format.seasonName(season),
                                    selected = season == header.selectedSeason,
                                    onClick = { title.selectSeason(season) },
                                )
                            }
                        }
                    }
                }
                items(title.episodes, key = { it.videoId }) { episode ->
                    EpisodeRow(episode, onClick = { onEpisode(episode) }, onToggleWatched = {
                        title.setWatched(episode.videoId, !episode.watched)
                    })
                }
            }
        }
    }
}

private const val HERO_RATIO = 1.6f

@Composable
private fun Info(
    title: TitleView,
    versions: VersionsSnapshot?,
    onPlay: (String, String) -> Unit,
    onVersions: (String) -> Unit,
) {
    val header = checkNotNull(title.header)
    val details = checkNotNull(header.details)
    val colors = Marquee.colors
    val target = header.playTarget
    val recommended = versions?.recommended
    Column(
        Modifier.padding(horizontal = Marquee.safe.side),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        MqText(TitleCopy.meta(details), role = TextRole.Callout, color = colors.muted)
        details.description?.let { MqText(it, role = TextRole.Body, maxLines = 6) }
        PrimaryButton(
            label = target?.let(TitleCopy::playLabel) ?: "Play",
            icon = Icons.Play,
            modifier = Modifier.fillMaxWidth(),
            onClick = {
                when {
                    target != null && recommended != null -> onPlay(recommended.key, target.videoId)
                    target != null -> onVersions(target.videoId)
                }
            },
        )
        MqText(VersionsCopy.chosen(versions), role = TextRole.Callout, color = colors.muted)
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            if (target !=
                null
            ) {
                SecondaryButton("Versions", icon = Icons.Layers, onClick = { onVersions(target.videoId) })
            }
            SecondaryButton(
                label = if (header.inLibrary) "In library" else "Add to library",
                icon = if (header.inLibrary) Icons.BookmarkCheck else Icons.BookmarkPlus,
                onClick = { title.setInLibrary(!header.inLibrary) },
            )
            if (details.kind == "movie") {
                SecondaryButton(
                    label = if (header.watched) "Watched" else "Mark watched",
                    icon = if (header.watched) Icons.Check else Icons.Eye,
                    onClick = { title.setWatched(details.id, !header.watched) },
                )
            }
        }
    }
}

@Composable
private fun EpisodeRow(
    episode: Episode,
    onClick: () -> Unit,
    onToggleWatched: () -> Unit,
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
                onLongClick = onToggleWatched,
                longClickLabel = if (episode.watched) "Mark as not watched" else "Mark as watched",
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
                listOf(Format.episodeLabel(episode.season, episode.episode), episode.title)
                    .filter { it.isNotBlank() }
                    .joinToString(", "),
                role = TextRole.Headline,
                maxLines = 2,
            )
            episode.overview?.let { MqText(it, role = TextRole.Caption, color = colors.muted, maxLines = 2) }
        }
        if (episode.watched) MqIcon(Icons.Check, tint = colors.muted)
    }
}
