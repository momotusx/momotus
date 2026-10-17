package app.marquee.android

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.HomeCatalog
import app.marquee.core.session.CoreOps
import app.marquee.core.session.CoreSession
import app.marquee.design.Chip
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.KeepInView
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.RoundButton
import app.marquee.design.TextRole
import app.marquee.design.focusClickable
import app.marquee.design.marqueeFocus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

// Home rows (plans/android-design-v2.md 13e, the web's home-rows screen): which addon catalogs Home
// shows after Continue Watching, and in what order. Home follows each change live.

/** The list from `homeCatalogs`, replaced by the reply to every edit; edits run one at a time, in order. */
class HomeRowsHolder(
    private val ops: CoreOps,
) : ViewModel() {
    var rows: List<HomeCatalog>? by mutableStateOf(null)
        private set
    var failure: String? by mutableStateOf(null)
        private set
    private val one = Mutex()

    init {
        edit { ops.homeCatalogs() }
    }

    fun toggle(row: HomeCatalog) = edit { ops.setHomeRow(row.rowId, !row.hidden) }

    fun move(
        row: HomeCatalog,
        by: Int,
    ) = edit { ops.moveHomeRow(row.rowId, by) }

    private fun edit(op: suspend () -> List<HomeCatalog>) {
        viewModelScope.launch {
            one.withLock {
                try {
                    rows = op()
                    failure = null
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    failure = (e as? CoreError)?.message ?: CoreLog.describe(e)
                }
            }
        }
    }
}

@Composable
fun HomeRowsScreen(session: CoreSession) {
    val holder =
        viewModel<HomeRowsHolder>(
            key = "home-rows",
            factory = viewModelFactory { initializer { HomeRowsHolder(session.ops) } },
        )
    val tv = Marquee.form == Form.Tv
    val rows = holder.rows
    // TV: the row picked up; Up and Down move it, select or Back puts it down.
    var picked by rememberSaveable { mutableStateOf<String?>(null) }
    BackHandler(enabled = picked != null) { picked = null }
    val state = rememberLazyListState()
    // A picked row stays in sight while it moves (one header item comes first).
    KeepInView(state, rows?.indexOfFirst { it.rowId == picked }?.takeIf { it >= 0 }?.plus(1) ?: -1)
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        state = state,
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        item(key = "head") {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                BrowseHeader("Settings", "Home rows")
                MqText(
                    "Continue Watching always comes first. Choose which catalogs follow it, and their order." +
                        if (tv) PICK_HINT else "",
                    modifier = Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 900.dp),
                    role = TextRole.Callout,
                    color = Marquee.colors.muted,
                )
                holder.failure?.let { Note(it, failed = true) }
                when {
                    rows == null && holder.failure == null -> Note("Loading")
                    rows?.isEmpty() == true -> Note("No addon gives Home a catalog yet. Addons with catalogs add rows.")
                }
            }
        }
        itemsIndexed(rows.orEmpty(), key = { _, row -> row.rowId }) { index, row ->
            CatalogRow(
                row = row,
                place = index + 1,
                count = rows.orEmpty().size,
                picked = picked == row.rowId,
                onPick = { picked = if (picked == row.rowId) null else row.rowId },
                onToggle = { holder.toggle(row) },
                onMove = { by -> holder.move(row, by) },
            )
        }
    }
}

private const val PICK_HINT = " Select a row to pick it up, move it with up and down, and select again to put it down."

/** A catalog: its place, name and addon; shown or hidden; Up and Down. */
@Composable
private fun CatalogRow(
    row: HomeCatalog,
    place: Int,
    count: Int,
    picked: Boolean,
    onPick: () -> Unit,
    onToggle: () -> Unit,
    onMove: (Int) -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val colors = Marquee.colors
    val first = place == 1
    val last = place == count
    val shape = RoundedCornerShape(Marquee.radius.m)
    val text =
        @Composable {
            Row(
                horizontalArrangement = Arrangement.spacedBy(if (tv) Marquee.space.s else Marquee.space.xs),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                // One width for every place, so the names line up from 1 to 99.
                MqText(
                    "$place",
                    modifier = Modifier.widthIn(min = if (tv) 40.dp else 20.dp),
                    role = TextRole.Headline,
                    color = colors.dim,
                    weight = FontWeight.Bold,
                    textAlign = TextAlign.Center,
                )
                Column(Modifier.alpha(if (row.hidden) 0.55f else 1f)) {
                    MqText(row.title, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 1)
                    MqText(
                        row.source +
                            if (!row.hidden) {
                                ""
                            } else if (tv) {
                                " · Hidden from Home"
                            } else {
                                " · Hidden"
                            },
                        role = TextRole.Caption,
                        color = colors.dim,
                        maxLines = 1,
                    )
                    if (picked) {
                        MqText(
                            "Up or down to move, select to put it down",
                            role = TextRole.Caption,
                            color = colors.accent,
                        )
                    }
                }
            }
        }
    Section {
        Column {
            Row(
                horizontalArrangement = Arrangement.spacedBy(if (tv) Marquee.space.s else Marquee.space.xs),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                if (tv) {
                    Row(
                        Modifier
                            .weight(1f)
                            .marqueeFocus(Marquee.radius.m, scale = 1.01f)
                            .then(if (picked) Modifier.background(colors.raised, shape) else Modifier)
                            .onPreviewKeyEvent { e ->
                                if (!picked) return@onPreviewKeyEvent false
                                when (e.key) {
                                    Key.DirectionUp, Key.DirectionDown -> {
                                        val by = if (e.key == Key.DirectionUp) -1 else 1
                                        if (e.type == KeyEventType.KeyDown && !(by < 0 && first) &&
                                            !(by > 0 && last)
                                        ) {
                                            onMove(by)
                                        }
                                        true
                                    }

                                    // A picked-up row stays put until it is dropped.
                                    Key.DirectionLeft, Key.DirectionRight -> {
                                        true
                                    }

                                    else -> {
                                        false
                                    }
                                }
                            }.semantics {
                                contentDescription = "${row.title}, ${row.source}, row $place of $count"
                                stateDescription =
                                    if (picked) {
                                        "Picked up"
                                    } else if (row.hidden) {
                                        "Hidden"
                                    } else {
                                        "Shown"
                                    }
                            }.focusClickable(onPick, Role.Button)
                            .padding(Marquee.space.xs),
                    ) { text() }
                    Chip(
                        if (row.hidden) "Hidden" else "Shown",
                        selected = !row.hidden,
                        onClick = onToggle,
                        modifier = Modifier.semantics { contentDescription = row.title },
                        switch = true,
                    )
                } else {
                    // One line, as on iOS: the eye is white while the row shows; each button says its row.
                    Row(Modifier.weight(1f)) { text() }
                    RoundButton(
                        if (row.hidden) Icons.EyeOff else Icons.Eye,
                        if (row.hidden) "Show ${row.title} on Home" else "Hide ${row.title} from Home",
                        onToggle,
                        selected = !row.hidden,
                    )
                    RoundButton(Icons.ChevronUp, "Move ${row.title} up", { onMove(-1) }, enabled = !first)
                    RoundButton(Icons.ChevronDown, "Move ${row.title} down", { onMove(1) }, enabled = !last)
                }
            }
        }
    }
}
