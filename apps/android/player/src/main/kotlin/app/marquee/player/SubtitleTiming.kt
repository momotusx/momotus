package app.marquee.player

import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.util.Consumer
import androidx.media3.extractor.text.CuesWithTiming
import androidx.media3.extractor.text.SubtitleParser

/**
 * The subtitle delay of one play, in milliseconds (positive: the words come later). Media3 has no delay
 * setting, so the parsers read it when an item is set ([DelayedSubtitles]), and a change sets the item
 * again at the same position. Written on the main thread, read on the loader's.
 */
class SubtitleDelay {
    @Volatile
    var ms = 0L
}

/**
 * Subtitle parsers whose cues are shifted by the play's [delay]: for files fetched beside the video (an
 * addon's subtitles) and the text tracks parsed out of the file. A cue never starts before zero.
 */
class DelayedSubtitles(
    private val base: SubtitleParser.Factory,
    private val delay: SubtitleDelay,
) : SubtitleParser.Factory {
    override fun supportsFormat(format: Format): Boolean = base.supportsFormat(format)

    override fun getCueReplacementBehavior(format: Format): Int = base.getCueReplacementBehavior(format)

    override fun create(format: Format): SubtitleParser {
        val parser = base.create(format)
        val shiftUs = delay.ms * 1000
        if (shiftUs == 0L) return parser
        return object : SubtitleParser {
            override fun parse(
                data: ByteArray,
                offset: Int,
                length: Int,
                outputOptions: SubtitleParser.OutputOptions,
                output: Consumer<CuesWithTiming>,
            ) = parser.parse(data, offset, length, outputOptions) { output.accept(shifted(it, shiftUs)) }

            override fun getCueReplacementBehavior(): Int = parser.cueReplacementBehavior

            override fun reset() = parser.reset()
        }
    }
}

/** [cues] [shiftUs] later (earlier when negative), never before zero; cues without a start stay as they are. */
internal fun shifted(
    cues: CuesWithTiming,
    shiftUs: Long,
): CuesWithTiming =
    if (cues.startTimeUs == C.TIME_UNSET) {
        cues
    } else {
        CuesWithTiming(cues.cues, (cues.startTimeUs + shiftUs).coerceAtLeast(0L), cues.durationUs)
    }

/** How far one press of Earlier or Later moves the subtitles. */
const val SUBTITLE_STEP_MS = 500L

/** The furthest the subtitles move either way. */
const val SUBTITLE_MAX_MS = 30_000L

/** "On time", "0.5 s later", "1.5 s earlier": what the Timing row says. */
fun timingWords(delayMs: Long): String {
    if (delayMs == 0L) return "On time"
    val seconds = "%.1f".format(java.util.Locale.ROOT, kotlin.math.abs(delayMs) / 1000.0)
    return if (delayMs > 0) "$seconds s later" else "$seconds s earlier"
}
