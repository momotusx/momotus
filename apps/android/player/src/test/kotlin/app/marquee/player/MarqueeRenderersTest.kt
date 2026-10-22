package app.marquee.player

import android.os.Handler
import android.os.Looper
import androidx.media3.common.C
import androidx.media3.exoplayer.audio.AudioRendererEventListener
import androidx.media3.exoplayer.text.TextRenderer
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
}
