package app.marquee.android

import android.app.Activity
import android.app.KeyguardManager
import android.content.Context
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.isImeVisible
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.focusRestorer
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import app.marquee.core.AccountRules
import app.marquee.core.ErrorCopy
import app.marquee.core.ProfileCopy
import app.marquee.core.client.CoreError
import app.marquee.core.model.ProfileInfo
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.session.CoreSession
import app.marquee.design.ActionMenu
import app.marquee.design.AvatarArt
import app.marquee.design.FocusInk
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MenuAction
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.MqTextField
import app.marquee.design.PrimaryButton
import app.marquee.design.SecondaryButton
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.TextRole
import app.marquee.design.Tokens
import app.marquee.design.focusClickable
import app.marquee.design.focusPalette
import app.marquee.design.focusSoon
import app.marquee.design.marqueeFocus
import app.marquee.ui.phone.pageTop
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import java.util.concurrent.atomic.AtomicBoolean

// Profiles (plans/m11-profiles.md, P1 apps and v2): the picker in its modes, the PIN keypad,
// Manage profiles and the edit page, equal to the web's screens/profiles.tsx.

/** What a PIN try came to. */
sealed interface Tried {
    data object Ok : Tried

    data object Wrong : Tried

    data class Wait(
        val ms: Long,
    ) : Tried

    data class Problem(
        val text: String,
    ) : Tried
}

internal fun tried(e: Throwable): Tried {
    val c = e as? CoreError
    return when {
        c?.code == "wrongPin" -> Tried.Wrong
        c?.code == "locked" && c.retryAfterMs != null -> Tried.Wait(c.retryAfterMs!!)
        else -> Tried.Problem(ProfileCopy.problem(c?.code, ErrorCopy.words(e)))
    }
}

/** The avatar of a profile as stored, at [size]. */
@Composable
internal fun ProfileAvatar(
    p: ProfileInfo,
    size: androidx.compose.ui.unit.Dp,
    lit: Boolean = false,
) {
    val a = ProfileCopy.parseAvatar(p.avatar)
    AvatarArt(a.glyph, a.color, ProfileCopy.initial(p.name), size, lit = lit)
}

/**
 * Four digits, one try at a time. The digits live only here and go when it closes. On TV the grid
 * takes the remote (focus starts on 5); number keys type anywhere; Back deletes, and with no digit
 * leaves.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun Keypad(
    title: String,
    onSubmit: suspend (String) -> Tried,
    onCancel: () -> Unit,
    note: String? = null,
    /** A line under the title: why this PIN is asked. */
    lede: String? = null,
    cancelLabel: String = "Choose another profile",
    extra: (@Composable () -> Unit)? = null,
    initialProblem: Tried? = null,
) {
    val tv = Marquee.form == Form.Tv
    val colors = Marquee.colors
    val scope = rememberCoroutineScope()
    var digits by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var said by remember { mutableStateOf("") }
    var until by remember { mutableLongStateOf(0L) }
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    var attempt by remember { mutableStateOf(0) }
    val five = remember { FocusRequester() }
    val cancel = remember { FocusRequester() }
    val surface = remember { FocusRequester() }
    val waiting = until > now
    // Going to the back forgets what was typed.
    LifecycleEventEffect(Lifecycle.Event.ON_STOP) { digits = "" }

    fun show(result: Tried) {
        when (result) {
            Tried.Ok -> {
                Unit
            }

            Tried.Wrong -> {
                said = "Wrong PIN"
            }

            is Tried.Wait -> {
                until = System.currentTimeMillis() + result.ms
                now = System.currentTimeMillis()
                val minutes = (result.ms + 59_999) / 60_000
                said = "Too many tries. Try again in $minutes ${if (minutes == 1L) "minute" else "minutes"}."
                if (tv) runCatching { cancel.requestFocus() }
            }

            is Tried.Problem -> {
                said = result.text
            }
        }
    }
    LaunchedEffect(Unit) { initialProblem?.let(::show) }
    // The digits never outlive the keypad.
    DisposableEffect(Unit) { onDispose { digits = "" } }
    var waited by remember { mutableStateOf(false) }
    LaunchedEffect(waiting) {
        if (waiting) {
            waited = true
            while (until > System.currentTimeMillis()) {
                delay(1000)
                now = System.currentTimeMillis()
            }
            now = System.currentTimeMillis()
        } else if (waited) {
            // The wait is over: said once, and once the keys are back, the remote goes to them.
            waited = false
            said = "You can try again."
            withFrameNanos { }
            if (tv) runCatching { five.requestFocus() }
        }
    }
    // The keypad owns the keys from the start: on TV its 5, on a phone the keypad itself, so a
    // hardware keyboard types into it wherever the touch focus was.
    LaunchedEffect(Unit) {
        focusSoon(if (tv) five else surface)
    }

    fun press(d: Char) {
        if (busy || waiting || digits.length >= 4) return
        digits += d
        if (digits.length < 4) return
        val pin = digits
        val mine = ++attempt
        busy = true
        scope.launch {
            val result =
                try {
                    onSubmit(pin)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    tried(e)
                }
            if (mine != attempt) return@launch
            busy = false
            digits = ""
            show(result)
        }
    }

    fun back() {
        if (busy) return
        if (digits.isNotEmpty()) digits = digits.dropLast(1) else onCancel()
    }
    BackHandler { back() }

    Column(
        Modifier
            .fillMaxWidth()
            .focusRequester(surface)
            .focusable()
            .onPreviewKeyEvent { e ->
                if (e.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                val d = digitOf(e.key)
                when {
                    // A held key types once: a repeat never spends a try.
                    d != null && e.nativeKeyEvent.repeatCount > 0 -> {
                        true
                    }

                    d != null -> {
                        press(d)
                        true
                    }

                    e.key == Key.Backspace || e.key == Key.Delete -> {
                        back()
                        true
                    }

                    else -> {
                        false
                    }
                }
            },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(if (tv) 22.dp else 16.dp),
    ) {
        MqText(title, role = if (tv) TextRole.Title2 else TextRole.Title3, weight = FontWeight.Bold)
        lede?.let {
            MqText(
                it,
                role = TextRole.Callout,
                color = colors.muted,
                textAlign = TextAlign.Center,
                modifier = Modifier.widthIn(max = 720.dp),
            )
        }
        Row(
            Modifier.semantics(mergeDescendants = true) {
                contentDescription = "${digits.length} of 4 digits entered"
            },
            horizontalArrangement = Arrangement.spacedBy(if (tv) 22.dp else 16.dp),
        ) {
            repeat(4) { i ->
                val on = digits.length > i
                Box(
                    Modifier
                        .size(if (tv) 22.dp else 16.dp)
                        .clip(CircleShape)
                        .background(if (on) colors.text else Color.Transparent)
                        .border(2.dp, if (on) colors.text else colors.control, CircleShape),
                )
            }
        }
        MqText(
            said,
            modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
            role = TextRole.Callout,
            color = colors.bad,
        )
        if (waiting) {
            MqText("Try again in ${ProfileCopy.waitText(until - now)}", role = TextRole.Callout, color = colors.muted)
        }
        val keys = listOf("1", "2", "3", "4", "5", "6", "7", "8", "9", "", "0", "del")
        val key = if (tv) 104.dp else 72.dp
        Column(verticalArrangement = Arrangement.spacedBy(if (tv) 18.dp else 14.dp)) {
            keys.chunked(3).forEach { row ->
                Row(horizontalArrangement = Arrangement.spacedBy(if (tv) 18.dp else 14.dp)) {
                    row.forEach { k ->
                        if (k.isEmpty()) {
                            Spacer(Modifier.size(key))
                        } else {
                            PinKey(
                                label = k,
                                size = key,
                                enabled = !busy && !waiting,
                                modifier = if (k == "5") Modifier.focusRequester(five) else Modifier,
                                onClick = { if (k == "del") back() else press(k[0]) },
                            )
                        }
                    }
                }
            }
        }
        note?.let {
            MqText(
                it,
                role = TextRole.Callout,
                color = colors.muted,
                modifier = Modifier.widthIn(max = 720.dp),
            )
        }
        extra?.invoke()
        // A try with the core cannot be called off half way: leaving waits for its answer.
        SecondaryButton(cancelLabel, onCancel, enabled = !busy, focusRequester = cancel)
    }
}

private fun digitOf(key: Key): Char? =
    when (key) {
        Key.Zero, Key.NumPad0 -> '0'
        Key.One, Key.NumPad1 -> '1'
        Key.Two, Key.NumPad2 -> '2'
        Key.Three, Key.NumPad3 -> '3'
        Key.Four, Key.NumPad4 -> '4'
        Key.Five, Key.NumPad5 -> '5'
        Key.Six, Key.NumPad6 -> '6'
        Key.Seven, Key.NumPad7 -> '7'
        Key.Eight, Key.NumPad8 -> '8'
        Key.Nine, Key.NumPad9 -> '9'
        else -> null
    }

@Composable
private fun PinKey(
    label: String,
    size: androidx.compose.ui.unit.Dp,
    enabled: Boolean,
    modifier: Modifier,
    onClick: () -> Unit,
) {
    val colors = Marquee.colors
    var hasFocus by remember { mutableStateOf(false) }
    // White on a TV focus; a phone keyboard's focus is the ring alone.
    val focused = hasFocus && Marquee.form == Form.Tv
    Box(
        modifier
            .size(size)
            .alpha(if (enabled) 1f else 0.4f)
            .marqueeFocus(size / 2)
            .onFocusChanged { hasFocus = it.isFocused }
            .clip(CircleShape)
            .background(if (focused) colors.primary else colors.raised)
            .border(2.dp, if (focused) colors.primary else colors.control, CircleShape)
            .semantics { contentDescription = if (label == "del") "Delete" else label }
            .focusClickable(onClick, Role.Button, enabled),
        contentAlignment = Alignment.Center,
    ) {
        if (label == "del") {
            MqIcon(Icons.Delete, tint = if (focused) colors.onPrimary else colors.text, size = size * 0.36f)
        } else {
            MqText(
                label,
                role = if (Marquee.form == Form.Tv) TextRole.Title2 else TextRole.Title3,
                weight = FontWeight.SemiBold,
                color = if (focused) colors.onPrimary else colors.text,
            )
        }
    }
}

/**
 * The active profile's avatar at the TV bar's end (plans/nav-profile.md), opening [ProfileMenu]. [lit] while Settings
 * shows: the tab's white 20 percent pill. [modifier] is the bar's (its entry and focus report) and comes first.
 */
@Composable
fun PillProfile(
    gate: ProfileGate,
    lit: Boolean,
    modifier: Modifier,
    onSettings: () -> Unit,
    onPin: (String) -> Unit,
) {
    val me = AccountRules.activeProfile(gate.snapshot)
    if (me == null) {
        // No profile known for a moment: while Settings shows the slot still holds the bar's entry, so Up and Back find
        // a target; otherwise it takes no focus.
        Box(modifier.padding(start = 8.dp).size(PILL_TARGET).then(if (lit) Modifier.focusable() else Modifier))
        return
    }
    var focused by remember { mutableStateOf(false) }
    var open by remember { mutableStateOf(false) }
    Box(
        modifier
            .padding(start = 8.dp)
            // In the bar, as the tabs: it scales and turns white, and does not rise out of the bar.
            .marqueeFocus(PILL_TARGET / 2, Tokens.FOCUS_SCALE_TILE, rise = false)
            .onFocusChanged { focused = it.isFocused }
            .clip(CircleShape)
            .background(if (lit && !focused) PILL_LIT else Color.Transparent)
            .padding(4.dp)
            .semantics(mergeDescendants = true) {
                contentDescription = "Profile: ${me.name}"
                // The lit pill said aloud: Settings, which has no tab, is the place shown.
                if (lit) stateDescription = SETTINGS_SHOWN
            }.focusClickable({ open = true }, Role.Button),
    ) { ProfileAvatar(me, PILL_TARGET - 8.dp, lit = focused && Marquee.form == Form.Tv) }
    if (open) ProfileMenu(gate, { open = false }, onSettings, onPin)
}

/** What the lit avatar says beside its name. */
internal const val SETTINGS_SHOWN = "Settings shown"

/** The TV avatar's target, over Material's 48; the lit pill is the tabs' white at 20 percent. */
private val PILL_TARGET = 52.dp
private val PILL_LIT = Color(0x33FFFFFF)

/**
 * The profile's menu (plans/nav-profile.md): every profile, the current one checked and another with a PIN locked,
 * then Incognito (not in a kids session), then Settings; nothing account-like. A row does what its picker tile does:
 * the current one closes the menu, a syncing one nothing, one with a PIN opens the picker on its keypad ([onPin]), any
 * other switches. In Incognito its row is the checked one, with Leave Incognito right under it; focus starts on the
 * current row, never on Leave (plans/batch-10-incognito.md 5.1).
 */
@Composable
fun ProfileMenu(
    gate: ProfileGate,
    onDismiss: () -> Unit,
    onSettings: () -> Unit,
    onPin: (String) -> Unit,
) {
    val s = gate.snapshot ?: return
    val incognito = ProfileCopy.inIncognito(s)
    val asks = s.incognitoPin != null && !incognito
    val incognitoRows =
        when {
            incognito -> {
                listOf(
                    MenuAction("Incognito", Icons.Check, spoken = "Incognito, current profile") {},
                    MenuAction(ProfileCopy.INCOGNITO_LEAVE, Icons.LogOut) { gate.startLeave() },
                )
            }

            s.kids -> {
                emptyList()
            }

            else -> {
                listOf(
                    MenuAction(
                        "Incognito",
                        Icons.Lock.takeIf { asks },
                        spoken = if (asks) "Incognito, needs a PIN" else "Incognito",
                    ) {
                        // The menu leaves with this choice, as a profile's row: the switch runs on the session's scope.
                        if (asks) onPin(ProfileCopy.INCOGNITO) else gate.startSwitch(ProfileCopy.INCOGNITO)
                    },
                )
            }
        }
    val rows =
        s.profiles.map { p ->
            val current = p.id == s.active && !incognito
            val locked = p.hasPin && !current && !p.syncing
            MenuAction(
                if (p.syncing) "${p.name} · Syncing" else p.name,
                when {
                    current -> Icons.Check
                    locked -> Icons.Lock
                    else -> null
                },
                spoken =
                    listOfNotNull(
                        p.name,
                        "current profile".takeIf { current },
                        "still syncing".takeIf { p.syncing },
                        "locked with a PIN".takeIf { locked },
                    ).joinToString(", "),
            ) {
                when {
                    current || p.syncing -> {}

                    p.hasPin -> {
                        onPin(p.id)
                    }

                    // The menu leaves with this choice: the switch runs on the session's scope, not the menu's.
                    else -> {
                        gate.startSwitch(p.id)
                    }
                }
            }
        } + incognitoRows + MenuAction("Settings", Icons.Settings, onClick = onSettings)
    ActionMenu("Profiles", rows, onDismiss, start = menuStart(s))
}

/** The menu's first focus: the current row, Incognito's while it runs (the row after the profiles), never Leave. */
internal fun menuStart(s: ProfilesSnapshot): Int =
    if (ProfileCopy.inIncognito(s)) s.profiles.size else s.profiles.indexOfFirst { it.id == s.active }.coerceAtLeast(0)

/** The modes of the picker: a cold start's question, a locked session, or opened over the app. */
enum class PickerMode { Start, Locked, Switch }

/**
 * Who's watching. [Start] and [Locked] own the screen (the gate is shut); [Switch] was pushed over the
 * app from the pill or Settings, and Back returns where it was. Choosing another profile shuts the
 * gate and switches; a refusal comes back to the root picker with the keypad and its words.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun PickerScreen(
    gate: ProfileGate,
    session: CoreSession,
    mode: PickerMode,
    onBack: () -> Unit = {},
    onAdd: (() -> Unit)? = null,
    onManage: (() -> Unit)? = null,
) {
    val tv = Marquee.form == Form.Tv
    val colors = Marquee.colors
    val scope = rememberCoroutineScope()
    val s = gate.snapshot ?: return
    val locked = s.locked
    val me = s.profiles.firstOrNull { it.id == s.active }
    val owner = me?.isOwner == true
    // The profile whose keypad is open: picked here, or a switch the gate holds as refused (its
    // keypad opens again with the reason, whichever picker asked).
    // Kept across a rotation, which the device's lock screen can bring while a reset waits.
    // Opened by the bar's menu on a profile with a PIN: its keypad first, and its Cancel goes back where the menu was.
    val asked by rememberSaveable { mutableStateOf(if (mode == PickerMode.Switch) gate.takeAsked() else null) }
    var picked by rememberSaveable { mutableStateOf<String?>(asked) }
    // Incognito's keypad from its tile (Leave then returns to the chooser), not from the bar's menu (plans/batch-10-incognito.md 2.7).
    var fromTile by rememberSaveable { mutableStateOf(false) }
    val target = picked ?: gate.switching.takeIf { gate.switchProblem != null }
    var said by remember { mutableStateOf("") }
    val first = remember { FocusRequester() }
    val incognito = ProfileCopy.inIncognito(s)
    // Incognito asks for the PIN of the profile the core names; a refusal that is not about the PIN has no keypad.
    val pinRefused = gate.switchProblem?.code.let { it == "wrongPin" || it == "locked" }
    val chosen =
        if (target == ProfileCopy.INCOGNITO) {
            ProfileCopy.incognitoPinProfile(s)?.takeIf { picked == target || pinRefused }
        } else {
            s.profiles.firstOrNull { it.id == target }
        }
    // Focus starts on the current profile (the Incognito tile in Incognito), else on the first tile (the current one
    // was removed).
    val focusId =
        if (incognito) {
            ProfileCopy.INCOGNITO
        } else {
            s.profiles.firstOrNull { it.id == s.active }?.id ?: s.profiles.firstOrNull()?.id
        }
    LaunchedEffect(target, s.profiles) {
        if (target == ProfileCopy.INCOGNITO && chosen == null) {
            // Incognito did not open (a refusal at entry): the chooser stays with the reason.
            picked = null
            said = gate.switchProblem?.let { ProfileCopy.problem(it.code, ErrorCopy.words(it)) }.orEmpty()
            if (gate.switching != null) gate.cancelSwitch(toPicker = true)
        } else if (target != null && chosen == null) {
            picked = null
            if (gate.switching != null) gate.cancelSwitch()
            said = "That profile was removed on another device."
            if (tv) {
                withFrameNanos { }
                runCatching { first.requestFocus() }
            }
        }
    }
    if (mode == PickerMode.Switch) BackHandler(target == null) { onBack() }

    suspend fun enter(
        p: ProfileInfo,
        pin: String?,
    ): Tried {
        val same = p.id == s.active
        return try {
            when {
                same && locked -> {
                    gate.unlock(pin)
                }

                same -> {
                    gate.markChosen()
                    if (mode == PickerMode.Switch) onBack()
                }

                // The gate shuts here; this screen leaves with the app when it is the pushed picker.
                else -> {
                    if (!gate.switchTo(p.id, pin)) {
                        return gate.switchProblem?.let(::tried)
                            ?: Tried.Problem("That did not work.")
                    }
                }
            }
            Tried.Ok
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            tried(e)
        }
    }

    /** Enters Incognito, with the PIN the core asked for (or none). */
    suspend fun enterIncognito(pin: String?): Tried =
        try {
            if (gate.switchTo(ProfileCopy.INCOGNITO, pin, fromChooser = fromTile)) {
                Tried.Ok
            } else {
                gate.switchProblem?.let(::tried) ?: Tried.Problem("That did not work.")
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            tried(e)
        }

    fun pickIncognito() {
        said = ""
        if (incognito) {
            if (mode == PickerMode.Switch) onBack() else gate.markChosen()
            return
        }
        fromTile = true
        if (s.incognitoPin != null) {
            picked = ProfileCopy.INCOGNITO
            return
        }
        scope.launch {
            val r = enterIncognito(null)
            if (r is Tried.Problem) said = r.text
        }
    }

    fun pick(p: ProfileInfo) {
        if (p.syncing) return
        said = ""
        if (p.id == s.active && !locked) {
            if (mode == PickerMode.Switch) onBack() else gate.markChosen()
            return
        }
        if (p.hasPin) {
            picked = p.id
            return
        }
        scope.launch {
            val r = enter(p, null)
            if (r is Tried.Problem) said = r.text
        }
    }

    Box(
        Modifier
            .fillMaxSize()
            .background(colors.bg)
            .verticalScroll(rememberScrollState())
            .padding(
                start = Marquee.safe.side,
                end = Marquee.safe.side,
                // Under the status bar and the back link's row when the shell draws one, where the page scrolls.
                top = maxOf(if (tv) 64.dp else 48.dp, pageTop()),
                bottom = if (tv) 64.dp else 48.dp,
            ),
        contentAlignment = Alignment.Center,
    ) {
        if (chosen != null && target == ProfileCopy.INCOGNITO) {
            Keypad(
                title = ProfileCopy.incognitoPinTitle(chosen.name),
                onSubmit = ::enterIncognito,
                onCancel = {
                    if (picked != null && picked == asked) {
                        onBack()
                    } else {
                        picked = null
                        if (gate.switching != null) gate.cancelSwitch()
                    }
                },
                lede = ProfileCopy.INCOGNITO_KIDS_REASON.takeIf { ProfileCopy.incognitoKidsReason(s) },
                cancelLabel = if (picked != null && picked == asked) "Cancel" else "Choose another profile",
                initialProblem = gate.switchProblem?.takeIf { gate.switching == target }?.let(::tried),
            )
            return@Box
        }
        if (chosen != null) {
            val problem = gate.switchProblem?.takeIf { gate.switching == chosen.id }
            Keypad(
                title = "Enter the PIN for ${chosen.name}",
                onSubmit = { pin -> enter(chosen, pin) },
                onCancel = {
                    if (picked != null && picked == asked) {
                        onBack()
                    } else {
                        picked = null
                        if (gate.switching != null) gate.cancelSwitch()
                    }
                },
                // Opened by the bar's menu: Cancel goes back where the menu was, not to the tiles.
                cancelLabel = if (picked != null && picked == asked) "Cancel" else "Choose another profile",
                extra =
                    if (chosen.isOwner) {
                        (
                            {
                                ForgotPin(
                                    gate,
                                    session,
                                    chosen,
                                    locked && chosen.id == s.active,
                                )
                            }
                        )
                    } else {
                        null
                    },
                initialProblem = problem?.let(::tried),
            )
            return@Box
        }
        if (gate.switching != null && gate.switchProblem == null) {
            if (gate.switching == ProfileCopy.INCOGNITO) {
                StatusWord(ProfileCopy.INCOGNITO_OPENING, Status.Warn)
            } else {
                val name = s.profiles.firstOrNull { it.id == gate.switching }?.name ?: "the profile"
                StatusWord("Opening $name", Status.Warn)
            }
            return@Box
        }
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(if (tv) 28.dp else 20.dp),
        ) {
            MqText(
                "Who's watching?",
                role = if (tv) TextRole.Title1 else TextRole.Title2,
                weight = FontWeight.ExtraBold,
            )
            // Only a profile with a PIN is locked (plans/batch-10-incognito.md 5.4): a Leave to the chooser from a
            // host without one says nothing of a lock.
            if (locked && me?.hasPin == true) {
                MqText(
                    "This profile is locked. Choose a profile, then enter its PIN.",
                    role = TextRole.Body,
                    color = colors.muted,
                )
            }
            // An Incognito end not read yet: the chooser's polite status, until the next profile choice.
            if (s.incognitoEnded) {
                MqText(
                    ProfileCopy.INCOGNITO_ENDED,
                    Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                    role = TextRole.Body,
                    color = colors.muted,
                )
            }
            FlowRow(
                Modifier.focusRestorer(first).widthIn(max = if (tv) 1400.dp else 360.dp),
                horizontalArrangement = Arrangement.spacedBy(if (tv) 40.dp else 20.dp, Alignment.CenterHorizontally),
                verticalArrangement = Arrangement.spacedBy(if (tv) 32.dp else 20.dp),
            ) {
                s.profiles.forEach { p ->
                    ProfileTile(
                        p = p,
                        active = p.id == s.active,
                        modifier = if (p.id == focusId) Modifier.focusRequester(first) else Modifier,
                        onClick = { pick(p) },
                    )
                }
                // Incognito after the profiles, before Add profile; never a way out of a kids session (5.1).
                if (!(s.kids && !locked)) {
                    QuietTile(
                        "Incognito",
                        Icons.HatGlasses,
                        ::pickIncognito,
                        Modifier.then(
                            if (focusId == ProfileCopy.INCOGNITO) Modifier.focusRequester(first) else Modifier,
                        ),
                        spoken =
                            listOfNotNull(
                                "Incognito",
                                "current profile".takeIf { incognito },
                                "needs a PIN".takeIf { s.incognitoPin != null && !incognito },
                            ).joinToString(", "),
                        glyph = Color.White,
                    )
                }
                if (onAdd != null && owner && !locked && s.profiles.size < s.max) AddTile(onAdd)
            }
            if (said.isNotEmpty()) {
                MqText(
                    said,
                    role = TextRole.Callout,
                    color = colors.bad,
                    modifier =
                        Modifier.semantics {
                            liveRegion =
                                LiveRegionMode.Polite
                        },
                )
            }
            if (onManage != null && owner && !locked) SecondaryButton("Manage profiles", onManage)
        }
        LaunchedEffect(Unit) {
            if (tv) focusSoon(first)
        }
    }
}

@Composable
internal fun ProfileTile(
    p: ProfileInfo,
    active: Boolean,
    modifier: Modifier,
    onClick: () -> Unit,
    /** Shows the focused look without focus (screenshots, tests). */
    forceFocus: Boolean = false,
) {
    val tv = Marquee.form == Form.Tv
    val colors = Marquee.colors
    val size = if (tv) 168.dp else 112.dp
    var hasFocus by remember { mutableStateOf(false) }
    // The ring follows the tile's focus on every form; the white and the name's brightening on TV only.
    val look = hasFocus || forceFocus
    val focused = look && tv
    val spoken =
        listOfNotNull(
            p.name,
            "current profile".takeIf { active },
            if (p.syncing) "still syncing" else "locked with a PIN".takeIf { p.hasPin },
        ).joinToString(", ")
    Column(
        modifier
            .width(if (tv) 200.dp else 140.dp)
            .alpha(if (p.syncing) 0.55f else 1f)
            .onFocusChanged { hasFocus = it.isFocused }
            .semantics(mergeDescendants = true) {
                contentDescription = spoken
                if (active) stateDescription = "Current profile"
            }.focusClickable(onClick, Role.Button, !p.syncing),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(if (tv) 16.dp else 10.dp),
    ) {
        // The focus treatment follows the clickable tile around it (the box itself never takes focus).
        Box(
            Modifier
                .marqueeFocus(size / 2, Tokens.FOCUS_SCALE_ART, forced = look, art = true),
        ) { ProfileAvatar(p, size, lit = focused) }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            MqText(
                p.name,
                role = TextRole.Headline,
                weight = FontWeight.Bold,
                color = if (focused) colors.accent else colors.muted,
                maxLines = 1,
            )
            if (p.hasPin && !p.syncing) MqIcon(Icons.Lock, tint = colors.dim, size = if (tv) 22.dp else 16.dp)
        }
        if (p.syncing) MqText("Syncing", role = TextRole.Caption, color = colors.dim)
    }
}

@Composable
internal fun AddTile(
    onClick: () -> Unit,
    /** Shows the focused look without focus (screenshots, tests). */
    forceFocus: Boolean = false,
) = QuietTile("Add profile", Icons.Plus, onClick, forceFocus = forceFocus)

/** A tile that is not a profile (Add profile, Incognito): a glyph on a quiet circle over its word. */
@Composable
internal fun QuietTile(
    label: String,
    icon: ImageVector,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    spoken: String = label,
    /** The glyph's ink at rest; Incognito's is white (plans/batch-10-incognito.md 5.1). */
    glyph: Color? = null,
    forceFocus: Boolean = false,
) {
    val tv = Marquee.form == Form.Tv
    val colors = Marquee.colors
    val size = if (tv) 168.dp else 112.dp
    var hasFocus by remember { mutableStateOf(false) }
    // The ring follows the tile's focus on every form; the name brightens on TV only.
    val look = hasFocus || forceFocus
    val focused = look && tv
    Column(
        modifier
            .width(if (tv) 200.dp else 140.dp)
            .onFocusChanged { hasFocus = it.isFocused }
            .semantics(mergeDescendants = true) { contentDescription = spoken }
            .focusClickable(onClick, Role.Button),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(if (tv) 16.dp else 10.dp),
    ) {
        Box(
            Modifier
                .marqueeFocus(size / 2, forced = look)
                .size(size)
                .clip(CircleShape)
                // A focused tile on TV turns white with a dark glyph, as a focused profile does.
                .background(if (focused) colors.primary else colors.surface),
            contentAlignment = Alignment.Center,
        ) { MqIcon(icon, tint = if (focused) colors.onPrimary else glyph ?: colors.muted, size = size * 0.4f) }
        MqText(
            label,
            role = TextRole.Headline,
            weight = FontWeight.Bold,
            color = if (focused) colors.accent else colors.muted,
        )
    }
}

/** The owner's way back in: the device's own lock screen, then the core clears the PIN. */
@Composable
private fun ForgotPin(
    gate: ProfileGate,
    session: CoreSession,
    owner: ProfileInfo,
    unlocksSession: Boolean,
) {
    val context = LocalContext.current
    val tv = Marquee.form == Form.Tv
    val scope = rememberCoroutineScope()
    var said by remember { mutableStateOf("") }
    val launcher =
        rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
            if (result.resultCode != Activity.RESULT_OK) return@rememberLauncherForActivityResult
            // The device's lock stands in for the PIN: the main profile then opens without one, as
            // the locked session's own profile or by a switch to it.
            scope.launch {
                said =
                    try {
                        session.ops.resetOwnerPin()
                        if (unlocksSession) {
                            gate.unlock(null)
                            ""
                        } else if (gate.switchTo(owner.id, null)) {
                            ""
                        } else {
                            // The PIN is gone; only the switch was refused (or one was already out).
                            val refused = gate.switchProblem
                            "The PIN was removed. " +
                                ProfileCopy.problem(
                                    refused?.code,
                                    refused?.let(ErrorCopy::words) ?: "Choose the profile again.",
                                )
                        }
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        ErrorCopy.words(e)
                    }
            }
        }
    Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        MqText("Forgot the PIN?", role = TextRole.Callout, color = Marquee.colors.muted)
        val keyguard = context.getSystemService(Context.KEYGUARD_SERVICE) as KeyguardManager
        // A TV seldom has a screen lock and has no way to add one here: it says where to reset instead
        // of offering a button that cannot work. A phone can add one, so it keeps the button.
        if (tv && !keyguard.isDeviceSecure) {
            MqText(
                "This device has no screen lock, so the PIN cannot be reset here. Reset it from Momotus on a phone " +
                    "with a screen lock: Settings, Profiles. This device gets the change at its next sync.",
                modifier = Modifier.widthIn(max = 720.dp),
                role = TextRole.Callout,
                color = Marquee.colors.muted,
                textAlign = TextAlign.Center,
            )
            return@Column
        }
        SecondaryButton("Reset with this device's lock", {
            if (!keyguard.isDeviceSecure) {
                said = "Set a screen lock on this device first, then try again."
            } else {
                @Suppress("DEPRECATION")
                val intent =
                    keyguard.createConfirmDeviceCredentialIntent(
                        "Reset the main profile's PIN",
                        "Unlock this device to remove the PIN.",
                    )
                if (intent !=
                    null
                ) {
                    launcher.launch(intent)
                } else {
                    said = "Set a screen lock on this device first, then try again."
                }
            }
        })
        if (said.isNotEmpty()) MqText(said, role = TextRole.Callout, color = Marquee.colors.bad)
    }
}

/** Manage profiles: every profile, each opening its edit page (the owner only). */
@Composable
fun ManageProfilesScreen(
    gate: ProfileGate,
    onEdit: (String) -> Unit,
    onAdd: () -> Unit,
) {
    val s = gate.snapshot
    val owner = s?.profiles?.firstOrNull { it.id == s.active }?.isOwner == true
    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        item(key = "head") { BrowseHeader("Manage profiles") }
        if (s == null || !owner) {
            item(key = "only") { Note("Only the main profile manages profiles. You can edit your own from Settings.") }
            return@LazyColumn
        }
        items(s.profiles, key = { it.id }) { p ->
            val detail =
                listOfNotNull(
                    "Main profile".takeIf { p.isOwner },
                    if (p.syncing) {
                        "Syncing"
                    } else if (p.hasPin) {
                        "PIN set"
                    } else {
                        null
                    },
                ).joinToString(" · ").ifEmpty { "No PIN" }
            ProfileRow(p, detail) { onEdit(p.id) }
        }
        item(key = "add") {
            if (s.profiles.size < s.max) {
                Section {
                    Row(
                        Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        SecondaryButton("Add profile", onAdd, icon = Icons.Plus)
                        MqText(
                            "A new profile starts with your addons and no PIN.",
                            role = TextRole.Caption,
                            color = Marquee.colors.muted,
                        )
                    }
                }
            } else {
                Note("This account has as many profiles as it can hold (${s.max}).")
            }
        }
    }
}

@Composable
private fun ProfileRow(
    p: ProfileInfo,
    detail: String,
    onClick: () -> Unit,
) {
    var focused by remember { mutableStateOf(false) }
    val shape = RoundedCornerShape(Marquee.radius.l)
    // On TV the focused row turns white with dark words (focusPalette).
    FocusInk(focusPalette(focused)) {
        val colors = Marquee.colors
        Row(
            Modifier
                .padding(horizontal = Marquee.safe.side)
                .fillMaxWidth()
                .marqueeFocus(Marquee.radius.l, Tokens.FOCUS_SCALE_TILE)
                .onFocusChanged { focused = it.isFocused }
                .clip(shape)
                .background(colors.surface)
                .border(1.dp, colors.edge, shape)
                .semantics(mergeDescendants = true) {}
                .focusClickable(onClick, Role.Button)
                .padding(if (Marquee.form == Form.Tv) 24.dp else 16.dp),
            horizontalArrangement = Arrangement.spacedBy(Marquee.space.s),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ProfileAvatar(p, if (Marquee.form == Form.Tv) 56.dp else 40.dp)
            Column(Modifier.weight(1f)) {
                MqText(p.name, role = TextRole.Headline, weight = FontWeight.Bold, maxLines = 1)
                MqText(detail, role = TextRole.Caption, color = colors.muted, maxLines = 1)
            }
            MqIcon(Icons.ChevronRight, tint = colors.muted)
        }
    }
}

/** Which keypad steps a PIN change runs: set (new twice), change (current, new twice), remove (current). */
private enum class PinFlow { Set, Change, Remove }

/**
 * The edit page, and Add when [id] is null. The stored avatar is kept unless the user picks one,
 * so a name edit never rewrites an avatar a newer app chose.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun EditProfileScreen(
    gate: ProfileGate,
    session: CoreSession,
    id: String?,
    onDone: () -> Unit,
) {
    val s = gate.snapshot ?: return
    val isNew = id == null
    val p = s.profiles.firstOrNull { it.id == id }
    val mine = p != null && p.id == s.active
    val owner = s.profiles.firstOrNull { it.id == s.active }?.isOwner == true
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    val colors = Marquee.colors
    val start =
        remember(id) { ProfileCopy.parseAvatar(p?.avatar ?: ProfileCopy.nextAvatar(s.profiles.map { it.avatar })) }
    // The draft survives a rotation or the process dying behind the page; the PIN steps hold no digit of it.
    var name by rememberSaveable(id) { mutableStateOf(p?.name.orEmpty()) }
    var glyph by rememberSaveable(id) { mutableStateOf(start.glyph) }
    // No color to choose (design/DECISION.md, Profiles): a profile keeps the grey step it was given.
    val color = start.color ?: "ember"
    var touched by rememberSaveable(id) { mutableStateOf(false) }
    // Back with a changed name or picture asks first; what is stored now is the baseline, so a save makes it clean.
    val stored = p?.let { ProfileCopy.parseAvatar(it.avatar) } ?: start
    val dirty = name.trim() != p?.name.orEmpty() || glyph != stored.glyph
    var askDiscard by remember { mutableStateOf(false) }
    val keepEditing = remember { FocusRequester() }
    val saveButton = remember { FocusRequester() }
    // Closing the question moves focus to Save first: with the focused Keep editing gone, Android would hand it to the
    // name field and open its keyboard.
    var closingDiscard by remember { mutableStateOf(false) }
    var saving by remember { mutableStateOf(false) }
    // The PIN of someone else, or one this version cannot read, goes after a question: it reaches every device.
    var askPin by remember { mutableStateOf(false) }
    // The PIN question and its trigger swap through a frame, as the delete question does, so focus never drops.
    var pinSwapping by remember { mutableStateOf(false) }
    val askKeep = remember { FocusRequester() }
    val swapPin = { ask: Boolean ->
        askPin = ask
        pinSwapping = true
    }
    var said by remember { mutableStateOf<Pair<String, Status>?>(null) }
    var flow by remember { mutableStateOf<PinFlow?>(null) }
    // The stored PIN came from a newer version: it can only be removed (the core lets its owner do
    // that without it).
    var unreadable by remember(id) { mutableStateOf(false) }
    val saves = viewModel<PinSaves>(key = "pin-saves:$id")
    LaunchedEffect(saves.outcome) {
        saves.outcome?.let {
            said = it
            saves.outcome = null
        }
    }
    val removeUnreadable = remember { FocusRequester() }
    LaunchedEffect(unreadable) {
        if (unreadable) {
            withFrameNanos { }
            runCatching { removeUnreadable.requestFocus() }
        }
    }
    var confirmDelete by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf(false) }
    // The confirmation opens on Keep, never on Delete: a held OK cannot delete a profile, and
    // closing it gives focus back to "Delete profile".
    val keep = remember { FocusRequester() }
    val askDelete = remember { FocusRequester() }
    // A swap shows both for a frame: focus moves to the new button before the focused one leaves,
    // or Android would hand it to the first field (the name, and its keyboard).
    var swapping by remember { mutableStateOf(false) }
    val swapDelete = { ask: Boolean ->
        confirmDelete = ask
        swapping = true
    }
    // A touch screen has no focus to scroll the list: the opened question is brought into view.
    val deleteView = remember { BringIntoViewRequester() }
    LaunchedEffect(confirmDelete, swapping) {
        if (!swapping) return@LaunchedEffect
        withFrameNanos { }
        runCatching { if (confirmDelete) keep.requestFocus() else askDelete.requestFocus() }
        // In the page's scope: clearing the flag restarts this effect, and a touch may interrupt it.
        if (confirmDelete) scope.launch { runCatching { deleteView.bringIntoView() } }
        swapping = false
    }

    LaunchedEffect(askPin, pinSwapping) {
        if (!pinSwapping) return@LaunchedEffect
        withFrameNanos { }
        runCatching { if (askPin) askKeep.requestFocus() else removeUnreadable.requestFocus() }
        pinSwapping = false
    }

    // The page leaves once: a delete from here also removes the profile from the snapshot.
    var left by remember { mutableStateOf(false) }
    val leave = {
        if (!left) {
            left = true
            onDone()
        }
    }
    // Removed on another device while open.
    LaunchedEffect(p == null) { if (!isNew && p == null) leave() }
    if (!isNew && p == null) return
    if (if (isNew) !owner else !(mine || owner)) {
        Box(Modifier.fillMaxSize().padding(pagePadding(framed = false))) {
            Note("Only the main profile can change other profiles.")
        }
        return
    }
    flow?.let { f ->
        // Scrolls on a short landscape phone, where the keypad is taller than the screen.
        BoxWithConstraints(
            Modifier
                .fillMaxSize()
                .background(colors.bg)
                // Under the status bar and the back link's row when the shell draws one.
                .padding(
                    start = Marquee.safe.side,
                    end = Marquee.safe.side,
                    top = maxOf(Marquee.safe.side, pageTop()),
                    bottom = Marquee.safe.side,
                ),
        ) {
            Column(
                Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).heightIn(min = maxHeight),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                PinSteps(session, p!!, f, saves, onUnreadable = {
                    flow = null
                    unreadable = true
                }) { text ->
                    flow = null
                    if (text != null) said = text to Status.Ok
                }
            }
        }
        return
    }

    fun save() {
        val n = name.trim()
        if (n.isEmpty() || n.length > 30) {
            said = ProfileCopy.problem("badName", "") to Status.Bad
            return
        }
        // One save at a time: a second tap while the core has the first would add the profile twice.
        if (saving) return
        saving = true
        scope.launch {
            said =
                try {
                    if (isNew) {
                        session.ops.createProfile(n, ProfileCopy.formatAvatar(glyph, color))
                        leave()
                        null
                    } else {
                        session.ops.updateProfile(
                            p!!.id,
                            n,
                            if (touched) ProfileCopy.formatAvatar(glyph, color) else null,
                        )
                        "Saved" to Status.Ok
                    }
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    ((tried(e) as? Tried.Problem)?.text ?: "That did not work.") to Status.Bad
                } finally {
                    saving = false
                }
        }
    }

    val list = rememberLazyListState()
    BackHandler(dirty && !left) { if (askDiscard) closingDiscard = true else askDiscard = true }
    LaunchedEffect(askDiscard) {
        if (!askDiscard) return@LaunchedEffect
        list.animateScrollToItem(0)
        withFrameNanos { }
        runCatching { keepEditing.requestFocus() }
    }
    LaunchedEffect(closingDiscard) {
        if (!closingDiscard) return@LaunchedEffect
        withFrameNanos { }
        runCatching { saveButton.requestFocus() }
        askDiscard = false
        closingDiscard = false
    }
    // While a question is open its safe button is the screen's one white: Save draws as a secondary.
    val asking = askDiscard || askPin || confirmDelete
    // On TV, a keyboard that closes leaves focus in the name field, and the first remote Back then goes to the field
    // and not to the page's guard. While the field still holds focus as the keyboard goes (and no question is open),
    // focus moves to the first picture, the next stop after the name on this page (name, picture, Save).
    val keyboard = WindowInsets.isImeVisible
    var keyboardWasUp by remember { mutableStateOf(false) }
    var nameFocused by remember { mutableStateOf(false) }
    val firstPicture = remember { FocusRequester() }
    val onTv = Marquee.form == Form.Tv
    LaunchedEffect(keyboard) {
        if (keyboard) {
            keyboardWasUp = true
        } else if (keyboardWasUp) {
            keyboardWasUp = false
            if (onTv && nameFocused && !asking) runCatching { firstPicture.requestFocus() }
        }
    }

    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        state = list,
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") {
            BrowseHeader(
                if (isNew) {
                    "Add profile"
                } else if (mine) {
                    "Your profile"
                } else {
                    "Edit ${p!!.name}"
                },
            )
        }
        if (askDiscard) {
            item(key = "discard") {
                Column(
                    Modifier.padding(horizontal = Marquee.safe.side),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
                ) {
                    MqText(if (isNew) "Discard this profile?" else "Discard your changes?", role = TextRole.Body)
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                        // Keep is the primary, the destructive word the plain secondary (design/DECISION.md).
                        PrimaryButton("Keep editing", { closingDiscard = true }, focusRequester = keepEditing)
                        SecondaryButton("Discard", { leave() })
                    }
                }
            }
        }
        item(key = "form") {
            Column(
                Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 1100.dp),
                verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
            ) {
                AvatarArt(
                    glyph,
                    color,
                    ProfileCopy.initial(name.ifBlank { "?" }),
                    if (Marquee.form ==
                        Form.Tv
                    ) {
                        168.dp
                    } else {
                        112.dp
                    },
                )
                MqTextField(
                    name,
                    { name = it.take(30) },
                    label = "Name",
                    placeholder = "Name",
                    modifier = Modifier.onFocusChanged { nameFocused = it.hasFocus },
                )
                MqText("PICTURE", role = TextRole.Caption, color = colors.dim, weight = FontWeight.Bold)
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    ProfileCopy.GLYPHS.forEachIndexed { index, g ->
                        val pick = if (Marquee.form == Form.Tv) 64.dp else 48.dp
                        Choice(
                            selected = glyph == g,
                            label = ProfileCopy.glyphName(g),
                            size = pick,
                            onClick = {
                                glyph = g
                                touched = true
                            },
                            focusRequester = firstPicture.takeIf { index == 0 },
                        ) { lit -> AvatarArt(g, color, ProfileCopy.initial(name.ifBlank { "?" }), pick, lit = lit) }
                    }
                }
                PrimaryButton(
                    if (isNew) "Add profile" else "Save",
                    ::save,
                    icon = Icons.Check,
                    enabled = !saving,
                    focusRequester = saveButton,
                    quiet = asking,
                )
                said?.let { (text, tone) -> StatusWord(text, tone) }
            }
        }
        if (!isNew && p != null) {
            item(key = "pin") {
                Column(
                    Modifier.padding(horizontal = Marquee.safe.side),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
                ) {
                    Kicker("PIN", side = 0.dp)
                    when {
                        p.syncing -> {
                            Note("Waiting for this profile's lock to arrive from your other devices.", side = 0.dp)
                        }

                        mine && unreadable -> {
                            Note(ProfileCopy.problem("pinUnreadable", ""), side = 0.dp)
                            if (!askPin || pinSwapping) {
                                SecondaryButton("Remove PIN", { swapPin(true) }, focusRequester = removeUnreadable)
                            }
                            if (askPin || pinSwapping) {
                                RemovePinAsk(
                                    "Remove this PIN? The profile opens for anyone until a new one is set.",
                                    askKeep,
                                    onRemove = {
                                        swapPin(false)
                                        scope.launch {
                                            said =
                                                try {
                                                    session.ops.setPin(p.id, null)
                                                    unreadable = false
                                                    "PIN removed" to Status.Ok
                                                } catch (e: CancellationException) {
                                                    throw e
                                                } catch (e: Exception) {
                                                    ((tried(e) as? Tried.Problem)?.text ?: "That did not work.") to
                                                        Status.Bad
                                                }
                                        }
                                    },
                                    onKeep = { swapPin(false) },
                                )
                            }
                        }

                        mine && !p.hasPin -> {
                            SecondaryButton("Set a PIN", { flow = PinFlow.Set }, icon = Icons.Lock)
                        }

                        mine -> {
                            FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                                SecondaryButton("Change PIN", { flow = PinFlow.Change }, icon = Icons.Lock)
                                SecondaryButton("Remove PIN", { flow = PinFlow.Remove })
                            }
                        }

                        owner && p.hasPin -> {
                            if (!askPin || pinSwapping) {
                                SecondaryButton(
                                    "Remove ${p.name}'s PIN",
                                    { swapPin(true) },
                                    focusRequester = removeUnreadable,
                                )
                            }
                            if (askPin || pinSwapping) {
                                RemovePinAsk(
                                    "Remove ${p.name}'s PIN? Only ${p.name} can set a new one, from their own profile.",
                                    askKeep,
                                    onRemove = {
                                        swapPin(false)
                                        scope.launch {
                                            said =
                                                try {
                                                    session.ops.setPin(p.id, null)
                                                    "${p.name}'s PIN was removed." to Status.Ok
                                                } catch (e: CancellationException) {
                                                    throw e
                                                } catch (e: Exception) {
                                                    ((tried(e) as? Tried.Problem)?.text ?: "That did not work.") to
                                                        Status.Bad
                                                }
                                        }
                                    },
                                    onKeep = { swapPin(false) },
                                )
                            }
                        }

                        owner -> {
                            Note("${p.name} has no PIN. They can set one from their own profile.", side = 0.dp)
                        }
                    }
                }
            }
        }
        if (!isNew && owner && p != null && !p.isOwner) {
            item(key = "delete") {
                Column(
                    Modifier
                        .bringIntoViewRequester(deleteView)
                        .padding(horizontal = Marquee.safe.side)
                        .widthIn(max = 900.dp),
                    verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
                ) {
                    if (!confirmDelete || swapping) {
                        SecondaryButton(
                            "Delete profile",
                            { swapDelete(true) },
                            icon = Icons.Trash,
                            focusRequester = askDelete,
                        )
                    }
                    if (confirmDelete || swapping) {
                        MqText(
                            "Delete ${p.name}? Their library, progress and settings are removed from this device now " +
                                "and from your other devices when they next sync.",
                            role = TextRole.Body,
                        )
                        FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
                            // One delete at a time: while it is with the core, neither Delete nor Keep acts.
                            SecondaryButton("Delete ${p.name}", {
                                if (!deleting) {
                                    deleting = true
                                    scope.launch {
                                        try {
                                            session.ops.deleteProfile(p.id)
                                            // Its Trakt sign-in leaves the keystore (the core's gone push does it too).
                                            (context.applicationContext as MarqueeApp).trakt.forget(p.id)
                                            leave()
                                        } catch (e: CancellationException) {
                                            throw e
                                        } catch (e: Exception) {
                                            said =
                                                ((tried(e) as? Tried.Problem)?.text ?: "That did not work.") to
                                                Status.Bad
                                            swapDelete(false)
                                        } finally {
                                            deleting = false
                                        }
                                    }
                                }
                            }, icon = Icons.Trash, enabled = !deleting)
                            PrimaryButton("Keep", { swapDelete(false) }, focusRequester = keep, enabled = !deleting)
                        }
                    }
                }
            }
        }
    }
}

/**
 * The question before a PIN goes without being typed: Keep is the primary and gets the focus (the page asks for it
 * through [keep]), the destructive word the plain secondary (design/DECISION.md, Destructive).
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun RemovePinAsk(
    question: String,
    keep: FocusRequester,
    onRemove: () -> Unit,
    onKeep: () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(Marquee.space.s)) {
        MqText(question, role = TextRole.Body)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(Marquee.space.xs)) {
            PrimaryButton("Keep", onKeep, focusRequester = keep)
            SecondaryButton("Remove PIN", onRemove)
        }
    }
}

private val CHOICE_PAD = 6.dp

/**
 * A picture choice: selected is a grey disc behind the picture; spoken by its name with its state. Focused on TV,
 * [content] is told to draw lit (white with a dark glyph), as a focused profile does.
 */
@Composable
internal fun Choice(
    selected: Boolean,
    label: String,
    size: Dp,
    onClick: () -> Unit,
    focusRequester: FocusRequester? = null,
    /** Shows the focused look without focus (screenshots, tests). */
    forceFocus: Boolean = false,
    content: @Composable (lit: Boolean) -> Unit,
) {
    var hasFocus by remember { mutableStateOf(false) }
    val look = hasFocus || forceFocus
    Box(
        Modifier
            .then(if (focusRequester != null) Modifier.focusRequester(focusRequester) else Modifier)
            .onFocusChanged { hasFocus = it.isFocused }
            .marqueeFocus((size + CHOICE_PAD * 2) / 2, forced = look)
            .clip(CircleShape)
            .background(if (selected) Tokens.selectedFill else Color.Transparent)
            .padding(CHOICE_PAD)
            .semantics(mergeDescendants = true) {
                contentDescription = label
                this.selected = selected
            }.focusClickable(onClick, Role.RadioButton),
    ) { content(look && Marquee.form == Form.Tv) }
}

/**
 * The edit page's PIN saves. They run in this view model, so a save outlives its PIN flow and a
 * rotation; one that ends after its flow left is told here, and the page shows it.
 */
class PinSaves : ViewModel() {
    var outcome: Pair<String, Status>? by mutableStateOf(null)

    fun start(block: suspend () -> Unit): Deferred<Unit> = viewModelScope.async { block() }

    fun tell(
        cause: Throwable?,
        done: String,
    ) {
        outcome =
            when (cause) {
                null -> {
                    done to Status.Ok
                }

                is CancellationException -> {
                    null
                }

                else -> {
                    ((tried(cause as? Exception ?: return) as? Tried.Problem)?.text ?: "The PIN was not changed.") to
                        Status.Bad
                }
            }
    }
}

private class OpenFlow {
    @Volatile var alive = true

    @Volatile var save: Pair<Deferred<Unit>, AtomicBoolean>? = null
}

/** The keypad steps for your own PIN, by the core's rules: change and remove ask the current one first. */
@Composable
private fun PinSteps(
    session: CoreSession,
    p: ProfileInfo,
    flow: PinFlow,
    saves: PinSaves,
    onUnreadable: () -> Unit,
    onDone: (String?) -> Unit,
) {
    // Whether this flow is still on screen, and its save still in flight: a save that ends after
    // the flow left is told to the page; one that ended just before is told when the flow leaves.
    val open = remember { OpenFlow() }
    DisposableEffect(Unit) {
        onDispose {
            open.alive = false
            open.save?.let { (save, told) ->
                if (save.isCompleted && told.compareAndSet(false, true)) {
                    saves.tell(
                        save.getCompletionExceptionOrNull(),
                        if (flow ==
                            PinFlow.Remove
                        ) {
                            "PIN removed"
                        } else {
                            "PIN saved"
                        },
                    )
                }
            }
        }
    }
    val steps =
        when (flow) {
            PinFlow.Set -> listOf("new", "again")
            PinFlow.Change -> listOf("current", "new", "again")
            PinFlow.Remove -> listOf("current")
        }
    var step by remember { mutableStateOf(0) }
    var current by remember { mutableStateOf<String?>(null) }
    var fresh by remember { mutableStateOf("") }
    DisposableEffect(Unit) {
        onDispose {
            current = null
            fresh = ""
        }
    }
    // Leaving the app half way through gives the PIN change up: no PIN stays in memory behind it.
    // Closed at once, not at disposal: a save that ends in between is told to the page.
    LifecycleEventEffect(Lifecycle.Event.ON_STOP) {
        open.alive = false
        onDone(null)
    }
    val title =
        when (steps[step]) {
            "current" -> "Enter your current PIN"
            "new" -> if (flow == PinFlow.Set) "Choose a PIN" else "Enter the new PIN"
            else -> "Enter it again"
        }
    Keypad(
        title = title,
        cancelLabel = "Cancel",
        onCancel = { onDone(null) },
        onSubmit = { pin ->
            val kind = steps[step]
            when {
                kind == "current" && flow == PinFlow.Change -> {
                    current = pin
                    step++
                    Tried.Ok
                }

                kind == "new" -> {
                    fresh = pin
                    step++
                    Tried.Ok
                }

                kind == "again" && pin != fresh -> {
                    fresh = ""
                    step = steps.indexOf("new")
                    Tried.Problem("The PINs did not match. Choose one again.")
                }

                else -> {
                    val done = if (flow == PinFlow.Remove) "PIN removed" else "PIN saved"
                    val (newPin, currentPin) = fresh to current
                    // The save belongs to the page's saves, not to this flow: when the flow leaves
                    // first (the app went to the back, a rotation), the page says how it ended,
                    // success or not, and a newer flow is never closed by it.
                    val save =
                        saves.start {
                            if (flow == PinFlow.Remove) {
                                session.ops.setPin(p.id, null, pin)
                            } else {
                                session.ops.setPin(p.id, newPin, currentPin)
                            }
                        }
                    val told = AtomicBoolean(false)
                    open.save = save to told
                    save.invokeOnCompletion { cause ->
                        if (!open.alive &&
                            told.compareAndSet(false, true)
                        ) {
                            saves.tell(cause, done)
                        }
                    }
                    try {
                        save.await()
                        // A flow already closed (the app went to the back) tells the page instead.
                        if (told.compareAndSet(false, true)) {
                            if (open.alive) onDone(done) else saves.tell(null, done)
                        }
                        Tried.Ok
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        if (!told.compareAndSet(false, true)) return@Keypad Tried.Ok
                        if (!open.alive) {
                            saves.tell(e, done)
                            return@Keypad Tried.Ok
                        }
                        if ((e as? CoreError)?.code == "pinUnreadable") {
                            onUnreadable()
                            return@Keypad Tried.Ok
                        }
                        val t = tried(e)
                        if (t == Tried.Wrong && flow == PinFlow.Change) step = 0
                        if ((e as? CoreError)?.code == "weakPin") step = steps.indexOf("new")
                        t
                    }
                }
            }
        },
    )
}

/**
 * Whether the intro says the debrid line ("Your debrid account keeps its own history."): with an account, yes; with none,
 * no; and when the device's debrid view cannot answer, yes, since a missing disclosure is the worse mistake. Null while the
 * view has not answered: the intro waits rather than drop the line by timing.
 */
internal fun introDebrid(
    accounts: Int?,
    failed: Boolean,
): Boolean? =
    when {
        accounts != null -> accounts > 0
        failed -> true
        else -> null
    }

/**
 * The intro on every Incognito entry (plans/batch-10-incognito.md 5.1, 6): the mark, what is not kept, what is still
 * kept or seen, how it ends; Continue takes the TV's focus, and Leave Incognito or Back leaves.
 */
@Composable
internal fun IncognitoIntro(
    debrid: Boolean,
    onContinue: () -> Unit,
    onLeave: () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val colors = Marquee.colors
    val intro = ProfileCopy.intro(debrid, tv)
    val go = remember { FocusRequester() }
    BackHandler(onBack = onLeave)
    LaunchedEffect(Unit) { if (tv) focusSoon(go) }
    Box(
        Modifier
            .fillMaxSize()
            .background(colors.bg)
            .verticalScroll(rememberScrollState())
            .padding(
                start = Marquee.safe.side,
                end = Marquee.safe.side,
                top = maxOf(if (tv) 64.dp else 48.dp, pageTop()),
                bottom = if (tv) 64.dp else 48.dp,
            ),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            Modifier.widthIn(max = if (tv) 960.dp else 560.dp),
            verticalArrangement = Arrangement.spacedBy(if (tv) 28.dp else 20.dp),
        ) {
            ProfileAvatar(ProfileCopy.incognito, if (tv) 128.dp else 96.dp)
            MqText(
                "You're Incognito",
                Modifier.semantics { heading() },
                role = if (tv) TextRole.Title1 else TextRole.Title2,
                weight = FontWeight.ExtraBold,
            )
            IntroGroup("Not kept", intro.notKept)
            IntroGroup("Still kept or seen", intro.seen)
            MqText(intro.ends, role = TextRole.Body, color = colors.muted)
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(if (tv) 20.dp else 12.dp),
                verticalArrangement = Arrangement.spacedBy(if (tv) 20.dp else 12.dp),
            ) {
                PrimaryButton("Continue", onContinue, focusRequester = go)
                SecondaryButton(ProfileCopy.INCOGNITO_LEAVE, onLeave)
            }
        }
    }
}

@Composable
private fun IntroGroup(
    title: String,
    lines: List<String>,
) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        MqText(title, Modifier.semantics { heading() }, role = TextRole.Headline, weight = FontWeight.Bold)
        lines.forEach { MqText(it, role = TextRole.Body, color = Marquee.colors.muted) }
    }
}

/**
 * Over Home of a single profile, where no chooser shows: the Incognito ended line as a polite notice with OK, which
 * marks it read ([onOk], `ackIncognitoEnded`). It stays until then (plans/batch-10-incognito.md 5.4).
 */
@Composable
internal fun IncognitoEndedCard(onOk: () -> Unit) {
    Column(
        Modifier.padding(horizontal = Marquee.safe.side).widthIn(max = 900.dp),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.s),
    ) {
        Box(Modifier.semantics { liveRegion = LiveRegionMode.Polite }) {
            Callout(Icons.Info, ProfileCopy.INCOGNITO_ENDED)
        }
        PrimaryButton("OK", onOk)
    }
}
