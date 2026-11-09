package app.marquee.player

import androidx.media3.common.C
import androidx.media3.common.PlaybackException
import app.marquee.core.model.Segment
import app.marquee.core.model.SegmentKind
import app.marquee.core.model.SubtitleTrack
import app.marquee.core.model.TraktPlayback
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class PlayerLogicTest {
    @Test
    fun `the Trakt notice starts only for reconnect, once per play`() {
        // Every other state shows nothing in the player (plans/player-bar.md D9); Settings, Trakt keeps them.
        val quiet =
            listOf(null, TraktPlayback.On, TraktPlayback.Off, TraktPlayback.Unmapped, TraktPlayback.ThisDeviceOff)
        for (state in quiet) {
            assertNull(traktNoticeSince(null, state, nowMs = 1_000), "$state started the notice")
        }
        val since = traktNoticeSince(null, TraktPlayback.Reconnect, nowMs = 1_000)
        assertEquals(1_000L, since)
        assertEquals(4_000L, TRAKT_NOTICE_MS)
        // A pause, a seek, a resume or a later state keeps the first time: never shown again.
        assertEquals(since, traktNoticeSince(since, TraktPlayback.Off, nowMs = 60_000))
        assertEquals(since, traktNoticeSince(since, TraktPlayback.Reconnect, nowMs = 120_000))
    }

    @Test
    fun `the core's start audio goes before the device's language`() {
        assertEquals(listOf("ja", "en"), audioLanguages(core = listOf("ja"), device = listOf("en")))
        assertEquals(listOf("en"), audioLanguages(core = listOf("en"), device = listOf("en")))
        assertEquals(listOf("tr", "en", "nl"), audioLanguages(core = listOf("tr", "en"), device = listOf("nl")))
        assertEquals(listOf("en"), audioLanguages(core = emptyList(), device = listOf("en")))
    }

    @Test
    fun `a saved position the core takes away sends the file to its start`() {
        assertEquals(true, resumeWithdrawn(seen = 300_000L, now = null))
        assertEquals(false, resumeWithdrawn(seen = null, now = null))
        assertEquals(false, resumeWithdrawn(seen = 300_000L, now = 300_000L))
    }

    @Test
    fun `a Try again counts from the first snapshot on`() {
        assertEquals(false, retriedSince(seen = null, retries = 3))
        assertEquals(true, retriedSince(seen = 0, retries = 1))
        assertEquals(false, retriedSince(seen = 1, retries = 1))
    }

    @Test
    fun `a decoder that did not start counts against the file only when the file is past it`() {
        val init = PlaybackException.ERROR_CODE_DECODER_INIT_FAILED
        // Sintel's trailer as 8K H.264 (walk NV-W1): past this device's decoders.
        assertEquals("decode", classifyFailure(init, null, C.FORMAT_EXCEEDS_CAPABILITIES).kind.wire)
        assertEquals("decode", classifyFailure(init, null, C.FORMAT_UNSUPPORTED_SUBTYPE).kind.wire)
        // A file this decoder takes: it was busy, the moment and not the file.
        assertEquals("other", classifyFailure(init, null, C.FORMAT_HANDLED).kind.wire)
        assertEquals("other", classifyFailure(init, null).kind.wire)
    }

    @Test
    fun `the failure headline says one version failed when another is offered, else every source`() {
        assertEquals("This version could not be played", failedHeadline(offered = true))
        assertEquals("None of the sources could be played", failedHeadline(offered = false))
    }

    @Test
    fun `nothing is reported before a frame was shown or while the length is unknown`() {
        assertNull(progressReport(firstFrame = false, positionMs = 5_000, durationMs = 60_000))
        assertNull(progressReport(firstFrame = true, positionMs = 5_000, durationMs = C.TIME_UNSET))
        assertNull(progressReport(firstFrame = true, positionMs = 5_000, durationMs = 0))
        assertNull(progressReport(firstFrame = true, positionMs = -1, durationMs = 60_000))
    }

    @Test
    fun `a report says when the viewer paused or it ended, and buffering is not a pause`() {
        assertEquals(false, progressReport(true, 5_000, 60_000, playWhenReady = true)?.paused)
        assertEquals(true, progressReport(true, 5_000, 60_000, playWhenReady = false)?.paused)
        assertEquals(true, progressReport(true, 60_000, 60_000, playWhenReady = true, ended = true)?.paused)
        // Play asked for, held by the system (audio focus lost): nothing plays, so Trakt hears a pause.
        assertEquals(true, progressReport(true, 5_000, 60_000, playWhenReady = true, suppressed = true)?.paused)
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
    fun `a held key steps further`() {
        assertEquals(10_000, seekStepMs(0))
        assertEquals(10_000, seekStepMs(2))
        assertEquals(20_000, seekStepMs(3))
        assertEquals(30_000, seekStepMs(6))
        assertEquals(60_000, seekStepMs(10))
        assertEquals(60_000, seekStepMs(400))
    }

    @Test
    fun `errors are sorted into what failed, with the status and a sentence`() {
        assertEquals(
            Failure(FailureKind.Http, "The link no longer exists", 404),
            classifyFailure(PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS, 404),
        )
        val kinds =
            mapOf(
                PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED to "network",
                PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT to "timeout",
                PlaybackException.ERROR_CODE_TIMEOUT to "timeout",
                PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND to "unreadable",
                PlaybackException.ERROR_CODE_IO_INVALID_HTTP_CONTENT_TYPE to "unreadable",
                PlaybackException.ERROR_CODE_PARSING_CONTAINER_MALFORMED to "unreadable",
                PlaybackException.ERROR_CODE_PARSING_MANIFEST_UNSUPPORTED to "unreadable",
                // Only a decoder that cannot decode this file is decode; a busy or unstarted one is the moment.
                PlaybackException.ERROR_CODE_DECODING_FAILED to "decode",
                PlaybackException.ERROR_CODE_DECODING_FORMAT_UNSUPPORTED to "decode",
                PlaybackException.ERROR_CODE_DECODING_FORMAT_EXCEEDS_CAPABILITIES to "decode",
                PlaybackException.ERROR_CODE_DECODER_INIT_FAILED to "other",
                PlaybackException.ERROR_CODE_DECODER_QUERY_FAILED to "other",
                PlaybackException.ERROR_CODE_DECODING_RESOURCES_RECLAIMED to "other",
                PlaybackException.ERROR_CODE_AUDIO_TRACK_INIT_FAILED to "audio",
                PlaybackException.ERROR_CODE_UNSPECIFIED to "other",
            )
        for ((code, wire) in kinds) {
            val failure = classifyFailure(code, null)
            assertEquals(wire, failure.kind.wire, PlaybackException.getErrorCodeName(code))
            assertEquals(null, failure.status)
        }
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
    fun `an audio track that names no language reads Audio alone, by its place among several`() {
        // The shared player words (apps/shared/stream-choice-words.json), never "Original": a dub is not.
        val untagged =
            androidx.media3.common.Format
                .Builder()
                .build()
        assertEquals("Audio", audioChoice("a", untagged, false).name)
        assertEquals("Audio 2", audioChoice("a", untagged, false, index = 1, count = 3).name)
        val english =
            androidx.media3.common.Format
                .Builder()
                .setLanguage("en")
                .build()
        assertEquals("English", audioChoice("a", english, false, index = 0, count = 3).name)
    }

    @Test
    fun `the speed button steps through its list and comes back to 1x`() {
        assertEquals(1.25f, nextSpeed(1f))
        assertEquals(0.75f, nextSpeed(2f))
        assertEquals(1f, nextSpeed(0.75f))
        assertEquals("1x", speedText(1f))
        assertEquals("1.25x", speedText(1.25f))
    }

    private val intro = Segment(SegmentKind.Intro, 30_000, 75_000, "TheIntroDB")
    private val credits = Segment(SegmentKind.Credits, 2_400_000, 2_520_000, "TheIntroDB")

    @Test
    fun `a segment is active from its start up to but not including its end`() {
        val all = listOf(intro, credits)
        assertNull(activeSegment(all, 29_999))
        assertEquals(intro, activeSegment(all, 30_000))
        assertEquals(intro, activeSegment(all, 74_999))
        assertNull(activeSegment(all, 75_000))
        assertEquals(credits, activeSegment(all, 2_450_000))
        assertNull(activeSegment(emptyList(), 40_000))
    }

    @Test
    fun `the skip buttons are named after what they skip`() {
        assertEquals("Skip intro", skipLabel(SegmentKind.Intro))
        assertEquals("Skip recap", skipLabel(SegmentKind.Recap))
        assertEquals("Skip credits", skipLabel(SegmentKind.Credits))
        assertEquals("Skip preview", skipLabel(SegmentKind.Preview))
    }

    @Test
    fun `credits that reach the end of the file lead into Up Next`() {
        assertEquals(true, creditsToEnd(credits, 2_520_500))
        assertEquals(false, creditsToEnd(credits, 3_000_000))
        assertEquals(false, creditsToEnd(intro, 100_000))
        assertEquals(false, creditsToEnd(credits, 0))
    }

    @Test
    fun `Up Next is due at the matched credits start, else in the last twenty seconds, or after Skip credits`() {
        val hour = 3_600_000L
        // The core knows where the credits start: due from there, not before.
        assertEquals(false, upNextDue(true, 3_400_000, 3_399_999, hour))
        assertEquals(true, upNextDue(true, 3_400_000, 3_400_000, hour))
        // The core does not know: the last 20 s.
        assertEquals(false, upNextDue(true, null, hour - 21_000, hour))
        assertEquals(true, upNextDue(true, null, hour - 20_000, hour))
        // Skip credits at 3,400 s: due at once, and not any more after seeking back before it.
        assertEquals(true, upNextDue(true, null, 3_401_000, hour, skippedAtMs = 3_400_000))
        assertEquals(false, upNextDue(true, null, 3_000_000, hour, skippedAtMs = 3_400_000))
        // No next episode, or a trailer: never.
        assertEquals(false, upNextDue(false, 3_400_000, hour - 1_000, hour))
        assertEquals(false, upNextDue(true, null, 55_000, 60_000))
    }

    @Test
    fun `the end time follows the speed and waits for the length`() {
        assertNull(endsAt(1_000, 0, 0, 1f))
        assertEquals(1_000 + 60_000L, endsAt(1_000, 40_000, 100_000, 1f))
        assertEquals(1_000 + 30_000L, endsAt(1_000, 40_000, 100_000, 2f))
    }

    @Test
    fun `sources that failed alike are said once with their count, in first order`() {
        val gone = "The link no longer exists"
        val slow = "The source did not answer in time"
        val attempts =
            listOf(
                app.marquee.core.model
                    .Attempt(key = "a", via = "Torrentio", error = slow),
                app.marquee.core.model
                    .Attempt(key = "b", via = "Torrentio", error = gone),
                app.marquee.core.model
                    .Attempt(key = "c", via = "Local", error = gone),
                app.marquee.core.model
                    .Attempt(key = "d", via = "Torrentio", error = gone),
            )
        val grouped = groupFailures(attempts)
        // Said by reason alone: no addon's name where a viewer reads it.
        assertEquals(listOf(slow, "$gone · 3 sources"), grouped.map { it.words })
    }

    @Test
    fun `status words match the core's, with no status code`() {
        assertEquals("The link no longer exists", statusWords(410))
        assertEquals("The source refused access", statusWords(403))
        assertEquals("The source is busy, try again soon", statusWords(429))
        assertEquals("The source had an error", statusWords(502))
        assertEquals("The source answered with an error", statusWords(400))
        // The owner's rule: the line under the player never shows the number.
        for (status in listOf(400, 401, 403, 404, 410, 429, 451, 500, 503, 599)) {
            assertFalse(statusWords(status).any(Char::isDigit), statusWords(status))
        }
    }

    @Test
    fun `the subtitle chosen stays on when another in its language comes first`() {
        val captions =
            TextCandidate(
                external = false,
                url = null,
                group = "main:cc",
                kind = "application/cea-608",
                language = "en",
            )
        val webvtt =
            TextCandidate(external = false, url = null, group = "subtitle:English", kind = "text/vtt", language = "en")
        val addon = TextCandidate(external = true, url = "u1", group = "1:0", kind = "text/vtt", language = "en")
        val pick = TextPick("en", external = false, group = "subtitle:English", kind = "text/vtt")
        assertEquals(1, findPick(pick, listOf(captions, webvtt, addon)))
        // A new item renames the groups: the same type and origin in that language still wins.
        val renamed = listOf(captions.copy(group = "0:main:cc"), webvtt.copy(group = "0:subtitle:English"), addon)
        assertEquals(1, findPick(pick, renamed))
        // An addon file is found by its address before anything else.
        assertEquals(2, findPick(TextPick("en", external = true, url = "u1"), listOf(captions, webvtt, addon)))
        assertEquals(0, findPick(TextPick("eng", external = false), listOf(captions, webvtt)))
        assertNull(findPick(TextPick("fr", external = false), listOf(captions, webvtt)))
        // Two English WebVTT renditions (one for the hard of hearing): the chosen group, not the first.
        val sdh = webvtt.copy(group = "subtitle:English SDH")
        assertEquals(1, findPick(pick.copy(group = "subtitle:English SDH"), listOf(webvtt, sdh)))
    }

    @Test
    fun `a new source carries the addon pick over to its file, else the best one in its language`() {
        fun sub(
            url: String,
            lang: String,
        ) = SubtitleTrack(url = url, lang = lang, source = "OpenSubtitles", forced = false, hearingImpaired = false)
        val subs = listOf(sub("a", "pob"), sub("b", "eng"), sub("c", "eng"), sub("d", "fre"))
        assertNull(carriedSubtitle(subs, pickLanguage = null, pickUrl = null))
        assertNull(carriedSubtitle(subs, pickLanguage = "", pickUrl = null))
        assertEquals(subs[2], carriedSubtitle(subs, pickLanguage = "en", pickUrl = "c"))
        assertEquals(subs[1], carriedSubtitle(subs, pickLanguage = "en", pickUrl = "gone"))
        assertNull(carriedSubtitle(subs, pickLanguage = "de", pickUrl = "gone"))
    }

    @Test
    fun `a carried language takes its best file, not whatever came first`() {
        fun sub(
            url: String,
            lang: String,
            forced: Boolean = false,
            hearingImpaired: Boolean = false,
        ) = SubtitleTrack(
            url = url,
            lang = lang,
            source = "SubSense",
            forced = forced,
            hearingImpaired = hearingImpaired,
        )
        // The core's order within Turkish: a forced file first, then a hearing-impaired one, then two full files.
        val subs =
            listOf(
                sub("eng", "eng"),
                sub("forced", "tur", forced = true),
                sub("hi", "tur", hearingImpaired = true),
                sub("full1", "tur"),
                sub("full2", "tur"),
            )
        assertEquals(subs[3], carriedSubtitle(subs, pickLanguage = "tr", pickUrl = "gone"))
        // With no full file, a hearing-impaired one beats a forced one.
        assertEquals(subs[2], bestInLanguage(subs.take(3), "tur"))
        // A forced file is never picked for the viewer: with nothing else, none.
        assertNull(bestInLanguage(subs.take(2), "tur"))
        assertNull(bestInLanguage(subs, ""))
    }

    @Test
    fun `the picture is waited for only before a first frame of a video the viewer wants, with what it needs`() {
        // Ready, or buffering with more than a start needs held: a frame should have come.
        assertTrue(
            waitsForPicture(firstFrame = false, hasVideo = true, wantPlay = true, ready = true, bufferedAheadMs = 0),
        )
        assertTrue(waitsForPicture(false, true, true, ready = false, bufferedAheadMs = HAS_DATA_MS))
        // Still loading what a start needs: the connection, not the decoder.
        assertFalse(waitsForPicture(false, true, true, ready = false, bufferedAheadMs = HAS_DATA_MS - 1))
        assertFalse(
            waitsForPicture(firstFrame = true, hasVideo = true, wantPlay = true, ready = true, bufferedAheadMs = 0),
        )
        assertFalse(waitsForPicture(false, hasVideo = false, wantPlay = true, ready = true, bufferedAheadMs = 0))
        assertFalse(waitsForPicture(false, true, wantPlay = false, ready = true, bufferedAheadMs = 0))
    }

    @Test
    fun `the wait runs out after it held without a break`() {
        val wait = PictureWait()
        assertFalse(wait.outOfTime(true, 1_000))
        assertFalse(wait.outOfTime(true, 1_000 + NO_PICTURE_MS - 1))
        assertTrue(wait.outOfTime(true, 1_000 + NO_PICTURE_MS))
        // A break starts it over.
        assertFalse(wait.outOfTime(false, 6_000))
        assertFalse(wait.outOfTime(true, 7_000))
        assertFalse(wait.outOfTime(true, 7_000 + NO_PICTURE_MS - 1))
        wait.reset()
        assertFalse(wait.outOfTime(true, 20_000))
    }
}
