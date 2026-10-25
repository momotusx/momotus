package app.marquee.android

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyGridScope
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.itemsIndexed
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.focusRestorer
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
import app.marquee.core.SearchCopy
import app.marquee.core.TileCopy
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
import app.marquee.design.Marquee
import app.marquee.design.MarqueeFonts
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.PosterTile
import app.marquee.design.PullDown
import app.marquee.design.RowHeader
import app.marquee.design.SecondaryButton
import app.marquee.design.SegmentTrack
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
import app.marquee.ui.tv.tvContentTop

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
 * Where a browse page starts and ends: under the TV pill, or under the status bar and over the phone
 * bar. A deep page ([framed] false) has no pill above it on TV, so it starts at the safe area.
 */
@Composable
internal fun pagePadding(framed: Boolean = true): PaddingValues =
    if (Marquee.form == Form.Tv) {
        PaddingValues(top = if (framed) tvContentTop() else Marquee.safe.top, bottom = Marquee.safe.bottom)
    } else {
        val status = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
        PaddingValues(top = status + Marquee.space.m, bottom = Marquee.space.l + LocalBarSpace.current)
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
                PullDown(label, choices, selected, Modifier.padding(horizontal = side), detail, onSelect)
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
) {
    Column(Modifier.padding(horizontal = side), verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (failed) MqIcon(Icons.CircleAlert, tint = Marquee.colors.bad)
            MqText(text, role = TextRole.Callout, color = Marquee.colors.muted)
        }
        onRetry?.let { SecondaryButton("Try again", it, icon = Icons.RefreshCw) }
    }
}

@Composable
fun SearchScreen(
    session: CoreSession,
    onOpen: (Tile) -> Unit,
    /** A person's name from a title page: searched once, as if typed. */
    request: String? = null,
    onRequestSent: () -> Unit = {},
    /** No addon can search: the recommended addons, which include one that does. */
    onAddons: () -> Unit,
) {
    val holder =
        viewModel<SearchHolder>(key = "search", factory = viewModelFactory { initializer { SearchHolder(session) } })
    val view = holder.view
    val snapshot = view.snapshot
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
        item(key = "field") { SearchField(holder.query, ::send) }
        when {
            view.status is ViewStatus.Failed -> {
                val reason = (view.status as ViewStatus.Failed).reason
                item(key = "failed") { Note("Search could not open: $reason", failed = true, onRetry = view::retry) }
            }

            !searching -> {
                val recent = snapshot?.recent.orEmpty()
                if (recent.isEmpty()) {
                    item(key = "hint") { Note("Type a title. Results come from every addon with a search catalog.") }
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
                        Note(SearchCopy.empty(holder.query, snapshot.searchable))
                        // No addon can search: nothing could have matched, so the way on is one that does.
                        if (snapshot.searchable == 0) {
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
                label = "Search titles",
                placeholder = "Search titles",
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
    caption: String? = tile.releaseInfo,
) {
    val (art, fallback) = TileCopy.art(tile, wide = false)
    PosterTile(
        title = tile.name,
        imageUrl = art,
        fallbackImageUrl = fallback,
        onClick = { onOpen(tile) },
        modifier = Modifier.padding(top = Marquee.tiles.pad),
        progress = tile.progress,
        caption = caption,
        ratings = TileCopy.ratings(tile.ratings),
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
) {
    val holder =
        viewModel<DiscoverHolder>(
            key = "discover",
            factory = viewModelFactory { initializer { DiscoverHolder(session) } },
        )
    val view = holder.view
    val snapshot = view.snapshot
    // One select of the kind and the genre, after the first snapshot: the core picks the catalog (C40).
    LaunchedEffect(request, snapshot != null) {
        val ask = genreAsk(request, opened = snapshot != null) ?: return@LaunchedEffect
        view.select(kind = ask.kind, genre = GenreChange.To(ask.genre))
        onRequestSent()
    }
    val metrics = Marquee.tiles
    val side = Marquee.safe.side
    val grid = rememberLazyGridState()
    // Ask for the next page as the end comes into view.
    LaunchedEffect(grid, view) {
        snapshotFlow {
            grid.layoutInfo.visibleItemsInfo
                .lastOrNull()
                ?.index to grid.layoutInfo.totalItemsCount
        }.collect { (last, total) ->
            val ready = view.snapshot?.state is DiscoverState.Ready
            if (last != null && ready && last >= total - LOAD_AHEAD) view.loadMore()
        }
    }
    LazyVerticalGrid(
        columns = GridCells.Adaptive(metrics.poster.width),
        state = grid,
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
        full("head") { BrowseHeader("Discover", side = 0.dp) }
        when {
            view.status is ViewStatus.Failed -> {
                val reason = (view.status as ViewStatus.Failed).reason
                full(
                    "failed",
                ) { Note("Discover could not open: $reason", failed = true, side = 0.dp, onRetry = view::retry) }
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
                                "Set up Marquee offers the recommended addons in one step.",
                        actionLabel = "Set up Marquee",
                        onAction = onSetup,
                        secondaryLabel = "Add an addon",
                        onSecondary = onAddons,
                    )
                }
            }

            else -> {
                full("kinds") {
                    ChipRow("Type", snapshot.kinds.map { it.first to it.second }, snapshot.kind, 0.dp) { kind ->
                        if (kind != null) view.select(kind = kind)
                    }
                }
                full("catalogs") {
                    // A catalog by its name alone, never its addon's (design/DECISION.md, Shelves).
                    ChipRow(
                        "Catalog",
                        snapshot.catalogs.map { it.id to it.name },
                        snapshot.catalog,
                        0.dp,
                    ) { catalog -> if (catalog != null) view.select(catalog = catalog) }
                }
                if (snapshot.genres.isNotEmpty()) {
                    full("genres") {
                        val all: List<Pair<String?, String>> =
                            if (snapshot.genreRequired) emptyList() else listOf(null to "All")
                        ChipRow("Genre", all + snapshot.genres.map { it to it }, snapshot.genre, 0.dp) { genre ->
                            view.select(genre = if (genre == null) GenreChange.Clear else GenreChange.To(genre))
                        }
                    }
                }
                // The catalog by its name, in its failed and empty states too.
                val chosen = snapshot.catalogs.firstOrNull { it.id == snapshot.catalog }
                val catalogName = chosen?.name ?: "this catalog"
                val state = snapshot.state
                when {
                    snapshot.genreRequired && snapshot.genre == null -> {
                        full("pick") {
                            Note("$catalogName only answers for one genre at a time. Pick a genre above.", side = 0.dp)
                        }
                    }

                    snapshot.items.isEmpty() && state is DiscoverState.Loading -> {
                        full("skeleton") {
                            Row(
                                Modifier.padding(top = metrics.pad),
                                horizontalArrangement = Arrangement.spacedBy(metrics.gap),
                            ) { repeat(6) { Skeleton(Modifier.size(metrics.poster.width, metrics.poster.height)) } }
                        }
                    }

                    snapshot.items.isEmpty() && state is DiscoverState.Failed -> {
                        full("failed-items") {
                            Note("$catalogName did not load: ${state.reason}", failed = true, side = 0.dp)
                        }
                    }

                    snapshot.items.isEmpty() -> {
                        full("nothing") { Note("Nothing in $catalogName for this choice.", side = 0.dp) }
                    }

                    else -> {
                        itemsIndexed(snapshot.items, key = { index, tile -> "${tile.id}#$index" }) { index, tile ->
                            Poster(tile, onOpen)
                        }
                        full("foot") {
                            val foot =
                                when (state) {
                                    is DiscoverState.Loading -> "Loading more"
                                    is DiscoverState.End -> "All ${snapshot.items.size} titles in $catalogName"
                                    is DiscoverState.Failed -> "Could not load more: ${state.reason}"
                                    else -> "${snapshot.items.size} titles"
                                }
                            MqText(
                                foot,
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
) {
    val holder =
        viewModel<LibraryHolder>(
            key = "library",
            factory = viewModelFactory { initializer { LibraryHolder(session) } },
        )
    val view = holder.view
    val snapshot = view.snapshot
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
        if (onCalendar != null) {
            full("calendar") { LinkRow(Icons.CalendarDays, "Calendar", CALENDAR_LINE, onCalendar, side = 0.dp) }
        }
        when {
            view.status is ViewStatus.Failed -> {
                val reason = (view.status as ViewStatus.Failed).reason
                full(
                    "failed",
                ) { Note("Library could not open: $reason", failed = true, side = 0.dp, onRetry = view::retry) }
            }

            snapshot == null -> {
                full("loading") { Note("Loading your library", side = 0.dp) }
            }

            snapshot.items.isEmpty() && snapshot.kind == null -> {
                full("empty") {
                    Note("Your library is empty. Add a title from its page and it shows here.", side = 0.dp)
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
                    full("none") { Note("Nothing of this type in your library.", side = 0.dp) }
                } else {
                    itemsIndexed(snapshot.items, key = { index, tile -> "${tile.id}#$index" }) { _, tile ->
                        Poster(tile, onOpen, caption = tile.releaseInfo ?: TileCopy.kind(tile.kind))
                    }
                }
            }
        }
    }
}
