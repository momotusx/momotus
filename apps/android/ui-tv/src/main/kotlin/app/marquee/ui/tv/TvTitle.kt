@file:OptIn(ExperimentalComposeUiApi::class)

package app.marquee.ui.tv

import androidx.compose.foundation.background
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import app.marquee.core.Format
import app.marquee.core.Mark
import app.marquee.core.PlayTo
import app.marquee.core.TitleCopy
import app.marquee.core.VersionLine
import app.marquee.core.VersionsCopy
import app.marquee.core.model.Episode
import app.marquee.core.model.Tile
import app.marquee.core.model.TitleDetails
import app.marquee.core.model.TitleHeader
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
import app.marquee.design.Marks
import app.marquee.design.Marquee
import app.marquee.design.MenuAction
import app.marquee.design.MqText
import app.marquee.design.People
import app.marquee.design.PlayFilled
import app.marquee.design.PosterTile
import app.marquee.design.PrimaryButton
import app.marquee.design.RatedCover
import app.marquee.design.RoundButton
import app.marquee.design.RowHeader
import app.marquee.design.SecondaryButton
import app.marquee.design.Score
import app.marquee.design.Scrims
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusNote
import app.marquee.design.TextRole
import app.marquee.design.TitleRatingLine
import app.marquee.design.endFade
import app.marquee.design.episodeLaneHeight
import app.marquee.design.laneHeight
import app.marquee.design.liftRoom
import app.marquee.design.marqueeFocus
import app.marquee.design.openLink
import app.marquee.design.scrollByKey
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

private const val PLAY = "play"
private const val EPISODES = "episodes"
private const val SEASONS = "seasons"
private const val ACTIONS = "actions"
private const val RETRY = "retry"
private const val SIMILAR = "similar"

/** The first pill of the strip under the episodes: where Down from an episode card lands. */
private const val STRIP_PLAY = "episode-actions"

/**
 * A title on TV: artwork behind the facts, then the primary action (Play, always, with the version it
 * will play or why nothing can under it; inert in the states whose only way on is Back), Versions, the
 * library and watched toggles, the details, and for a series the seasons and the episodes of the selected
 * one. Focus starts on Play, or on the next action when Play is inert, and comes back to what it was on
 * after Versions.
 *
 * @param onPlay Play was chosen with version [key] of [videoId], or with none while versions are still being
 * searched: the player opens at once and starts the core's pick.
 * @param onVersions the Versions screen for [videoId] was asked for (also what Play does once nothing is
 * recommended).
 * @param onEpisode an episode was chosen: it plays.
 * @param onEpisodeVersions "Choose a version" for an episode, from its hold-select menu.
 */
@Composable
fun TvTitle(
    title: TitleView,
    versions: VersionsSnapshot?,
    focus: FocusMemory,
    onPlay: (key: String?, videoId: String) -> Unit,
    onVersions: (videoId: String) -> Unit,
    onEpisode: (Episode) -> Unit,
    onEpisodeVersions: (Episode) -> Unit,
    modifier: Modifier = Modifier,
    versionsFailed: Boolean = false,
    /** A genre chip: Discover on the title's kind and that genre. */
    onGenre: (kind: String, genre: String) -> Unit = { _, _ -> },
    /** A title in More like this. */
    onOpen: (Tile) -> Unit = {},
    /** A person's name: Search for them. */
    onPerson: (String) -> Unit = {},
    /** Remove from Continue Watching, on every synced device; null where the page has no such button. */
    onUncontinue: (suspend () -> Unit)? = null,
) {
    val safe = Marquee.safe
    val header = title.header
    val details = header?.details
    val failure = title.failure
    val failed = failure != null
    // Focus goes to Play once the page has loaded (or to Try again when it could not), not before:
    // the buttons do not exist while the skeleton shows. Try again asks again. An inert or disabled Play
    // takes no focus: it starts on the next action, Versions, or Add to Library when there is no video.
    val target = header?.playTarget
    val live = details != null && TitleCopy.live(TitleCopy.primary(target, versions, details.runtime), target)
    focus.FirstFocus(
        when {
            failed -> RETRY
            details == null -> null
            live -> PLAY
            target != null -> "versions"
            else -> "library"
        },
        ask = failed,
    )

    Box(
        modifier.fillMaxSize().onPreviewKeyEvent { event ->
            if (event.type == KeyEventType.KeyDown) focus.interacted()
            false
        },
    ) {
        // The text column sits on the left half: the art stays clear on the right, and fades into the page below.
        Backdrop(details?.background ?: details?.poster, Modifier.fillMaxSize(), Scrims.titleSide, Scrims.titleFoot)

        when {
            failure != null -> {
                StateMessage(
                    title = "This title could not load",
                    body = failure,
                    icon = Icons.CircleAlert,
                    actionLabel = "Try again",
                    onAction = title::retry,
                    spoken = true,
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
                                Info(
                                    title,
                                    versions,
                                    versionsFailed,
                                    focus,
                                    onPlay,
                                    onVersions,
                                    onGenre,
                                    onUncontinue,
                                )
                            }
                            // The cover sits beside the facts and the people, which keep their full 560: the
                            // column grows by the cover's width (Info is weighted and capped at 760).
                            val cover = details.ratedPoster != null
                            Column(
                                Modifier
                                    .width(if (cover) 560.dp + COVER_WIDTH + Marquee.space.m else 560.dp)
                                    .padding(top = Marquee.space.xl),
                                verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
                            ) {
                                // Rotten Tomatoes shows on the page through the rated cover, at the right over
                                // the art (plans/title-page-ratings-genres-similar.md): decoration, no focus stop,
                                // beside the facts and the people so the column stays inside the safe area.
                                Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.m)) {
                                    details.ratedPoster?.let { RatedCover(it, COVER_WIDTH) }
                                    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.m)) {
                                        FactsTable(TitleCopy.facts(details, TitleCopy.seasonCount(header.seasons)))
                                        // Each name opens Search for that person, a few of each.
                                        // A kids profile's Search finds only its list: the names are words there.
                                        People(TitleCopy.people(details), onPerson, linked = header.genresLinked)
                                    }
                                }
                            }
                        }
                    }
                    if (header.seasons.isNotEmpty()) item(key = "seasons") { Seasons(title, focus) }
                    if (title.episodes.isNotEmpty()) {
                        item(key = "episodes") { Episodes(title, focus, onEpisode, onEpisodeVersions) }
                    }
                    if (title.similar.isNotEmpty()) {
                        item(key = "similar") { MoreLikeThis(title.similar, focus, onOpen) }
                    }
                }
            }
        }
    }
}

/**
 * Five lines of the synopsis. As on the web, a synopsis that is cut takes focus (Up from the
 * actions) and shows whole on glass while it has it; one that fits is plain text. Open, it is held
 * to [OPEN_MAX] and scrolls with Up and Down, so a long one is read to its end before focus moves on.
 */
@Composable
private fun Synopsis(text: String) {
    var cut by remember(text) { mutableStateOf(false) }
    var open by remember { mutableStateOf(false) }
    val scroll = rememberScrollState()
    val scope = rememberCoroutineScope()
    val shape = RoundedCornerShape(Marquee.radius.m)
    val inset = 14.dp
    LaunchedEffect(open) { if (!open) scroll.scrollTo(0) }
    MqText(
        text,
        // Half the screen: the scrim is dark enough for text only that far. The inset is pulled
        // back, so the text lines up with the logo whether the glass shows or not.
        modifier =
            Modifier
                .offset(x = -inset)
                .widthIn(max = 640.dp + inset * 2)
                .then(
                    if (cut || open) {
                        Modifier
                            .onFocusChanged { open = it.isFocused }
                            .onPreviewKeyEvent { open && scrollByKey(it, scroll, scope) }
                            .semantics { stateDescription = if (open) "Expanded" else "Collapsed" }
                            .marqueeFocus(Marquee.radius.m, scale = 1f)
                            .clip(shape)
                            .background(if (open) Marquee.colors.glassStrong else Color.Transparent)
                            .focusable()
                            .heightIn(max = OPEN_MAX)
                            .endFade(open && scroll.canScrollForward, vertical = true, length = 48.dp)
                            .verticalScroll(scroll, enabled = false)
                    } else {
                        Modifier
                    },
                ).padding(horizontal = inset, vertical = 10.dp),
        role = TextRole.Body,
        maxLines = if (open) Int.MAX_VALUE else 5,
        onTextLayout = { if (!open) cut = it.hasVisualOverflow },
    )
}

/** The open synopsis' height: from under a two-line title to the safe foot of a 1080 canvas. */
private val OPEN_MAX = 600.dp

/** The rated cover's width on TV: 300 by 450 on the 1920 canvas (TvCanvas: one dp is one canvas point). */
private val COVER_WIDTH = 300.dp

/** The marks of the title page's ratings row. */
private fun scores(details: TitleDetails): List<Score> =
    TitleCopy.scoreItems(details.ratings.imdb, details.scores, details.imdbUrl).map {
        Score(
            when (it.mark) {
                Mark.Imdb -> Marks.Imdb
                Mark.Tmdb -> Marks.Tmdb
                Mark.Trakt -> Marks.Trakt
            },
            it.text,
            it.spoken,
            it.url,
            when (it.mark) {
                Mark.Imdb -> "IMDb"
                Mark.Tmdb -> "TMDB"
                Mark.Trakt -> "Trakt"
            },
        )
    }

/** The genres as focusable chips that open Discover; on a kids profile, which has no Discover, plain words. */
@Composable
private fun Genres(
    header: TitleHeader,
    details: TitleDetails,
    onGenre: (String, String) -> Unit,
) {
    val genres = details.genres.take(3)
    if (genres.isEmpty()) return
    if (!header.genresLinked) {
        MqText(genres.joinToString(", "), role = TextRole.Callout, color = Marquee.colors.muted)
        return
    }
    // Entering the row from below lands on its first chip, as the D-pad reads left to right.
    val first = remember { FocusRequester() }
    Row(
        Modifier.focusProperties { enter = { first } }.focusGroup(),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        genres.forEachIndexed { i, genre ->
            Chip(
                label = genre,
                selected = false,
                onClick = { onGenre(details.kind, genre) },
                clickLabel = "Opens Discover",
                focusRequester = if (i == 0) first else null,
            )
        }
    }
}

/** "More like this": the titles Trakt relates to this one, 2:3 posters with their captions, no source named. */
@Composable
private fun MoreLikeThis(
    tiles: List<Tile>,
    focus: FocusMemory,
    onOpen: (Tile) -> Unit,
) {
    val side = Marquee.safe.side
    val metrics = Marquee.tiles
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        RowHeader("More like this", null, Modifier.padding(horizontal = side))
        LazyRow(
            // Back from a title opened here lands on its tile (the page's first focus takes the last key).
            modifier =
                Modifier
                    .focusProperties { enter = { focus.rowEntry(SIMILAR) ?: FocusRequester.Default } }
                    .focusGroup(),
            contentPadding = PaddingValues(horizontal = side, vertical = metrics.pad),
            horizontalArrangement = Arrangement.spacedBy(metrics.gap),
        ) {
            // Keys must be unique: a title Trakt lists twice shows once.
            items(tiles.distinctBy { it.id }, key = { it.id }) { tile ->
                val key = "similar:${tile.id}"
                PosterTile(
                    title = tile.name,
                    imageUrl = tile.poster,
                    fallbackImageUrl = tile.posterFallback,
                    onClick = { onOpen(tile) },
                    caption = tile.releaseInfo,
                    focusRequester = focus.requesterFor(key),
                    onFocusChange = focus.watch(SIMILAR, key),
                )
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
    onPlay: (String?, String) -> Unit,
    onVersions: (String) -> Unit,
    onGenre: (String, String) -> Unit,
    onUncontinue: (suspend () -> Unit)?,
) {
    val header = checkNotNull(title.header)
    val details = checkNotNull(header.details)
    val colors = Marquee.colors
    val target = header.playTarget
    val primary = TitleCopy.primary(target, versions, details.runtime)
    val context = LocalContext.current
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        Logo(details.logo, details.name, height = 160.dp, maxWidth = 480.dp)
        TitleRatingLine(
            TitleCopy.meta(details, TitleCopy.seasonCount(header.seasons), genres = false),
            scores(details),
            Modifier.widthIn(max = 760.dp),
        )
        // Their own row between the meta line and the synopsis (design/DECISION.md): Up from the synopsis
        // (or from Play when the synopsis fits) reaches them.
        Genres(header, details, onGenre)
        details.description?.let { Synopsis(it) }
        Row(
            Modifier
                .padding(top = Marquee.space.xs)
                .focusProperties { enter = { focus.rowEntry(ACTIONS) ?: FocusRequester.Default } }
                .focusGroup(),
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.m),
            verticalAlignment = Alignment.Top,
        ) {
            // One button for every state, so its node survives the addons answering. Play may turn inert (a
            // state whose only way on is Back), and an inert Play takes no focus: the first focus passes it by.
            PrimaryButton(
                label = primary.label,
                icon = PlayFilled,
                enabled = TitleCopy.live(primary, target),
                onClick = {
                    when (val to = TitleCopy.playTo(primary, target, versions)) {
                        is PlayTo.Player -> target?.let { onPlay(to.key, it.videoId) }
                        PlayTo.Versions -> target?.let { onVersions(it.videoId) }
                        PlayTo.Nowhere -> Unit
                    }
                },
                // An inert Play is no focus target, so the first focus and the memory pass it by.
                focusRequester = if (TitleCopy.live(primary, target)) focus.requesterFor(PLAY) else null,
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
                    onClick = { openLink(context, trailer.url) },
                    showLabel = true,
                    focusRequester = focus.requesterFor("trailer"),
                    onFocusChange = focus.watch(ACTIONS, "trailer"),
                )
            }
            // A kids profile's Library is the owner's list: the core says so, and no dead button shows.
            if (header.librarySettable) {
                RoundButton(
                    icon = if (header.inLibrary) Icons.BookmarkCheck else Icons.BookmarkPlus,
                    label = if (header.inLibrary) "In Library" else "Add to Library",
                    onClick = { title.setInLibrary(!header.inLibrary) },
                    selected = header.inLibrary,
                    showLabel = true,
                    focusRequester = focus.requesterFor("library"),
                    onFocusChange = focus.watch(ACTIONS, "library"),
                )
            }
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
            StatusNote(TitleCopy.nothingToPlay(header.nextAirs), Modifier.widthIn(max = 640.dp))
        } else {
            VersionStatus(VersionsCopy.line(versions, versionsFailed), Modifier.widthIn(max = 760.dp))
        }
    }
}

/**
 * Remove from Continue Watching reaches every synced device, so it asks once ("Press again to remove", for four
 * seconds) and says when the core refused. Once it went, focus moves to Play before the button leaves with the header.
 */
@Composable
private fun Uncontinue(
    remove: suspend () -> Unit,
    focus: FocusMemory,
) {
    var ask by remember { mutableStateOf(Ask.Idle) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(ask) {
        if (ask == Ask.Idle) return@LaunchedEffect
        delay(ASK_MS)
        ask = Ask.Idle
    }
    RoundButton(
        icon = Icons.CircleX,
        label =
            when (ask) {
                Ask.Idle -> "Remove from Continue Watching"
                Ask.Confirm -> "Press again to remove"
                Ask.Failed -> "Not removed, try again"
            },
        onClick = {
            if (ask != Ask.Confirm) {
                ask = Ask.Confirm
            } else {
                ask = Ask.Idle
                scope.launch {
                    try {
                        remove()
                        focus.requester(PLAY)?.requestFocus()
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        ask = Ask.Failed
                    }
                }
            }
        },
        showLabel = true,
        focusRequester = focus.requesterFor("uncontinue"),
        onFocusChange = focus.watch(ACTIONS, "uncontinue"),
    )
}

private enum class Ask { Idle, Confirm, Failed }

private const val ASK_MS = 4_000L

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
                line.spoken,
                modifier,
            )
        }

        is VersionLine.Problem -> {
            StatusNote(line.text, modifier, tone = Status.Warn, maxLines = Int.MAX_VALUE)
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
                // Room above and below for a focused chip to rise over its shadow; the page does not move.
                .liftRoom(Marquee.tiles.pad)
                // First entry lands on the selected season, which may be far along the row.
                .focusProperties {
                    enter = {
                        focus.rowEntry(SEASONS)
                            ?: focus.requester("season:${header.selectedSeason}")
                            ?: FocusRequester.Default
                    }
                }.focusGroup(),
        contentPadding = PaddingValues(horizontal = Marquee.safe.side, vertical = Marquee.space.xs + Marquee.tiles.pad),
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
    // The strip under the lane acts on the episode focus last stood on (the first before it stood on any); Down from
    // a card enters it, as holding select on the card opens the same two actions.
    var picked by remember { mutableStateOf<String?>(null) }
    val shown = title.episodes.firstOrNull { it.videoId == picked } ?: title.episodes.first()
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
                val watch = focus.watch(EPISODES, key)
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
                    modifier =
                        Modifier
                            .padding(top = metrics.pad)
                            .onPreviewKeyEvent {
                                val down = it.type == KeyEventType.KeyDown && it.key == Key.DirectionDown
                                down && focus.requester(STRIP_PLAY)?.requestFocus() == true
                            },
                    focusRequester = focus.requesterFor(key),
                    onFocusChange = {
                        watch(it)
                        if (it) picked = episode.videoId
                    },
                    onLongClick = { menuFor = episode },
                    longClickLabel = "More options",
                )
            }
        }
        Row(
            Modifier
                .padding(horizontal = Marquee.safe.side)
                .focusProperties { enter = { focus.requester(STRIP_PLAY) ?: FocusRequester.Default } }
                .focusGroup(),
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.m),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // Up goes back to the card the pills belong to, wherever the search would have put it.
            val back = Modifier.focusProperties {
                up = focus.requester("episode:${shown.videoId}") ?: FocusRequester.Default
            }
            MqText(
                "${Format.episodeLabel(shown.season, shown.episode)} ${shown.title}".trim(),
                role = TextRole.Callout,
                color = Marquee.colors.muted,
                maxLines = 1,
                modifier = Modifier.widthIn(max = 360.dp),
            )
            SecondaryButton(
                "Choose a version",
                { onVersions(shown) },
                back,
                icon = Icons.Layers,
                focusRequester = focus.requesterFor(STRIP_PLAY),
                spoken = "Choose a version for ${TitleCopy.episode(shown).removePrefix("Watched, ")}",
            )
            SecondaryButton(
                if (shown.watched) "Mark as not watched" else "Mark as watched",
                { title.setWatched(shown.videoId, !shown.watched) },
                back,
                icon = if (shown.watched) Icons.EyeOff else Icons.Check,
                spoken = (if (shown.watched) "Mark as not watched: " else "Mark as watched: ") +
                    TitleCopy.episode(shown).removePrefix("Watched, "),
            )
        }
    }
}
