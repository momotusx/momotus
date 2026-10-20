package app.marquee.android

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusDirection
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusProperties
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDirection
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.FindAddons
import app.marquee.core.FindAddons.Action
import app.marquee.core.FindAddonsCopy
import app.marquee.core.FindAddonsPage
import app.marquee.core.InstallCards
import app.marquee.core.LinkInstall
import app.marquee.core.model.ListState
import app.marquee.core.model.ListedAddon
import app.marquee.core.session.CoreSession
import app.marquee.core.views.ViewStatus
import app.marquee.design.Chip
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.KeyPresses
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.SecondaryButton
import app.marquee.design.Skeleton
import app.marquee.design.Spinner
import app.marquee.design.Status
import app.marquee.design.StatusNote
import app.marquee.design.TextRole
import app.marquee.design.Tokens
import app.marquee.design.marqueeFocus
import app.marquee.design.openLink
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlin.math.max

// Find addons (plans/m12-addon-discovery.md 3.4 to 3.7): Cinemeta's Official and Community lists as
// glass cards, each installed or turned on in one press. The web's screens/find-addons.tsx.

private const val OFFICIAL = "official"
private const val COMMUNITY = "community"

class FindAddonsHolder(
    session: CoreSession,
    cards: InstallCards,
) : ViewModel() {
    val page = FindAddonsPage(session.ops, cards, session::addonCatalog)

    /** The list and filter, kept while the page is on the stack; a new visit opens on Official, All. */
    var list by mutableStateOf(OFFICIAL)
    var filter by mutableStateOf(FindAddons.Filter.All)

    /** Each card's memory by its key, so a regrouped row never loses it. */
    internal val memories = CardMemories()

    /** TV: the card that has focus, by entry key (memory only); null once a control above has it. */
    var focusedCard by mutableStateOf<String?>(null)

    /** TV: the card focus last left for the controls above, so Down into the cards comes back to it. */
    var lastCard: String? = null

    fun cardFocused(key: String) {
        focusedCard = key
        lastCard = key
    }

    override fun onCleared() = page.close()
}

/**
 * Where TV focus lands on Find addons (plans/m12-addon-discovery.md 3.6), apart from Compose: at
 * open the List chip, or Try again when the list failed, or Paste a link when no list came in
 * time; after Back from the cards, Refresh or Try again the List chip; Down into the cards the card
 * left last, else the first; when the focused card goes, the first card, else the chip.
 */
internal object FindAddonsFocus {
    sealed interface Target {
        data object Chip : Target

        data object TryAgain : Target

        data object Paste : Target

        data class Card(
            val key: String,
        ) : Target
    }

    enum class Ask { Open, Back, Retry, EnterCards, CardGone }

    /** Null: nothing to land on yet (the list is on its way and [waited] is false); ask again next frame. */
    fun target(
        ask: Ask,
        failed: Boolean,
        loading: Boolean,
        visible: List<String>,
        lastCard: String?,
        waited: Boolean,
    ): Target? =
        when (ask) {
            Ask.Open -> {
                when {
                    failed -> Target.TryAgain
                    !loading -> Target.Chip
                    waited -> Target.Paste
                    else -> null
                }
            }

            Ask.Back, Ask.Retry -> {
                Target.Chip
            }

            Ask.EnterCards -> {
                (lastCard?.takeIf { it in visible } ?: visible.firstOrNull())?.let { Target.Card(it) }
            }

            Ask.CardGone -> {
                visible.firstOrNull()?.let { Target.Card(it) } ?: Target.Chip
            }
        }
}

/**
 * A TV card's select acts on the key up only after a key down on the same card: a key up whose down
 * went to another control (a select that moved focus here) does nothing.
 */
internal class SelectGate {
    private var down = false

    fun keyDown() {
        down = true
    }

    /** True when this up ends a press that began here. */
    fun keyUp(): Boolean = down.also { down = false }

    fun reset() {
        down = false
    }
}

/**
 * Phone: whether a card's button still holds the focus when its request ends, so only then does the
 * card take it back. A loss while the button is there (the user moved to another control or the link
 * field) gives it up; a loss because the button went (the addon installed) does not.
 */
internal class OwnedFocus {
    private var held = false

    fun changed(
        focused: Boolean,
        buttonGone: Boolean,
    ) {
        if (focused) {
            held = true
        } else if (!buttonGone) {
            held = false
        }
    }

    /** At the end of a request: true when the focus should come back to the card. Read once. */
    fun landed(): Boolean = held.also { held = false }
}

/**
 * What a card remembers across compositions. Rows are composed per row, so a card that comes or goes
 * ahead of the others regroups every row after it; this memory is kept by the card's own key, apart
 * from the rows, so it stays with its card.
 */
internal class CardMemory {
    var noBrowser by mutableStateOf(false)
    val owned = OwnedFocus()
    val gate = SelectGate()
    var sawBusy by mutableStateOf(false)
    var landed by mutableStateOf(false)
}

/** Every card's memory by its key, for the page's life (memory only; a list holds a few hundred at most). */
internal class CardMemories {
    private val byKey = HashMap<String, CardMemory>()

    operator fun get(key: String): CardMemory = byKey.getOrPut(key) { CardMemory() }

    /** A row's cards with their own memories, whatever row they fall in now. */
    fun forRow(keys: List<String>): List<CardMemory> = keys.map { get(it) }
}

/** What the landing reads each frame, newer than the composition that started it. */
private class Now(
    val failed: Boolean,
    val loading: Boolean,
    val visible: List<String>,
    val rows: Int,
    val columns: Int,
)

@Composable
fun FindAddonsScreen(
    session: CoreSession,
    cards: InstallCards,
    link: LinkInstall,
    /** TV: opens the page with the link field (a remote types there; this page has no field). */
    onPasteLink: () -> Unit,
) {
    val holder =
        viewModel<FindAddonsHolder>(
            key = "find-addons",
            factory = viewModelFactory { initializer { FindAddonsHolder(session, cards) } },
        )
    val tv = Marquee.form == Form.Tv
    val page = holder.page
    val view = page.view
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
    val visibleKeys = remember(visible) { visible.map { it.key } }

    fun count(id: String) =
        lists
            .firstOrNull { it.id == id }
            ?.takeIf { it.state == ListState.Ready }
            ?.entries
            ?.size

    val listState = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val listChip = remember { FocusRequester() }
    val tryAgain = remember { FocusRequester() }
    val paste = remember { FocusRequester() }
    val cardFocus = remember { HashMap<String, FocusRequester>() }
    val landing = remember { arrayOfNulls<Job>(1) }

    BoxWithConstraints(Modifier.fillMaxSize()) {
        val side = Marquee.safe.side
        val gap = if (tv) 22.dp else 12.dp
        // As many columns as fit cards of this width, as the web's auto-fill grid; a row is as tall
        // as its tallest card.
        val columns = max(1, ((maxWidth - side * 2 + gap) / ((if (tv) 660.dp else 340.dp) + gap)).toInt())
        val rows = remember(visible, columns) { visible.chunked(columns) }
        val now = rememberUpdatedState(Now(failure != null, loading, visibleKeys, rows.size, columns))

        // Puts TV focus where [ask] says, scrolling its item into view first (a card far down the list
        // is not composed, so it has no node to focus until it is).
        fun land(ask: FindAddonsFocus.Ask) {
            if (!tv) return
            landing[0]?.cancel()
            val asked = KeyPresses.count
            landing[0] =
                scope.launch {
                    repeat(LAND_FRAMES) { frame ->
                        withFrameNanos {}
                        // The page's first focus never takes back a move the viewer made while it loaded.
                        if (ask == FindAddonsFocus.Ask.Open && KeyPresses.count != asked) return@launch
                        val state = now.value
                        val target =
                            FindAddonsFocus.target(
                                ask,
                                state.failed,
                                state.loading,
                                state.visible,
                                holder.lastCard,
                                waited = frame == LAND_FRAMES - 1,
                            ) ?: return@repeat
                        val (requester, item) =
                            when (target) {
                                FindAddonsFocus.Target.Chip -> {
                                    listChip to LISTS_ITEM
                                }

                                FindAddonsFocus.Target.TryAgain -> {
                                    tryAgain to LISTS_ITEM
                                }

                                FindAddonsFocus.Target.Paste -> {
                                    paste to PASTE_ITEM
                                }

                                is FindAddonsFocus.Target.Card -> {
                                    val row = state.visible.indexOf(target.key) / state.columns
                                    cardFocus[target.key] to listState.layoutInfo.totalItemsCount - state.rows + row
                                }
                            }
                        // False while its node is not composed (Compose then only prints a warning).
                        if (requester?.requestFocus(FocusDirection.Enter) == true) return@launch
                        listState.scrollToItem(item.coerceAtLeast(0))
                    }
                }
        }

        fun retry() {
            page.retry()
            land(FindAddonsFocus.Ask.Retry)
        }

        LaunchedEffect(Unit) { land(FindAddonsFocus.Ask.Open) }
        // The focused card went (the list changed under it): the first card, else the chip.
        LaunchedEffect(visibleKeys) {
            val gone = holder.focusedCard
            if (gone != null && gone !in visibleKeys) land(FindAddonsFocus.Ask.CardGone)
        }
        BackHandler(enabled = tv && holder.focusedCard != null) {
            holder.focusedCard = null
            land(FindAddonsFocus.Ask.Back)
        }
        // Focus on a control above the cards: Back leaves from there, and the cards remember theirs.
        val onControl: (Boolean) -> Unit = { if (it) holder.focusedCard = null }

        LazyColumn(
            state = listState,
            modifier = Modifier.fillMaxSize(),
            contentPadding =
                pagePadding(framed = false).let {
                    PaddingValues(
                        start = side,
                        end = side,
                        top = it.calculateTopPadding(),
                        bottom = it.calculateBottomPadding(),
                    )
                },
            verticalArrangement = Arrangement.spacedBy(gap),
        ) {
            item(key = "head") { BrowseHeader(FindAddonsCopy.EYEBROW, FindAddonsCopy.TITLE, side = 0.dp) }
            item(key = "lede") { MqText(FindAddonsCopy.LEDE, color = Marquee.colors.muted) }
            item(key = "paste") {
                if (tv) {
                    Row {
                        SecondaryButton(
                            FindAddonsCopy.PASTE_LINK,
                            onPasteLink,
                            icon = Icons.Link,
                            focusRequester = paste,
                            onFocusChange = onControl,
                        )
                    }
                } else {
                    PasteLink(session, link)
                }
            }
            item(key = "lists") {
                Choices(
                    FindAddonsCopy.LIST_LABEL,
                    listOf(
                        OFFICIAL to listOfNotNull(FindAddonsCopy.OFFICIAL, count(OFFICIAL)).joinToString(" "),
                        COMMUNITY to listOfNotNull(FindAddonsCopy.COMMUNITY_LIST, count(COMMUNITY)).joinToString(" "),
                    ),
                    holder.list,
                    listChip,
                    onControl,
                ) { holder.list = it }
            }
            item(key = "filters") {
                Choices(
                    FindAddonsCopy.SHOW_LABEL,
                    FindAddons.Filter.entries.map { it to it.label },
                    filter,
                    null,
                    onControl,
                ) { holder.filter = it }
            }
            if (holder.list == COMMUNITY) item(key = "community") { CommunityNote() }
            when {
                loading -> {
                    item(key = "loading") { StatusNote(FindAddonsCopy.LOADING, tone = Status.Warn, busy = true) }
                    items(3, key = { "skeleton:$it" }) { Skeleton(Modifier.fillMaxWidth().heightIn(min = 160.dp)) }
                }

                failure != null -> {
                    item(key = "failed") {
                        Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                            Note(failure, failed = true, side = 0.dp)
                            Row {
                                SecondaryButton(
                                    FindAddonsCopy.TRY_AGAIN,
                                    ::retry,
                                    icon = Icons.RotateCw,
                                    focusRequester = tryAgain,
                                    onFocusChange = onControl,
                                )
                            }
                        }
                    }
                }

                else -> {
                    item(key = "refresh") {
                        Row {
                            SecondaryButton(
                                FindAddonsCopy.REFRESH,
                                ::retry,
                                // Down from here goes into the cards at the one focus left last.
                                modifier =
                                    Modifier.onPreviewKeyEvent { e ->
                                        val into =
                                            tv && visible.isNotEmpty() && e.key == Key.DirectionDown &&
                                                e.type == KeyEventType.KeyDown
                                        if (into) land(FindAddonsFocus.Ask.EnterCards)
                                        into
                                    },
                                icon = Icons.RotateCw,
                                onFocusChange = onControl,
                            )
                        }
                    }
                    when {
                        entries.isEmpty() -> {
                            item(key = "empty") { Note(FindAddonsCopy.EMPTY, side = 0.dp) }
                        }

                        visible.isEmpty() -> {
                            item(key = "filter-empty") { Note(FindAddons.filterEmpty(filter).orEmpty(), side = 0.dp) }
                        }

                        else -> {
                            // Keyed by the first card's id and host, never its link (item keys are saved): an
                            // entry added or moved keeps every other row's remembered state.
                            val ids = FindAddons.stableIds(entries)
                            items(rows, key = { "row:${holder.list}:${ids[it.first().key]}" }) { row ->
                                val memories = holder.memories.forRow(row.map { it.key })
                                Row(
                                    Modifier.height(IntrinsicSize.Max),
                                    horizontalArrangement = Arrangement.spacedBy(gap),
                                ) {
                                    for ((entry, memory) in row.zip(memories)) {
                                        key(entry.key) {
                                            Card(
                                                entry = entry,
                                                memory = memory,
                                                sameName = entry.name in repeated,
                                                state = cards[entry.key],
                                                focus = cardFocus,
                                                onFocus = { holder.cardFocused(entry.key) },
                                                onPress = { page.press(entry, it) },
                                                modifier = Modifier.weight(1f).fillMaxHeight(),
                                            )
                                        }
                                    }
                                    repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/** Frames a focus landing tries for (about 1.5 s): the list may still be on its way. */
private const val LAND_FRAMES = 90

/** The page's items above the cards, by place: the header, the lede, Paste a link, then the List chips. */
private const val PASTE_ITEM = 2
private const val LISTS_ITEM = 3

/**
 * A row of choices under its kicker; it wraps on a narrow phone. [requester] goes to the selected
 * one; Left and Right stop at the row's ends instead of leaving it.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun <T : Any> Choices(
    label: String,
    choices: List<Pair<T, String>>,
    selected: T,
    requester: FocusRequester?,
    onFocusChange: (Boolean) -> Unit,
    onSelect: (T) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
        Kicker(label, side = 0.dp)
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        ) {
            // Keyed by value, so a count arriving in a label keeps focus on its chip.
            choices.forEachIndexed { at, (value, name) ->
                key(value) {
                    Chip(
                        name,
                        selected = value == selected,
                        onClick = { onSelect(value) },
                        modifier =
                            Modifier.focusProperties {
                                if (at == 0) left = FocusRequester.Cancel
                                if (at == choices.lastIndex) right = FocusRequester.Cancel
                            },
                        focusRequester = requester.takeIf { value == selected },
                        onFocusChange = onFocusChange,
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

/** The phone's Paste a link field at the top: Settings' field and its one add-by-link flow. */
@Composable
private fun PasteLink(
    session: CoreSession,
    link: LinkInstall,
) {
    val focus = remember { FocusRequester() }
    LinkField(session, link, focus, side = 0.dp, compact = true)
}

/**
 * One entry. On a phone the card holds its buttons (Install or Turn on, and Configure); on a TV the
 * card is one focus stop whose select performs Install or Turn on, and Configure is a line saying
 * where to set it up. While a request runs the control stays and says Installing with a spinner, so
 * focus never jumps.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Card(
    entry: ListedAddon,
    /** Kept by the card's key outside the rows: a regroup recomposes the card, never forgets this. */
    memory: CardMemory,
    sameName: Boolean,
    state: InstallCards.State?,
    /** TV: each card's requester by entry key, while it is composed. */
    focus: HashMap<String, FocusRequester>,
    onFocus: () -> Unit,
    onPress: (Action) -> Unit,
    modifier: Modifier = Modifier,
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
    val help =
        when {
            action != Action.Configure -> null
            tv -> FindAddonsCopy.CONFIGURE_TV
            else -> FindAddonsCopy.CONFIGURE_HELP
        }
    val perform = { if (!busy) onPress(action) }
    val shape = RoundedCornerShape(if (tv) Marquee.radius.l else Marquee.radius.card)
    // The press, the busy state and the failure, as a screen reader says them: on TV the card is one
    // stop and says all of it (what it is when select does nothing); on a phone the button says what
    // it does or that it is installing, and the card's lines say the rest.
    val pressWords = FindAddons.described(entry, action, sameName, busyWord.takeIf { busy })
    val cardWords =
        FindAddons.described(
            entry,
            if (pressable) action else Action.None(word.orEmpty()),
            sameName,
            busyWord.takeIf { busy },
            help,
            failure,
        )
    var focused by remember { mutableStateOf(false) }
    val requester = remember { FocusRequester() }
    val gate = memory.gate
    // Phone: this card's own install just landed while its button held the focus. The button goes, so
    // the focus (a keyboard's, a screen reader's) moves to the card's state instead of falling to the
    // link field at the top. Focus the user took elsewhere meanwhile stays where it is.
    val stay = remember { FocusRequester() }
    val owned = memory.owned
    val buttonGone by rememberUpdatedState(!pressable)
    LaunchedEffect(busy) {
        if (busy) {
            memory.sawBusy = true
        } else if (memory.sawBusy) {
            memory.sawBusy = false
            val held = owned.landed()
            memory.landed = held && state?.words == null
        }
    }
    val keyboard = LocalSoftwareKeyboardController.current
    LaunchedEffect(memory.landed, action is Action.None) {
        if (!tv && memory.landed && action is Action.None) {
            // The link field may have caught focus for a moment as the button went, and raised the keyboard.
            stay.requestFocus(FocusDirection.Enter)
            keyboard?.hide()
            memory.landed = false
        }
    }
    if (tv) {
        DisposableEffect(entry.key, requester) {
            focus[entry.key] = requester
            onDispose { if (focus[entry.key] === requester) focus.remove(entry.key) }
        }
    }
    Column(
        modifier
            .fillMaxWidth()
            .then(
                if (tv) {
                    Modifier
                        .focusRequester(requester)
                        .onFocusChanged {
                            focused = it.isFocused
                            if (it.isFocused) onFocus() else gate.reset()
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
                            contentDescription = cardWords
                            // An installed card is not a button; a failure is read when it comes.
                            if (pressable && !busy) {
                                role = Role.Button
                                onClick {
                                    perform()
                                    true
                                }
                            }
                            if (failure != null) liveRegion = LiveRegionMode.Polite
                        }.onKeyEvent { e ->
                            val select = e.key == Key.DirectionCenter || e.key == Key.Enter || e.key == Key.NumPadEnter
                            when {
                                !select -> Unit
                                e.type == KeyEventType.KeyDown -> gate.keyDown()
                                e.type == KeyEventType.KeyUp -> if (gate.keyUp() && pressable) perform()
                            }
                            select
                        }.focusable()
                } else {
                    Modifier.semantics(mergeDescendants = true) {}
                },
            ).padding(if (tv) 28.dp else 16.dp),
        verticalArrangement = Arrangement.spacedBy(if (tv) 22.dp else 12.dp),
    ) {
        val about: @Composable (Modifier) -> Unit = { textModifier ->
            Row(textModifier, horizontalArrangement = Arrangement.spacedBy(if (tv) 26.dp else 14.dp)) {
                Mark(entry.name)
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(if (tv) 6.dp else 4.dp)) {
                    // The addon's own words read in their own direction (the web's dir="auto").
                    MqText(
                        entry.name,
                        modifier = Modifier.fillMaxWidth(),
                        role = TextRole.Headline,
                        color = if (focused) colors.accent else colors.text,
                        weight = FontWeight.Bold,
                        textDirection = TextDirection.Content,
                    )
                    // The link may hold keys: the host stands in for it, cut in the middle so the
                    // owner's domain at its end always shows.
                    MqText(
                        entry.host,
                        role = TextRole.Caption,
                        color = colors.dim,
                        maxLines = 1,
                        fontFamily = FontFamily.Monospace,
                        overflow = TextOverflow.MiddleEllipsis,
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
                            modifier = Modifier.fillMaxWidth(),
                            role = if (tv) TextRole.Callout else TextRole.Caption,
                            color = colors.muted,
                            maxLines = 2,
                            textDirection = TextDirection.Content,
                        )
                    }
                    help?.let {
                        MqText(
                            it,
                            modifier = Modifier.padding(top = if (tv) 8.dp else 4.dp),
                            role = if (tv) TextRole.Callout else TextRole.Caption,
                            weight = FontWeight.Medium,
                        )
                    }
                    // Always there, so a failure that comes is a change of a live region and is read out.
                    Box(Modifier.semantics(mergeDescendants = true) { liveRegion = LiveRegionMode.Polite }) {
                        failure?.let { Note(it, failed = true, side = 0.dp) }
                    }
                    if (memory.noBrowser) Note(FindAddonsCopy.NO_BROWSER, failed = true, side = 0.dp)
                }
            }
        }
        if (tv) {
            EndOrBelow(Modifier.clearAndSetSemantics { }, { about(Modifier) }) {
                if (word != null || pressable) {
                    Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        word?.let { StateWord(it, installed = action is Action.None) }
                        if (pressable) ActionPill(if (busy) busyWord else verb, action is Action.TurnOn, busy, focused)
                    }
                }
            }
        } else {
            about(Modifier)
        }
        if (!tv && (word != null || pressable || configure)) {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
                itemVerticalAlignment = Alignment.CenterVertically,
            ) {
                word?.let {
                    StateWord(
                        it,
                        installed = action is Action.None,
                        // An installed card's one stop, which a landed install hands focus to.
                        modifier = if (action is Action.None) Modifier.focusRequester(stay).focusable() else Modifier,
                        spoken = cardWords.takeIf { action is Action.None },
                    )
                }
                if (pressable) {
                    // The button stays while its request runs, so focus stays on it.
                    SecondaryButton(
                        if (busy) busyWord else verb,
                        perform,
                        icon = if (action is Action.TurnOn) Icons.Power else Icons.Plus,
                        busy = busy,
                        spoken = pressWords,
                        onFocusChange = { owned.changed(it, buttonGone) },
                    )
                }
                if (configure) {
                    SecondaryButton(
                        FindAddonsCopy.CONFIGURE,
                        { memory.noBrowser = !openLink(context, entry.configureUrl) },
                        icon = Icons.ExternalLink,
                        spoken = FindAddons.spoken(entry, Action.Configure, sameName),
                    )
                }
            }
        }
    }
}

/**
 * A TV card's text with its action at the end, or under the text when the card is too narrow for
 * both (a large font, a third column): the card wraps by its own width, not a screen breakpoint.
 */
@Composable
private fun EndOrBelow(
    modifier: Modifier,
    text: @Composable () -> Unit,
    end: @Composable () -> Unit,
) {
    Layout(contents = listOf(text, end), modifier = modifier) { (texts, ends), constraints ->
        val gap = 26.dp.roundToPx()
        val loose = constraints.copy(minWidth = 0, minHeight = 0)
        val endPlaced = ends.map { it.measure(loose) }
        val endWidth = endPlaced.maxOfOrNull { it.width } ?: 0
        val endHeight = endPlaced.maxOfOrNull { it.height } ?: 0
        val beside = endWidth == 0 || constraints.maxWidth - endWidth - gap >= MIN_TEXT.roundToPx()
        val textWidth = if (beside && endWidth > 0) constraints.maxWidth - endWidth - gap else constraints.maxWidth
        val textPlaced = texts.map { it.measure(loose.copy(maxWidth = textWidth)) }
        val textHeight = textPlaced.maxOfOrNull { it.height } ?: 0
        val height =
            when {
                endWidth == 0 -> textHeight
                beside -> max(textHeight, endHeight)
                else -> textHeight + gap + endHeight
            }
        layout(constraints.maxWidth, height) {
            textPlaced.forEach { it.place(0, 0) }
            endPlaced.forEach { it.place(constraints.maxWidth - it.width, if (beside) 0 else textHeight + gap) }
        }
    }
}

/** The narrowest a TV card's text may get before its action goes under it. */
private val MIN_TEXT = 380.dp

/**
 * The name's first letter on the brand surface: no remote logo, so a list contacts no image host.
 * Decoration: a screen reader skips it.
 */
@Composable
private fun Mark(name: String) {
    val tv = Marquee.form == Form.Tv
    val shape = RoundedCornerShape(if (tv) Marquee.radius.card else 12.dp)
    Box(
        Modifier
            .size(if (tv) 80.dp else 48.dp)
            .clearAndSetSemantics { }
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
    modifier: Modifier = Modifier,
    /** Said in place of the word, as one stop (the phone's installed card). */
    spoken: String? = null,
) {
    val tv = Marquee.form == Form.Tv
    Row(
        modifier
            .then(
                // Its own node (not merged into the card), saying [spoken] alone.
                if (spoken !=
                    null
                ) {
                    Modifier.semantics(mergeDescendants = true) { contentDescription = spoken }
                } else {
                    Modifier
                },
            ).heightIn(min = if (tv) 64.dp else 44.dp),
        horizontalArrangement = Arrangement.spacedBy(if (tv) 12.dp else 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (installed) MqIcon(Icons.Check, tint = Marquee.colors.ok, size = if (tv) 26.dp else 18.dp)
        MqText(
            text,
            modifier = if (spoken != null) Modifier.clearAndSetSemantics { } else Modifier,
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
        if (busy) {
            Spinner(
                size = 26.dp,
            )
        } else {
            MqIcon(if (turnOn) Icons.Power else Icons.Plus, tint = content, size = 26.dp)
        }
        MqText(label, role = TextRole.Callout, color = content, weight = FontWeight.SemiBold, maxLines = 1)
    }
}
