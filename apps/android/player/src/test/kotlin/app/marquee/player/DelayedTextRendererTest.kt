package app.marquee.player

import android.graphics.Bitmap
import android.os.Bundle
import android.os.Looper
import android.os.Parcel
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.common.text.Cue
import androidx.media3.common.text.CueGroup
import androidx.media3.common.util.Clock
import androidx.media3.decoder.DecoderInputBuffer
import androidx.media3.exoplayer.FormatHolder
import androidx.media3.exoplayer.RendererConfiguration
import androidx.media3.exoplayer.analytics.PlayerId
import androidx.media3.exoplayer.source.MediaSource
import androidx.media3.exoplayer.source.SampleStream
import androidx.media3.exoplayer.text.TextOutput
import androidx.media3.extractor.text.CueEncoder
import androidx.media3.extractor.text.CuesWithTiming
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The file's own subtitle track drawn with its delay, either way, from what the player has loaded (no new item). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class DelayedTextRendererTest {
    /** A text track as the extractors hand it over: Media3 cues, a sample each, loaded up to [samples]' end. */
    private class Track(
        private val samples: List<Pair<Long, ByteArray>>,
        /** The track ends after [samples]. */
        private val ends: Boolean = false,
    ) : SampleStream {
        /** How many samples the renderer took. */
        var next = 0
        private var formatRead = false

        override fun isReady() = true

        override fun maybeThrowError() = Unit

        override fun readData(
            formatHolder: FormatHolder,
            buffer: DecoderInputBuffer,
            readFlags: Int,
        ): Int {
            if (!formatRead) {
                formatRead = true
                formatHolder.format = FORMAT
                return C.RESULT_FORMAT_READ
            }
            if (next == samples.size && ends) {
                buffer.setFlags(C.BUFFER_FLAG_END_OF_STREAM)
                return C.RESULT_BUFFER_READ
            }
            val (timeUs, data) = samples.getOrNull(next) ?: return C.RESULT_NOTHING_READ
            next++
            buffer.ensureSpaceForWrite(data.size)
            buffer.data!!.put(data)
            buffer.timeUs = timeUs
            buffer.setFlags(C.BUFFER_FLAG_KEY_FRAME)
            return C.RESULT_BUFFER_READ
        }

        override fun skipData(positionUs: Long) = 0
    }

    private val shown = ArrayList<List<String>>()
    private val output =
        object : TextOutput {
            override fun onCues(cueGroup: CueGroup) {
                shown += cueGroup.cues.map { it.text.toString() }
            }
        }
    private val delay = SubtitleDelay()

    private fun line(
        text: String,
        startS: Long,
        lengthS: Long,
    ) = startS * 1_000_000 to
        CueEncoder().encode(listOf(Cue.Builder().setText(text).build()), lengthS * 1_000_000)

    /** A picture cue (PGS, DVB) at [startS] for 1 s, its bitmap [px] square. */
    private fun picture(
        startS: Long,
        px: Int = 8,
    ) = startS * 1_000_000 to
        CueEncoder().encode(
            listOf(Cue.Builder().setBitmap(Bitmap.createBitmap(px, px, Bitmap.Config.ARGB_8888)).build()),
            1_000_000,
        )

    // A line at 1 s for 2 s, and one 30 s in, well past the second Media3's own renderer reads ahead.
    private fun renderer(track: Track = Track(listOf(line("One", 1, 2), line("Far", 30, 2)))): DelayedTextRenderer {
        val renderer = DelayedTextRenderer(output, Looper.getMainLooper(), delay)
        renderer.init(0, PlayerId.UNSET, Clock.DEFAULT)
        renderer.enable(
            RendererConfiguration.DEFAULT,
            arrayOf(FORMAT),
            track,
            0,
            false,
            true,
            0,
            0,
            MediaSource.MediaPeriodId(Any()),
        )
        renderer.start()
        return renderer
    }

    /** What shows after a render at [seconds] of the video. */
    private fun DelayedTextRenderer.at(seconds: Double): List<String> {
        render((seconds * 1_000_000).toLong(), 0)
        shadowOf(Looper.getMainLooper()).idle()
        return shown.lastOrNull() ?: emptyList()
    }

    @Test
    fun `cues follow the position minus the delay, later or earlier, at once`() {
        val renderer = renderer()
        assertEquals(emptyList(), renderer.at(0.5))
        assertEquals(listOf("One"), renderer.at(1.5))
        assertEquals(emptyList(), renderer.at(3.5))
        // A second later: the line at 1 to 3 s shows at 2 to 4 s; read from what was kept.
        delay.ms = 1_000
        assertEquals(listOf("One"), renderer.at(3.5))
        // 28 s earlier: the line at 30 s shows at 2 s, read far ahead of the position.
        delay.ms = -28_000
        assertEquals(listOf("Far"), renderer.at(2.5))
        delay.ms = 0
        assertEquals(emptyList(), renderer.at(4.0))
    }

    @Test
    fun `a seek forgets what was read, and the disabled renderer clears the screen`() {
        val renderer = renderer()
        assertEquals(listOf("One"), renderer.at(1.5))
        renderer.resetPosition(20_000_000, true)
        shadowOf(Looper.getMainLooper()).idle()
        assertEquals(emptyList(), shown.last())
        renderer.stop()
        renderer.disable()
        shadowOf(Looper.getMainLooper()).idle()
        assertEquals(emptyList(), shown.last())
    }

    @Test
    fun `it takes the extractors' cues and leaves captions in the video to Media3's renderer`() {
        val renderer = DelayedTextRenderer(output, Looper.getMainLooper(), delay)
        assertEquals(C.FORMAT_HANDLED, renderer.supportsFormat(FORMAT) and 0b111)
        val captions = Format.Builder().setSampleMimeType(MimeTypes.APPLICATION_CEA608).build()
        assertEquals(C.FORMAT_UNSUPPORTED_SUBTYPE, renderer.supportsFormat(captions) and 0b111)
    }

    @Test
    fun `a delay written between renders is drawn at once, as the player sends it`() {
        val renderer = renderer()
        assertEquals(listOf("One"), renderer.at(1.5))
        delay.ms = -28_500
        // No render: the player's message alone.
        renderer.handleMessage(DelayedTextRenderer.MSG_REDRAW, null)
        shadowOf(Looper.getMainLooper()).idle()
        assertEquals(listOf("Far"), shown.last())
        // Disabled, a message draws nothing.
        renderer.stop()
        renderer.disable()
        shadowOf(Looper.getMainLooper()).idle()
        val count = shown.size
        renderer.handleMessage(DelayedTextRenderer.MSG_REDRAW, null)
        shadowOf(Looper.getMainLooper()).idle()
        assertEquals(count, shown.size)
    }

    @Test
    fun `cues with no length last until the next starts, and the track ends with its stream`() {
        val endless = { text: String, startS: Long ->
            startS * 1_000_000 to CueEncoder().encode(listOf(Cue.Builder().setText(text).build()), C.TIME_UNSET)
        }
        val renderer = renderer(Track(listOf(endless("First", 1), endless("Second", 4)), ends = true))
        renderer.setCurrentStreamFinal()
        assertEquals(listOf("First"), renderer.at(3.9))
        assertEquals(listOf("Second"), renderer.at(4.1))
        assertTrue(renderer.isEnded, "the stream ended")
    }

    @Test
    fun `a long track is read in batches, and a cue that does not decode is skipped`() {
        val lines = (0 until 100).map { line("Line $it", it.toLong(), 1) }
        // A sample that unmarshals and carries no cues: the decoder throws, as on a malformed sample.
        val empty = Parcel.obtain().apply { writeBundle(Bundle()) }
        val bad = 50_500_000L to empty.marshall().also { empty.recycle() }
        val track = Track(lines.take(50) + bad + lines.drop(50))
        val renderer = renderer(track)
        renderer.at(0.5)
        assertEquals(63, track.next, "a render reads 64: the format, then 63 samples")
        repeat(3) { renderer.at(0.5) }
        assertEquals(101, track.next)
        assertEquals(listOf("Line 60"), renderer.at(60.5))
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `picture cues are read 16 ahead of what shows at most, and the rest wait in the player's buffer`() {
        val track = Track((0 until 40).map { picture(it.toLong()) })
        val renderer = renderer(track)
        renderer.at(0.5)
        // The pictures at 0 to 16 s, 16 of them after 0.5 s: the next waits.
        assertEquals(17, track.next)
        renderer.at(20.5)
        // Ahead of 20.5 the pictures at 21 to 36 s.
        assertEquals(37, track.next)
    }

    @Test
    @GraphicsMode(GraphicsMode.Mode.NATIVE)
    fun `behind what shows only the last 8 pictures stay, and text cues are kept`() {
        val bitmap = Bitmap.createBitmap(8, 8, Bitmap.Config.ARGB_8888)
        val cues = TimedCues()
        (0 until 30).forEach {
            cues.add(CuesWithTiming(listOf(Cue.Builder().setBitmap(bitmap).build()), it * 1_000_000L, 1_000_000))
        }
        cues.add(CuesWithTiming(listOf(Cue.Builder().setText("Words").build()), 2_000_000, 1_000_000))
        cues.trimPictures(20_500_000, 8)
        // 20 ended by 20.5 s, the last 8 of them stay; the 10 from 20 s on all stay.
        assertEquals(18, cues.picturesFrom(0))
        assertEquals(listOf("Words"), cues.at(2_500_000).mapNotNull { it.text?.toString() })
        assertEquals(18L * bitmap.allocationByteCount, cues.pictureBytes())
    }

    private companion object {
        val FORMAT: Format = Format.Builder().setSampleMimeType(MimeTypes.APPLICATION_MEDIA3_CUES).build()
    }
}
