package app.marquee.player

import androidx.media3.common.C
import androidx.media3.common.PlaybackException
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class PlayerLogicTest {
    @Test
    fun `nothing is reported before a frame was shown or while the length is unknown`() {
        assertNull(progressReport(firstFrame = false, positionMs = 5_000, durationMs = 60_000))
        assertNull(progressReport(firstFrame = true, positionMs = 5_000, durationMs = C.TIME_UNSET))
        assertNull(progressReport(firstFrame = true, positionMs = 5_000, durationMs = 0))
        assertNull(progressReport(firstFrame = true, positionMs = -1, durationMs = 60_000))
    }

    @Test
    fun `a report keeps the position inside the video`() {
        assertEquals(ProgressReport(5_000, 60_000), progressReport(true, 5_000, 60_000))
        assertEquals(ProgressReport(60_000, 60_000), progressReport(true, 61_500, 60_000))
    }

    @Test
    fun `a touch on the bar maps to a position, and to none while the length is unknown`() {
        assertEquals(30_000L, scrubTo(500f, 1000, 60_000))
        assertEquals(60_000L, scrubTo(1500f, 1000, 60_000))
        assertEquals(0L, scrubTo(-20f, 1000, 60_000))
        assertNull(scrubTo(500f, 1000, 0))
        assertNull(scrubTo(500f, 0, 60_000))
    }

    @Test
    fun `a scrim is dark near its edge and holds the dark away from it`() {
        assertEquals(0.85f, scrimAlpha(top = false, peak = 0.85f, at = 1f), 0.001f)
        assertEquals(0f, scrimAlpha(top = false, peak = 0.85f, at = 0f), 0.001f)
        assertEquals(0.85f, scrimAlpha(top = true, peak = 0.85f, at = 0f), 0.001f)
        assertEquals(0f, scrimAlpha(top = true, peak = 0.85f, at = 1f), 0.001f)
        // A quarter of the way up from the foot it is already at 80% of its peak, which is what keeps
        // the position bar readable over a white picture (scripts/player-contrast.ts).
        assertEquals(0.68f, scrimAlpha(top = false, peak = 0.85f, at = 0.25f), 0.001f)
        assertTrue(scrimAlpha(top = false, peak = 0.85f, at = 0.5f) > scrimAlpha(top = false, peak = 0.85f, at = 0.25f))
    }

    @Test
    fun `source failures are put plainly`() {
        assertEquals("The link no longer exists (404)", attemptReason("source did not answer: http status 404"))
        assertEquals("The source refused the request (503)", attemptReason("source did not answer: http status 503"))
        assertEquals("No answer in 8 seconds", attemptReason("source did not answer: timed out after 8s"))
        assertEquals("No answer in time", attemptReason("source did not answer in time"))
        assertEquals("something else", attemptReason("something else"))
    }

    @Test
    fun `a held key steps further`() {
        assertEquals(10_000, seekStepMs(0))
        assertEquals(10_000, seekStepMs(2))
        assertEquals(20_000, seekStepMs(3))
        assertEquals(30_000, seekStepMs(6))
        assertEquals(60_000, seekStepMs(10))
        assertEquals(60_000, seekStepMs(400))
    }

    @Test
    fun `errors are sorted by the part that failed, with a sentence`() {
        assertEquals(
            Failure(FailureKind.Source, "The source answered with HTTP 404"),
            classifyFailure(PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS, 404),
        )
        assertEquals(
            FailureKind.Network,
            classifyFailure(PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT, null).kind,
        )
        assertEquals(
            FailureKind.Source,
            classifyFailure(PlaybackException.ERROR_CODE_PARSING_CONTAINER_MALFORMED, null).kind,
        )
        assertEquals(FailureKind.Decoder, classifyFailure(PlaybackException.ERROR_CODE_DECODER_INIT_FAILED, null).kind)
        assertEquals(
            FailureKind.Audio,
            classifyFailure(PlaybackException.ERROR_CODE_AUDIO_TRACK_INIT_FAILED, null).kind,
        )
        assertEquals(
            "Playback stopped unexpectedly",
            classifyFailure(PlaybackException.ERROR_CODE_UNSPECIFIED, null).message,
        )
    }

    @Test
    fun `a subtitle type comes from the end of the path and defaults to SRT`() {
        assertEquals("text/vtt", subtitleMime("http://127.0.0.1:1/x/en.vtt?lang=en"))
        assertEquals("text/x-ssa", subtitleMime("https://a/b.ASS"))
        assertEquals("application/x-subrip", subtitleMime("https://a/b.srt"))
        assertEquals("application/x-subrip", subtitleMime("https://a/subtitles/12345"))
    }

    @Test
    fun `codecs and channel layouts read as people say them`() {
        assertEquals("AC-3", codecName("audio/ac3"))
        assertEquals("SRT", codecName("application/x-subrip"))
        assertNull(codecName("audio/unknown"))
        assertEquals("5.1", channelLayout(6))
        assertEquals("stereo", channelLayout(2))
        assertNull(channelLayout(androidx.media3.common.Format.NO_VALUE))
    }

    @Test
    fun `the speed button steps through its list and comes back to 1x`() {
        assertEquals(1.25f, nextSpeed(1f))
        assertEquals(0.75f, nextSpeed(2f))
        assertEquals(1f, nextSpeed(0.75f))
        assertEquals("1x", speedText(1f))
        assertEquals("1.25x", speedText(1.25f))
    }

    @Test
    fun `the end time follows the speed and waits for the length`() {
        assertNull(endsAt(1_000, 0, 0, 1f))
        assertEquals(1_000 + 60_000L, endsAt(1_000, 40_000, 100_000, 1f))
        assertEquals(1_000 + 30_000L, endsAt(1_000, 40_000, 100_000, 2f))
    }
}
