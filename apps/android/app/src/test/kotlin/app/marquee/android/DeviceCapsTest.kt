package app.marquee.android

import android.media.AudioFormat
import android.net.NetworkCapabilities
import app.marquee.core.client.DeviceCapsParams
import app.marquee.core.model.Network
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import java.util.Locale
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class DeviceCapsTest {
    private val hardwareAvc = DecoderInfo(listOf("video/avc"), hardware = true)
    private val softwareHevc = DecoderInfo(listOf("video/hevc"), hardware = false)
    private val aacAndOpus = DecoderInfo(listOf("audio/mp4a-latm", "audio/opus"), hardware = false)

    @Test
    fun `only video codecs with a hardware decoder count, and H264 always plays`() {
        assertEquals(listOf("h264"), DeviceCaps.videoDecoders(listOf(hardwareAvc, softwareHevc)))
        // No decoder flagged as hardware must not hide every release.
        assertEquals(listOf("h264"), DeviceCaps.videoDecoders(listOf(softwareHevc)))
        assertEquals(listOf("h264"), DeviceCaps.videoDecoders(emptyList()))
    }

    @Test
    fun `Dolby Vision needs a decoder for it, not only a display that shows it`() {
        val display = listOf("dolby_vision", "hdr10")
        assertEquals(listOf("hdr10"), DeviceCaps.withDolbyVision(display, dolbyVisionDecoder = false))
        assertEquals(display, DeviceCaps.withDolbyVision(display, dolbyVisionDecoder = true))
        val scan = DeviceCaps.scan(listOf(hardwareAvc, DecoderInfo(listOf("video/dolby-vision"), hardware = true)))
        assertEquals(true, scan.dolbyVision)
        assertEquals(false, DeviceCaps.scan(listOf(hardwareAvc)).dolbyVision)
    }

    @Test
    fun `ten bit HEVC needs a hardware decoder that takes Main 10`() {
        val main10 = DecoderInfo(listOf("video/hevc"), hardware = true, tenBitHevc = true)
        assertEquals(true, DeviceCaps.scan(listOf(main10)).tenBitHevc)
        assertEquals(false, DeviceCaps.scan(listOf(main10.copy(hardware = false))).tenBitHevc)
        assertEquals(false, DeviceCaps.scan(listOf(DecoderInfo(listOf("video/hevc"), hardware = true))).tenBitHevc)
    }

    @Test
    fun `the baseline is a plain stick with no fallback engine`() {
        val baseline = DeviceCaps.baseline()
        assertEquals(listOf("h264"), baseline.decoders)
        assertEquals(false, baseline.hasFallbackEngine)
    }

    @Test
    fun `Media3 plays HLS and DASH through the core, so the caps list both`() {
        val containers = DeviceCaps.baseline().containers.orEmpty()
        assertTrue("m3u8" in containers, "$containers")
        assertTrue("mpd" in containers, "$containers")
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
        // The old Java codes for Hebrew and Indonesian become what the core reads.
        assertEquals("he", DeviceCaps.deviceLanguage(Locale("iw", "IL")))
        assertEquals("id", DeviceCaps.deviceLanguage(Locale("in", "ID")))
    }

    @Test
    fun `the subtitle pickers take a three letter language too`() {
        // Cantonese, a tag the core lists; Filipino goes as it is, and the core reads it as Tagalog ("tl").
        assertEquals("yue", DeviceCaps.languageCode(Locale.forLanguageTag("yue-HK")))
        assertEquals("fil", DeviceCaps.languageCode(Locale.forLanguageTag("fil-PH")))
        assertEquals("he", DeviceCaps.languageCode(Locale("iw", "IL")))
        assertNull(DeviceCaps.languageCode(Locale.ROOT))
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

    @Test
    fun `the network type comes from the default network's transports`() {
        fun type(vararg on: Int) = DeviceCaps.networkType { it in on }
        assertEquals(Network.Wifi, type(NetworkCapabilities.TRANSPORT_WIFI))
        assertEquals(Network.Ethernet, type(NetworkCapabilities.TRANSPORT_ETHERNET))
        assertEquals(Network.Cellular, type(NetworkCapabilities.TRANSPORT_CELLULAR))
        // A VPN over Wi-Fi is Wi-Fi; a VPN alone or Bluetooth tethering is another network.
        assertEquals(Network.Wifi, type(NetworkCapabilities.TRANSPORT_VPN, NetworkCapabilities.TRANSPORT_WIFI))
        assertEquals(Network.Other, type(NetworkCapabilities.TRANSPORT_VPN))
        assertEquals(Network.Other, type(NetworkCapabilities.TRANSPORT_BLUETOOTH))
        assertEquals(
            "wifi",
            DeviceCapsParams(1080, emptyList(), network = Network.Wifi).toJson()["network"]?.let {
                (it as JsonPrimitive).content
            },
        )
    }

    @Test
    fun `each new default network is sent once with its type, the one at start too`() {
        val sent = mutableListOf<Network?>()
        val default = DefaultNetwork { type, _ -> sent += type }
        default.seen("net-1", Network.Wifi, false)
        default.seen("net-1", Network.Wifi, false)
        // The core starts each run on "other": it hears the network current at registration.
        assertEquals(listOf<Network?>(Network.Wifi), sent)
        default.seen("net-2", Network.Cellular, true)
        default.seen("net-2", Network.Cellular, true)
        default.seen("net-3", Network.Wifi, false)
        assertEquals(listOf<Network?>(Network.Wifi, Network.Cellular, Network.Wifi), sent)
    }

    @Test
    fun `a lost or blocked default network is none, and the cost flag says metered`() {
        val sent = mutableListOf<Pair<Network?, Boolean>>()
        val default = DefaultNetwork { type, metered -> sent += type to metered }
        // A phone's hotspot: Wi-Fi, metered by its cost flag.
        default.seen("net-1", Network.Wifi, true)
        default.seen("net-1", Network.Wifi, false)
        // Blocked for this app (the walk's package cut, Data Saver): none until it lifts.
        default.blocked("net-1", true)
        default.blocked("net-1", true)
        default.blocked("net-1", false)
        // A loss of another network is old news; the current one's is none.
        default.lost("net-0")
        default.lost("net-1")
        default.lost("net-1")
        default.seen("net-2", Network.Cellular, true)
        assertEquals(
            listOf(
                Network.Wifi to true,
                Network.Wifi to false,
                null to false,
                Network.Wifi to false,
                null to false,
                Network.Cellular to true,
            ),
            sent,
        )
    }
}
