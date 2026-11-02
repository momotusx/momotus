package app.marquee.android

import androidx.activity.compose.LocalActivity
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusGroup
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridScope
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.DiscoverCopy
import app.marquee.core.KidsCopy
import app.marquee.core.ProfileCopy
import app.marquee.core.SearchCopy
import app.marquee.core.TileCopy
import app.marquee.core.model.DiscoverSnapshot
import app.marquee.core.model.DiscoverState
import app.marquee.core.model.HomeRow
import app.marquee.core.model.LibrarySort
import app.marquee.core.model.RowNotice
import app.marquee.core.model.Tile
import app.marquee.core.model.notice
import app.marquee.core.model.tileKeys
import app.marquee.core.session.CoreSession
import app.marquee.core.views.DiscoverView
import app.marquee.core.views.GenreChange
import app.marquee.core.views.LibraryView
import app.marquee.core.views.SearchView
import app.marquee.core.views.ViewStatus
import app.marquee.design.Chip
import app.marquee.design.FieldInput
import app.marquee.design.FocusInk
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.KeepInView
import app.marquee.design.KeyPresses
import app.marquee.design.LiveSpoken
import app.marquee.design.LocalIncognito
import app.marquee.design.Marquee
import app.marquee.design.MarqueeFonts
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.ON_ARRIVAL_MS
import app.marquee.design.PosterTile
import app.marquee.design.PullDown
import app.marquee.design.RowHeader
import app.marquee.design.SecondaryButton
import app.marquee.design.SegmentTrack
import app.marquee.design.ServiceTile
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.TextRole
import app.marquee.design.TileShape
import app.marquee.design.Tokens
import app.marquee.design.focusPalette
import app.marquee.design.laneHeight
import app.marquee.design.liftRoom
import app.marquee.design.marqueeFocus
import app.marquee.ui.phone.LocalBarSpace
import app.marquee.ui.phone.pageTop
import app.marquee.ui.tv.FocusMemory
import app.marquee.ui.tv.LocalTvNav
import app.marquee.ui.tv.requesterFor
import app.marquee.ui.tv.tvContentTop
import app.marquee.ui.tv.watch

// Search and Discover (plans/android-design-v2.md 5.1, 5.2): one layout for TV and phone; the design
// components size themselves by form, and the D-pad moves between them by geometry.

class SearchHolder(
    session: CoreSession,
) : ViewModel() {
    val view: SearchView = session.search()

    /** Survives leaving the screen, so Back lands on the same results. */
    var query by mutableStateOf("")

    override fun onCleared() = view.close()
}

class DiscoverHolder(
    session: CoreSession,
) : ViewModel() {
    val view: DiscoverView = session.discover()

    override fun onCleared() = view.close()
}

/** A genre chip's ask goes only once Discover has its first snapshot: a select sent before it may be dropped. */
internal fun genreAsk(
    request: DiscoverRequest?,
    opened: Boolean,
): DiscoverRequest? = request?.takeIf { opened }

/**
 * A title page's genre ask on Discover: one [select] once Discover is [ready] (its first snapshot), then [onSent]
 * clears it. An ask that could not go is dropped when Discover really leaves, never sent on a later visit; a
 * configuration change ([changingConfigurations]) keeps it for the Discover that comes back.
 */
@Composable
internal fun GenreAskEffect(
    request: DiscoverRequest?,
    ready: Boolean,
    select: (DiscoverRequest) -> Unit,
    onSent: () -> Unit,
    changingConfigurations: () -> Boolean,
) {
    val sent by rememberUpdatedState(onSent)
    val changing by rememberUpdatedState(changingConfigurations)
    DisposableEffect(Unit) { onDispose { if (!changing()) sent() } }
    LaunchedEffect(request, ready) {
        val ask = genreAsk(request, opened = ready) ?: return@LaunchedEffect
        select(ask)
        onSent()
    }
}

/**
 * Where a browse page starts and ends: under the TV pill, or under the status bar and over the phone
 * bar. A deep page ([framed] false) has no pill above it on TV, so it starts at the safe area.
 */
@Composable
internal fun pagePadding(framed: Boolean = true): PaddingValues =
    if (Marquee.form == Form.Tv) {
        PaddingValues(top = if (framed) tvContentTop() else Marquee.safe.top, bottom = Marquee.safe.bottom)
    } else {
        PaddingValues(top = pageTop() + Marquee.space.m, bottom = Marquee.space.l + LocalBarSpace.current)
    }

/** The page title in display type, with no eyebrow over it (design/DECISION.md, Grids). */
@Composable
internal fun BrowseHeader(
    title: String,
    side: Dp = Marquee.safe.side,
) {
    MqText(
        title,
        Modifier.padding(horizontal = side),
        role = if (Marquee.form == Form.Tv) TextRole.Title1 else TextRole.Title2,
        maxLines = 1,
    )
}

/** A small label in `dim` capitals over a row of choices. */
@Composable
internal fun Kicker(
    text: String,
    side: Dp = Marquee.safe.side,
) {
    MqText(
        text.uppercase(),
        modifier = Modifier.padding(horizontal = side),
        role = TextRole.Caption,
        color = Marquee.colors.dim,
        weight = FontWeight.Bold,
    )
}

/** The Calendar row's line in Library. */
private const val CALENDAR_LINE = "New episodes of the series in your library"

/** The Downloads row's line in Library. */
private const val DOWNLOADS_LINE = "Movies and episodes kept on this device"

/** Past this many choices a row of chips is a pull-down on TV. */
private const val SEGMENTS = 4

@Composable
internal fun <T> ChipRow(
    label: String,
    choices: List<Pair<T, String>>,
    selected: T,
    side: Dp = Marquee.safe.side,
    /** A quieter word after a chip's label (a catalog's addon). */
    detail: (T) -> String? = { null },
    onSelect: (T) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        Kicker(label, side)
        // Up to four choices are a segmented control; more are a pull-down on TV (design/DECISION.md, Grids).
        when {
            choices.size <= SEGMENTS -> {
                SegmentTrack(Modifier.focusRestorer(), side = side) {
                    for ((value, name) in choices) {
                        Chip(name, value == selected, { onSelect(value) }, detail = detail(value), inTrack = true)
                    }
                }
            }

            Marquee.form == Form.Tv -> {
                PullDown(label, choices, selected, Modifier.padding(horizontal = side), detail, onSelect = onSelect)
            }

            else -> {
                LazyRow(
                    // Room for a focused chip to rise over its shadow; the page does not move.
                    Modifier.liftRoom(Marquee.tiles.pad).focusRestorer(),
                    contentPadding = PaddingValues(horizontal = side, vertical = Marquee.tiles.pad),
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                ) {
                    // By value: two addons can each have a "Popular" catalog of one kind, and a repeated key throws.
                    items(choices, key = { it.first.toString() }) { (value, name) ->
                        Chip(name, selected = value == selected, onClick = { onSelect(value) }, detail = detail(value))
                    }
                }
            }
        }
    }
}

/** A line of guidance or a reason, with a warning mark when something failed, and Try again when there is one. */
@Composable
internal fun Note(
    text: String,
    failed: Boolean = false,
    side: Dp = Marquee.safe.side,
    onRetry: (() -> Unit)? = null,
    /** Read out once when it appears or changes (a failure or an empty result): off for a hint or a line in a list. */
    spoken: Boolean = false,
    /** What a screen reader says for Try again when the page has more than one. */
    retrySpoken: String? = null,
) {
    Column(Modifier.padding(horizontal = side), verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        val words: @Composable () -> Unit = {
            Row(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (failed) MqIcon(Icons.CircleAlert, tint = Marquee.colors.bad)
                MqText(text, role = TextRole.Callout, color = Marquee.colors.muted)
            }
        }
        if (spoken) LiveSpoken(text, ON_ARRIVAL_MS, words) else words()
        onRetry?.let { SecondaryButton("Try again", it, icon = Icons.RefreshCw, spoken = retrySpoken) }
    }
}

@Composable
fun SearchScreen(
    session: CoreSession,
    onOpen: (Tile) -> Unit,
    /** A person's name from a title page: searched once, as if typed. */
    request: String? = null,
    onRequestSent: () -> Unit = {},
    /** Nothing came back: the recommended addons, which include ones that search. */
    onAddons: () -> Unit,
    /** A kids session: its own list only, never a word of addons (plans/m11-kids.md 6.4). */
    kids: Boolean = false,
) {
    val holder =
        viewModel<SearchHolder>(key = "search", factory = viewModelFactory { initializer { SearchHolder(session) } })
    val view = holder.view
    val snapshot = view.current()
    LaunchedEffect(holder) { if (holder.query.isNotEmpty()) view.setQuery(holder.query) }

    fun send(text: String) {
        holder.query = text
        view.setQuery(text)
    }
    LaunchedEffect(request) {
        request?.let {
            send(it)
            onRequestSent()
        }
    }
    val searching = holder.query.isNotBlank()
    val loading = snapshot?.rows.orEmpty().any { it.notice() is RowNotice.Waiting }
    LazyColumn(
        // Down from the TV pill comes back to what had focus (plans 2.1).
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") { BrowseHeader("Find a title") }
        item(key = "field") { SearchField(holder.query, if (kids) KidsCopy.SEARCH_FIELD else "Search titles", ::send) }
        when {
            view.status is ViewStatus.Failed -> {
                val reason = (view.status as ViewStatus.Failed).reason
                item(key = "failed") {
                    Note("Search could not open: $reason", failed = true, onRetry = view::retry, spoken = true)
                }
            }

            !searching -> {
                val recent = snapshot?.recent.orEmpty()
                // A kid has no recent searches (the core keeps none) and searches no addon.
                if (recent.isEmpty()) {
                    if (!kids) {
                        item(
                            key = "hint",
                        ) { Note("Type a title. Results come from every addon with a search catalog.") }
                    }
                } else {
                    item(key = "recent") {
                        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                            Kicker("Recent searches")
                            LazyRow(
                                Modifier.liftRoom(Marquee.tiles.pad).focusRestorer(),
                                contentPadding =
                                    PaddingValues(
                                        horizontal = Marquee.safe.side,
                                        vertical = Marquee.tiles.pad,
                                    ),
                                horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
                            ) {
                                items(recent, key = { it }) { query ->
                                    Chip(query, selected = false, onClick = { send(query) })
                                }
                            }
                        }
                    }
                }
            }

            snapshot != null && snapshot.query == holder.query && !loading && snapshot.rows.isEmpty() -> {
                item(key = "none") {
                    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                        Note(SearchCopy.empty(holder.query, snapshot.searchable, kids), spoken = true)
                        // No addon can search, or none matched: either way the way on is more addons (not a kid's).
                        if (!kids) {
                            SecondaryButton(
                                SearchCopy.RECOMMENDED,
                                onAddons,
                                Modifier.padding(start = Marquee.safe.side),
                                icon = Icons.ChevronRight,
                            )
                        }
                    }
                }
            }

            else -> {
                val rows = snapshot?.rows.orEmpty()
                itemsIndexed(rows, key = { _, row -> row.rowId }) { _, row ->
                    ResultRow(row) { tile ->
                        view.remember()
                        onOpen(tile)
                    }
                }
            }
        }
    }
}

/** A pill field (design/DECISION.md v2, Search); the edge turns accent while it has focus. */
@Composable
internal fun SearchField(
    text: String,
    label: String = "Search titles",
    onText: (String) -> Unit,
) {
    // The keyboard's Search key puts the keyboard away: the results come as the text changes.
    val keyboard = LocalSoftwareKeyboardController.current
    val tv = Marquee.form == Form.Tv
    val shape = RoundedCornerShape(Marquee.radius.pill)
    var focused by remember { mutableStateOf(false) }
    // On TV the focused field turns white with dark words (focusPalette).
    val colors = focusPalette(focused)
    Row(
        Modifier
            .padding(horizontal = Marquee.safe.side)
            .fillMaxWidth()
            .height(if (tv) 80.dp else 52.dp)
            // The shared focus look, driven by the text input's own focus: never color alone.
            .marqueeFocus(Marquee.radius.pill, Tokens.FOCUS_SCALE_TILE, forced = focused && tv)
            .clip(shape)
            .background(colors.raised)
            // A phone field shows a 2 dp key edge on any focus (a tap included), as the web; no ring.
            .then(if (focused && !tv) Modifier.border(2.dp, colors.key, shape) else Modifier)
            .padding(horizontal = if (tv) 28.dp else 18.dp),
        horizontalArrangement = Arrangement.spacedBy(if (tv) 18.dp else 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(Icons.Search, tint = colors.muted)
        // On a TV a button until it is selected, as every field: Fire TV's keyboard would take the D-pad.
        FocusInk(colors) {
            FieldInput(
                text,
                onText,
                label = label,
                placeholder = label,
                textStyle =
                    TextStyle(
                        color = colors.text,
                        fontSize = Marquee.type.body,
                        fontFamily = MarqueeFonts.text,
                    ),
                onFocus = { focused = it },
                modifier = Modifier.weight(1f),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                keyboardActions = KeyboardActions(onSearch = { keyboard?.hide() }),
            )
        }
    }
}

/** One result row: the header, then the tiles, or its loading or failed state. */
@Composable
private fun ResultRow(
    row: HomeRow,
    onOpen: (Tile) -> Unit,
) {
    val metrics = Marquee.tiles
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        // The group's kind alone ("Movies"), never its addon (design/DECISION.md, Search).
        RowHeader(row.title, null, Modifier.padding(horizontal = Marquee.safe.side))
        when (val notice = row.notice()) {
            is RowNotice.Waiting -> {
                Row(
                    Modifier
                        .height(metrics.laneHeight(TileShape.Poster))
                        .padding(start = Marquee.safe.side, top = metrics.pad),
                    horizontalArrangement = Arrangement.spacedBy(metrics.gap),
                ) { repeat(6) { Skeleton(Modifier.size(metrics.poster.width, metrics.poster.height)) } }
            }

            is RowNotice.Explained -> {
                StateMessage(
                    title = notice.title,
                    body = notice.body,
                    icon = if (notice.failed) Icons.CircleAlert else null,
                    modifier = Modifier.padding(horizontal = Marquee.safe.side),
                )
            }

            null -> {
                val keys = remember(row.tiles) { row.tileKeys() }
                LazyRow(
                    Modifier.height(metrics.laneHeight(TileShape.Poster)).focusRestorer(),
                    contentPadding = PaddingValues(horizontal = Marquee.safe.side),
                    horizontalArrangement = Arrangement.spacedBy(metrics.gap),
                ) {
                    itemsIndexed(row.tiles, key = { index, _ -> keys[index] }) { _, tile ->
                        Poster(tile, onOpen)
                    }
                }
            }
        }
    }
}

@Composable
internal fun Poster(
    tile: Tile,
    onOpen: (Tile) -> Unit,
    modifier: Modifier = Modifier,
    caption: String? = tile.releaseInfo,
    corner: String? = null,
    focusRequester: FocusRequester? = null,
    onFocusChange: ((Boolean) -> Unit)? = null,
    enabled: Boolean = true,
) {
    val (art, fallback) = TileCopy.art(tile, wide = false)
    PosterTile(
        title = tile.name,
        imageUrl = art,
        fallbackImageUrl = fallback,
        onClick = { onOpen(tile) },
        modifier = Modifier.padding(top = Marquee.tiles.pad).then(modifier),
        progress = tile.progress,
        caption = caption,
        ratings = TileCopy.ratings(tile.ratings),
        cornerLabel = corner,
        focusRequester = focusRequester,
        onFocusChange = onFocusChange,
        enabled = enabled,
    )
}

@Composable
fun DiscoverScreen(
    session: CoreSession,
    onOpen: (Tile) -> Unit,
    onSetup: () -> Unit,
    onAddons: () -> Unit,
    /** A genre chip's ask (a title page's genre), sent once Discover has its first snapshot. */
    request: DiscoverRequest? = null,
    onRequestSent: () -> Unit = {},
    /** Where focus was on Discover (TV): each control row and the grid remember their last focused item. */
    focus: FocusMemory = remember { FocusMemory() },
) {
    val holder =
        viewModel<DiscoverHolder>(
            key = "discover",
            factory = viewModelFactory { initializer { DiscoverHolder(session) } },
        )
    val view = holder.view
    val snapshot = view.current()
    // One select of the kind and the genre, after the first snapshot: the core reads it as a link (C40).
    val activity = LocalActivity.current
    GenreAskEffect(
        request,
        ready = snapshot != null,
        select = { view.select(kind = it.kind, genre = GenreChange.To(it.genre)) },
        onSent = onRequestSent,
        changingConfigurations = { activity?.isChangingConfigurations == true },
    )
    DiscoverContent(
        snapshot,
        failed = view.status is ViewStatus.Failed,
        pick = {
            view.select(
                kind = it.kind,
                genre = it.genre,
                platform = it.platform,
                sort = it.sort,
                decade = it.decade,
            )
        },
        loadMore = view::loadMore,
        retry = view::retry,
        onOpen = onOpen,
        onSetup = onSetup,
        onAddons = onAddons,
        focus = focus,
    )
}

/** One `select` of Discover (plans/discover-v2.md section 4): only the keys that change. */
internal data class DiscoverPick(
    val kind: String? = null,
    val genre: GenreChange = GenreChange.Keep,
    val platform: GenreChange = GenreChange.Keep,
    val sort: String? = null,
    val decade: GenreChange = GenreChange.Keep,
)

/** Discover's page for a snapshot, apart from its view, so its rules are tested with snapshots made by hand. */
@Composable
internal fun DiscoverContent(
    snapshot: DiscoverSnapshot?,
    failed: Boolean,
    pick: (DiscoverPick) -> Unit,
    loadMore: () -> Unit,
    retry: () -> Unit,
    onOpen: (Tile) -> Unit,
    onSetup: () -> Unit,
    onAddons: () -> Unit,
    focus: FocusMemory,
) {
    val metrics = Marquee.tiles
    val side = Marquee.safe.side
    val tv = Marquee.form == Form.Tv
    val grid = rememberLazyGridState()
    // What the grid shows (6.1): the core's items, except that on TV a late list that reorders the grid waits while
    // focus is in it ("More titles arrived. Show"), and a list loading after a change keeps the one before, dimmed.
    var shown by remember { mutableStateOf<List<Tile>>(emptyList()) }
    var held by remember { mutableStateOf(false) }
    var inGrid by remember { mutableStateOf(false) }
    // One tile per id: the grid is keyed by id, so a late list keeps the tiles under the thumb in place (6.1).
    val items = remember(snapshot?.items) { snapshot?.items.orEmpty().distinctBy { it.id } }
    LaunchedEffect(items, inGrid) {
        val grows = items.size >= shown.size && items.take(shown.size).map { it.id } == shown.map { it.id }
        if (!tv || !inGrid || grows || items.isEmpty()) {
            if (items.isNotEmpty() || snapshot?.state !is DiscoverState.Loading) shown = items
            held = false
        } else {
            held = items.map { it.id } != shown.map { it.id }
        }
    }
    val dimmed = items.isEmpty() && snapshot?.state is DiscoverState.Loading && shown.isNotEmpty()
    // TV, Back from a title: focus returns to the tile that opened it (or wherever it was last), as on Home. The grid's
    // scroll is saved with the page, so that tile is composed again. Not when the page was shown from the bar.
    val nav = LocalTvNav.current
    LaunchedEffect(focus) {
        if (!tv || nav?.heldByKey == true) return@LaunchedEffect
        val key = focus.focusedKey ?: return@LaunchedEffect
        val asked = KeyPresses.count
        repeat(RESTORE_FRAMES) {
            withFrameNanos { }
            if (KeyPresses.count != asked) return@LaunchedEffect
            val requester = focus.requester(key) ?: return@repeat
            if (runCatching { requester.requestFocus() }.getOrDefault(false)) return@LaunchedEffect
        }
    }
    // Ask for the next page as the end comes into view.
    val current by rememberUpdatedState(snapshot)
    val more by rememberUpdatedState(loadMore)
    LaunchedEffect(grid) {
        snapshotFlow {
            grid.layoutInfo.visibleItemsInfo
                .lastOrNull()
                ?.index to grid.layoutInfo.totalItemsCount
        }.collect { (last, total) ->
            if (last != null && current?.state is DiscoverState.Ready && last >= total - LOAD_AHEAD) more()
        }
    }
    LazyVerticalGrid(
        columns = GridCells.Adaptive(metrics.poster.width),
        state = grid,
        modifier = Modifier.fillMaxSize(),
        contentPadding =
            pagePadding().let {
                PaddingValues(
                    start = side,
                    end = side,
                    top = it.calculateTopPadding(),
                    bottom = it.calculateBottomPadding(),
                )
            },
        horizontalArrangement = Arrangement.spacedBy(metrics.gap),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        full("head") { BrowseHeader("Discover", side = 0.dp) }
        when {
            failed -> {
                // The core's reason can name a host or an addon: the shared line instead.
                full("failed") { Note(DiscoverCopy.OPEN_FAILED, failed = true, side = 0.dp, onRetry = retry) }
            }

            snapshot == null -> {
                full("loading") { Note("Loading catalogs", side = 0.dp) }
            }

            snapshot.kinds.isEmpty() -> {
                full("empty") {
                    // The ways out, as on Home: setup offers the recommended addons in one step (no typing
                    // with a remote), Settings holds the addons. On TV focus lands on the first.
                    StateMessage(
                        title = "Nothing to browse yet",
                        body =
                            "Discover lists the catalogs of the addons you have on, and none of them has one. " +
                                "Set up Momotus offers the recommended addons in one step.",
                        actionLabel = "Set up Momotus",
                        onAction = onSetup,
                        secondaryLabel = "Add an addon",
                        onSecondary = onAddons,
                    )
                }
            }

            else -> {
                full(ROW_KINDS) { DiscoverKinds(snapshot, pick, focus) }
                if (snapshot.platforms.isNotEmpty()) full(ROW_RAIL) { DiscoverRail(snapshot, pick, focus) }
                full(ROW_FILTERS) {
                    DiscoverFilters(snapshot, pick, focus, held) {
                        shown = items
                        held = false
                    }
                }
                val state = snapshot.state
                when {
                    snapshot.genreRequired && snapshot.genre == null -> {
                        full("pick") {
                            Note(
                                "This list only answers for one ${snapshot.genreLabel.lowercase()} at a time. Pick one above.",
                                side = 0.dp,
                            )
                        }
                    }

                    dimmed -> {
                        // The list before, dimmed and out of focus's reach, until the new one lands.
                        items(shown, key = { "prev:${it.id}" }) { tile ->
                            Poster(tile, onOpen, modifier = Modifier.alpha(DIMMED), enabled = false)
                        }
                    }

                    shown.isEmpty() && state is DiscoverState.Loading -> {
                        // Blank covers in the grid's own cells, so they are the size and count the tiles will be.
                        items(SKELETONS, key = { "skeleton:$it" }) {
                            Skeleton(
                                Modifier.padding(top = metrics.pad).size(metrics.poster.width, metrics.poster.height),
                            )
                        }
                    }

                    items.isEmpty() && state is DiscoverState.Failed -> {
                        full(
                            "failed-items",
                        ) { Note(DiscoverCopy.FAILED, failed = true, side = 0.dp, onRetry = loadMore) }
                    }

                    items.isEmpty() -> {
                        full("nothing") { Note(DiscoverCopy.empty(snapshot), side = 0.dp) }
                    }

                    else -> {
                        items(shown, key = { it.id }) { tile ->
                            val key = "tile:${tile.id}"
                            // A tile taken out while it has focus never says it lost it.
                            DisposableEffect(key) { onDispose { if (focus.focusedKey == key) inGrid = false } }
                            Poster(
                                tile,
                                onOpen,
                                corner = DiscoverCopy.corner(snapshot.sort, tile),
                                caption = DiscoverCopy.caption(snapshot.sort, tile),
                                focusRequester = focus.requesterFor(key),
                                onFocusChange = { gained ->
                                    focus.watch(ROW_GRID, key)(gained)
                                    inGrid = gained
                                },
                            )
                        }
                        full("foot") {
                            when (state) {
                                is DiscoverState.Failed -> {
                                    Note(DiscoverCopy.MORE_FAILED, failed = true, side = 0.dp, onRetry = loadMore)
                                }

                                else -> {
                                    MqText(
                                        if (state is DiscoverState.Loading) {
                                            "Loading more"
                                        } else {
                                            DiscoverCopy.scope(
                                                snapshot,
                                            )
                                        },
                                        modifier = Modifier.padding(vertical = Marquee.space.s),
                                        role = TextRole.Callout,
                                        color = Marquee.colors.muted,
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/**
 * Movies, Series and Anime as the segmented control; any other kind is in a More pull-down, so the control keeps its
 * shape (plans/discover-v2.md 6.1). On a narrow phone More goes to a second line rather than off the screen.
 */
@Composable
private fun DiscoverKinds(
    s: DiscoverSnapshot,
    pick: (DiscoverPick) -> Unit,
    focus: FocusMemory,
) {
    val main = s.kinds.filter { it.first in MAIN_KINDS }
    val more = s.kinds.filter { it.first !in MAIN_KINDS }
    FlowRow(
        Modifier.rowEntry(focus, ROW_KINDS, DISCOVER_KIND_ENTRY),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        if (main.isNotEmpty()) {
            SegmentTrack {
                for ((value, name) in main) {
                    val key = "kind:$value"
                    val requester = focus.requesterFor(key)
                    if (value == s.kind) focus.EntryAlias(DISCOVER_KIND_ENTRY, requester)
                    Chip(
                        name,
                        value == s.kind,
                        { pick(DiscoverPick(kind = value)) },
                        inTrack = true,
                        focusRequester = requester,
                        onFocusChange = focus.watch(ROW_KINDS, key),
                    )
                }
            }
        }
        if (more.isNotEmpty()) {
            val key = "kind:more"
            val chosen = s.kind?.takeIf { it !in MAIN_KINDS }
            val requester = focus.requesterFor(key)
            if (chosen != null || main.isEmpty()) focus.EntryAlias(DISCOVER_KIND_ENTRY, requester)
            PullDown(
                "More",
                more.map { it.first to it.second },
                chosen,
                Modifier
                    .focusRequester(requester)
                    .onFocusChanged { focus.watch(ROW_KINDS, key)(it.isFocused) },
            ) { kind -> kind?.let { pick(DiscoverPick(kind = it)) } }
        }
    }
}

/** The services as their logos, All first (6.6); the chosen one is ringed and kept in view, with room for its ring. */
@Composable
private fun DiscoverRail(
    s: DiscoverSnapshot,
    pick: (DiscoverPick) -> Unit,
    focus: FocusMemory,
) {
    val rail = rememberLazyListState()
    // Again when the logos come: a name tile and a logo are not one width, so the chosen one may have moved.
    key(s.platforms.map { it.logo }) { KeepInView(rail, s.platforms.indexOfFirst { it.id == s.platform } + 1) }
    LazyRow(
        state = rail,
        modifier = Modifier.liftRoom(Marquee.tiles.pad).rowEntry(focus, ROW_RAIL),
        contentPadding = PaddingValues(horizontal = RING_ROOM, vertical = Marquee.tiles.pad),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        item(key = "all") {
            ServiceTile(
                "All",
                null,
                s.platform == null,
                { pick(DiscoverPick(platform = GenreChange.Clear)) },
                focusRequester = focus.requesterFor("rail:all"),
                onFocusChange = focus.watch(ROW_RAIL, "rail:all"),
            )
        }
        items(s.platforms, key = { it.id }) { p ->
            val key = "rail:${p.id}"
            ServiceTile(
                p.name,
                p.logo,
                s.platform == p.id,
                { pick(DiscoverPick(platform = GenreChange.To(p.id))) },
                focusRequester = focus.requesterFor(key),
                onFocusChange = focus.watch(ROW_RAIL, key),
            )
        }
    }
}

/**
 * Genre, Year, Clear and Sort (6.1): one line where it fits with Sort at its end, wrapping on a narrow phone or at a
 * large font, Sort last.
 */
@Composable
private fun DiscoverFilters(
    s: DiscoverSnapshot,
    pick: (DiscoverPick) -> Unit,
    focus: FocusMemory,
    held: Boolean,
    onShow: () -> Unit,
) {
    val genreKey = "filters:genre"
    val yearKey = "filters:year"
    val filtered = s.genre != null || s.decade != null || s.platform != null
    // Clear hands focus to the first menu that stays, else the rail's All (6.5); its requester is looked up at the
    // press, as it is registered only after the first composition.
    val handOff =
        when {
            s.genreChoices.isNotEmpty() -> genreKey
            s.decades.isNotEmpty() -> yearKey
            else -> "rail:all"
        }
    FlowRow(
        Modifier.fillMaxWidth().rowEntry(focus, ROW_FILTERS),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        if (s.genreChoices.isNotEmpty()) {
            val any: List<Pair<String?, String>> =
                if (s.genreRequired) emptyList() else listOf(null to "Any ${s.genreLabel.lowercase()}")
            PullDown(
                s.genreLabel,
                any + s.genreChoices.map { it.name to it.name },
                s.genre,
                Modifier
                    .focusRequester(focus.requesterFor(genreKey))
                    .onFocusChanged { focus.watch(ROW_FILTERS, genreKey)(it.isFocused) },
            ) { pick(DiscoverPick(genre = if (it == null) GenreChange.Clear else GenreChange.To(it))) }
        }
        if (s.decades.isNotEmpty()) {
            PullDown(
                "Year",
                listOf<Pair<String?, String>>(null to "Any year") + s.decades.map { it to it },
                s.decade,
                Modifier
                    .focusRequester(focus.requesterFor(yearKey))
                    .onFocusChanged { focus.watch(ROW_FILTERS, yearKey)(it.isFocused) },
            ) { pick(DiscoverPick(decade = if (it == null) GenreChange.Clear else GenreChange.To(it))) }
        }
        if (filtered) {
            SecondaryButton(
                "Clear",
                {
                    runCatching { focus.requester(handOff)?.requestFocus() }
                    pick(
                        DiscoverPick(
                            genre = GenreChange.Clear,
                            platform = GenreChange.Clear,
                            decade = GenreChange.Clear,
                        ),
                    )
                },
                Modifier.onFocusChanged { focus.watch(ROW_FILTERS, "filters:clear")(it.isFocused) },
                spoken = "Clear filters",
            )
        }
        if (held) SecondaryButton("More titles arrived. Show", onShow)
        val sorts: List<Pair<String?, String>> =
            s.sorts.map { it.id to it.label } + s.lists.map { "list:${it.id}" to it.name }
        if (sorts.isNotEmpty()) {
            Spacer(Modifier.weight(1f))
            PullDown(
                "Sort",
                sorts,
                s.sort,
                Modifier
                    .focusRequester(focus.requesterFor("filters:sort"))
                    .onFocusChanged { focus.watch(ROW_FILTERS, "filters:sort")(it.isFocused) },
                heading = { if (it?.startsWith("list:") == true) DiscoverCopy.MORE_LISTS else null },
            ) { sort -> sort?.let { pick(DiscoverPick(sort = it)) } }
        }
    }
}

/** Vertical entry into a control row lands where the row last had focus (6.5), else on [fallback], as Home's lanes do. */
@OptIn(ExperimentalComposeUiApi::class)
private fun Modifier.rowEntry(
    focus: FocusMemory,
    row: String,
    fallback: String? = null,
): Modifier =
    focusProperties {
        enter = { focus.rowEntry(row) ?: fallback?.let(focus::requester) ?: FocusRequester.Default }
    }.focusGroup()

/** [requester] registered under [key] as well while composed: the selected kind is where Discover is entered first. */
@Composable
private fun FocusMemory.EntryAlias(
    key: String,
    requester: FocusRequester,
) {
    DisposableEffect(key, requester) {
        attach(key, requester)
        onDispose { detach(key, requester) }
    }
}

/** Where the tab bar's Down lands on a first open: the selected kind (6.5), never the grid by geometry. */
internal const val DISCOVER_KIND_ENTRY = "kinds:entry"

/** Blank covers while a first page loads. */
private const val SKELETONS = 6

/** Frames a return to Discover tries to put focus back, while the grid composes its tiles. */
private const val RESTORE_FRAMES = 60

/** Room at the rail's ends for the chosen tile's ring. */
private val RING_ROOM = 8.dp

/** The previous list's opacity while the next one loads. */
private const val DIMMED = 0.45f

private const val ROW_KINDS = "kinds"
private const val ROW_RAIL = "rail"
private const val ROW_FILTERS = "filters"
private const val ROW_GRID = "grid"

/** Movies, Series and Anime: the segmented control (plans/discover-v2.md 6.1). */
private val MAIN_KINDS = setOf("movie", "series", "anime")

/** A grid item across the whole width. */
internal fun LazyGridScope.full(
    key: String,
    content: @Composable () -> Unit,
) = item(key = key, span = { GridItemSpan(maxLineSpan) }) { content() }

/** How many tiles before the end the next page is asked for. */
private const val LOAD_AHEAD = 12

class LibraryHolder(
    session: CoreSession,
) : ViewModel() {
    val view: LibraryView = session.library()

    override fun onCleared() = view.close()
}

/**
 * The library (plans/android-design-v2.md 5.3): type chips with their counts, the sort, and a
 * poster grid whose line under each tile is the core's own ("S2:E3 · 36 min left", "Watched"),
 * or the type for a title never played.
 */
@Composable
fun LibraryScreen(
    session: CoreSession,
    onOpen: (Tile) -> Unit,
    /** The phone's Calendar row at the top, where the TV has a Calendar tab; null shows none. */
    onCalendar: (() -> Unit)? = null,
    /** An empty library's way on: Discover. */
    onDiscover: (() -> Unit)? = null,
    /** The phone's Downloads row, first (plans/offline-downloads.md 3.4); null shows none. */
    onDownloads: (() -> Unit)? = null,
) {
    val holder =
        viewModel<LibraryHolder>(
            key = "library",
            factory = viewModelFactory { initializer { LibraryHolder(session) } },
        )
    val view = holder.view
    val snapshot = view.current()
    val metrics = Marquee.tiles
    val side = Marquee.safe.side
    LazyVerticalGrid(
        columns = GridCells.Adaptive(metrics.poster.width),
        modifier = Modifier.fillMaxSize().focusRestorer(),
        contentPadding =
            pagePadding().let {
                PaddingValues(
                    start = side,
                    end = side,
                    top = it.calculateTopPadding(),
                    bottom = it.calculateBottomPadding(),
                )
            },
        horizontalArrangement = Arrangement.spacedBy(metrics.gap),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        full("head") { BrowseHeader("Library", side = 0.dp) }
        if (onDownloads != null) {
            full("downloads") { LinkRow(Icons.ArrowDownToLine, "Downloads", DOWNLOADS_LINE, onDownloads, side = 0.dp) }
        }
        if (onCalendar != null) {
            full("calendar") { LinkRow(Icons.CalendarDays, "Calendar", CALENDAR_LINE, onCalendar, side = 0.dp) }
        }
        when {
            view.status is ViewStatus.Failed -> {
                val reason = (view.status as ViewStatus.Failed).reason
                full(
                    "failed",
                ) {
                    Note(
                        "Library could not open: $reason",
                        failed = true,
                        side = 0.dp,
                        onRetry = view::retry,
                        spoken = true,
                    )
                }
            }

            snapshot == null -> {
                full("loading") { Note("Loading your library", side = 0.dp) }
            }

            snapshot.items.isEmpty() && snapshot.kind == null -> {
                full("empty") {
                    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                        Note(
                            // Incognito's Library is its own, and goes with it (plans/batch-10-incognito.md 6).
                            if (LocalIncognito.current) {
                                ProfileCopy.INCOGNITO_LIBRARY_EMPTY
                            } else {
                                "Your library is empty. Add a title from its page and it shows here."
                            },
                            side = 0.dp,
                        )
                        onDiscover?.let { SecondaryButton("Discover", it, icon = Icons.Compass) }
                    }
                }
            }

            else -> {
                full("kinds") {
                    val total = snapshot.kinds.sumOf { it.count }
                    val all: List<Pair<String?, String>> = listOf(null to "All $total")
                    val kinds = all + snapshot.kinds.map { it.kind to "${it.label} ${it.count}" }
                    ChipRow("Type", kinds, snapshot.kind, 0.dp) { view.setFilter(it) }
                }
                full("sort") {
                    val sorts =
                        listOf(
                            LibrarySort.Recent to "Recently watched",
                            LibrarySort.Added to "Added",
                            LibrarySort.Name to "Name",
                        )
                    ChipRow("Sort", sorts, snapshot.sort, 0.dp) { view.setSort(it) }
                }
                if (snapshot.items.isEmpty()) {
                    full("none") { Note("Nothing of this type in your library.", side = 0.dp, spoken = true) }
                } else {
                    itemsIndexed(snapshot.items, key = { index, tile -> "${tile.id}#$index" }) { _, tile ->
                        Poster(tile, onOpen, caption = tile.releaseInfo ?: TileCopy.kind(tile.kind))
                    }
                }
            }
        }
    }
}
