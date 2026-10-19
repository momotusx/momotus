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
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
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
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import app.marquee.core.ProfileCopy
import app.marquee.core.client.CoreError
import app.marquee.core.model.ProfileInfo
import app.marquee.core.session.CoreSession
import app.marquee.design.AvatarArt
import app.marquee.design.Form
import app.marquee.design.Icons
import app.marquee.design.Marquee
import app.marquee.design.MqIcon
import app.marquee.design.MqText
import app.marquee.design.MqTextField
import app.marquee.design.PrimaryButton
import app.marquee.design.SecondaryButton
import app.marquee.design.Status
import app.marquee.design.StatusWord
import app.marquee.design.TextRole
import app.marquee.design.avatarColor
import app.marquee.design.focusClickable
import app.marquee.design.marqueeFocus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
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
        else -> Tried.Problem(ProfileCopy.problem(c?.code, e.message ?: "That did not work."))
    }
}

/** The avatar of a profile as stored, at [size]. */
@Composable
internal fun ProfileAvatar(
    p: ProfileInfo,
    size: androidx.compose.ui.unit.Dp,
) {
    val a = ProfileCopy.parseAvatar(p.avatar)
    AvatarArt(a.glyph, a.color, ProfileCopy.initial(p.name), size)
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
        withFrameNanos { }
        runCatching { if (tv) five.requestFocus() else surface.requestFocus() }
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
    var focused by remember { mutableStateOf(false) }
    Box(
        modifier
            .size(size)
            .alpha(if (enabled) 1f else 0.4f)
            .marqueeFocus(size / 2)
            .onFocusChanged { focused = it.isFocused }
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

/** The active profile's avatar at the TV pill's end: it opens the picker. */
@Composable
fun PillProfile(
    gate: ProfileGate,
    onClick: () -> Unit,
) {
    val s = gate.snapshot ?: return
    val me = s.profiles.firstOrNull { it.id == s.active } ?: return
    val colors = Marquee.colors
    var focused by remember { mutableStateOf(false) }
    Box(
        Modifier
            .padding(start = 8.dp)
            .marqueeFocus(14.dp)
            .onFocusChanged { focused = it.isFocused }
            .border(
                if (focused) 3.dp else 0.dp,
                if (focused) colors.text else Color.Transparent,
                RoundedCornerShape(14.dp),
            ).padding(3.dp)
            .semantics(mergeDescendants = true) { contentDescription = "Profile: ${me.name}, switch profile" }
            .focusClickable(onClick, Role.Button),
    ) { ProfileAvatar(me, 40.dp) }
}

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
    var picked by rememberSaveable { mutableStateOf<String?>(null) }
    val target = picked ?: gate.switching.takeIf { gate.switchProblem != null }
    var said by remember { mutableStateOf("") }
    val first = remember { FocusRequester() }
    val chosen = s.profiles.firstOrNull { it.id == target }
    // Focus starts on the current profile, else on the first tile (the current one was removed).
    val focusId = s.profiles.firstOrNull { it.id == s.active }?.id ?: s.profiles.firstOrNull()?.id
    LaunchedEffect(target, s.profiles) {
        if (target != null && chosen == null) {
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
            .padding(horizontal = Marquee.safe.side, vertical = if (tv) 64.dp else 48.dp),
        contentAlignment = Alignment.Center,
    ) {
        if (chosen != null) {
            val problem = gate.switchProblem?.takeIf { gate.switching == chosen.id }
            Keypad(
                title = "Enter the PIN for ${chosen.name}",
                onSubmit = { pin -> enter(chosen, pin) },
                onCancel = {
                    picked = null
                    if (gate.switching != null) gate.cancelSwitch()
                },
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
            val name = s.profiles.firstOrNull { it.id == gate.switching }?.name ?: "the profile"
            StatusWord("Opening $name", Status.Warn)
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
            if (locked) {
                MqText(
                    "This profile is locked. Choose a profile, then enter its PIN.",
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
            if (tv) {
                withFrameNanos { }
                runCatching { first.requestFocus() }
            }
        }
    }
}

@Composable
private fun ProfileTile(
    p: ProfileInfo,
    active: Boolean,
    modifier: Modifier,
    onClick: () -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val colors = Marquee.colors
    val size = if (tv) 168.dp else 112.dp
    var focused by remember { mutableStateOf(false) }
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
            .onFocusChanged { focused = it.isFocused }
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
                .marqueeFocus(size * 0.2f, forced = focused)
                .border(
                    if (focused) 4.dp else 0.dp,
                    if (focused) colors.text else Color.Transparent,
                    RoundedCornerShape(size * 0.2f),
                ),
        ) { ProfileAvatar(p, size) }
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
private fun AddTile(onClick: () -> Unit) {
    val tv = Marquee.form == Form.Tv
    val colors = Marquee.colors
    val size = if (tv) 168.dp else 112.dp
    var focused by remember { mutableStateOf(false) }
    Column(
        Modifier
            .width(if (tv) 200.dp else 140.dp)
            .onFocusChanged { focused = it.isFocused }
            .semantics(mergeDescendants = true) { contentDescription = "Add profile" }
            .focusClickable(onClick, Role.Button),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(if (tv) 16.dp else 10.dp),
    ) {
        val shape = RoundedCornerShape(size * 0.2f)
        Box(
            Modifier
                .marqueeFocus(size * 0.2f, forced = focused)
                .size(size)
                .clip(shape)
                .background(colors.raised)
                .border(if (focused) 4.dp else 2.dp, if (focused) colors.text else colors.control, shape),
            contentAlignment = Alignment.Center,
        ) { MqIcon(Icons.Plus, tint = colors.muted, size = size * 0.4f) }
        MqText(
            "Add profile",
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
                                ProfileCopy.problem(refused?.code, refused?.message ?: "Choose the profile again.")
                        }
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        e.message ?: "The PIN could not be reset."
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
                "This device has no screen lock, so the PIN cannot be reset here. Reset it from Marquee on a phone " +
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
        item(key = "head") { BrowseHeader("Settings", "Manage profiles") }
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
    val shape = RoundedCornerShape(Marquee.radius.l)
    val colors = Marquee.colors
    Row(
        Modifier
            .padding(horizontal = Marquee.safe.side)
            .fillMaxWidth()
            .marqueeFocus(Marquee.radius.l, scale = 1.01f)
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
    val trakt = (LocalContext.current.applicationContext as MarqueeApp).trakt
    val colors = Marquee.colors
    val start =
        remember(id) { ProfileCopy.parseAvatar(p?.avatar ?: ProfileCopy.nextAvatar(s.profiles.map { it.avatar })) }
    var name by remember(id) { mutableStateOf(p?.name.orEmpty()) }
    var glyph by remember(id) { mutableStateOf(start.glyph) }
    var color by remember(id) { mutableStateOf(start.color ?: "ember") }
    var touched by remember(id) { mutableStateOf(false) }
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
        BoxWithConstraints(Modifier.fillMaxSize().background(colors.bg).padding(Marquee.safe.side)) {
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
                }
        }
    }

    LazyColumn(
        Modifier.fillMaxSize().focusRestorer(),
        contentPadding = pagePadding(framed = false),
        verticalArrangement = Arrangement.spacedBy(Marquee.space.m),
    ) {
        item(key = "head") {
            BrowseHeader(
                "Profiles",
                if (isNew) {
                    "Add profile"
                } else if (mine) {
                    "Your profile"
                } else {
                    "Edit ${p!!.name}"
                },
            )
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
                MqTextField(name, { name = it.take(30) }, label = "Name", placeholder = "Name")
                MqText("PICTURE", role = TextRole.Caption, color = colors.dim, weight = FontWeight.Bold)
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    ProfileCopy.GLYPHS.forEach { g ->
                        Choice(selected = glyph == g, label = ProfileCopy.glyphName(g), onClick = {
                            glyph = g
                            touched = true
                        }) {
                            AvatarArt(
                                g,
                                color,
                                ProfileCopy.initial(name.ifBlank { "?" }),
                                if (Marquee.form ==
                                    Form.Tv
                                ) {
                                    64.dp
                                } else {
                                    48.dp
                                },
                            )
                        }
                    }
                }
                MqText("COLOR", role = TextRole.Caption, color = colors.dim, weight = FontWeight.Bold)
                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    ProfileCopy.COLORS.forEach { c ->
                        Choice(selected = color == c, label = c.replaceFirstChar { it.uppercase() }, onClick = {
                            color = c
                            touched = true
                        }) {
                            Box(
                                Modifier
                                    .size(if (Marquee.form == Form.Tv) 64.dp else 48.dp)
                                    .clip(CircleShape)
                                    .background(avatarColor(c) ?: colors.raised),
                                contentAlignment = Alignment.Center,
                            ) { if (color == c) MqIcon(Icons.Check, tint = Color.White) }
                        }
                    }
                }
                PrimaryButton(if (isNew) "Add profile" else "Save", ::save, icon = Icons.Check)
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
                            SecondaryButton("Remove PIN", focusRequester = removeUnreadable, onClick = {
                                scope.launch {
                                    said =
                                        try {
                                            session.ops.setPin(p.id, null)
                                            unreadable = false
                                            "PIN removed" to Status.Ok
                                        } catch (e: CancellationException) {
                                            throw e
                                        } catch (e: Exception) {
                                            ((tried(e) as? Tried.Problem)?.text ?: "That did not work.") to Status.Bad
                                        }
                                }
                            })
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
                            SecondaryButton("Remove ${p.name}'s PIN", {
                                scope.launch {
                                    said =
                                        try {
                                            session.ops.setPin(p.id, null)
                                            "${p.name}'s PIN was removed." to Status.Ok
                                        } catch (e: CancellationException) {
                                            throw e
                                        } catch (e: Exception) {
                                            ((tried(e) as? Tried.Problem)?.text ?: "That did not work.") to Status.Bad
                                        }
                                }
                            })
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
                            PrimaryButton("Delete ${p.name}", {
                                if (!deleting) {
                                    deleting = true
                                    scope.launch {
                                        try {
                                            session.ops.deleteProfile(p.id)
                                            // Its Trakt sign-in leaves the keystore (the core's gone push does it too).
                                            trakt.forget(p.id)
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
                            SecondaryButton("Keep", { swapDelete(false) }, focusRequester = keep, enabled = !deleting)
                        }
                    }
                }
            }
        }
    }
}

/** A picture or color choice: selected is the white ring; spoken by its name with its state. */
@Composable
private fun Choice(
    selected: Boolean,
    label: String,
    onClick: () -> Unit,
    content: @Composable () -> Unit,
) {
    val colors = Marquee.colors
    val shape = RoundedCornerShape(16.dp)
    Box(
        Modifier
            .marqueeFocus(16.dp)
            .clip(shape)
            .border(3.dp, if (selected) colors.text else Color.Transparent, shape)
            .padding(4.dp)
            .semantics(mergeDescendants = true) {
                contentDescription = label
                this.selected = selected
            }.focusClickable(onClick, Role.RadioButton),
    ) { content() }
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
