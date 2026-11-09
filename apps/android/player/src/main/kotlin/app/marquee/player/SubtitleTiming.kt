package app.marquee.player

import app.marquee.core.StreamChoiceCopy

/**
 * The delay of the subtitle track that is on, in milliseconds (positive: the words come later). The file's own
 * tracks read it as they draw ([DelayedTextRenderer]), on the playback thread; an addon track through
 * [PlaybackController.addonCues]. Written on the main thread.
 */
class SubtitleDelay {
    @Volatile
    var ms = 0L
        set(value) {
            field = value
            onChange?.invoke()
        }

    /** Told of every change, on the writer's thread: the player wakes the file's renderer with it (PlayerHost). */
    @Volatile
    var onChange: (() -> Unit)? = null
}

/** How far one press of the Timing row's minus or plus moves the subtitles, as Stremio's delay steps. */
const val SUBTITLE_STEP_MS = 100L

/** The furthest the subtitles move either way (Kodi's and Nuvio's): room for a cut or an advert. */
const val SUBTITLE_MAX_MS = 60_000L

/**
 * How long minus or plus is held before its repeat interval starts, and the interval (stremio-web subtitleDelay.ts):
 * as its Stepper, the first repeat comes one interval after the wait, at 350 ms.
 */
internal const val HOLD_START_MS = 250L
internal const val HOLD_EVERY_MS = 100L

/**
 * The delay one step from [delayMs] in [direction] (1 later, -1 earlier). A press ([heldMs] null) moves a tenth of a
 * second; a repeat while held moves 0.3 s in the first second of holding, then 0.4, 0.5, and 0.6 s from the third
 * second on. The result snaps to [DELAY_GRID_MS] ([snapDelay]); it stays within [SUBTITLE_MAX_MS].
 */
fun steppedDelay(
    delayMs: Long,
    direction: Int,
    heldMs: Long? = null,
): Long {
    val times = if (heldMs == null) 1 else minOf(heldMs / 1000 + 3, 6)
    return snapDelay(delayMs + direction * SUBTITLE_STEP_MS * times, direction)
}

/** How far one tap of the bar's quick shift moves the subtitles (plans/player-bar.md D2). */
const val QUICK_STEP_MS = 250L

/** The quick shift held: its first repeat comes after this, then one every [QUICK_EVERY_MS] until it is let go (D2). */
const val QUICK_START_MS = 400L
const val QUICK_EVERY_MS = 150L

/** The grid every delay lands on, so the quick shift's 0.25 s and the picker's 0.1 s steps share one (D3). */
const val DELAY_GRID_MS = 50L

/**
 * The delay one quick shift tick from [delayMs] in [direction]: a tap ([heldMs] null) moves 0.25 s; a tick while held
 * moves 0.25 s times the whole seconds held plus one, at most four times, so a 2 s hold moves 4.5 s (D2).
 */
fun quickDelay(
    delayMs: Long,
    direction: Int,
    heldMs: Long? = null,
): Long {
    val times = if (heldMs == null) 1 else minOf(heldMs / 1000 + 1, 4)
    return snapDelay(delayMs + direction * QUICK_STEP_MS * times, direction)
}

/**
 * [moved] on the [DELAY_GRID_MS] grid, short of where it was going as Stremio's step is, so a delay off the grid lands
 * on it at the first press; within [SUBTITLE_MAX_MS].
 */
private fun snapDelay(
    moved: Long,
    direction: Int,
): Long {
    val steps = if (direction > 0) Math.floorDiv(moved, DELAY_GRID_MS) else -Math.floorDiv(-moved, DELAY_GRID_MS)
    return (steps * DELAY_GRID_MS).coerceIn(-SUBTITLE_MAX_MS, SUBTITLE_MAX_MS)
}

/**
 * The line said once a shift settles (D12): "Subtitles 0.25 seconds later", "Subtitles 1.0 seconds earlier",
 * "Subtitles in sync".
 */
fun shiftSaid(delayMs: Long): String =
    when {
        delayMs > 0 -> StreamChoiceCopy.Bar.SAID_LATER.replace("{n}", seconds(delayMs))
        delayMs < 0 -> StreamChoiceCopy.Bar.SAID_EARLIER.replace("{n}", seconds(delayMs))
        else -> StreamChoiceCopy.Bar.SAID_IN_SYNC
    }

/**
 * The shift's value as a screen reader names it: "+0.25 s, subtitles later, press to reset" ("press to reset" only
 * while it is not 0, where a press does nothing); on TV "press OK to adjust" at every value.
 */
fun shiftName(
    delayMs: Long,
    tv: Boolean,
): String {
    val names = StreamChoiceCopy.Bar.Names
    val value =
        when {
            delayMs > 0 -> names.VALUE_LATER
            delayMs < 0 -> names.VALUE_EARLIER
            else -> names.VALUE_IN_SYNC
        }.replace("{value}", delayText(delayMs))
    return when {
        tv -> "$value, ${names.PRESS_OK_TO_ADJUST}"
        delayMs != 0L -> "$value, ${names.PRESS_TO_RESET}"
        else -> value
    }
}

/** "+0.25 s", "+0.5 s", "0.0 s", "−1.2 s" (a true minus sign, U+2212): the delay as shown, later being plus (D3). */
fun delayText(delayMs: Long): String {
    val seconds = seconds(delayMs)
    return when {
        delayMs > 0 -> "+$seconds s"
        delayMs < 0 -> "\u2212$seconds s"
        else -> "$seconds s"
    }
}

/** "Plus 0.3 seconds", "0.0 seconds", "Minus 1.2 seconds": what a screen reader says for the Timing row, as on web and Apple. */
fun timingWords(delayMs: Long): String {
    val seconds = seconds(delayMs)
    return when {
        delayMs > 0 -> "Plus $seconds seconds"
        delayMs < 0 -> "Minus $seconds seconds"
        else -> "$seconds seconds"
    }
}

/** The seconds of [delayMs], unsigned, with one decimal or two when the second is not 0: "0.25", "0.5", "1.0". */
private fun seconds(delayMs: Long): String {
    val two = "%.2f".format(java.util.Locale.ROOT, kotlin.math.abs(delayMs) / 1000.0)
    return if (two.endsWith("0")) two.dropLast(1) else two
}
