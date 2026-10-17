package app.marquee.android

import android.media.AudioFormat
import app.marquee.core.client.DeviceCapsParams
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import java.util.Locale
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class DeviceCapsTest {
    private val hardwareAvc = DecoderInfo(listOf("video/avc"), hardware = true)
    private val softwareHevc = DecoderInfo(listOf("video/hevc"), hardware = false)
    private val aacAndOpus = DecoderInfo(listOf("audio/mp4a-latm", "audio/opus"), hardware = false)

    @Test
    fun `only video codecs with a hardware decoder count`() {
        assertEquals(listOf("h264"), DeviceCaps.videoDecoders(listOf(hardwareAvc, softwareHevc)))
        assertEquals(emptyList(), DeviceCaps.videoDecoders(listOf(softwareHevc)))
    }

    @Test
    fun `any audio decoder counts and PCM always plays`() {
        assertEquals(listOf("aac", "opus", "pcm"), DeviceCaps.audioDecoders(listOf(aacAndOpus)))
        assertEquals(listOf("pcm"), DeviceCaps.audioDecoders(emptyList()))
    }

    @Test
    fun `passthrough encodings and HDR types get the core's names`() {
        assertEquals(
            listOf("ac3", "eac3", "atmos"),
            DeviceCaps.passthroughNames(
                listOf(AudioFormat.ENCODING_AC3, AudioFormat.ENCODING_E_AC3, AudioFormat.ENCODING_E_AC3_JOC, 9999),
            ),
        )
        // Display.HdrCapabilities: 1 Dolby Vision, 2 HDR10, 3 HLG, 4 HDR10+.
        assertEquals(listOf("dolby_vision", "hdr10", "hlg"), DeviceCaps.hdrNames(intArrayOf(1, 2, 3, 2)))
    }

    @Test
    fun `the device language is sent only as a two letter code`() {
        assertEquals("tr", DeviceCaps.deviceLanguage(Locale.forLanguageTag("tr-TR")))
        assertNull(DeviceCaps.deviceLanguage(Locale.forLanguageTag("fil-PH")))
    }

    @Test
    fun `the versions params carry every capability under the names the core reads`() {
        val json =
            DeviceCapsParams(
                displayHeight = 2160,
                hdr = listOf("hdr10"),
                decoders = listOf("h264"),
                tenBitHevc = true,
                passthrough = listOf("ac3"),
                audioDecoders = listOf("aac"),
                containers = listOf("mkv"),
                language = "tr",
                hasFallbackEngine = false,
            ).toJson()
        assertEquals(
            setOf(
                "displayHeight",
                "hdr",
                "decoders",
                "tenBitHevc",
                "passthrough",
                "audioDecoders",
                "containers",
                "language",
                "hasFallbackEngine",
            ),
            json.keys,
        )
        assertEquals(JsonArray(listOf(JsonPrimitive("mkv"))), json["containers"])
        assertEquals(JsonPrimitive("tr"), json["language"])
    }
}
