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
