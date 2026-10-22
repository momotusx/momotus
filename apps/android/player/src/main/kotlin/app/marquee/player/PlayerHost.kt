package app.marquee.player

import android.content.Context
import android.os.Looper
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.Renderer
import androidx.media3.exoplayer.text.TextOutput
import androidx.media3.exoplayer.trackselection.DefaultTrackSelector
import androidx.media3.session.MediaSession

/**
 * One ExoPlayer and its media session, alive while the Player route is on the back stack.
 *
 * Choices that matter on a TV stick, each from reading the alternatives:
 * - platform decoders first, with decoder fallback on, so a broken hardware codec tries the next one;
 * - tunneling stays off (it is device specific and stalls on underruns);
 * - subtitles start off (the viewer picks, as on the web player), and the file's own draw with their
 *   track's delay ([DelayedTextRenderer]);
 * - audio focus and "becoming noisy" are handled, the screen and Wi-Fi stay awake while playing;
 * - playback starts after one second of buffer, a rebuffer waits three.
 *
 * Video goes to a SurfaceView (see [PlayerVideo]); a TextureView would tone-map HDR to SDR.
 */
class PlayerHost(
    context: Context,
    audioLanguage: String?,
) {
    val player: ExoPlayer

    /** The subtitle delay of the track that is on, which the file's own tracks read as they draw. */
    val subtitleDelay = SubtitleDelay()

    private val session: MediaSession

    /** Debug walks only (`--ez walklog true`): the once-a-second playback line. */
    private val walkLog: WalkLog?

    init {
        val renderers = MarqueeRenderers(context, subtitleDelay).setEnableDecoderFallback(true)
        val selector =
            DefaultTrackSelector(context).apply {
                setParameters(
                    buildUponParameters()
                        .setTunnelingEnabled(false)
                        .setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
                        .apply { audioLanguage?.let(::setPreferredAudioLanguage) },
                )
            }
        val loadControl =
            DefaultLoadControl
                .Builder()
                .setBufferDurationsMs(
                    DefaultLoadControl.DEFAULT_MIN_BUFFER_MS,
                    DefaultLoadControl.DEFAULT_MAX_BUFFER_MS,
                    START_BUFFER_MS,
                    REBUFFER_MS,
                ).build()
        player =
            ExoPlayer
                .Builder(context, renderers)
                .setTrackSelector(selector)
                .setLoadControl(loadControl)
                .setAudioAttributes(
                    AudioAttributes
                        .Builder()
                        .setUsage(C.USAGE_MEDIA)
                        .setContentType(C.AUDIO_CONTENT_TYPE_MOVIE)
                        .build(),
                    true,
                ).setHandleAudioBecomingNoisy(true)
                .setWakeMode(C.WAKE_MODE_NETWORK)
                .build()
        if (WalkMute.on) player.volume = 0f
        walkLog = WalkLog.attach(context, player)
        // Media keys and the system's now-playing card; no service, so nothing plays in the background.
        // The id is unique per host: a new player can start while the old one is still being cleared.
        session =
            try {
                MediaSession.Builder(context, player).setId("marquee-${ids.incrementAndGet()}").build()
            } catch (e: RuntimeException) {
                player.release()
                throw e
            }
    }

    fun release() {
        walkLog?.release()
        session.release()
        player.release()
    }

    private companion object {
        val ids =
            java.util.concurrent.atomic
                .AtomicInteger()
    }
}

/** Media3's renderers, with the file's own subtitles drawn by [DelayedTextRenderer] (Media3's still takes captions). */
internal class MarqueeRenderers(
    context: Context,
    private val delay: SubtitleDelay,
) : DefaultRenderersFactory(context) {
    override fun buildTextRenderers(
        context: Context,
        output: TextOutput,
        outputLooper: Looper,
        extensionRendererMode: Int,
        out: ArrayList<Renderer>,
    ) {
        out += DelayedTextRenderer(output, outputLooper, delay)
        super.buildTextRenderers(context, output, outputLooper, extensionRendererMode, out)
    }
}

/** Media held before the first frame plays, and before play goes on after a stall; the loading screen fills to them. */
internal const val START_BUFFER_MS = 1_000
internal const val REBUFFER_MS = 3_000
