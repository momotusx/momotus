package app.marquee.player

import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.common.text.Cue
import androidx.media3.common.util.Consumer
import androidx.media3.extractor.text.CuesWithTiming
import androidx.media3.extractor.text.SubtitleParser
import app.marquee.core.model.SubtitleSize
import kotlin.test.Test
import kotlin.test.assertEquals

/** The per-play subtitle delay and the profile's subtitle size (journey reviews 2026-10-20). */
class SubtitleTimingTest {
    private val format = Format.Builder().setSampleMimeType(MimeTypes.TEXT_VTT).build()

    /** A parser that gives one cue at 1 s for 2 s. */
    private val base =
        object : SubtitleParser.Factory {
            override fun supportsFormat(format: Format) = true

            override fun getCueReplacementBehavior(format: Format) = Format.CUE_REPLACEMENT_BEHAVIOR_MERGE

            override fun create(format: Format): SubtitleParser =
                object : SubtitleParser {
                    override fun parse(
                        data: ByteArray,
                        offset: Int,
                        length: Int,
                        outputOptions: SubtitleParser.OutputOptions,
                        output: Consumer<CuesWithTiming>,
                    ) = output.accept(CuesWithTiming(listOf(Cue.Builder().setText("Hi").build()), 1_000_000, 2_000_000))

                    override fun getCueReplacementBehavior() = Format.CUE_REPLACEMENT_BEHAVIOR_MERGE
                }
        }

    private fun startOf(delayMs: Long): Long {
        val delay = SubtitleDelay().apply { ms = delayMs }
        var start = C.TIME_UNSET
        DelayedSubtitles(base, delay)
            .create(format)
            .parse(ByteArray(0), 0, 0, SubtitleParser.OutputOptions.allCues()) { start = it.startTimeUs }
        return start
    }

    @Test
    fun `the play's delay moves every cue both ways`() {
        assertEquals(1_500_000L, startOf(500))
        assertEquals(500_000L, startOf(-500))
        assertEquals(1_000_000L, startOf(0))
    }

    @Test
    fun `an MKV text cue, relative to its block at 0, moves earlier too`() {
        // Media3 adds the block's time to this start: clamped at 0, Earlier would do nothing.
        val inFile = CuesWithTiming(emptyList(), 0, 2_000_000)
        assertEquals(-500_000L, shifted(inFile, -500_000).startTimeUs)
    }

    @Test
    fun `a cue with no start (picture subtitles) takes the shift as its start`() {
        val loose = CuesWithTiming(emptyList(), C.TIME_UNSET, C.TIME_UNSET)
        assertEquals(500_000L, shifted(loose, 500_000).startTimeUs)
        assertEquals(-500_000L, shifted(loose, -500_000).startTimeUs)
    }

    @Test
    fun `the Timing row says where the words are`() {
        assertEquals("On time", timingWords(0))
        assertEquals("0.5 s later", timingWords(500))
        assertEquals("1.5 s earlier", timingWords(-1_500))
        assertEquals("0.0 s", delayText(0))
        assertEquals("+0.3 s", delayText(300))
        assertEquals("-1.2 s", delayText(-1_200))
    }

    @Test
    fun `a press steps a tenth of a second and snaps to one, a hold steps 0_3 s rising to 0_6 s, within a minute`() {
        assertEquals(100L, SUBTITLE_STEP_MS)
        assertEquals(100L, steppedDelay(0, 1))
        assertEquals(-100L, steppedDelay(0, -1))
        // A delay off the tenths (a remembered one) lands on a tenth at the first press, short of the full step.
        assertEquals(300L, steppedDelay(250, 1))
        assertEquals(200L, steppedDelay(250, -1))
        assertEquals(-300L, steppedDelay(-250, -1))
        // Held: the first repeat after 250 ms, then every 100 ms; 0.3 s a tick in the first second, then 0.4, 0.5, 0.6.
        assertEquals(250L, HOLD_START_MS)
        assertEquals(100L, HOLD_EVERY_MS)
        assertEquals(300L, steppedDelay(0, 1, heldMs = 250))
        assertEquals(300L, steppedDelay(0, 1, heldMs = 950))
        assertEquals(400L, steppedDelay(0, 1, heldMs = 1_050))
        assertEquals(500L, steppedDelay(0, 1, heldMs = 2_050))
        assertEquals(-600L, steppedDelay(0, -1, heldMs = 3_050))
        assertEquals(600L, steppedDelay(0, 1, heldMs = 60_000))
        // Plus or minus a minute at most.
        assertEquals(60_000L, SUBTITLE_MAX_MS)
        assertEquals(60_000L, steppedDelay(59_900, 1, heldMs = 5_000))
        assertEquals(-60_000L, steppedDelay(-60_000, -1))
        // Held from 0, a minute is 109 ticks away: 11 s of holding.
        val ticks =
            generateSequence(HOLD_START_MS) { it + HOLD_EVERY_MS }
                .runningFold(0L) { at, held -> steppedDelay(at, 1, held) }
                .indexOfFirst { it == SUBTITLE_MAX_MS }
        assertEquals(109, ticks)
    }

    @Test
    fun `the profile's size scales the system caption size`() {
        assertEquals(1f, subtitleScale(SubtitleSize.Medium))
        assertEquals(0.75f, subtitleScale(SubtitleSize.Small))
        assertEquals(1.3f, subtitleScale(SubtitleSize.Large))
        assertEquals(1.6f, subtitleScale(SubtitleSize.ExtraLarge))
        assertEquals(1f, subtitleScale(SubtitleSize.Unknown))
    }
}
