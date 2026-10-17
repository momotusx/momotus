package app.marquee.player

import android.content.Context
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.trackselection.DefaultTrackSelector
import androidx.media3.session.MediaSession

/**
 * One ExoPlayer and its media session, alive while the Player route is on the back stack.
 *
 * Choices that matter on a TV stick, each from reading the alternatives:
 * - platform decoders first, with decoder fallback on, so a broken hardware codec tries the next one;
 * - tunneling stays off (it is device specific and stalls on underruns);
 * - subtitles start off (the viewer picks, as on the web player);
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

    private val session: MediaSession

    init {
        val renderers = DefaultRenderersFactory(context).setEnableDecoderFallback(true)
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
        // Media keys and the system's now-playing card; no service, so nothing plays in the background.
        session = MediaSession.Builder(context, player).setId("marquee").build()
    }

    fun release() {
        session.release()
        player.release()
    }

    private companion object {
        const val START_BUFFER_MS = 1_000
        const val REBUFFER_MS = 3_000
    }
}
