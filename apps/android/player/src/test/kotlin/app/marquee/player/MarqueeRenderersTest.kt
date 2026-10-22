package app.marquee.player

import android.os.Handler
import android.os.Looper
import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.common.Timeline
import androidx.media3.common.TrackGroup
import androidx.media3.exoplayer.audio.AudioRendererEventListener
import androidx.media3.exoplayer.source.MediaSource
import androidx.media3.exoplayer.source.TrackGroupArray
import androidx.media3.exoplayer.text.TextRenderer
import androidx.media3.exoplayer.trackselection.DefaultTrackSelector
import androidx.media3.exoplayer.upstream.DefaultBandwidthMeter
import androidx.media3.exoplayer.video.VideoRendererEventListener
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import kotlin.test.assertEquals
import kotlin.test.assertIs

/** The player's renderers: the file's own subtitles go to the delayed renderer first, captions to Media3's. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class MarqueeRenderersTest {
    @Test
    fun `the delayed text renderer comes before Media3's, which keeps the captions`() {
        val text =
            MarqueeRenderers(RuntimeEnvironment.getApplication(), SubtitleDelay())
                .createRenderers(
                    Handler(Looper.getMainLooper()),
                    object : VideoRendererEventListener {},
                    object : AudioRendererEventListener {},
                    {},
                    {},
                ).filter { it.trackType == C.TRACK_TYPE_TEXT }
        assertEquals(2, text.size)
        assertIs<DelayedTextRenderer>(text[0])
        assertIs<TextRenderer>(text[1])
    }

    @Test
    fun `the track selector gives a file's own text track to the delayed renderer`() {
        val context = RuntimeEnvironment.getApplication()
        val renderers =
            MarqueeRenderers(context, SubtitleDelay()).createRenderers(
                Handler(Looper.getMainLooper()),
                object : VideoRendererEventListener {},
                object : AudioRendererEventListener {},
                {},
                {},
            )
        val selector = DefaultTrackSelector(context)
        selector.init({}, DefaultBandwidthMeter.getSingletonInstance(context))
        selector.setParameters(selector.buildUponParameters().setPreferredTextLanguage("en"))
        val text =
            Format
                .Builder()
                .setSampleMimeType(MimeTypes.APPLICATION_MEDIA3_CUES)
                .setLanguage("en")
                .setSelectionFlags(C.SELECTION_FLAG_DEFAULT)
                .build()
        val result =
            selector.selectTracks(
                renderers.map { it.capabilities }.toTypedArray(),
                TrackGroupArray(TrackGroup(text)),
                MediaSource.MediaPeriodId(Any()),
                Timeline.EMPTY,
            )
        val chosen = renderers.indices.filter { result.selections[it] != null }
        assertEquals(listOf(renderers.indexOfFirst { it is DelayedTextRenderer }), chosen)
    }
}
