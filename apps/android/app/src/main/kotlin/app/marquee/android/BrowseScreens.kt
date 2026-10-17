package app.marquee.android

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.foundation.text.BasicTextField
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
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
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
import app.marquee.design.Eyebrow
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MarqueeFonts
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.PosterTile
import app.marquee.design.RowHeader
import app.marquee.design.Skeleton
import app.marquee.design.StateMessage
import app.marquee.design.TextRole
import app.marquee.design.TileShape
import app.marquee.design.dpadExits
import app.marquee.design.laneHeight
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

/** An accent eyebrow over the page title in display type (design/DECISION.md v2, Browse pages). */
@Composable
internal fun BrowseHeader(
    eyebrow: String,
    title: String,
    side: Dp = Marquee.safe.side,
) {
    Column(
        Modifier.padding(horizontal = side),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        Eyebrow(eyebrow)
        MqText(title, role = if (Marquee.form == Form.Tv) TextRole.Title1 else TextRole.Title2, maxLines = 1)
    }
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

@Composable
internal fun <T> ChipRow(
    label: String,
    choices: List<Pair<T, String>>,
    selected: T,
    side: Dp = Marquee.safe.side,
    onSelect: (T) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        Kicker(label, side)
        LazyRow(
            Modifier.focusRestorer(),
            contentPadding = PaddingValues(horizontal = side),
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            items(choices, key = { it.second }) { (value, name) ->
                Chip(name, selected = value == selected, onClick = { onSelect(value) })
            }
        }
    }
}

/** A line of guidance or a reason, with a warning mark when something failed. */
@Composable
internal fun Note(
    text: String,
    failed: Boolean = false,
    side: Dp = Marquee.safe.side,
) {
    Row(
        Modifier.padding(horizontal = side),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (failed) MqIcon(Icons.CircleAlert, tint = Marquee.colors.bad)
        MqText(text, role = TextRole.Callout, color = Marquee.colors.muted)
    }
}

@Composable
fun SearchScreen(
    session: CoreSession,
    onOpen: (Tile) -> Unit,
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
    val searching = holder.query.isNotBlank()
    val loading = snapshot?.rows.orEmpty().any { it.notice() is RowNotice.Waiting }
    LazyColumn(
        // Down from the TV pill comes back to what had focus (plans 2.1).
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") { BrowseHeader("Search", "Find a title") }
        item(key = "field") { SearchField(holder.query, ::send) }
        when {
            view.status is ViewStatus.Failed -> {
                val reason = (view.status as ViewStatus.Failed).reason
                item(key = "failed") { Note("Search could not open: $reason", failed = true) }
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
                                Modifier.focusRestorer(),
                                contentPadding = PaddingValues(horizontal = Marquee.safe.side),
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
                item(key = "none") { Note("No matches for \"${holder.query}\" in your catalogs.") }
            }

            else -> {
                itemsIndexed(snapshot?.rows.orEmpty(), key = { _, row -> row.rowId }) { _, row ->
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
private fun SearchField(
    text: String,
    onText: (String) -> Unit,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    val shape = RoundedCornerShape(Marquee.radius.pill)
    var focused by remember { mutableStateOf(false) }
    Row(
        Modifier
            .padding(horizontal = Marquee.safe.side)
            .fillMaxWidth()
            .height(if (tv) 80.dp else 52.dp)
            .clip(shape)
            .background(colors.raised)
            .border(2.dp, if (focused) colors.accent else colors.control, shape)
            .padding(horizontal = if (tv) 28.dp else 18.dp),
        horizontalArrangement = Arrangement.spacedBy(if (tv) 18.dp else 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(Icons.Search, tint = colors.muted)
        BasicTextField(
            value = text,
            onValueChange = onText,
            singleLine = true,
            textStyle = TextStyle(color = colors.text, fontSize = Marquee.type.body, fontFamily = MarqueeFonts.text),
            cursorBrush = SolidColor(colors.accent),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            modifier =
                Modifier
                    .weight(1f)
                    .dpadExits()
                    .onFocusChanged { focused = it.isFocused }
                    .semantics { contentDescription = "Search titles" },
            decorationBox = { inner ->
                Box(contentAlignment = Alignment.CenterStart) {
                    if (text.isEmpty()) MqText("Search titles", color = colors.dim, maxLines = 1)
                    inner()
                }
            },
        )
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
        RowHeader(row.title, TileCopy.source(row.rowId, row.source), Modifier.padding(horizontal = Marquee.safe.side))
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
    PosterTile(
        title = tile.name,
        imageUrl = tile.poster,
        onClick = { onOpen(tile) },
        modifier = Modifier.padding(top = Marquee.tiles.pad),
        progress = tile.progress,
        caption = caption,
        rating = tile.rating,
    )
}

@Composable
fun DiscoverScreen(
    session: CoreSession,
    onOpen: (Tile) -> Unit,
) {
    val holder =
        viewModel<DiscoverHolder>(
            key = "discover",
            factory = viewModelFactory { initializer { DiscoverHolder(session) } },
        )
    val view = holder.view
    val snapshot = view.snapshot
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
        full("head") { BrowseHeader("Browse", "Discover", side = 0.dp) }
        when {
            view.status is ViewStatus.Failed -> {
                val reason = (view.status as ViewStatus.Failed).reason
                full("failed") { Note("Discover could not open: $reason", failed = true, side = 0.dp) }
            }

            snapshot == null -> {
                full("loading") { Note("Loading catalogs", side = 0.dp) }
            }

            snapshot.kinds.isEmpty() -> {
                full("empty") {
                    Note(
                        "Nothing to browse yet: Discover lists the catalogs of your addons, and none is installed.",
                        side = 0.dp,
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
                    ChipRow("Catalog", snapshot.catalogs.map { it.id to it.name }, snapshot.catalog, 0.dp) { catalog ->
                        if (catalog != null) view.select(catalog = catalog)
                    }
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
                val catalogName = snapshot.catalogs.firstOrNull { it.id == snapshot.catalog }?.name ?: "this catalog"
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
                            Note("$catalogName did not answer: ${state.reason}", failed = true, side = 0.dp)
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
        full("head") { BrowseHeader("Yours", "Library", side = 0.dp) }
        when {
            view.status is ViewStatus.Failed -> {
                val reason = (view.status as ViewStatus.Failed).reason
                full("failed") { Note("Library could not open: $reason", failed = true, side = 0.dp) }
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
