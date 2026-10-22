package app.marquee.player

import androidx.media3.common.MimeTypes
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config
import kotlin.test.assertEquals

/** An addon file as the core's loopback serves it (WebVTT), parsed once and drawn by the app. */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class AddonCuesTest {
    private val vtt =
        """
        WEBVTT

        00:00:01.000 --> 00:00:03.000
        One

        00:00:02.500 --> 00:00:04.000
        Two
        """.trimIndent()

    private fun AddonCues.textAt(ms: Long) = at(ms * 1000).map { it.text.toString() }.sorted()

    @Test
    fun `what shows at a moment, overlapping cues together, nothing between or after`() {
        val cues = parseAddonCues(vtt.toByteArray(), MimeTypes.TEXT_VTT)
        assertEquals(emptyList(), cues.textAt(500))
        assertEquals(listOf("One"), cues.textAt(1_000))
        assertEquals(listOf("One", "Two"), cues.textAt(2_700))
        assertEquals(listOf("Two"), cues.textAt(3_000))
        assertEquals(emptyList(), cues.textAt(4_000))
        assertEquals(emptyList(), cues.textAt(-1_000))
    }
}
