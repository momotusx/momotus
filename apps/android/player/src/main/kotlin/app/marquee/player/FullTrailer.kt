package app.marquee.player

import androidx.compose.foundation.background
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.Key
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import app.marquee.core.model.Trailer
import app.marquee.design.Form
import app.marquee.design.HeroTrailers
import app.marquee.design.Icons
import app.marquee.design.LOST
import app.marquee.design.LiveSpoken
import app.marquee.design.Marquee
import app.marquee.design.MqText
import app.marquee.design.SecondaryButton
import app.marquee.design.Spinner
import app.marquee.design.TextRole
import app.marquee.design.TrailerHost
import app.marquee.design.TrailerState
import app.marquee.design.TrailerSurface
import app.marquee.design.marqueeFocus
import app.marquee.design.trailerSeekTarget
import kotlinx.coroutines.delay

/**
 * A title's trailer full screen (plans/trailer-inline.md 7), in a window of its own so the remote's focus cannot leave
 * it: YouTube's own player (the title page's [host]), taking no input, under our controls, so Back closes it and nothing
 * leaves for YouTube. Its sound is its own, never the session's. A trailer YouTube will not play here gives way to the
 * next one, then to a quiet line; its end, or the app going to the background, closes it.
 */
@Composable
fun TrailerDialog(
    trailers: List<Trailer>,
    host: TrailerHost,
    onClose: () -> Unit,
) {
    val state = remember { TrailerState() }
    var index by remember { mutableIntStateOf(0) }
    var failed by remember { mutableStateOf(false) }
    // On at open (the viewer asked for it), except in a walk build, which plays every trailer silent.
    var loud by remember { mutableStateOf(!HeroTrailers.walk) }
    val close by rememberUpdatedState(onClose)
    LaunchedEffect(index) {
        val trailer = trailers.getOrNull(index) ?: return@LaunchedEffect
        failed = false
        host.play(trailer.youtubeId, state, cap = 0, full = true, loud = loud) { why ->
            // Refused (embedding off, gone): the next trailer, then the line. Never ready (YouTube unreachable): the line
            // at once, not another 15 s per trailer.
            if (why != "error" && why != "timeout" && why != LOST) {
                close()
            } else if (why == "error" && index + 1 < trailers.size) {
                index += 1
            } else {
                failed = true
            }
        }
    }
    // Closed: the player and its renderer go too (the header does not play again this visit).
    DisposableEffect(host) { onDispose { host.destroy() } }
    val density = LocalDensity.current
    Dialog(
        onDismissRequest = onClose,
        properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
    ) {
        CompositionLocalProvider(LocalDensity provides density) {
            FullTrailer(
                title = trailers.getOrNull(index)?.title.orEmpty(),
                state = state,
                failed = failed,
                loud = loud,
                onToggle = host::toggle,
                onSeek = { to ->
                    state.seconds = to
                    host.seek(to)
                },
                onLoud = {
                    loud = !loud
                    host.loud(loud)
                },
                onClose = onClose,
            ) { if (state.loading) TrailerSurface(host, Modifier.matchParentSize()) }
        }
    }
}

/**
 * The full-screen trailer's page: the picture at 16:9, the position bar under it (elapsed, the film player's
 * [Scrubber], remaining), then the trailer's name with Pause or Play, Sound and Close. On a TV the bar has the focus:
 * Left and Right preview steps that grow while held ([seekStepMs]) and seek once let go, select pauses and plays; on a
 * phone a tap or a drag on the bar seeks. The remote's play, rewind and fast-forward keys work from anywhere.
 */
@Composable
fun FullTrailer(
    title: String,
    state: TrailerState,
    failed: Boolean,
    loud: Boolean,
    onToggle: () -> Unit,
    onSeek: (Float) -> Unit,
    onLoud: () -> Unit,
    onClose: () -> Unit,
    picture: @Composable BoxScope.() -> Unit,
) {
    val tv = Marquee.form == Form.Tv
    val bar = remember { FocusRequester() }
    val toggle = remember { FocusRequester() }
    val closer = remember { FocusRequester() }
    // Where a held key or a drag will land, drawn on the bar until it is let go.
    var target by remember { mutableStateOf<Float?>(null) }
    var barFocused by remember { mutableStateOf(false) }
    val seekBy = { by: Float ->
        if (state.length > 0f) onSeek(trailerSeekTarget(target ?: state.seconds, state.length, by))
    }
    // The bar on a TV, Pause elsewhere; Close once nothing can play.
    LaunchedEffect(failed) {
        withFrameNanos { }
        withFrameNanos { }
        runCatching {
            when {
                failed -> closer
                tv -> bar
                else -> toggle
            }.requestFocus()
        }
    }
    // The wait shows only past 300 ms, so a quick start never flashes it.
    var waiting by remember { mutableStateOf(false) }
    LaunchedEffect(state.buffering, failed) {
        waiting = false
        if (state.buffering && !failed) {
            delay(300)
            waiting = true
        }
    }
    Column(
        Modifier
            .fillMaxSize()
            .background(Marquee.colors.bg)
            .testTag(FULL_TRAILER_TAG)
            .onPreviewKeyEvent { mediaKey(it, state.paused, onToggle, seekBy) }
            .windowInsetsPadding(WindowInsets.safeDrawing)
            .padding(horizontal = if (tv) 96.dp else 16.dp, vertical = if (tv) 48.dp else 16.dp),
        verticalArrangement = Arrangement.spacedBy(if (tv) 28.dp else 14.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(
            Modifier
                .weight(1f, fill = false)
                .aspectRatio(16f / 9f, matchHeightConstraintsFirst = true)
                .clip(RoundedCornerShape(if (tv) Marquee.radius.l else 12.dp))
                .background(Color.Black),
        ) {
            picture()
            if (failed) {
                Box(Modifier.align(Alignment.Center).padding(24.dp)) {
                    LiveSpoken(
                        FAILED_LINE,
                    ) { MqText(FAILED_LINE, role = TextRole.Callout, color = Marquee.colors.muted) }
                }
            } else if (waiting) {
                // Said once when it shows, as the line is.
                Box(Modifier.align(Alignment.Center)) {
                    LiveSpoken(LOADING) { Spinner(Modifier.semantics { contentDescription = LOADING }) }
                }
            }
        }
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.spacedBy(if (tv) 26.dp else 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val ms = { s: Float -> (s * 1000).toLong() }
            val role = if (tv) TextRole.Callout else TextRole.Caption
            MqText(clockText(ms(target ?: state.seconds)), role = role)
            Scrubber(
                position = { ms(state.seconds) },
                duration = { ms(state.length) },
                buffered = { 0L },
                preview = { target?.let(ms) },
                emphasized = barFocused || (!tv && target != null),
                onSeek = { onSeek(trailerSeekTarget(it / 1000f, state.length, 0f)) },
                modifier =
                    Modifier
                        .weight(1f)
                        .height(if (tv) 30.dp else 48.dp)
                        .focusRequester(bar)
                        .marqueeFocus(8.dp, scale = 1f)
                        .onFocusChanged {
                            barFocused = it.isFocused
                            if (!it.isFocused) target = null
                        }.onKeyEvent {
                            barKey(
                                it,
                                state.seconds,
                                state.length,
                                target,
                                { to -> target = to },
                                onToggle,
                                onSeek,
                            )
                        }.pointerInput(Unit) {
                            detectTapGestures { at -> barAt(at.x, size.width, state)?.let(onSeek) }
                        }.pointerInput(Unit) {
                            detectHorizontalDragGestures(
                                onDragStart = { at -> target = barAt(at.x, size.width, state) },
                                onHorizontalDrag = { change, _ ->
                                    target =
                                        barAt(change.position.x, size.width, state) ?: target
                                },
                                onDragEnd = {
                                    target?.let(onSeek)
                                    target = null
                                },
                                onDragCancel = { target = null },
                            )
                        }.focusable(enabled = !failed),
            )
            MqText("-" + clockText(ms((state.length - (target ?: state.seconds)).coerceAtLeast(0f))), role = role)
        }
        // A phone gives the name its own line (beside three buttons it kept four letters, the Pixel walk of 2026-10-29).
        if (!tv) MqText(title, Modifier.fillMaxWidth(), role = TextRole.Headline, maxLines = 1)
        Row(
            Modifier.fillMaxWidth(),
            horizontalArrangement =
                Arrangement.spacedBy(
                    if (tv) 20.dp else 8.dp,
                    if (tv) Alignment.Start else Alignment.End,
                ),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (tv) MqText(title, Modifier.weight(1f), role = TextRole.Headline, maxLines = 1)
            SecondaryButton(
                if (state.paused) "Play" else "Pause",
                onToggle,
                icon = if (state.paused) Icons.Play else Icons.Pause,
                enabled = !failed,
                focusRequester = toggle,
            )
            SecondaryButton(
                "Sound",
                onLoud,
                icon = if (loud) Icons.Volume2 else Icons.VolumeX,
                enabled = !failed,
                selected = loud,
            )
            SecondaryButton("Close", onClose, icon = Icons.X, focusRequester = closer)
        }
    }
}

/** The position a touch at [x] on a bar [width] px wide stands for, in seconds, or null with no length yet. */
private fun barAt(
    x: Float,
    width: Int,
    state: TrailerState,
): Float? = scrubTo(x, width, (state.length * 1000).toLong())?.let { trailerSeekTarget(it / 1000f, state.length, 0f) }

/** Left and Right on the bar: steps that grow while the key is held, committed when it is let go; select toggles. */
internal fun barKey(
    event: KeyEvent,
    seconds: Float,
    length: Float,
    target: Float?,
    set: (Float?) -> Unit,
    onToggle: () -> Unit,
    onSeek: (Float) -> Unit,
): Boolean {
    val direction =
        when (event.key) {
            Key.DirectionLeft -> -1
            Key.DirectionRight -> 1
            else -> 0
        }
    if (direction == 0) {
        if (event.type == KeyEventType.KeyDown && event.key in SELECT_KEYS && event.nativeKeyEvent.repeatCount == 0) {
            onToggle()
            return true
        }
        return false
    }
    return when (event.type) {
        KeyEventType.KeyDown -> {
            if (length > 0f) {
                // The film player's growing step, held to a tenth of a trailer: a held key never leaps to its end.
                val step = minOf(seekStepMs(event.nativeKeyEvent.repeatCount) / 1000f, maxOf(SEEK_S, length / 10f))
                set(trailerSeekTarget(target ?: seconds, length, direction * step))
            }
            true
        }

        KeyEventType.KeyUp -> {
            target?.let(onSeek)
            set(null)
            true
        }

        else -> {
            false
        }
    }
}

/** The remote's media keys, wherever the focus is: play and pause (Play only plays, Pause only pauses), rewind and fast
 * forward by 10 s. A held play key toggles once. */
private fun mediaKey(
    event: KeyEvent,
    paused: Boolean,
    onToggle: () -> Unit,
    seekBy: (Float) -> Unit,
): Boolean {
    if (event.type != KeyEventType.KeyDown) return event.key in MEDIA_KEYS
    val first = event.nativeKeyEvent.repeatCount == 0
    when (event.key) {
        Key.MediaPlayPause -> if (first) onToggle()
        Key.MediaPlay -> if (first && paused) onToggle()
        Key.MediaPause -> if (first && !paused) onToggle()
        Key.MediaRewind -> if (first) seekBy(-SEEK_S)
        Key.MediaFastForward -> if (first) seekBy(SEEK_S)
        else -> return false
    }
    return true
}

private const val SEEK_S = 10f
private const val FAILED_LINE = "This trailer can't play here."
private const val LOADING = "Loading trailer"
private val SELECT_KEYS = setOf(Key.DirectionCenter, Key.Enter, Key.NumPadEnter)
private val MEDIA_KEYS = setOf(Key.MediaPlayPause, Key.MediaPlay, Key.MediaPause, Key.MediaRewind, Key.MediaFastForward)

/** The full-screen trailer's page, for tests and walks. */
const val FULL_TRAILER_TAG = "full-trailer"
