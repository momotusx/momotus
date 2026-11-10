package app.marquee.android

import android.content.Context
import android.os.SystemClock
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import androidx.navigation3.runtime.NavKey
import app.marquee.core.DefaultsCopy
import app.marquee.core.DefaultsStep
import app.marquee.core.FirstRun
import app.marquee.core.FirstRunCopy
import app.marquee.core.FirstSetup
import app.marquee.core.LanguagePicks
import app.marquee.core.ProfileCopy
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.client.CoreLog
import app.marquee.core.model.DefaultAddonInfo
import app.marquee.core.model.DefaultAddons
import app.marquee.core.model.DefaultInstalled
import app.marquee.core.model.LanguageChoice
import app.marquee.core.model.Prefs
import app.marquee.core.model.ProfileInfo
import app.marquee.core.route.DefaultsMode
import app.marquee.core.route.Route
import app.marquee.core.session.CoreSession
import app.marquee.design.AvatarArt
import app.marquee.design.BrandLockup
import app.marquee.design.FocusInk
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.LiveSpoken
import app.marquee.design.MarkTile
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.MqTextField
import app.marquee.design.PosterWall
import app.marquee.design.PrimaryButton
import app.marquee.design.SecondaryButton
import app.marquee.design.Spinner
import app.marquee.design.StateMessage
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.SwitchRow
import app.marquee.design.TextRole
import app.marquee.design.Tokens
import app.marquee.design.WallDrift
import app.marquee.design.WallGeometry
import app.marquee.design.avatarColor
import app.marquee.design.focusClickable
import app.marquee.design.focusPalette
import app.marquee.design.focusSelectable
import app.marquee.design.marqueeFocus
import app.marquee.design.wallColumns
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

// First run, Poster Wall (plans/first-screens.md, design/exploration/first-screens/round2.html combo A): Welcome over
// the drifting wall, then sheets over the same wall held still: Login, Make your first profile, the one-page setup,
// then Home. The order is FirstRun.next's; FirstRunNav applies it to the back stack.

/**
 * First run's order on the back stack: [FirstRun.next] over the app's [state], each step pushed so Back follows
 * plans/first-screens.md 5 (an account made here makes the profile form the root, so Back leaves the app).
 */
internal class FirstRunNav(
    private val stack: MutableList<NavKey>,
    private val tv: Boolean,
    private val state: () -> FirstRun.State,
    private val onHome: () -> Unit,
    private val onWho: () -> Unit,
) {
    fun go(after: FirstRun.After) {
        when (FirstRun.next(state(), after)) {
            FirstRun.Step.Login -> {
                stack.add(
                    if (tv) {
                        Route.TvSignIn
                    } else {
                        Route.SignIn(
                            firstRun = true,
                            returning =
                                after == FirstRun.After.AlreadyUse,
                        )
                    },
                )
            }

            FirstRun.Step.Profile -> {
                when {
                    after == FirstRun.After.Created -> {
                        stack.clear()
                        stack.add(Route.FirstProfile)
                    }

                    // A sign-in pushed over the form (a pairing link) whose Not now comes back to it, not a second form.
                    Route.FirstProfile in stack -> {
                        while (stack.last() != Route.FirstProfile) stack.removeAt(stack.lastIndex)
                    }

                    else -> {
                        stack.add(Route.FirstProfile)
                    }
                }
            }

            FirstRun.Step.Setup -> {
                stack.add(Route.Setup)
            }

            FirstRun.Step.Home -> {
                onHome()
            }

            FirstRun.Step.Who -> {
                onWho()
            }

            FirstRun.Step.Welcome, FirstRun.Step.None -> {
                Unit
            }
        }
    }

    /** Start watching or Skip went through: the Import sheet when the Stremio library comes, else Home. */
    fun setupDone(import: Boolean) {
        if (import) stack.add(Route.Import) else onHome()
    }

    companion object {
        /** The stack a (re)start opens on while the profile is fresh. */
        fun initial(s: FirstRun.State): List<Route> =
            when (FirstRun.next(s, FirstRun.After.Start)) {
                FirstRun.Step.Profile -> listOf(Route.FirstProfile)

                // Back from setup is the profile form (5), on a restart too.
                FirstRun.Step.Setup -> listOf(Route.FirstProfile, Route.Setup)

                else -> listOf(Route.Welcome)
            }
    }
}

/** Welcome's Pause motion, kept on this device and never synced. */
internal object WallPause {
    private const val PREFS = "first-wall"

    fun on(context: Context): Boolean =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean("paused", false)

    fun set(
        context: Context,
        on: Boolean,
    ) = context
        .getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        .edit()
        .putBoolean("paused", on)
        .apply()
}

/**
 * The wall with the walk build's line (plans/first-screens.md 3.1, 9): `first-wall poster <ms> tiles6s <n>/<tiles>`,
 * the time from its first frame to its first poster and, at 6 s, the tiles on screen with their poster drawn over
 * the tiles on screen. It reads the posters and the screen as they are at 6 s, not as the first frame saw them.
 */
@Composable
internal fun FirstWall(
    posters: List<String>,
    drift: WallDrift,
    sheet: Boolean,
) {
    val walk = BuildConfig.WALK
    val shown = remember { SystemClock.elapsedRealtime() }
    val first = remember { longArrayOf(-1L) }
    val landed = remember { mutableSetOf<String>() }
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val wall by rememberUpdatedState(WallGeometry(maxWidth.value, maxHeight.value, wallColumns(Marquee.form)))
        val now by rememberUpdatedState(posters)
        if (walk) {
            LaunchedEffect(Unit) {
                delay(6_000)
                val (filled, visible) = wall.filled(drift.ms, now, landed)
                CoreLog.d("first-wall poster ${first[0].takeIf { it >= 0 } ?: "none"} tiles6s $filled/$visible")
            }
        }
        PosterWall(
            posters,
            drift,
            sheet,
            onPoster =
                if (walk) {
                    { url ->
                        if (first[0] < 0) first[0] = SystemClock.elapsedRealtime() - shown
                        landed += url
                    }
                } else {
                    null
                },
        )
    }
}

/**
 * A first-run sheet over the still wall (round2.html `.sheet`): on the phone a bottom sheet (radius 30, the grabber
 * when [onDrag] is given, a drag down going back), on TV a centered sheet [width] wide. Its title is a heading and
 * names the pane; [corner] sits at its right (Not now, Skip). The body scrolls and [foot] stays in sight under it,
 * above the keyboard.
 */
@Composable
internal fun FirstSheet(
    title: String?,
    modifier: Modifier = Modifier,
    width: Dp = 980.dp,
    corner: (@Composable () -> Unit)? = null,
    line: String? = null,
    onDrag: (() -> Unit)? = null,
    scroll: Boolean = true,
    pad: Boolean = true,
    foot: (@Composable () -> Unit)? = null,
    body: @Composable ColumnScope.() -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val side = if (tv) 48.dp else 20.dp
    val shape =
        if (tv) {
            RoundedCornerShape(
                28.dp,
            )
        } else {
            RoundedCornerShape(topStart = Tokens.sheetRadius, topEnd = Tokens.sheetRadius)
        }
    Box(
        modifier
            .fillMaxSize()
            .then(
                if (tv) {
                    Modifier.padding(vertical = Marquee.safe.top, horizontal = Marquee.safe.side)
                } else {
                    Modifier.windowInsetsPadding(
                        WindowInsets.safeDrawing.only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal),
                    )
                },
            ),
        contentAlignment = if (tv) Alignment.Center else Alignment.BottomCenter,
    ) {
        Column(
            Modifier
                .then(if (tv) Modifier.widthIn(max = width) else Modifier.fillMaxWidth().padding(top = 24.dp))
                .clip(shape)
                .background(Tokens.sheetFill)
                .semantics { title?.let { paneTitle = it } }
                .then(if (tv) Modifier else Modifier.windowInsetsPadding(WindowInsets.navigationBars).imePadding())
                .padding(top = if (tv) 36.dp else 10.dp, bottom = if (tv) 40.dp else 24.dp),
        ) {
            if (onDrag != null && !tv) Grabber(onDrag)
            if (title != null || corner != null) {
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = side),
                    horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    MqText(
                        title.orEmpty(),
                        Modifier.weight(1f).semantics { heading() },
                        role = if (tv) TextRole.Title1 else TextRole.Title2,
                        color = Color.White,
                        weight = FontWeight.Bold,
                    )
                    corner?.invoke()
                }
            }
            line?.let {
                MqText(
                    it,
                    Modifier.padding(horizontal = side).padding(top = 6.dp),
                    role = TextRole.Callout,
                    color = Marquee.colors.muted,
                )
            }
            Column(
                Modifier
                    .weight(1f, fill = false)
                    .then(if (scroll) Modifier.verticalScroll(rememberScrollState()) else Modifier)
                    .padding(horizontal = if (pad) side else 0.dp)
                    .padding(top = if (tv) 26.dp else 18.dp),
                content = body,
            )
            foot?.let { Box(Modifier.padding(horizontal = side).padding(top = 16.dp)) { it() } }
        }
    }
}

/** The phone sheet's grabber: a drag down of 48 dp goes back; Back does the same, so it is not read out. */
@Composable
private fun ColumnScope.Grabber(onDrag: () -> Unit) {
    var dragged by remember { mutableStateOf(0f) }
    val far = with(LocalDensity.current) { 48.dp.toPx() }
    Box(
        Modifier
            .align(Alignment.CenterHorizontally)
            .size(width = 96.dp, height = 24.dp)
            .clearAndSetSemantics { }
            .draggable(
                rememberDraggableState { dragged += it },
                Orientation.Vertical,
                onDragStarted = { dragged = 0f },
                onDragStopped = { if (dragged > far) onDrag() },
            ),
        contentAlignment = Alignment.Center,
    ) { Box(Modifier.size(40.dp, 5.dp).clip(CircleShape).background(Color.White.copy(alpha = 0.3f))) }
}

/** A sheet's text action at its top right (Not now, Skip): 48 dp tall, the key colour; white with dark words on TV focus. */
@Composable
internal fun SheetLink(
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    focusRequester: FocusRequester? = null,
) {
    var focused by remember { mutableStateOf(false) }
    val tv = Marquee.form == Form.Tv
    val lit = focused && tv
    Box(
        modifier
            .heightIn(min = 48.dp)
            .widthIn(min = 48.dp)
            .then(focusRequester?.let { Modifier.focusRequester(it) } ?: Modifier)
            .marqueeFocus(Marquee.radius.m)
            .onFocusChanged { focused = it.isFocused }
            .clip(RoundedCornerShape(Marquee.radius.m))
            .background(if (lit) Marquee.colors.primary else Color.Transparent)
            .focusClickable(onClick, Role.Button)
            .padding(horizontal = Marquee.space.s),
        contentAlignment = Alignment.Center,
    ) {
        MqText(
            label,
            role = TextRole.Headline,
            weight = FontWeight.SemiBold,
            color =
                when {
                    lit -> Marquee.colors.onPrimary
                    tv -> Color.White
                    else -> Marquee.colors.key
                },
        )
    }
}

/**
 * Welcome (3.2): the lockup, the line, the lead and the two ways in over the drifting wall, on a ramp into black; the
 * Pause motion toggle at the top right. [onAlreadyUse] null: no sync server, no "I already use Momotus". TV centers a
 * 760 block with the buttons in a row, Get started focused.
 */
@Composable
internal fun WelcomeScreen(
    drift: WallDrift,
    onGetStarted: () -> Unit,
    onAlreadyUse: (() -> Unit)?,
) {
    val tv = Marquee.form == Form.Tv
    val go = remember { FocusRequester() }
    AutoFocus(go)
    BoxWithConstraints(Modifier.fillMaxSize().semantics { paneTitle = FirstRunCopy.WELCOME_TITLE }) {
        Box(Modifier.matchParentSize().background(Brush.verticalGradient(*welcomeShade(tv))))
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .heightIn(min = maxHeight)
                .windowInsetsPadding(WindowInsets.safeDrawing)
                .padding(
                    start = if (tv) Marquee.safe.side else 24.dp,
                    end = if (tv) Marquee.safe.side else 24.dp,
                    top = if (tv) Marquee.safe.top else 4.dp,
                    bottom = if (tv) Marquee.safe.bottom else 40.dp,
                ),
            verticalArrangement = Arrangement.SpaceBetween,
            horizontalAlignment = if (tv) Alignment.CenterHorizontally else Alignment.Start,
        ) {
            Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterEnd) { MotionToggle(drift) }
            Column(
                Modifier.padding(top = Marquee.space.l).then(if (tv) Modifier.widthIn(max = 760.dp) else Modifier),
                horizontalAlignment = if (tv) Alignment.CenterHorizontally else Alignment.Start,
            ) {
                BrandLockup(if (tv) 64.dp else 52.dp)
                MqText(
                    FirstRunCopy.WELCOME_TITLE,
                    Modifier.padding(top = 22.dp).semantics { heading() },
                    role = if (tv) TextRole.Hero else TextRole.Title1,
                    color = Color.White,
                    weight = FontWeight.Bold,
                    textAlign = if (tv) TextAlign.Center else null,
                )
                MqText(
                    FirstRunCopy.WELCOME_LEAD,
                    Modifier.padding(top = 12.dp),
                    role = if (tv) TextRole.Title3 else TextRole.Body,
                    color = Marquee.colors.muted,
                    textAlign = if (tv) TextAlign.Center else null,
                )
                if (tv) {
                    Row(Modifier.padding(top = 26.dp), horizontalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                        PrimaryButton(
                            FirstRunCopy.GET_STARTED,
                            onGetStarted,
                            Modifier.widthIn(min = 220.dp),
                            big = true,
                            focusRequester = go,
                        )
                        onAlreadyUse?.let {
                            SecondaryButton(
                                FirstRunCopy.ALREADY_USE,
                                it,
                                Modifier.widthIn(min = 220.dp),
                                big = true,
                            )
                        }
                    }
                } else {
                    Column(Modifier.padding(top = 26.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        PrimaryButton(
                            FirstRunCopy.GET_STARTED,
                            onGetStarted,
                            Modifier.fillMaxWidth(),
                            big = true,
                            focusRequester = go,
                        )
                        onAlreadyUse?.let {
                            SecondaryButton(
                                FirstRunCopy.ALREADY_USE,
                                it,
                                Modifier.fillMaxWidth(),
                                big = true,
                            )
                        }
                    }
                }
            }
        }
    }
}

/** While a first-run join pulls the account (15 s at most): the sheet says so over the still wall (6). */
@Composable
internal fun GettingProfiles() {
    // It holds every input while it shows: taps, the remote's keys (focus is taken here) and, in AppNav, Back.
    val hold = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { hold.requestFocus() } }
    Box(
        Modifier
            .fillMaxSize()
            .testTag(GETTING_PROFILES_LAYER)
            .background(Tokens.wallDimNoBlur)
            .pointerInput(
                Unit,
            ) { awaitPointerEventScope { while (true) awaitPointerEvent().changes.forEach { it.consume() } } }
            .onPreviewKeyEvent { true }
            .focusRequester(hold)
            .focusable(),
    ) {
        FirstSheet(null, width = 760.dp) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Spinner()
                LiveSpoken(FirstRunCopy.GETTING_PROFILES) {
                    MqText(FirstRunCopy.GETTING_PROFILES, role = TextRole.Headline, color = Color.White)
                }
            }
        }
    }
}

/** The pull's layer, for tests. */
internal const val GETTING_PROFILES_LAYER = "getting-profiles"

/** Welcome's ramp into black (round2.html `.v1 .shade`), the TV's rising sooner under its centered block. */
internal fun welcomeShade(tv: Boolean): Array<Pair<Float, Color>> =
    if (tv) {
        arrayOf(0f to black(0.2f), 0.3f to black(0.5f), 0.54f to black(0.9f), 0.72f to Color.Black)
    } else {
        arrayOf(0f to black(0.25f), 0.35f to black(0.45f), 0.66f to black(0.92f), 0.82f to Color.Black)
    }

private fun black(alpha: Float) = Color.Black.copy(alpha = alpha)

/** Pause motion or Play motion (WCAG 2.2.2, D4): named by what pressing it does, kept on this device. */
@Composable
private fun MotionToggle(drift: WallDrift) {
    val context = LocalContext.current
    // The choice this device kept: a paused wall stays paused at the next first run's Welcome.
    LaunchedEffect(Unit) { if (WallPause.on(context)) drift.paused = true }
    val tv = Marquee.form == Form.Tv
    var focused by remember { mutableStateOf(false) }
    val lit = focused && tv
    val size = if (tv) 64.dp else 48.dp
    Box(
        Modifier
            .size(size)
            .marqueeFocus(size / 2)
            .onFocusChanged { focused = it.isFocused }
            .clip(CircleShape)
            .background(if (lit) Marquee.colors.primary else Color.Black.copy(alpha = 0.4f))
            .semantics {
                contentDescription = if (drift.paused) FirstRunCopy.PLAY_MOTION else FirstRunCopy.PAUSE_MOTION
            }.focusClickable({
                drift.paused = !drift.paused
                WallPause.set(context, drift.paused)
            }, Role.Button),
        contentAlignment = Alignment.Center,
    ) {
        MqIcon(
            if (drift.paused) Icons.Play else Icons.Pause,
            tint = if (lit) Marquee.colors.onPrimary else Color.White,
            size = size * 0.45f,
        )
    }
}

/** The swatches Make your first profile offers: the first five avatar colours. */
internal val SWATCHES = ProfileCopy.COLORS.take(5)

/**
 * Make your first profile (3.4): the avatar, the Name field ("Main" as its placeholder, never focused on open), five
 * colour swatches and Continue. [account]: a made account's line, else the local one. Continue sends the typed name
 * or "" and the pressed swatch or "" ([onContinue]): the swatch the form opens on is never sent. TV focuses Continue;
 * up reaches the swatches, then the field.
 */
@Composable
internal fun FirstProfileSheet(
    owner: ProfileInfo?,
    account: Boolean,
    busy: Boolean,
    problem: String?,
    onBack: (() -> Unit)?,
    onContinue: (name: String, avatar: String) -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val stored = ProfileCopy.parseAvatar(owner?.avatar.orEmpty())
    var name by rememberSaveable { mutableStateOf("") }
    var picked by rememberSaveable { mutableStateOf<String?>(null) }
    val go = remember { FocusRequester() }
    AutoFocus(go)
    FirstSheet(
        FirstRunCopy.PROFILE_TITLE,
        width = 760.dp,
        line = if (account) FirstRunCopy.PROFILE_LINE_ACCOUNT else FirstRunCopy.PROFILE_LINE_LOCAL,
        onDrag = onBack,
        foot = {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                problem?.let { StatusWord(it, Status.Bad) }
                PrimaryButton(
                    FirstRunCopy.CONTINUE,
                    { onContinue(name, picked?.let { ProfileCopy.formatAvatar(stored.glyph, it) }.orEmpty()) },
                    if (tv) Modifier else Modifier.fillMaxWidth(),
                    big = true,
                    busy = busy,
                    focusRequester = go,
                )
            }
        },
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.CenterVertically) {
            AvatarArt(
                stored.glyph,
                picked ?: stored.color,
                ProfileCopy.initial(name.ifBlank { owner?.name ?: FirstRunCopy.NAME_PLACEHOLDER }),
                72.dp,
            )
            Box(Modifier.weight(1f)) {
                MqTextField(
                    name,
                    { name = it.take(30) },
                    label = FirstRunCopy.NAME_LABEL,
                    placeholder = FirstRunCopy.NAME_PLACEHOLDER,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
        Row(
            Modifier
                .padding(
                    top = 16.dp,
                ).selectableGroup()
                .semantics { contentDescription = FirstRunCopy.COLOUR_LABEL },
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            for (color in SWATCHES) {
                Choice(
                    selected = color == (picked ?: stored.color),
                    label = color.replaceFirstChar { it.uppercase() },
                    size = 48.dp,
                    onClick = { picked = color },
                ) {
                    Box(
                        Modifier.size(48.dp).clip(CircleShape).background(avatarColor(color) ?: Marquee.colors.raised),
                    )
                }
            }
        }
    }
}

/** Setup's subtitle chips over the core's languages: the shown codes, the one chosen, and the names. */
internal class SetupChips(
    val rule: FirstRun.Chips,
    val all: List<LanguageChoice>,
    /** The profile's subtitle languages as stored, kept whole when its first is the chip shown chosen. */
    val chosen: List<String> = emptyList(),
    /** The core's languages and the profile's answered (or failed): Start watching waits for it. */
    val loaded: Boolean = true,
) {
    /** The chips with a language found under More placed before No subtitles. */
    fun shown(picked: String?): List<String> =
        if (picked == null || picked in rule.chips) {
            rule.chips
        } else {
            rule.chips.dropLast(2) + picked + rule.chips.takeLast(2)
        }

    fun name(code: String): String =
        when (code) {
            FirstRun.NONE -> FirstRunCopy.NO_SUBTITLES
            FirstRun.MORE -> FirstRunCopy.MORE_LANGUAGES
            else -> LanguagePicks.name(code, all)
        }
}

/**
 * The one-page setup (3.5): Bring anything from Stremio?, How should Momotus pick what to play?, Subtitles start in;
 * every answer pre-chosen, so Start watching alone finishes first run (TV focuses it). Skip at the top right, the
 * sources line with Review left of Start watching. A failure names its part with Try again and keeps every answer.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun SetupSheet(
    setup: FirstSetup,
    profile: String,
    chips: SetupChips,
    onStart: (subtitle: String, chosen: List<String>) -> Unit,
    onSkip: () -> Unit,
    onReview: () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val go = remember { FocusRequester() }
    AutoFocus(go)
    var more by rememberSaveable { mutableStateOf(false) }
    val selected = setup.picked ?: chips.rule.selected
    val bring: @Composable () -> Unit = {
        SetupSection(FirstRunCopy.BRING_TITLE) {
            Column(Modifier.selectableGroup(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OptionRow(FirstRunCopy.BRING_LIBRARY, FirstRunCopy.BRING_LIBRARY_LINE.takeIf { tv }, setup.bring) {
                    setup.bring = true
                }
                OptionRow(FirstRunCopy.START_FRESH, FirstRunCopy.START_FRESH_LINE.takeIf { tv }, !setup.bring) {
                    setup.bring = false
                }
            }
        }
    }
    val stream: @Composable () -> Unit = {
        SetupSection(StreamChoiceCopy.QUESTION_TITLE) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .clip(RoundedCornerShape(14.dp))
                    .background(Color.White.copy(alpha = 0.08f))
                    .padding(4.dp)
                    .selectableGroup(),
            ) {
                Segment(StreamChoiceCopy.FAST.label, Icons.Zap, !setup.best) { setup.best = false }
                Segment(StreamChoiceCopy.BEST.label, Icons.Gem, setup.best) { setup.best = true }
            }
            MqText(
                if (setup.best) StreamChoiceCopy.BEST.text else StreamChoiceCopy.FAST.text,
                Modifier.padding(top = 8.dp),
                role = TextRole.Caption,
                color = Marquee.colors.muted,
            )
        }
    }
    val subtitles: @Composable () -> Unit = {
        SetupSection(FirstRunCopy.SUBTITLES_TITLE) {
            FlowRow(
                Modifier.selectableGroup(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                for (code in chips.shown(setup.picked)) {
                    if (code == FirstRun.MORE) {
                        SetupChip(chips.name(code), more, Icons.Search) { more = !more }
                    } else {
                        SetupChip(chips.name(code), code == selected) {
                            setup.picked = code
                            more = false
                        }
                    }
                }
            }
            if (more) {
                MoreLanguages(chips.all) { code ->
                    setup.picked = code
                    more = false
                }
            }
        }
    }
    FirstSheet(
        FirstRunCopy.SETUP_TITLE.replace("{profile}", profile),
        // Wider than the design's 1140: the TV's type scale needs it for three columns.
        width = 1500.dp,
        corner = { SheetLink(FirstRunCopy.SKIP, { if (!setup.busy) onSkip() }) },
        line = FirstRunCopy.SETUP_LINE.takeIf { tv },
        foot = {
            Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
                setup.failed?.let { part ->
                    val words =
                        if (part ==
                            FirstSetup.Part.Prefs
                        ) {
                            FirstRunCopy.SETUP_PREFS_FAILED
                        } else {
                            FirstRunCopy.SETUP_SOURCES_FAILED
                        }
                    LiveSpoken(words) { StatusWord(words, Status.Bad) }
                }
                val sources: @Composable RowScope.() -> Unit = {
                    MqIcon(Icons.Layers, tint = Marquee.colors.muted)
                    MqText(
                        FirstRunCopy.SOURCES_ON,
                        Modifier.weight(1f, fill = false),
                        role = TextRole.Callout,
                        color = Marquee.colors.muted,
                    )
                    SheetLink(FirstRunCopy.REVIEW, onReview)
                }
                val start =
                    if (setup.failed != null) FirstRunCopy.TRY_AGAIN else FirstRunCopy.START_WATCHING
                if (tv) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Row(
                            Modifier.weight(1f),
                            horizontalArrangement = Arrangement.spacedBy(10.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            content = sources,
                        )
                        PrimaryButton(
                            start,
                            {
                                if (chips.loaded) onStart(selected, chips.chosen)
                            },
                            Modifier.widthIn(
                                min = 240.dp,
                            ),
                            big = true,
                            busy = setup.busy || !chips.loaded,
                            focusRequester = go,
                        )
                    }
                } else {
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        content = sources,
                    )
                    PrimaryButton(
                        start,
                        { if (chips.loaded) onStart(selected, chips.chosen) },
                        Modifier.fillMaxWidth(),
                        big = true,
                        busy = setup.busy || !chips.loaded,
                        focusRequester = go,
                    )
                }
            }
        },
    ) {
        if (tv) {
            Row(horizontalArrangement = Arrangement.spacedBy(36.dp)) {
                Box(Modifier.weight(1.15f)) { bring() }
                Box(Modifier.weight(1f)) { stream() }
                Box(Modifier.weight(1f)) { subtitles() }
            }
        } else {
            Column(verticalArrangement = Arrangement.spacedBy(22.dp)) {
                bring()
                stream()
                subtitles()
            }
        }
    }
}

@Composable
private fun SetupSection(
    title: String,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column {
        MqText(
            title,
            Modifier.padding(bottom = 10.dp).semantics { heading() },
            role = TextRole.Headline,
            color = Color.White,
            weight = FontWeight.Bold,
        )
        content()
    }
}

/** One of Bring's two answers: a radio row, chosen with a white edge (round2.html `.opt`). */
@Composable
private fun OptionRow(
    title: String,
    line: String?,
    chosen: Boolean,
    onClick: () -> Unit,
) {
    var focused by remember { mutableStateOf(false) }
    val tv = Marquee.form == Form.Tv
    val shape = RoundedCornerShape(16.dp)
    FocusInk(focusPalette(focused)) {
        val colors = Marquee.colors
        val lit = focused && tv
        Row(
            Modifier
                .fillMaxWidth()
                .marqueeFocus(16.dp, Tokens.FOCUS_SCALE_TILE)
                .onFocusChanged { focused = it.isFocused }
                .clip(shape)
                .background(
                    when {
                        lit -> colors.primary
                        chosen -> Color.White.copy(alpha = 0.14f)
                        else -> Color.White.copy(alpha = 0.06f)
                    },
                ).then(if (chosen && !lit) Modifier.border(1.5.dp, Color.White.copy(alpha = 0.7f), shape) else Modifier)
                .focusSelectable(chosen, onClick)
                .heightIn(min = 56.dp)
                .padding(horizontal = 14.dp, vertical = 10.dp),
            horizontalArrangement = Arrangement.spacedBy(14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                MqText(title, role = TextRole.Headline, weight = FontWeight.SemiBold, color = colors.text)
                line?.let {
                    MqText(
                        it,
                        role = TextRole.Caption,
                        color = if (lit) colors.onPrimary.copy(alpha = 0.6f) else colors.muted,
                    )
                }
            }
            Box(
                Modifier
                    .size(22.dp)
                    .clip(CircleShape)
                    .border(if (chosen) 7.dp else 2.dp, if (chosen) colors.text else colors.dim, CircleShape),
            )
        }
    }
}

/** One half of the segmented Start fast / Best quality. */
@Composable
private fun RowScope.Segment(
    label: String,
    icon: ImageVector,
    chosen: Boolean,
    onClick: () -> Unit,
) {
    var focused by remember { mutableStateOf(false) }
    val white = chosen || (focused && Marquee.form == Form.Tv)
    val ink = if (white) Marquee.colors.onPrimary else Marquee.colors.muted
    Row(
        Modifier
            .weight(1f)
            .heightIn(min = 48.dp)
            .marqueeFocus(11.dp)
            .onFocusChanged { focused = it.isFocused }
            .clip(RoundedCornerShape(11.dp))
            .background(if (white) Marquee.colors.primary else Color.Transparent)
            .focusSelectable(chosen, onClick),
        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MqIcon(icon, tint = ink)
        MqText(label, role = TextRole.Headline, weight = FontWeight.SemiBold, color = ink)
    }
}

/** A subtitle chip, 48 high (round2.html `.chip`): the chosen one white with dark words. */
@Composable
private fun SetupChip(
    label: String,
    chosen: Boolean,
    icon: ImageVector? = null,
    onClick: () -> Unit,
) {
    var focused by remember { mutableStateOf(false) }
    val white = chosen || (focused && Marquee.form == Form.Tv)
    val ink = if (white) Marquee.colors.onPrimary else Color.White
    Row(
        Modifier
            .heightIn(min = 48.dp)
            .widthIn(min = 48.dp)
            .marqueeFocus(24.dp)
            .onFocusChanged { focused = it.isFocused }
            .clip(RoundedCornerShape(24.dp))
            .background(if (white) Marquee.colors.primary else Color.White.copy(alpha = 0.08f))
            .then(
                if (icon ==
                    null
                ) {
                    Modifier.focusSelectable(chosen, onClick)
                } else {
                    Modifier.focusClickable(onClick, Role.Button)
                },
            ).padding(start = 12.dp, end = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        icon?.let { MqIcon(it, tint = ink) }
        MqText(label, role = TextRole.Callout, weight = FontWeight.SemiBold, color = ink)
    }
}

/** More: every language the core knows, found by name. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun MoreLanguages(
    all: List<LanguageChoice>,
    onPick: (String) -> Unit,
) {
    var query by remember { mutableStateOf("") }
    Column(Modifier.padding(top = 12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        MqTextField(query, { query = it }, label = "Find a language", placeholder = "Polish, Korean, Arabic")
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            for (choice in LanguagePicks.search(all, query, emptyList())) {
                SetupChip(choice.name, false) { onPick(choice.code) }
            }
        }
    }
}

/** The setup sheet's chips: the core's languages (the device's as it lists it) and the profile's chosen ones. */
@Composable
internal fun rememberSetupChips(session: CoreSession): SetupChips {
    val device = DeviceCaps.languageCode(androidx.compose.ui.platform.LocalConfiguration.current.locales[0])
    // The device's language from the first frame; a list that failed to come keeps it.
    var chips by remember {
        mutableStateOf(
            SetupChips(FirstRun.chips(device, emptyList()), emptyList(), loaded = false),
        )
    }
    LaunchedEffect(device) {
        val all = runCatching { session.ops.languages(device) }.getOrDefault(emptyList())
        val chosen = prefsNow(session)?.subtitleLanguages.orEmpty()
        val mine = if (all.isEmpty()) device else all.firstOrNull { it.device }?.code
        chips = SetupChips(FirstRun.chips(mine, chosen), all, chosen)
    }
    return chips
}

/** The defaults step for the page's life (the state and the install live in [DefaultsStep]). */
class DefaultsHolder(
    session: CoreSession,
) : ViewModel() {
    val step = DefaultsStep(session.ops, viewModelScope).also { it.load() }
}

/**
 * The recommended addons (plans/m13-default-addons.md 6): the core's list grouped, a switch per row, on unless the
 * profile has it off, and one Continue that installs them in one write: the Home card's Review, setup's Review link
 * and Settings. Continue stays in sight under the list; on TV it has focus first, so one press accepts them all.
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
    val title = if (mode == DefaultsMode.Review) DefaultsCopy.REVIEW_TITLE else DefaultsCopy.TITLE
    val rows = DefaultsCopy.rows(answer?.addons.orEmpty(), mode)
    Column(
        Modifier
            .fillMaxSize()
            .background(Marquee.colors.bg)
            .semantics { paneTitle = title }
            .windowInsetsPadding(WindowInsets.safeDrawing)
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
                MqText(title, role = if (tv) TextRole.Title1 else TextRole.Title2)
                MqText(
                    if (mode == DefaultsMode.Settings) DefaultsCopy.SETTINGS_LEAD else DefaultsCopy.REVIEW_LEAD,
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
            // Read here: a TV focus turns the switch row white, and its words dark.
            MqText(addon.line, role = TextRole.Caption, color = Marquee.colors.muted)
        }
    }
    if (on != null) {
        SwitchRow(
            on,
            onToggle,
            Modifier.marqueeFocus(Marquee.radius.l, Tokens.FOCUS_SCALE_TILE).then(card),
            padding,
            content = words,
        )
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
internal suspend fun defaultsSeen(session: CoreSession): Int? = prefsNow(session)?.defaultsSeen

/** The profile's preferences, read once from the settings view (null when it does not answer in time). */
private suspend fun prefsNow(session: CoreSession): Prefs? {
    val view = session.settings()
    return try {
        withTimeoutOrNull(OFFER_WAIT_MS) { snapshotFlow { view.snapshot }.filterNotNull().first() }
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
    val missing = answer.addons.count { it.installed == DefaultInstalled.Absent }
    // A remote landing on a button brings the whole card into view, so its words are read first.
    val whole = remember { BringIntoViewRequester() }
    val scope = rememberCoroutineScope()
    Box(
        Modifier
            .bringIntoViewRequester(whole)
            .onFocusChanged { if (it.hasFocus) scope.launch { whole.bringIntoView() } },
    ) {
        // Secondary pills: Home's one white pill is the hero's Play.
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
