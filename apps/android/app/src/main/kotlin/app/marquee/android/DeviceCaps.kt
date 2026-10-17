package app.marquee.android

import android.content.Context
import android.hardware.display.DisplayManager
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.os.Build
import android.view.Display
import app.marquee.core.client.DeviceCapsParams
import java.util.Locale

/** What one decoder can decode, reduced to what the capability mapping needs (testable without a device). */
data class DecoderInfo(
    val mimes: List<String>,
    val hardware: Boolean,
)

/**
 * What this device can play, for the `versions` view (plans/core-protocol.md): screen height and
 * HDR types, hardware video decoders, ten-bit HEVC, audio decoders, audio the receiver takes as
 * passthrough, container formats, and the device language. The ranking uses it to put releases
 * that play directly first and to hide ones that cannot play. Probing walks the codec list, so
 * call it off the main thread, once.
 */
object DeviceCaps {
    private val videoMimes =
        mapOf(
            "video/avc" to "h264",
            "video/hevc" to "h265",
            "video/av01" to "av1",
            "video/x-vnd.on2.vp9" to "vp9",
        )

    private val audioMimes =
        mapOf(
            "audio/mp4a-latm" to "aac",
            "audio/ac3" to "ac3",
            "audio/eac3" to "eac3",
            "audio/vnd.dts" to "dts",
            "audio/vnd.dts.hd" to "dts",
            "audio/true-hd" to "true_hd",
            "audio/opus" to "opus",
            "audio/flac" to "flac",
            "audio/mpeg" to "mp3",
            "audio/raw" to "pcm",
        )

    /**
     * Containers the player's extractors read. Checked against Media3's extractor list when the
     * player lands (P5); anything else needs the fallback engine.
     */
    private val containers = listOf("mkv", "mp4", "m4v", "webm", "ts", "mov")

    private val hdrTypes =
        mapOf(
            Display.HdrCapabilities.HDR_TYPE_DOLBY_VISION to "dolby_vision",
            Display.HdrCapabilities.HDR_TYPE_HDR10 to "hdr10",
            Display.HdrCapabilities.HDR_TYPE_HLG to "hlg",
            Display.HdrCapabilities.HDR_TYPE_HDR10_PLUS to "hdr10_plus",
        )

    private val passthroughEncodings =
        mapOf(
            AudioFormat.ENCODING_AC3 to "ac3",
            AudioFormat.ENCODING_E_AC3 to "eac3",
            AudioFormat.ENCODING_DTS to "dts",
            AudioFormat.ENCODING_DTS_HD to "dts_hd",
            AudioFormat.ENCODING_DOLBY_TRUEHD to "true_hd",
            AudioFormat.ENCODING_E_AC3_JOC to "atmos",
        )

    fun probe(context: Context): DeviceCapsParams {
        val decoders = decoders()
        val display = context.getSystemService(DisplayManager::class.java)?.getDisplay(Display.DEFAULT_DISPLAY)
        return DeviceCapsParams(
            displayHeight = display?.mode?.physicalHeight ?: 1080,
            hdr = hdrNames(display),
            decoders = videoDecoders(decoders),
            tenBitHevc = tenBitHevc(),
            passthrough = passthrough(context),
            audioDecoders = audioDecoders(decoders),
            containers = containers,
            language = deviceLanguage(Locale.getDefault()),
            hasFallbackEngine = false,
        )
    }

    /** Video codecs with a hardware decoder: software decoding of 4K on a stick is not playing. */
    internal fun videoDecoders(all: List<DecoderInfo>): List<String> =
        videoMimes.filterKeys { mime -> all.any { it.hardware && mime in it.mimes } }.values.toList()

    /** Audio codecs with any decoder: software audio decoding is cheap. PCM always plays. */
    internal fun audioDecoders(all: List<DecoderInfo>): List<String> =
        (audioMimes.filterKeys { mime -> all.any { mime in it.mimes } }.values + "pcm").distinct()

    internal fun deviceLanguage(locale: Locale): String? = locale.language.takeIf { it.length == 2 }

    internal fun hdrNames(types: IntArray): List<String> = types.toList().mapNotNull { hdrTypes[it] }.distinct()

    internal fun passthroughNames(encodings: Collection<Int>): List<String> =
        encodings.mapNotNull { passthroughEncodings[it] }.distinct()

    @Suppress("DEPRECATION")
    private fun hdrNames(display: Display?): List<String> {
        display ?: return emptyList()
        val types =
            if (Build.VERSION.SDK_INT >= 34) {
                display.mode.supportedHdrTypes
            } else {
                display.hdrCapabilities?.supportedHdrTypes ?: IntArray(0)
            }
        return hdrNames(types)
    }

    private fun decoders(): List<DecoderInfo> =
        MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos.filterNot { it.isEncoder }.map {
            DecoderInfo(it.supportedTypes.map(String::lowercase), isHardware(it))
        }

    private fun isHardware(info: MediaCodecInfo): Boolean =
        if (Build.VERSION.SDK_INT >= 29) {
            info.isHardwareAccelerated && !info.isSoftwareOnly
        } else {
            val name = info.name.lowercase()
            !name.startsWith("omx.google.") && !name.startsWith("c2.android.") && !name.startsWith("omx.ffmpeg.")
        }

    private fun tenBitHevc(): Boolean =
        MediaCodecList(MediaCodecList.REGULAR_CODECS)
            .codecInfos
            .filter { !it.isEncoder && isHardware(it) && "video/hevc" in it.supportedTypes.map(String::lowercase) }
            .any { info ->
                info
                    .getCapabilitiesForType("video/hevc")
                    .profileLevels
                    .any { it.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10 }
            }

    private fun passthrough(context: Context): List<String> {
        val audio = context.getSystemService(AudioManager::class.java) ?: return emptyList()
        if (Build.VERSION.SDK_INT >= 29) {
            val attributes = AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).build()
            return passthroughNames(
                passthroughEncodings.keys.filter { encoding ->
                    val format =
                        AudioFormat
                            .Builder()
                            .setEncoding(encoding)
                            .setSampleRate(48_000)
                            .setChannelMask(AudioFormat.CHANNEL_OUT_5POINT1)
                            .build()
                    AudioTrack.isDirectPlaybackSupported(format, attributes)
                },
            )
        }
        // Before Android 10: what the connected outputs list (an HDMI receiver lists its encodings).
        return passthroughNames(audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS).flatMap { it.encodings.toList() })
    }
}
