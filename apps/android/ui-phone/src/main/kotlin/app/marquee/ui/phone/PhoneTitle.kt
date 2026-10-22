package app.marquee.ui.phone

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.layout.windowInsetsTopHeight
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.derivedStateOf
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.marquee.core.Format
import app.marquee.core.PrimaryAction
import app.marquee.core.TileCopy
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
import app.marquee.design.FactsTable
import app.marquee.design.Icons
import app.marquee.design.KeepInView
import app.marquee.design.LightStatusBarIcons
import app.marquee.design.Logo
import app.marquee.design.Marquee
import app.marquee.design.MenuAction
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.PlayFilled
import app.marquee.design.PrimaryButton
import app.marquee.design.ProgressBar
import app.marquee.design.RatingLine
import app.marquee.design.RemoteImage
import app.marquee.design.RoundButton
import app.marquee.design.Scrims
import app.marquee.design.SecondaryButton
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusNote
import app.marquee.design.TextRole
import app.marquee.design.Tokens
import app.marquee.design.focusClickable
import app.marquee.design.liftRoom
import app.marquee.design.openLink
import app.marquee.design.windowDp

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
 * @param onAddAccount the versions play only through a streaming service account: go where one is added.
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
    onAddAccount: () -> Unit,
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
            // The artwork's block runs edge to edge as the loaded artwork does, so nothing jumps.
            Column(modifier.fillMaxSize(), verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                Skeleton(Modifier.fillMaxWidth().height(heroHeight()))
                Skeleton(Modifier.padding(horizontal = side).size(200.dp, 24.dp))
                Skeleton(Modifier.padding(horizontal = side).size(300.dp, 80.dp))
            }
        }

        else -> {
            val hasArt = details.background != null || details.poster != null
            var menuFor by remember { mutableStateOf<Episode?>(null) }
            val list = rememberLazyListState()
            val bar = WindowInsets.statusBars.getTop(LocalDensity.current)
            // The page reaches the status bar once the artwork has scrolled out from under it (without
            // artwork, at the first scroll). From then on a strip of page color sits behind the bar,
            // so its icons never cross the text, and they take the page's colors.
            val underBar by remember(bar, hasArt) {
                derivedStateOf {
                    val first = list.layoutInfo.visibleItemsInfo.firstOrNull() ?: return@derivedStateOf false
                    first.index > 0 || if (hasArt) first.offset + first.size < bar else first.offset < 0
                }
            }
            if (hasArt && !underBar) LightStatusBarIcons()
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
            Box(modifier.fillMaxSize()) {
                LazyColumn(
                    state = list,
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(bottom = Marquee.space.xl),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
                ) {
                    item(
                        key = "art",
                    ) { Hero(details.background ?: details.poster, details.logo, details.name, hasArt, side) }
                    item(
                        key = "info",
                    ) { Info(title, versions, versionsFailed, onPlay, onVersions, onAddSource, onAddAccount) }
                    if (header.seasons.isNotEmpty()) {
                        item(key = "seasons") {
                            val compact = TitleCopy.compactSeasons(header.seasons)
                            val order = TitleCopy.seasonOrder(header.seasons)
                            val seasons = rememberLazyListState()
                            val selected = order.indexOf(header.selectedSeason)
                            KeepInView(seasons, if (selected < 0 || !compact) selected else selected + 1)
                            LazyRow(
                                state = seasons,
                                // Room for a keyboard-focused chip and its ring; the page does not move.
                                modifier = Modifier.liftRoom(Marquee.tiles.pad),
                                contentPadding = PaddingValues(horizontal = side, vertical = Marquee.tiles.pad),
                                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                if (compact) {
                                    item(
                                        key = "label",
                                    ) { MqText("Season", role = TextRole.Callout, color = Marquee.colors.muted) }
                                }
                                items(order, key = { it }) { season ->
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
                    val facts = TitleCopy.facts(details, TitleCopy.seasonCount(header.seasons))
                    if (facts.isNotEmpty()) {
                        item(key = "details") { FactsTable(facts, Modifier.padding(horizontal = side).fillMaxWidth()) }
                    }
                }
                if (underBar) {
                    Box(
                        Modifier
                            .fillMaxWidth()
                            .windowInsetsTopHeight(WindowInsets.statusBars)
                            .background(Marquee.colors.bg),
                    )
                }
            }
        }
    }
}

private const val HERO_RATIO = 1.45f

/** The narrowest the title's primary gets beside Watched before Watched moves under it. */
private val PRIMARY_MIN = 220.dp

/** The artwork's height: the width at [HERO_RATIO], held to 60% of the window (landscape, tablets). */
@Composable
private fun heroHeight(): Dp {
    val window = windowDp()
    return minOf(window.width / HERO_RATIO, window.height * 0.6f)
}

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
    Box(Modifier.fillMaxWidth().height(heroHeight())) {
        // Dark under the light status bar icons and toward the light logo.
        Backdrop(art, Modifier.matchParentSize(), foot = Scrims.phoneTitleFoot, top = Scrims.phoneTitleTop)
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
    onAddAccount: () -> Unit,
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
        val context = LocalContext.current
        // The IMDb score links to the title's IMDb page.
        val imdb = details.imdbUrl?.takeIf { details.ratings.imdb != null }
        RatingLine(
            TileCopy.ratings(details.ratings),
            TitleCopy.meta(details, TitleCopy.seasonCount(header.seasons)),
            onFirst = imdb?.let { url -> { openLink(context, url) } },
        )
        details.description?.let { Synopsis(it) }
        // As the web lays a phone title out: the primary with Watched beside it as a pill (movies),
        // then the other actions as equal circles in one row, their words on up to two lines. On a
        // small phone or with large text Watched moves under the primary rather than squeeze its words.
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            PrimaryButton(
                label = primary.label,
                icon =
                    when (primary) {
                        is PrimaryAction.Play -> PlayFilled
                        PrimaryAction.ChooseVersion -> Icons.Layers
                        PrimaryAction.AddSource -> Icons.Plus
                        PrimaryAction.AddAccount -> Icons.KeyRound
                    },
                modifier = Modifier.widthIn(min = PRIMARY_MIN).weight(1f),
                enabled = primary !is PrimaryAction.Play || target != null,
                onClick = {
                    when {
                        primary == PrimaryAction.AddSource -> onAddSource()
                        primary == PrimaryAction.AddAccount -> onAddAccount()
                        target == null -> Unit
                        primary == PrimaryAction.ChooseVersion || recommended == null -> onVersions(target.videoId)
                        else -> onPlay(recommended.key, target.videoId)
                    }
                },
            )
            if (details.kind == "movie") {
                SecondaryButton(
                    label = if (header.watched) "Watched" else "Mark as watched",
                    onClick = { title.setWatched(details.id, !header.watched) },
                    icon = Icons.Check,
                    selected = header.watched,
                )
            }
        }
        if (target == null) {
            StatusNote(TitleCopy.nothingToPlay(header.nextAirs))
        } else {
            VersionStatus(VersionsCopy.line(versions, versionsFailed))
        }
        Row(Modifier.fillMaxWidth().padding(top = Marquee.space.xs)) {
            if (target != null) {
                RoundButton(
                    Icons.Layers,
                    "Versions",
                    { onVersions(target.videoId) },
                    Modifier.weight(1f),
                    showLabel = true,
                )
            }
            details.trailers.firstOrNull()?.let { trailer ->
                RoundButton(
                    Icons.Film,
                    "Trailer",
                    { openLink(context, trailer.url) },
                    Modifier.weight(1f),
                    showLabel = true,
                )
            }
            RoundButton(
                icon = if (header.inLibrary) Icons.BookmarkCheck else Icons.BookmarkPlus,
                label = if (header.inLibrary) "In Library" else "Add to Library",
                onClick = { title.setInLibrary(!header.inLibrary) },
                modifier = Modifier.weight(1f),
                selected = header.inLibrary,
                showLabel = true,
            )
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
            ChosenVersion(line.facts, line.status, if (line.good) Status.Ok else Status.Warn, line.spoken)
        }

        is VersionLine.Problem -> {
            StatusNote(line.text, tone = Status.Warn, maxLines = Int.MAX_VALUE)
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
                    .defaultMinSize(minWidth = 48.dp, minHeight = 48.dp)
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

@Composable
private fun EpisodeRow(
    episode: Episode,
    runtime: String?,
    onClick: () -> Unit,
    onMore: () -> Unit,
) {
    val colors = Marquee.colors
    val spoken = TitleCopy.episode(episode)
    // A glass card per episode (design/DECISION.md v2, Episodes).
    val card = RoundedCornerShape(Marquee.radius.card)
    Row(
        Modifier
            .padding(horizontal = Marquee.safe.side, vertical = Marquee.space.xs / 2)
            .fillMaxWidth()
            .clip(card)
            .background(colors.raised)
            .border(1.dp, colors.edge, card)
            .semantics { contentDescription = spoken }
            .focusClickable(
                onClick,
                Role.Button,
                onLongClick = onMore,
                longClickLabel = "More options",
            ).padding(Marquee.space.xs),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        Box(Modifier.clip(RoundedCornerShape(Marquee.radius.s))) {
            RemoteImage(
                episode.thumbnail,
                128.dp,
                72.dp,
                fallbackText = Format.episodeLabel(episode.season, episode.episode),
            ) {
                MqText(
                    "E${episode.episode}",
                    modifier =
                        Modifier
                            .align(Alignment.BottomStart)
                            .padding(4.dp)
                            .clip(RoundedCornerShape(Marquee.radius.pill))
                            .background(colors.bg)
                            .padding(horizontal = 6.dp, vertical = 1.dp),
                    role = TextRole.Caption,
                    weight = FontWeight.Bold,
                )
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
        if (episode.watched) MqIcon(Icons.Check, tint = colors.text)
    }
}
