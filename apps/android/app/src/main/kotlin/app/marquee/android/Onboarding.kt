package app.marquee.android

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import app.marquee.core.DefaultsCopy
import app.marquee.core.DefaultsStep
import app.marquee.core.InstallCards
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.StreamChoiceSettings
import app.marquee.core.model.DefaultAddonInfo
import app.marquee.core.model.DefaultAddons
import app.marquee.core.model.DefaultInstalled
import app.marquee.core.route.DefaultsMode
import app.marquee.core.session.CoreSession
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.LiveSpoken
import app.marquee.design.MarkTile
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.PrimaryButton
import app.marquee.design.SecondaryButton
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.SwitchRow
import app.marquee.design.TextRole
import app.marquee.design.focusClickable
import app.marquee.design.marqueeFocus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

/**
 * A new user's first screen, one decision: how to begin. "I use Stremio" brings a library over (only
 * offered when [onImport] is given), "Start fresh" and "Browse first" open the recommended addons
 * ([onDefaults]), and "I already use Marquee" joins that device's sync account (only offered when
 * [onPair] is given). Nothing is typed here, and nothing is installed before the addons step.
 */
@Composable
fun OnboardingScreen(
    onDefaults: () -> Unit,
    onImport: (() -> Unit)? = null,
    onPair: (() -> Unit)? = null,
) {
    val tv = Marquee.form == Form.Tv
    val first = remember { FocusRequester() }
    AutoFocus(first)

    Column(
        Modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.systemBars)
            .padding(horizontal = Marquee.safe.side, vertical = Marquee.safe.top),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.l),
    ) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            MqText("Marquee", role = TextRole.Title2, modifier = Modifier.weight(1f))
            SkipLink("Browse first", onDefaults)
        }
        // Scrolls when it does not fit (a small phone at a large font scale); centered when it does.
        BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
            Column(
                Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).heightIn(min = maxHeight),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.m, Alignment.CenterVertically),
            ) {
                MqText("How would you like to begin?", role = if (tv) TextRole.Title1 else TextRole.Title2)
                MqText(
                    "No account needed. Everything stays on this device until you turn on sync.",
                    role = TextRole.Body,
                    color = Marquee.colors.muted,
                    modifier = Modifier.widthIn(max = 720.dp),
                )
                val doors: @Composable (Modifier) -> Unit = { door ->
                    onImport?.let {
                        Door(
                            "I use Stremio",
                            "Bring your addons, library and Continue Watching over with a code.",
                            Icons.Link,
                            it,
                            door,
                            first,
                        )
                    }
                    Door(
                        "Start fresh",
                        "Choose from the recommended addons, then start watching.",
                        Icons.Plus,
                        onDefaults,
                        door,
                        first.takeIf { onImport == null },
                    )
                    onPair?.let {
                        Door(
                            "I already use Marquee",
                            "Join your other device's sync account with a code. Library and addons follow.",
                            Icons.QrCode,
                            it,
                            door,
                            null,
                        )
                    }
                }
                val single = onImport == null && onPair == null
                if (tv) {
                    // The doors share the width; a single one keeps the width of a card, not of the screen.
                    Row(
                        Modifier.height(IntrinsicSize.Min),
                        horizontalArrangement = Arrangement.spacedBy(Marquee.space.l),
                    ) {
                        doors(if (single) Modifier.widthIn(max = 640.dp) else Modifier.weight(1f).fillMaxHeight())
                    }
                } else {
                    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                        doors(Modifier.fillMaxWidth())
                    }
                }
            }
        }
    }
}

/** The question's write for the page's life. */
class StreamQuestionHolder(
    session: CoreSession,
) : ViewModel() {
    val page = StreamChoiceSettings(session.ops, viewModelScope)
}

/**
 * The one question of a first run, after the recommended addons (plans/m13-stream-choice.md 4.5): how
 * Marquee should pick what to play. Start fast has focus; each answer writes its patch, then [onDone].
 * Skip writes nothing, since the defaults are Start fast's.
 */
@Composable
fun StreamQuestionScreen(
    session: CoreSession,
    onDone: () -> Unit,
) {
    val holder =
        viewModel<StreamQuestionHolder>(
            key = "stream-question",
            factory = viewModelFactory { initializer { StreamQuestionHolder(session) } },
        )
    val page = holder.page
    val tv = Marquee.form == Form.Tv
    val fast = remember { FocusRequester() }
    AutoFocus(fast)
    Column(
        Modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.systemBars)
            .padding(horizontal = Marquee.safe.side, vertical = Marquee.safe.top),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.l),
    ) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            MqText("Marquee", role = TextRole.Title2, modifier = Modifier.weight(1f))
            SkipLink(StreamChoiceCopy.SKIP, onDone)
        }
        // In the middle of the screen when it fits, as the first step; it scrolls when it does not.
        BoxWithConstraints(Modifier.weight(1f).fillMaxWidth()) {
            Column(
                Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).heightIn(min = maxHeight),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.m, Alignment.CenterVertically),
            ) {
                MqText(StreamChoiceCopy.QUESTION_TITLE, role = if (tv) TextRole.Title1 else TextRole.Title2)
                MqText(StreamChoiceCopy.QUESTION_LEAD, role = TextRole.Body, color = Marquee.colors.muted)
                val doors: @Composable (Modifier) -> Unit = { door ->
                    Door(
                        StreamChoiceCopy.FAST.label,
                        StreamChoiceCopy.FAST.text,
                        Icons.Zap,
                        { page.answer(StreamChoiceCopy.FAST, onDone) },
                        door,
                        fast,
                    )
                    Door(
                        StreamChoiceCopy.BEST.label,
                        StreamChoiceCopy.BEST.text,
                        Icons.Gem,
                        { page.answer(StreamChoiceCopy.BEST, onDone) },
                        door,
                        null,
                    )
                }
                if (tv) {
                    Row(
                        Modifier.height(IntrinsicSize.Min),
                        horizontalArrangement = Arrangement.spacedBy(Marquee.space.l),
                    ) { doors(Modifier.weight(1f).fillMaxHeight()) }
                } else {
                    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                        doors(Modifier.fillMaxWidth())
                    }
                }
                page.problem?.let { StatusWord("Could not save the choice: $it", Status.Bad) }
            }
        }
    }
}

/** The defaults step for the page's life (the state and the install live in [DefaultsStep]). */
class DefaultsHolder(
    session: CoreSession,
) : ViewModel() {
    val step = DefaultsStep(session.ops, viewModelScope).also { it.load() }
}

/**
 * The recommended addons (plans/m13-default-addons.md 6): the core's list grouped, a switch per row,
 * on unless the profile has it off, and one Continue that installs them in one write. Continue stays
 * in sight under the list; on TV it has focus first, so one press accepts them all, and down from the
 * last row reaches it.
 */
@Composable
fun DefaultsScreen(
    session: CoreSession,
    mode: DefaultsMode,
    onDone: () -> Unit,
) {
    val holder =
        viewModel<DefaultsHolder>(
            key = "defaults",
            factory = viewModelFactory { initializer { DefaultsHolder(session) } },
        )
    val tv = Marquee.form == Form.Tv
    val go = remember { FocusRequester() }
    val step = holder.step
    val answer = step.answer
    if (answer != null) AutoFocus(go)
    val rows = DefaultsCopy.rows(answer?.addons.orEmpty(), mode)
    Column(
        Modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.systemBars)
            .padding(horizontal = Marquee.safe.side, vertical = Marquee.safe.top),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        val width = Modifier.widthIn(max = 900.dp).fillMaxWidth()
        // More air between groups than between the rows of a group.
        Column(
            width.weight(1f).verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(Marquee.space.l),
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                MqText(
                    if (mode == DefaultsMode.Review) DefaultsCopy.REVIEW_TITLE else DefaultsCopy.TITLE,
                    role = if (tv) TextRole.Title1 else TextRole.Title2,
                )
                MqText(
                    when (mode) {
                        DefaultsMode.Review -> DefaultsCopy.REVIEW_LEAD
                        DefaultsMode.Settings -> DefaultsCopy.SETTINGS_LEAD
                        else -> DefaultsCopy.LEAD
                    },
                    role = TextRole.Body,
                    color = Marquee.colors.muted,
                )
            }
            step.problem?.let {
                StateMessage(
                    DefaultsCopy.NOT_LOADED,
                    it,
                    icon = Icons.CircleAlert,
                    actionLabel = "Try again",
                    onAction = { step.load() },
                )
            }
            if (answer == null && step.problem == null) StatusWord("Loading", Status.Warn)
            for ((group, addons) in rows.groupBy { it.group }) {
                Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    // The group's label sits over the marks.
                    Kicker(DefaultsCopy.group(group), side = 0.dp)
                    for (addon in addons) {
                        // After an import, the ones it brought keep how they are and say where they came from.
                        val on = DefaultsCopy.on(addon, mode, step.switches)
                        DefaultRow(addon, on.takeIf { DefaultsCopy.hasSwitch(addon, mode) }) {
                            if (!step.busy) step.switches[addon.key] = !on
                        }
                    }
                    if (group == DefaultsCopy.CATALOGS &&
                        DefaultsCopy.catalogMissing(answer?.addons.orEmpty(), mode, step.switches)
                    ) {
                        LiveSpoken(
                            DefaultsCopy.NO_CATALOG,
                        ) { Note(DefaultsCopy.NO_CATALOG, failed = true, side = 0.dp) }
                    }
                }
            }
            step.failed?.let {
                StateMessage(DefaultsCopy.FAILED, "$it. Try again.", icon = Icons.CircleAlert)
            }
            if (tv && (mode == DefaultsMode.FirstRun || mode == DefaultsMode.AfterImport)) {
                Note(DefaultsCopy.TV_MORE, side = 0.dp)
            }
        }
        Row(width) {
            PrimaryButton(
                when {
                    step.busy -> DefaultsCopy.WORKING
                    mode == DefaultsMode.Settings -> DefaultsCopy.SAVE
                    else -> DefaultsCopy.CONTINUE
                },
                { step.save(mode, onDone) },
                icon = Icons.Check,
                enabled = answer != null && !step.busy,
                focusRequester = go,
            )
        }
    }
}

/**
 * One default: its mark, its name and line from the core, and its switch ([on] null: no switch, it
 * came from Stremio). The whole row is the switch, one focus stop on TV; TalkBack reads the name, the
 * line and the state.
 */
@Composable
private fun DefaultRow(
    addon: DefaultAddonInfo,
    on: Boolean?,
    onToggle: () -> Unit,
) {
    val colors = Marquee.colors
    val tv = Marquee.form == Form.Tv
    val shape = RoundedCornerShape(Marquee.radius.l)
    val card =
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surface)
            .border(1.dp, colors.edge, shape)
    val padding = PaddingValues(if (tv) 16.dp else 12.dp)
    val words: @Composable RowScope.() -> Unit = {
        MarkTile(addon.logo, addon.name)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            MqText(addon.name, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 1)
            MqText(addon.line, role = TextRole.Caption, color = colors.muted)
        }
    }
    if (on != null) {
        SwitchRow(on, onToggle, Modifier.marqueeFocus(Marquee.radius.l, scale = 1.01f).then(card), padding, words)
    } else {
        Row(
            card.semantics(mergeDescendants = true) {}.padding(padding),
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            words()
            MqText(DefaultsCopy.FROM_STREMIO, role = TextRole.Caption, color = colors.muted)
        }
    }
}

/** The profile's `defaultsSeen`, read once from the settings view (null when it does not answer in time). */
internal suspend fun defaultsSeen(session: CoreSession): Int? {
    val view = session.settings()
    return try {
        withTimeoutOrNull(OFFER_WAIT_MS) { snapshotFlow { view.snapshot }.filterNotNull().first().defaultsSeen }
    } finally {
        view.close()
    }
}

private const val OFFER_WAIT_MS = 5_000L

/** The one-time Home card: the defaults this profile does not have, Review or Not now. */
@Composable
internal fun RecommendedCard(
    answer: DefaultAddons,
    onReview: () -> Unit,
    onNotNow: () -> Unit,
) {
    val missing = answer.addons.filter { it.installed == DefaultInstalled.Absent }.map { it.name }
    // A remote landing on a button brings the whole card into view, so its words are read first.
    val whole = remember { BringIntoViewRequester() }
    val scope = rememberCoroutineScope()
    Box(
        Modifier
            .bringIntoViewRequester(whole)
            .onFocusChanged { if (it.hasFocus) scope.launch { whole.bringIntoView() } },
    ) {
        // Glass buttons: Home's one white pill is the hero's Play.
        Section {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                MqText(DefaultsCopy.REVIEW_TITLE, role = TextRole.Title3)
                MqText(DefaultsCopy.cardLine(missing), role = TextRole.Callout, color = Marquee.colors.muted)
                Row(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                    SecondaryButton(DefaultsCopy.REVIEW, onReview, icon = Icons.ListChecks)
                    SecondaryButton(DefaultsCopy.NOT_NOW, onNotNow)
                }
            }
        }
    }
}

/** One of the ways to begin: a card with an icon, a title and one line. */
@Composable
private fun Door(
    title: String,
    body: String,
    icon: ImageVector,
    onClick: () -> Unit,
    modifier: Modifier,
    focusRequester: FocusRequester?,
) {
    val colors = Marquee.colors
    val shape = RoundedCornerShape(Marquee.radius.l)
    Column(
        modifier
            .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
            .marqueeFocus(Marquee.radius.l)
            .clip(shape)
            .background(colors.surface)
            .border(2.dp, colors.control, shape)
            .focusClickable(onClick, Role.Button)
            .padding(Marquee.space.m),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.xs),
    ) {
        MqIcon(icon)
        MqText(title, role = TextRole.Title3, maxLines = 2)
        MqText(body, role = TextRole.Callout, color = colors.muted)
    }
}

/** A quiet text action in a screen's corner. */
@Composable
private fun SkipLink(
    label: String,
    onClick: () -> Unit,
) {
    Row(
        Modifier
            .heightIn(min = 48.dp)
            .marqueeFocus(Marquee.radius.m)
            .clip(RoundedCornerShape(Marquee.radius.m))
            .focusClickable(onClick, Role.Button)
            .padding(horizontal = Marquee.space.s),
        horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqText(label, role = TextRole.Callout, weight = FontWeight.SemiBold, color = Marquee.colors.muted)
        MqIcon(Icons.ChevronRight, tint = Marquee.colors.muted)
    }
}
