package app.marquee.player

/**
 * The delay of the subtitle track that is on, in milliseconds (positive: the words come later). The file's own
 * tracks read it as they draw ([DelayedTextRenderer]), on the playback thread; an addon track through
 * [PlaybackController.addonCues]. Written on the main thread.
 */
class SubtitleDelay {
    @Volatile
    var ms = 0L
}

/** How far one press of the Timing row's minus or plus moves the subtitles, as Stremio's delay steps. */
const val SUBTITLE_STEP_MS = 100L

/** The furthest the subtitles move either way (Kodi's and Nuvio's): room for a cut or an advert. */
const val SUBTITLE_MAX_MS = 60_000L

/** How long minus or plus is held before it repeats, and how often it repeats then (stremio-web subtitleDelay.ts). */
internal const val HOLD_START_MS = 250L
internal const val HOLD_EVERY_MS = 100L

/**
 * The delay one step from [delayMs] in [direction] (1 later, -1 earlier). A press ([heldMs] null) moves a tenth of a
 * second; a repeat while held moves 0.3 s in the first second of holding, then 0.4, 0.5, and 0.6 s from the third
 * second on. The result snaps to a tenth of a second, short of where it was going as Stremio's does, so a delay off
 * the tenths lands on one at the first press; it stays within [SUBTITLE_MAX_MS].
 */
fun steppedDelay(
    delayMs: Long,
    direction: Int,
    heldMs: Long? = null,
): Long {
    val times = if (heldMs == null) 1 else minOf(heldMs / 1000 + 3, 6)
    val moved = delayMs + direction * SUBTITLE_STEP_MS * times
    val tenths = if (direction > 0) Math.floorDiv(moved, SUBTITLE_STEP_MS) else -Math.floorDiv(-moved, SUBTITLE_STEP_MS)
    return (tenths * SUBTITLE_STEP_MS).coerceIn(-SUBTITLE_MAX_MS, SUBTITLE_MAX_MS)
}

/** "+0.3 s", "0.0 s", "-1.2 s": the delay as the Timing row shows it, later being plus. */
fun delayText(delayMs: Long): String {
    val seconds = "%.1f".format(java.util.Locale.ROOT, kotlin.math.abs(delayMs) / 1000.0)
    return when {
        delayMs > 0 -> "+$seconds s"
        delayMs < 0 -> "-$seconds s"
        else -> "$seconds s"
    }
}

/** "On time", "0.5 s later", "1.5 s earlier": what the Timing row says to a screen reader. */
fun timingWords(delayMs: Long): String {
    if (delayMs == 0L) return "On time"
    val seconds = "%.1f".format(java.util.Locale.ROOT, kotlin.math.abs(delayMs) / 1000.0)
    return if (delayMs > 0) "$seconds s later" else "$seconds s earlier"
}
