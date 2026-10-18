package app.marquee.android

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.FindAddons
import app.marquee.core.FindAddons.Action
import app.marquee.core.FindAddonsCopy
import app.marquee.core.InstallCards
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.ListState
import app.marquee.core.model.ListedAddon
import app.marquee.core.session.CoreSession
import app.marquee.core.views.AddonCatalogView
import app.marquee.core.views.ViewStatus
import app.marquee.design.Chip
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.SecondaryButton
import app.marquee.design.Skeleton
import app.marquee.design.Status
import app.marquee.design.StatusNote
import app.marquee.design.TextRole
import app.marquee.design.Tokens
import app.marquee.design.marqueeFocus
import app.marquee.design.openLink
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch

// Find addons (plans/m12-addon-discovery.md 3.4 to 3.7): Cinemeta's Official and Community lists as
// glass cards, each installed or turned on in one press. The web's screens/find-addons.tsx.

private const val OFFICIAL = "official"
private const val COMMUNITY = "community"

class FindAddonsHolder(
    private val session: CoreSession,
) : ViewModel() {
    var view: AddonCatalogView by mutableStateOf(session.addonCatalog())
        private set

    /** The list and filter, kept while the page is on the stack; a new visit opens on Official, All. */
    var list by mutableStateOf(OFFICIAL)
    var filter by mutableStateOf(FindAddons.Filter.All)

    /** TV: focus is on a card, so Back goes to the chips. */
    var inCards by mutableStateOf(false)

    /** Try again and Refresh: the view closes and opens with `fresh`, which waits for the network. */
    fun reopen() {
        view.close()
        view = session.addonCatalog(fresh = true)
    }

    override fun onCleared() = view.close()
}

@Composable
fun FindAddonsScreen(
    session: CoreSession,
    cards: InstallCards,
    /** TV: opens the page with the link field (a remote types there; this page has no field). */
    onPasteLink: () -> Unit,
) {
    val holder =
        viewModel<FindAddonsHolder>(
            key = "find-addons",
            factory = viewModelFactory { initializer { FindAddonsHolder(session) } },
        )
    val tv = Marquee.form == Form.Tv
    val view = holder.view
    val snapshot = view.snapshot
    // A result is about the state it was pressed in: a new installed state clears it.
    LaunchedEffect(snapshot) { snapshot?.let { s -> cards.follow(s.lists.flatMap { it.entries }) } }
    val lists = snapshot?.lists.orEmpty()
    val current = lists.firstOrNull { it.id == holder.list }
    val failure =
        (view.status as? ViewStatus.Failed)?.reason
            ?: (current?.state as? ListState.Failed)?.words?.ifEmpty { FindAddonsCopy.FAILED }
    val loading = failure == null && (current == null || current.state == ListState.Loading)
    val entries = current?.entries.orEmpty()
    val filter = holder.filter
    val visible = remember(entries, filter) { entries.filter { FindAddons.shows(it, filter) } }
    val repeated = remember(entries) { FindAddons.repeatedNames(entries) }

    fun count(id: String) =
        lists
            .firstOrNull { it.id == id }
            ?.takeIf { it.state == ListState.Ready }
            ?.entries
            ?.size

    // TV focus lands on the list chip: at open (a return from Paste a link included), after Back
    // from the cards, and after Refresh or Try again, whose button goes while the list loads. No
    // other page opens from this one on TV, and within the page the grid's focusRestorer brings
    // focus back to the card it left.
    var chipAsks by remember { mutableIntStateOf(0) }

    fun retry() {
        cards.forgetAll(running = false)
        holder.reopen()
        chipAsks++
    }

    fun press(
        entry: ListedAddon,
        action: Action,
    ) {
        when (action) {
            is Action.Install -> cards.run(entry.key) { session.ops.installListed(entry.key) }
            is Action.TurnOn -> cards.run(entry.key) { session.ops.setAddonEnabled(action.url, true) }
            else -> Unit
        }
    }

    val grid = rememberLazyGridState()
    val listChip = remember { FocusRequester() }

    LaunchedEffect(chipAsks) {
        if (!tv) return@LaunchedEffect
        repeat(LAND_FRAMES) {
            withFrameNanos {}
            if (runCatching { listChip.requestFocus() }.isSuccess) return@LaunchedEffect
            // The chip is off screen, so not composed: bring the top of the page back first.
            grid.scrollToItem(0)
        }
    }
    BackHandler(enabled = tv && holder.inCards) {
        holder.inCards = false
        chipAsks++
    }

    LazyVerticalGrid(
        columns = GridCells.Adaptive(if (tv) 660.dp else 340.dp),
        state = grid,
        modifier = Modifier.fillMaxSize().focusRestorer(listChip),
        contentPadding =
            pagePadding(framed = false).let {
                PaddingValues(
                    start = Marquee.safe.side,
                    end = Marquee.safe.side,
                    top = it.calculateTopPadding(),
                    bottom = it.calculateBottomPadding(),
                )
            },
        horizontalArrangement = Arrangement.spacedBy(if (tv) 22.dp else 12.dp),
        verticalArrangement = Arrangement.spacedBy(if (tv) 22.dp else 12.dp),
    ) {
        full("head") { BrowseHeader(FindAddonsCopy.EYEBROW, FindAddonsCopy.TITLE, side = 0.dp) }
        full("lede") { MqText(FindAddonsCopy.LEDE, color = Marquee.colors.muted) }
        full("paste") {
            if (tv) {
                Row {
                    SecondaryButton(FindAddonsCopy.PASTE_LINK, onPasteLink, icon = Icons.Link)
                }
            } else {
                PasteLink(session)
            }
        }
        full("lists") {
            Choices(
                FindAddonsCopy.LIST_LABEL,
                listOf(
                    OFFICIAL to listOfNotNull(FindAddonsCopy.OFFICIAL, count(OFFICIAL)).joinToString(" "),
                    COMMUNITY to listOfNotNull(FindAddonsCopy.COMMUNITY_LIST, count(COMMUNITY)).joinToString(" "),
                ),
                holder.list,
                listChip,
            ) { holder.list = it }
        }
        full("filters") {
            Choices(FindAddonsCopy.SHOW_LABEL, FindAddons.Filter.entries.map { it to it.label }, filter, null) {
                holder.filter = it
            }
        }
        if (holder.list == COMMUNITY) full("community") { CommunityNote() }
        when {
            loading -> {
                full("loading") { StatusNote(FindAddonsCopy.LOADING, tone = Status.Warn, busy = true) }
                items(6, key = { "skeleton:$it" }) { Skeleton(Modifier.fillMaxWidth().heightIn(min = 160.dp)) }
            }

            failure != null -> {
                full("failed") {
                    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                        Note(failure, failed = true, side = 0.dp)
                        Row {
                            SecondaryButton(FindAddonsCopy.TRY_AGAIN, ::retry, icon = Icons.RotateCw)
                        }
                    }
                }
            }

            else -> {
                full("refresh") { Row { SecondaryButton(FindAddonsCopy.REFRESH, ::retry, icon = Icons.RotateCw) } }
                when {
                    entries.isEmpty() -> {
                        full("empty") { Note(FindAddonsCopy.EMPTY, side = 0.dp) }
                    }

                    visible.isEmpty() -> {
                        full("filter-empty") { Note(FindAddons.filterEmpty(filter).orEmpty(), side = 0.dp) }
                    }

                    else -> {
                        // Keyed by place in the whole list: an entry's key may hold keys, and item keys are saved.
                        val places = entries.withIndex().associate { (at, e) -> e.key to at }
                        items(visible, key = { "card:${holder.list}:${places[it.key]}" }) { entry ->
                            Card(
                                entry = entry,
                                sameName = entry.name in repeated,
                                state = cards[entry.key],
                                onFocus = { holder.inCards = it },
                                onPress = { press(entry, it) },
                            )
                        }
                    }
                }
            }
        }
    }
}

/** Frames a focus landing tries for (about 1.5 s): the page may still be laying out. */
private const val LAND_FRAMES = 90

/** A row of choices under its kicker; it wraps on a narrow phone. [requester] goes to the selected one. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun <T : Any> Choices(
    label: String,
    choices: List<Pair<T, String>>,
    selected: T,
    requester: FocusRequester?,
    onSelect: (T) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        Kicker(label, side = 0.dp)
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            // Keyed by value, so a count arriving in a label keeps focus on its chip.
            for ((value, name) in choices) {
                key(value) {
                    Chip(
                        name,
                        selected = value == selected,
                        onClick = { onSelect(value) },
                        focusRequester = requester.takeIf { value == selected },
                    )
                }
            }
        }
    }
}

/** Community's caution, on glass. */
@Composable
private fun CommunityNote() {
    val tv = Marquee.form == Form.Tv
    val shape = RoundedCornerShape(Marquee.radius.card)
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(Marquee.colors.glass)
            .border(1.dp, Marquee.colors.edge, shape)
            .padding(horizontal = if (tv) 28.dp else 16.dp, vertical = if (tv) 22.dp else 14.dp),
        horizontalArrangement = Arrangement.spacedBy(if (tv) 18.dp else 12.dp),
    ) {
        MqIcon(Icons.Info, tint = Marquee.colors.text, size = if (tv) 28.dp else 20.dp)
        MqText(FindAddonsCopy.COMMUNITY, role = TextRole.Callout, color = Marquee.colors.muted)
    }
}

/** The phone's Paste a link field at the top: Settings' own install field, with its own state. */
@Composable
private fun PasteLink(session: CoreSession) {
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    var said by remember { mutableStateOf<Said?>(null) }
    val focus = remember { FocusRequester() }
    InstallField(busy, focus, said, side = 0.dp) { url, clear ->
        busy = true
        said = Said(Place.Install, "Adding the addon", failed = false, pending = true)
        scope.launch {
            said =
                try {
                    val added = session.ops.installAddon(url)
                    clear()
                    Said(Place.Install, "Added ${added.name}", failed = false)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    val why = (e as? CoreError)?.message ?: CoreLog.describe(e)
                    Said(Place.Install, "Adding the addon did not work: $why", failed = true)
                } finally {
                    busy = false
                }
        }
    }
}

/**
 * One entry. On a phone the card holds its buttons (Install or Turn on, and Configure); on a TV the
 * card is one focus stop whose select performs Install or Turn on, and Configure is a line saying
 * where to set it up. While a request runs the control stays and says Installing, so focus never
 * jumps.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Card(
    entry: ListedAddon,
    sameName: Boolean,
    state: InstallCards.State?,
    onFocus: (Boolean) -> Unit,
    onPress: (Action) -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val colors = Marquee.colors
    val context = LocalContext.current
    val busy = state?.busy == true
    val action = FindAddons.cardAction(entry, state?.code)
    val pressable = FindAddons.pressable(action)
    val failure = state?.words.takeIf { !busy && action !is Action.None }
    val word = (action as? Action.None)?.word ?: (action as? Action.TurnOn)?.word
    val verb = if (action is Action.TurnOn) FindAddonsCopy.TURN_ON else FindAddonsCopy.INSTALL
    val busyWord = if (action is Action.TurnOn) FindAddonsCopy.TURNING_ON else FindAddonsCopy.INSTALLING
    val configure = !tv && (action == Action.Configure || (action as? Action.Install)?.configure == true)
    var noBrowser by remember(entry.key) { mutableStateOf(false) }
    val perform = { if (!busy) onPress(action) }
    val shape = RoundedCornerShape(if (tv) Marquee.radius.l else Marquee.radius.card)
    // What a screen reader says for the card: its action, or on TV what it is when select does nothing.
    val spoken =
        FindAddons.spoken(entry, if (!tv || pressable) action else Action.None(word.orEmpty()), sameName)
    var focused by remember { mutableStateOf(false) }
    Column(
        Modifier
            .fillMaxWidth()
            .then(
                if (tv) {
                    Modifier
                        .onFocusChanged {
                            focused = it.isFocused
                            onFocus(it.isFocused)
                        }.marqueeFocus(Marquee.radius.l, Tokens.FOCUS_SCALE_TILE)
                } else {
                    Modifier
                },
            ).clip(shape)
            .background(colors.glass)
            .border(1.dp, colors.edge, shape)
            .then(
                if (tv) {
                    // One stop, the same node whatever the state, so an install that ends keeps focus here.
                    Modifier
                        .semantics(mergeDescendants = true) {
                            contentDescription = spoken
                            // An installed card is not a button.
                            if (pressable) {
                                role = Role.Button
                                onClick {
                                    perform()
                                    true
                                }
                            }
                        }.onKeyEvent { e ->
                            val select = e.key == Key.DirectionCenter || e.key == Key.Enter || e.key == Key.NumPadEnter
                            if (select && e.type == KeyEventType.KeyUp && pressable) perform()
                            select
                        }.focusable()
                } else {
                    Modifier.semantics(mergeDescendants = true) {}
                },
            ).padding(if (tv) 28.dp else 16.dp),
        verticalArrangement = Arrangement.spacedBy(if (tv) 22.dp else 12.dp),
    ) {
        Row(
            if (tv) Modifier.clearAndSetSemantics { } else Modifier,
            horizontalArrangement = Arrangement.spacedBy(if (tv) 26.dp else 14.dp),
        ) {
            Mark(entry.name)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(if (tv) 6.dp else 4.dp)) {
                MqText(
                    entry.name,
                    role = TextRole.Headline,
                    color = if (focused) colors.accent else colors.text,
                    weight = FontWeight.Bold,
                )
                // The link may hold keys: the host stands in for it.
                MqText(
                    entry.host,
                    role = TextRole.Caption,
                    color = colors.dim,
                    maxLines = 1,
                    fontFamily = FontFamily.Monospace,
                )
                MqText(
                    FindAddons.providesLine(entry),
                    modifier = Modifier.padding(top = if (tv) 8.dp else 4.dp),
                    role = TextRole.Caption,
                    weight = FontWeight.SemiBold,
                )
                entry.description?.let {
                    MqText(
                        it,
                        role = if (tv) TextRole.Callout else TextRole.Caption,
                        color = colors.muted,
                        maxLines = 2,
                    )
                }
                if (action == Action.Configure) {
                    MqText(
                        if (tv) FindAddonsCopy.CONFIGURE_TV else FindAddonsCopy.CONFIGURE_HELP,
                        modifier = Modifier.padding(top = if (tv) 8.dp else 4.dp),
                        role = if (tv) TextRole.Callout else TextRole.Caption,
                        weight = FontWeight.Medium,
                    )
                }
                failure?.let { Note(it, failed = true, side = 0.dp) }
                if (noBrowser) Note(FindAddonsCopy.NO_BROWSER, failed = true, side = 0.dp)
            }
            if (tv && (word != null || pressable)) {
                Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    word?.let { StateWord(it, installed = action is Action.None) }
                    if (pressable) ActionPill(if (busy) busyWord else verb, action is Action.TurnOn, busy, focused)
                }
            }
        }
        if (!tv && (word != null || pressable || configure)) {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
                itemVerticalAlignment = Alignment.CenterVertically,
            ) {
                word?.let { StateWord(it, installed = action is Action.None) }
                if (pressable) {
                    // The button stays while its request runs, so focus stays on it.
                    SecondaryButton(
                        if (busy) busyWord else verb,
                        perform,
                        modifier = Modifier.semantics { contentDescription = spoken },
                        icon =
                            if (busy) {
                                null
                            } else if (action is Action.TurnOn) {
                                Icons.Power
                            } else {
                                Icons.Plus
                            },
                    )
                }
                if (configure) {
                    SecondaryButton(
                        FindAddonsCopy.CONFIGURE,
                        { noBrowser = !openLink(context, entry.configureUrl) },
                        modifier =
                            Modifier.semantics {
                                contentDescription = FindAddons.spoken(entry, Action.Configure, sameName)
                            },
                        icon = Icons.ExternalLink,
                    )
                }
            }
        }
    }
}

/** The name's first letter on the brand surface: no remote logo, so a list contacts no image host. */
@Composable
private fun Mark(name: String) {
    val tv = Marquee.form == Form.Tv
    val shape = RoundedCornerShape(if (tv) Marquee.radius.card else 12.dp)
    Box(
        Modifier
            .size(if (tv) 80.dp else 48.dp)
            .clip(shape)
            .background(Marquee.colors.raised)
            .border(1.dp, Marquee.colors.edge, shape),
        contentAlignment = Alignment.Center,
    ) {
        MqText(FindAddons.initial(name), role = if (tv) TextRole.Title2 else TextRole.Title3, maxLines = 1)
    }
}

/** Installed, Set up, Off: a word, with a check when the addon is on. */
@Composable
private fun StateWord(
    text: String,
    installed: Boolean,
) {
    val tv = Marquee.form == Form.Tv
    Row(
        Modifier.heightIn(min = if (tv) 64.dp else 44.dp),
        horizontalArrangement = Arrangement.spacedBy(if (tv) 12.dp else 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (installed) MqIcon(Icons.Check, tint = Marquee.colors.ok, size = if (tv) 26.dp else 18.dp)
        MqText(
            text,
            role = TextRole.Callout,
            color = if (installed) Marquee.colors.text else Marquee.colors.muted,
            weight = FontWeight.SemiBold,
            maxLines = 1,
        )
    }
}

/** TV: the card's action drawn as a glass pill; the card itself takes the press. */
@Composable
private fun ActionPill(
    label: String,
    turnOn: Boolean,
    busy: Boolean,
    /** The card has focus: the pill turns white, unless its request runs. */
    lit: Boolean,
) {
    val colors = Marquee.colors
    val shape = RoundedCornerShape(Marquee.radius.pill)
    val white = lit && !busy
    val content = if (white) colors.onPrimary else colors.text
    Row(
        Modifier
            .heightIn(min = 64.dp)
            .clip(shape)
            .background(if (white) colors.primary else colors.raised)
            .then(if (white) Modifier else Modifier.border(2.dp, colors.control, shape))
            .padding(horizontal = 26.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (!busy) MqIcon(if (turnOn) Icons.Power else Icons.Plus, tint = content, size = 26.dp)
        MqText(label, role = TextRole.Callout, color = content, weight = FontWeight.SemiBold, maxLines = 1)
    }
}
