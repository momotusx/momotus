package app.marquee.player

import android.os.Looper
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
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** The file's own subtitle track drawn with its delay, either way, from what the player has loaded (no new item). */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class DelayedTextRendererTest {
    /** A text track as the extractors hand it over: Media3 cues, a sample each, loaded up to [samples]' end. */
    private class Track(
        private val samples: List<Pair<Long, ByteArray>>,
    ) : SampleStream {
        private var next = 0
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

    private fun renderer(): DelayedTextRenderer {
        val renderer = DelayedTextRenderer(output, Looper.getMainLooper(), delay)
        renderer.init(0, PlayerId.UNSET, Clock.DEFAULT)
        // A line at 1 s for 2 s, and one 30 s in, well past the second Media3's own renderer reads ahead.
        renderer.enable(
            RendererConfiguration.DEFAULT,
            arrayOf(FORMAT),
            Track(listOf(line("One", 1, 2), line("Far", 30, 2))),
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

    private companion object {
        val FORMAT: Format = Format.Builder().setSampleMimeType(MimeTypes.APPLICATION_MEDIA3_CUES).build()
    }
}
