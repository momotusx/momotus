package app.marquee.ui.phone

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
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
import androidx.compose.runtime.mutableIntStateOf
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.marquee.core.DownloadCopy
import app.marquee.core.DownloadItem
import app.marquee.core.Format
import app.marquee.core.GroupAct
import app.marquee.core.Mark
import app.marquee.core.PlayTo
import app.marquee.core.TitleCopy
import app.marquee.core.VersionLine
import app.marquee.core.VersionsCopy
import app.marquee.core.model.DownloadBadge
import app.marquee.core.model.DownloadState
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
import app.marquee.design.FactsTable
import app.marquee.design.HeroTrailers
import app.marquee.design.Icons
import app.marquee.design.KeepInView
import app.marquee.design.LightStatusBarIcons
import app.marquee.design.Logo
import app.marquee.design.Marks
import app.marquee.design.Marquee
import app.marquee.design.MenuAction
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.People
import app.marquee.design.PlayFilled
import app.marquee.design.PosterTile
import app.marquee.design.PrimaryButton
import app.marquee.design.ProgressBar
import app.marquee.design.RatedCover
import app.marquee.design.RemoteImage
import app.marquee.design.RoundButton
import app.marquee.design.RowHeader
import app.marquee.design.Score
import app.marquee.design.Scrims
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusNote
import app.marquee.design.TextRole
import app.marquee.design.TitleHeaderTrailer
import app.marquee.design.TitleRatingLine
import app.marquee.design.TitleTrailers
import app.marquee.design.Tokens
import app.marquee.design.TrailerState
import app.marquee.design.TrailerSurface
import app.marquee.design.focusClickable
import app.marquee.design.liftRoom
import app.marquee.design.openLink
import app.marquee.design.rememberTrailerHost
import app.marquee.design.roundLabelsFit
import app.marquee.design.windowDp
import app.marquee.player.TrailerDialog

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
 * @param onEpisode an episode was tapped: it plays.
 * @param onEpisodeVersions "Choose a version" for an episode, from its press-and-hold menu.
 * @param downloads what the download controls do; null shows none (a kids profile).
 */
@Composable
fun PhoneTitle(
    title: TitleView,
    versions: VersionsSnapshot?,
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
    downloads: TitleDownloads? = null,
) {
    // A snapshot that came while this page first drew reaches every reader (LiveView.follow).
    title.follow()
    val header = title.header
    val details = header?.details
    val failure = title.failure
    val side = Marquee.safe.side
    when {
        failure != null -> {
            StateMessage(
                title = "This title could not load",
                body = failure,
                icon = Icons.CircleAlert,
                actionLabel = "Try again",
                onAction = title::retry,
                spoken = true,
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
            var downloadMenu by remember { mutableStateOf<DownloadAsk?>(null) }
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
                        ) + episodeDownloadItems(episode, downloads),
                    onDismiss = { menuFor = null },
                )
            }
            // The cover's sources in order: the large cover, then the addon's poster (the same 2:3); each that does not
            // load gives way to the next, then to the backdrop band with the logo. Held with the title, outside the
            // list, so scrolling the art away and back never asks a failed one again.
            val covers =
                remember(details.cover, details.poster) { listOfNotNull(details.cover, details.poster).distinct() }
            var coversFailed by remember(details.id) { mutableIntStateOf(0) }
            // The header's muted trailer, as the hero's (plans/trailer-inline.md 3, 4), and Trailer full screen, on one
            // player for the page.
            val trailers = rememberTrailerHost()
            val headerTrailer = remember { TrailerState() }
            var fullTrailer by remember { mutableStateOf(false) }
            val artVisible by remember(list) {
                derivedStateOf {
                    val item = list.layoutInfo.visibleItemsInfo.firstOrNull { it.key == "art" }
                    item != null && item.size > 0 && (item.size + minOf(0, item.offset)) * 2 >= item.size
                }
            }
            TitleHeaderTrailer(
                trailers,
                headerTrailer,
                details.id,
                // Offline the page plays no trailer (plans/offline-downloads.md 4.18).
                details.trailers
                    .firstOrNull()
                    ?.youtubeId
                    ?.takeIf { !header.offline },
                artVisible && !fullTrailer,
                tv = false,
            )
            val art by animateFloatAsState(
                if (headerTrailer.shown) 0f else 1f,
                tween(600, easing = Tokens.ease),
                label = "titleArt",
            )
            if (fullTrailer) TrailerDialog(details.trailers, trailers) { fullTrailer = false }
            downloadMenu?.let { ask ->
                DownloadMenu(
                    ask.title,
                    DownloadCopy.control(ask.badge),
                    onItem = { downloads?.item(it, ask.videoId, ask.badge) },
                    onDismiss = { downloadMenu = null },
                    items = ask.items ?: DownloadCopy.control(ask.badge).items,
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
                    ) {
                        // The cover first: covers are vertical, and a backdrop is often a crop of the same art.
                        // Only where its 2:3 fits.
                        val at = coversFailed
                        val cover = covers.getOrNull(at)?.takeIf { coverFits() }
                        Hero(
                            cover ?: details.background ?: details.poster,
                            details.logo,
                            details.name,
                            hasArt,
                            side,
                            cover = cover != null,
                            titled = details.coverTitled,
                            onCoverError = { if (coversFailed == at) coversFailed = at + 1 },
                            under = if (cover != null) details.poster else null,
                            artAlpha = { art },
                            trailer =
                                if (headerTrailer.loading && !fullTrailer) {
                                    { TrailerSurface(trailers, Modifier.matchParentSize()) }
                                } else {
                                    null
                                },
                            // Its sound at the art's top right, where nothing else sits (the back link is top left).
                            overlay =
                                if (headerTrailer.playing && (!HeroTrailers.walk || HeroTrailers.walkSwitch)) {
                                    {
                                        TrailerSoundSwitch(
                                            trailers,
                                            Modifier
                                                .align(Alignment.TopEnd)
                                                .windowInsetsPadding(WindowInsets.statusBars)
                                                .padding(top = 4.dp, end = side - 4.dp),
                                        )
                                    }
                                } else {
                                    null
                                },
                        )
                    }
                    item(
                        key = "info",
                    ) {
                        Info(
                            title,
                            versions,
                            versionsFailed,
                            onPlay,
                            onVersions,
                            onGenre,
                            onTrailer = {
                                TitleTrailers.spent += details.id
                                // Stopped here, before the full screen starts on the same player.
                                if (headerTrailer.loading) trailers.stop()
                                fullTrailer = true
                            },
                            downloads = downloads,
                        ) {
                            downloadMenu =
                                it
                        }
                    }
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
                            download =
                                downloads?.takeIf { TitleCopy.airs(episode) == null }?.let { actions ->
                                    {
                                        val badge = episode.download
                                        if (DownloadCopy.control(badge).items.isEmpty()) {
                                            actions.item(DownloadItem.Start, episode.videoId, null)
                                        } else {
                                            downloadMenu = DownloadAsk(episodeName(episode), episode.videoId, badge)
                                        }
                                    }
                                },
                        )
                    }
                    // Rotten Tomatoes shows on the page through the rated cover, above Information
                    // (plans/title-page-ratings-genres-similar.md): wide enough that the band reads.
                    details.ratedPoster?.let { cover ->
                        item(key = "cover") { RatedCover(cover, COVER_WIDTH, Modifier.padding(horizontal = side)) }
                    }
                    if (title.similar.isNotEmpty()) {
                        item(key = "similar") { MoreLikeThis(title.similar, onOpen) }
                    }
                    val people = TitleCopy.people(details)
                    if (people.isNotEmpty()) {
                        item(key = "people") {
                            // A kids profile's Search finds only its list: the names are words there.
                            People(people, onPerson, Modifier.padding(horizontal = side), linked = header.genresLinked)
                        }
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

/** The title's artwork, for tests. */
internal const val TITLE_ART_TAG = "title-art"

/** The rated cover's width on a phone: the band's digits stay legible from 200 dp up. */
private val COVER_WIDTH = 200.dp

/** The marks of the title page's ratings row. */
internal fun scores(details: TitleDetails): List<Score> =
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

/**
 * The genres, a row of their own under the meta line (design/DECISION.md, Title page): chips that open Discover on
 * the title's kind and that genre, or plain words on a kids profile, which has no Discover.
 */
@Composable
internal fun Genres(
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
    FlowRow(
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        genres.forEach { genre ->
            Chip(
                label = genre,
                selected = false,
                onClick = { onGenre(details.kind, genre) },
                clickLabel = "Opens Discover",
            )
        }
    }
}

/** "More like this": the titles Trakt relates to this one, 2:3 posters as on Home, no source named. */
@Composable
private fun MoreLikeThis(
    tiles: List<Tile>,
    onOpen: (Tile) -> Unit,
) {
    val side = Marquee.safe.side
    val metrics = Marquee.tiles
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        RowHeader("More like this", null, Modifier.padding(horizontal = side))
        LazyRow(
            contentPadding = PaddingValues(horizontal = side),
            horizontalArrangement = Arrangement.spacedBy(metrics.gap),
        ) {
            // Keys must be unique: a title Trakt lists twice shows once.
            items(tiles.distinctBy { it.id }, key = { it.id }) { tile ->
                PosterTile(
                    title = tile.name,
                    imageUrl = tile.poster,
                    fallbackImageUrl = tile.posterFallback,
                    onClick = { onOpen(tile) },
                    modifier = Modifier.padding(top = metrics.pad),
                    captioned = false,
                )
            }
        }
    }
}

/**
 * The room a cover leaves under it in the window: the meta line and Play at large text (about 125 dp) and the
 * system's navigation bar over the page's foot (up to 56 dp), so Play is always on the first screen.
 */
private val UNDER_COVER = 180.dp

/** The cover's height: its own 2:3 across the window, held to 75% of it and to what leaves [UNDER_COVER] free. */
@Composable
private fun coverHeight(): Dp {
    val window = windowDp()
    // The words under it grow with the reader's text size, and so does their room.
    val room = UNDER_COVER * maxOf(1f, LocalDensity.current.fontScale)
    return minOf(window.width * 1.5f, window.height * 0.75f, window.height - room)
}

/**
 * A cover shows only where its 2:3 fits: a portrait phone (up to 600 dp wide), losing at most an eighth at its
 * foot. A landscape phone, a short one or a tablet keeps the backdrop band.
 */
@Composable
internal fun coverFits(): Boolean = windowDp().width <= 600.dp && coverHeight() >= windowDp().width * 1.5f * 0.88f

/**
 * The artwork's height: a cover as [coverHeight]; a backdrop at [HERO_RATIO], held to 60% (landscape,
 * tablets).
 */
@Composable
internal fun heroHeight(cover: Boolean = false): Dp {
    val window = windowDp()
    return if (cover) {
        coverHeight()
    } else {
        minOf(
            window.width / HERO_RATIO,
            window.height * 0.6f,
        )
    }
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
    cover: Boolean,
    titled: Boolean,
    onCoverError: () -> Unit,
    // The small poster from the lists, in the cache, under a large cover until it lands.
    under: String?,
    artAlpha: () -> Float = { 1f },
    // The header's trailer, under the art, which fades over it ([artAlpha]).
    trailer: (@Composable BoxScope.() -> Unit)? = null,
    // Drawn over the art: the trailer's sound switch.
    overlay: (@Composable BoxScope.() -> Unit)? = null,
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
                    )
                    // Under the floating back link, which the shell draws over this corner.
                    .padding(start = side, end = side, top = Marquee.space.s + BACK_HEIGHT, bottom = Marquee.space.s),
        )
        return
    }
    val page = Marquee.colors.bg
    // A cover known to print its title (the core says so: metahub's do) gets no logo, only the heading for
    // screen readers, and the meta line starts under it, on the page color. Another cover keeps the logo,
    // under it.
    val heading =
        if (cover && titled) {
            Modifier.clearAndSetSemantics {
                contentDescription = name
                heading()
            }
        } else {
            Modifier
        }
    Column {
        Box(
            Modifier
                .fillMaxWidth()
                .height(heroHeight(cover))
                .testTag(TITLE_ART_TAG)
                .then(heading),
        ) {
            // Dark under the light status bar icons and toward the light logo. A cover held under its 2:3
            // keeps its top, where the faces are, and only its bottom edge fades into the page.
            Backdrop(
                art,
                Modifier.matchParentSize(),
                foot = if (cover) emptyList() else Scrims.phoneTitleFoot,
                top = Scrims.phoneTitleTop,
                alignment = if (cover) Alignment.TopCenter else Alignment.Center,
                onError = if (cover) onCoverError else null,
                under = under,
                artAlpha = artAlpha,
                trailer = trailer,
            )
            overlay?.invoke(this)
            Box(
                Modifier
                    .align(Alignment.BottomStart)
                    .fillMaxWidth()
                    .height(HERO_FADE)
                    .background(Brush.verticalGradient(0f to Color.Transparent, 1f to page)),
            )
            if (!cover) {
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
        if (cover && !titled) {
            Logo(
                logo,
                name,
                height = 72.dp,
                maxWidth = 280.dp,
                role = TextRole.Title1,
                modifier = Modifier.padding(start = side, end = side, top = Marquee.space.s),
            )
        }
    }
}

/** A download's menu to open: for [videoId], titled [title]; [items] in place of the state's (a press and hold). */
private data class DownloadAsk(
    val title: String,
    val videoId: String,
    val badge: DownloadBadge?,
    val items: List<DownloadItem>? = null,
)

private fun episodeName(episode: Episode) =
    episode.title.ifBlank {
        Format.episodeLabel(episode.season, episode.episode)
    }

/** An episode's press-and-hold items for its download: Download and the version, or the state's. */
private fun episodeDownloadItems(
    episode: Episode,
    downloads: TitleDownloads?,
): List<MenuAction> {
    if (downloads == null || TitleCopy.airs(episode) != null) return emptyList()
    val badge = episode.download
    val items = if (badge == null) START_ITEMS else DownloadCopy.control(badge).items
    return items.map { item -> MenuAction(item.label, item.icon) { downloads.item(item, episode.videoId, badge) } }
}

private val START_ITEMS = listOf(DownloadItem.Start, DownloadItem.Choose)

@Composable
private fun Info(
    title: TitleView,
    versions: VersionsSnapshot?,
    versionsFailed: Boolean,
    onPlay: (String?, String) -> Unit,
    onVersions: (String) -> Unit,
    onGenre: (String, String) -> Unit,
    onTrailer: () -> Unit,
    downloads: TitleDownloads?,
    onDownloadMenu: (DownloadAsk) -> Unit,
) {
    val header = checkNotNull(title.header)
    val details = checkNotNull(header.details)
    val colors = Marquee.colors
    val target = header.playTarget
    val primary = TitleCopy.primary(target, versions, details.runtime)
    Column(
        Modifier.padding(horizontal = Marquee.safe.side),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        val context = LocalContext.current
        // Offline, the page shows what the downloads kept (plans/offline-downloads.md 4.18).
        if (header.offline) StatusNote(DownloadCopy.TITLE_OFFLINE)
        // Each score links to the title's page on its service (none on a kids profile: the core sends no URL).
        TitleRatingLine(
            TitleCopy.meta(details, TitleCopy.seasonCount(header.seasons), genres = false),
            scores(details),
            onOpen = { url -> openLink(context, url) },
        )
        // Under the meta line the primary across the page, then the other actions as equal circles in one
        // row, their words on up to two lines (Watched one of those small circles, never a big button: the
        // owner's rule); the genres and the synopsis after them, so Play stays on the first screen under a
        // tall cover, as on the iPhone.
        PrimaryButton(
            label = primary.label,
            icon = PlayFilled,
            modifier = Modifier.fillMaxWidth(),
            enabled = TitleCopy.live(primary, target),
            onClick = {
                when (val to = TitleCopy.playTo(primary, target, versions)) {
                    is PlayTo.Player -> target?.let { onPlay(to.key, it.videoId) }
                    PlayTo.Versions -> target?.let { onVersions(it.videoId) }
                    PlayTo.Nowhere -> Unit
                }
            },
        )
        if (target == null) {
            StatusNote(TitleCopy.nothingToPlay(header.nextAirs))
        } else {
            VersionStatus(VersionsCopy.line(versions, versionsFailed))
        }
        downloads?.problem?.let { StatusNote(it, tone = Status.Warn) }
        var seriesMenu by remember { mutableStateOf(false) }
        var deleteSeason by remember { mutableStateOf<List<DownloadBadge>?>(null) }
        if (seriesMenu && downloads != null) {
            val kept = title.episodes.mapNotNull { it.download }
            val ids = kept.map { it.id }
            ActionMenu(
                title = details.name,
                actions =
                    DownloadCopy.seriesMenu(header.selectedSeason, header.seasonDownload, header.seriesDownload).map {
                        val icon =
                            when (it.act) {
                                GroupAct.DeleteSeason -> Icons.Trash
                                GroupAct.None -> null
                                else -> Icons.ArrowDownToLine
                            }
                        MenuAction(it.label, icon) {
                            // A whole season's delete asks first (code review r1).
                            if (it.act == GroupAct.DeleteSeason) {
                                deleteSeason = kept
                            } else {
                                downloads.group(it.act, header.selectedSeason, ids)
                            }
                        }
                    },
                onDismiss = { seriesMenu = false },
            )
        }
        deleteSeason?.takeIf { downloads != null }?.let { kept ->
            ActionMenu(
                // The episodes already downloaded, never the queued ones the delete also cancels (code review r2 n6).
                title = DownloadCopy.deleteSeries(kept.map { it.state }, details.name, header.selectedSeason),
                actions =
                    listOf(
                        MenuAction("Delete", Icons.Trash) {
                            downloads?.group(GroupAct.DeleteSeason, header.selectedSeason, kept.map { it.id })
                        },
                        MenuAction("Cancel", null) {},
                    ),
                onDismiss = { deleteSeason = null },
            )
        }
        val labels =
            listOfNotNull(
                "Versions".takeIf { target != null && !header.offline },
                "Trailer".takeIf { details.trailers.isNotEmpty() && !header.offline },
                (if (header.inLibrary) "In Library" else "Add to Library").takeIf { header.librarySettable },
                "Watched".takeIf { details.kind == "movie" },
                "Download".takeIf { downloads != null },
            )
        BoxWithConstraints(Modifier.fillMaxWidth().padding(top = Marquee.space.xs)) {
            // The words under the circles only where they fit whole (two lines, no word broken); a large font on a narrow
            // phone gets the circles alone, each still named for TalkBack.
            val words = roundLabelsFit(labels, maxWidth)
            Row(Modifier.fillMaxWidth()) {
                if (target != null && !header.offline) {
                    RoundButton(
                        Icons.Layers,
                        "Versions",
                        { onVersions(target.videoId) },
                        Modifier.weight(1f),
                        showLabel = words,
                    )
                }
                if (details.trailers.isNotEmpty() && !header.offline) {
                    RoundButton(
                        Icons.Film,
                        "Trailer",
                        onTrailer,
                        Modifier.weight(1f),
                        showLabel = words,
                    )
                }
                // A kids profile's Library is the owner's list: the core says so, and no dead button shows.
                if (header.librarySettable) {
                    RoundButton(
                        icon = if (header.inLibrary) Icons.BookmarkCheck else Icons.BookmarkPlus,
                        label = if (header.inLibrary) "In Library" else "Add to Library",
                        onClick = { title.setInLibrary(!header.inLibrary) },
                        modifier = Modifier.weight(1f),
                        selected = header.inLibrary,
                        showLabel = words,
                    )
                }
                // Movies only: an episode carries its own mark.
                if (details.kind == "movie") {
                    RoundButton(
                        icon = Icons.CircleCheck,
                        label = "Watched",
                        onClick = { title.setWatched(details.id, !header.watched) },
                        modifier = Modifier.weight(1f),
                        selected = header.watched,
                        showLabel = words,
                        toggle = true,
                    )
                }
                // Download last (design/DECISION.md, the title's action order): a film's own control, a series' menu.
                if (downloads != null) {
                    if (details.kind == "movie") {
                        val videoId = target?.videoId ?: details.id
                        val badge = header.download
                        val control = DownloadCopy.control(badge)
                        DownloadButton(
                            control,
                            onClick = {
                                if (control.items.isEmpty()) {
                                    downloads.item(DownloadItem.Start, videoId, null)
                                } else {
                                    onDownloadMenu(DownloadAsk(details.name, videoId, badge))
                                }
                            },
                            modifier = Modifier.weight(1f),
                            showLabel = words,
                            onLongClick =
                                if (badge ==
                                    null
                                ) {
                                    ({ onDownloadMenu(DownloadAsk(details.name, videoId, null, START_ITEMS)) })
                                } else {
                                    null
                                },
                        )
                    } else if (header.seriesDownload != null || header.seasonDownload != null) {
                        DownloadButton(
                            DownloadCopy.seriesControl(header.seriesDownload),
                            onClick = { seriesMenu = true },
                            modifier = Modifier.weight(1f),
                            showLabel = words,
                        )
                    }
                } else if (header.download?.state == DownloadState.Done) {
                    // A kids profile: a plain badge, no control (plans/offline-downloads.md 3.2).
                    DownloadedBadge(Modifier.weight(1f))
                }
            }
        }
        Genres(header, details, onGenre)
        details.description?.let { Synopsis(it) }
    }
}

/** "Downloaded" with its check, nothing to press: what a kids profile sees of a download. */
@Composable
private fun DownloadedBadge(modifier: Modifier) {
    Column(
        modifier.semantics(mergeDescendants = true) {},
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Box(Modifier.size(48.dp), contentAlignment = Alignment.Center) {
            MqIcon(Icons.CircleCheck, tint = Marquee.colors.text)
        }
        MqText(
            "Downloaded",
            role = TextRole.Caption,
            color = Marquee.colors.muted,
            weight = FontWeight.SemiBold,
        )
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
    /** The download control's tap; null shows none. */
    download: (() -> Unit)? = null,
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
        download?.let {
            DownloadButton(
                DownloadCopy.control(episode.download),
                it,
                Modifier.align(Alignment.CenterVertically),
                // Ten rows, ten different names: the episode follows the state (code review r1 M3).
                subject = Format.episodeLabel(episode.season, episode.episode),
            )
        }
    }
}
