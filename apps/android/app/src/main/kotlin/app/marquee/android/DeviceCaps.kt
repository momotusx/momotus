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
import app.marquee.core.client.CoreLog
import app.marquee.core.client.DeviceCapsParams
import java.util.Locale

/** What one decoder can decode, reduced to what the capability mapping needs (testable without a device). */
data class DecoderInfo(
    val mimes: List<String>,
    val hardware: Boolean,
    /** Takes ten-bit HEVC (Main 10, with or without HDR10 metadata). */
    val tenBitHevc: Boolean = false,
)

/** The codec list reduced to what the ranking asks about. A device's codecs never change. */
data class CodecScan(
    val video: List<String>,
    val audio: List<String>,
    val tenBitHevc: Boolean,
    /** Any decoder for `video/dolby-vision`: without one a Dolby Vision file is not native here. */
    val dolbyVision: Boolean,
)

/**
 * What this device can play, for the `versions` view (plans/core-protocol.md): screen height and
 * HDR types, hardware video decoders, ten-bit HEVC, audio decoders, audio the receiver takes as
 * passthrough, container formats, and the device language. The ranking uses it to put releases
 * that play directly first and to hide ones that cannot play.
 *
 * Walking the codec list is slow, so [scanCodecs] runs once, off the main thread; [read] is cheap
 * and runs for every Versions view, because the display mode, the receiver and the language can
 * change while a TV process stays cached for days.
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
     * Containers Media3's extractors read (MKV, MP4 and fragmented MP4, WebM, MPEG-TS, MOV), and HLS
     * playlists (`m3u8`, through media3-exoplayer-hls and the core's rewrite); anything else needs the
     * fallback engine.
     */
    private val containers = listOf("mkv", "mp4", "m4v", "webm", "ts", "m2ts", "mov", "m3u8")

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

    /** Walks the codec list once. A platform that throws on it leaves the baseline: H.264 and PCM. */
    fun scanCodecs(): CodecScan {
        val decoders =
            runCatching { decoders() }
                .onFailure { CoreLog.w("the codec list could not be read: ${CoreLog.describe(it)}") }
                .getOrDefault(emptyList())
        return scan(decoders)
    }

    internal fun scan(decoders: List<DecoderInfo>) =
        CodecScan(
            video = videoDecoders(decoders),
            audio = audioDecoders(decoders),
            tenBitHevc = decoders.any { it.hardware && it.tenBitHevc },
            dolbyVision = decoders.any { "video/dolby-vision" in it.mimes },
        )

    fun read(
        context: Context,
        scan: CodecScan,
    ): DeviceCapsParams {
        val display =
            runCatching {
                context.getSystemService(DisplayManager::class.java)?.getDisplay(Display.DEFAULT_DISPLAY)
            }.getOrNull()
        val mode = display?.mode
        return DeviceCapsParams(
            // The shorter side: a phone held upright reports its long edge as the "height".
            displayHeight = mode?.let { minOf(it.physicalWidth, it.physicalHeight) } ?: 1080,
            hdr = withDolbyVision(runCatching { hdrNames(display) }.getOrDefault(emptyList()), scan.dolbyVision),
            decoders = scan.video,
            tenBitHevc = scan.tenBitHevc,
            passthrough = runCatching { passthrough(context) }.getOrNull(),
            audioDecoders = scan.audio,
            containers = containers,
            language = deviceLanguage(Locale.getDefault()),
            hasFallbackEngine = false,
        )
    }

    /** When even the scan failed: a 1080p SDR screen, H.264 and PCM, no fallback engine. */
    fun baseline() =
        DeviceCapsParams(
            displayHeight = 1080,
            hdr = emptyList(),
            decoders = listOf("h264"),
            audioDecoders = listOf("aac", "pcm"),
            containers = containers,
            hasFallbackEngine = false,
        )

    /**
     * Video codecs with a hardware decoder: software decoding of 4K on a stick is not playing.
     * H.264 is always listed, because every device plays it and an empty list would hide everything.
     */
    internal fun videoDecoders(all: List<DecoderInfo>): List<String> =
        (videoMimes.filterKeys { mime -> all.any { it.hardware && mime in it.mimes } }.values + "h264").distinct()

    /** Audio codecs with any decoder: software audio decoding is cheap. PCM always plays. */
    internal fun audioDecoders(all: List<DecoderInfo>): List<String> =
        (audioMimes.filterKeys { mime -> all.any { mime in it.mimes } }.values + "pcm").distinct()

    /** A two letter ISO 639-1 code; `iw`, `in` and `ji` (the old Java codes) are mapped to what the core reads. */
    internal fun deviceLanguage(locale: Locale): String? =
        locale
            .toLanguageTag()
            .substringBefore('-')
            .takeIf { it.length == 2 }

    internal fun hdrNames(types: IntArray): List<String> = types.toList().mapNotNull { hdrTypes[it] }.distinct()

    /** The display's HDR types, with Dolby Vision only when a decoder for it exists too. */
    internal fun withDolbyVision(
        names: List<String>,
        dolbyVisionDecoder: Boolean,
    ): List<String> = if (dolbyVisionDecoder) names else names - "dolby_vision"

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
        MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos.filterNot { it.isEncoder }.map { info ->
            val mimes = info.supportedTypes.map(String::lowercase)
            DecoderInfo(mimes, isHardware(info), tenBitHevc = "video/hevc" in mimes && takesTenBit(info))
        }

    private fun isHardware(info: MediaCodecInfo): Boolean =
        if (Build.VERSION.SDK_INT >= 29) {
            info.isHardwareAccelerated && !info.isSoftwareOnly
        } else {
            val name = info.name.lowercase()
            !name.startsWith("omx.google.") && !name.startsWith("c2.android.") && !name.startsWith("omx.ffmpeg.")
        }

    private fun takesTenBit(info: MediaCodecInfo): Boolean =
        runCatching {
            info.getCapabilitiesForType("video/hevc").profileLevels.any {
                it.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10 ||
                    it.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10 ||
                    it.profile == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10Plus
            }
        }.getOrDefault(false)

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
