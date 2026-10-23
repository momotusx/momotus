package app.marquee.player

import androidx.media3.common.C
import androidx.media3.common.PlaybackException
import androidx.media3.common.util.Util
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.model.Attempt
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.Segment
import app.marquee.core.model.SegmentKind
import app.marquee.core.model.SubtitleTrack

/** A position the core can store: both numbers are known and the position is inside the video. */
data class ProgressReport(
    val positionMs: Long,
    val durationMs: Long,
    /** The viewer paused, or it ended: Trakt hears a pause (plans/m7-trakt.md section 3). */
    val paused: Boolean = false,
)

/**
 * What to tell the core about where the player is, or null while there is nothing true to say.
 * The core takes unsigned integers and keeps the last report as the resume point, so a report is
 * held back until a frame has been shown (a Back during buffering sits at 0 and would wipe the
 * saved position) and until the duration is known (Media3 says `C.TIME_UNSET`, a large negative).
 */
fun progressReport(
    firstFrame: Boolean,
    positionMs: Long,
    durationMs: Long,
    playWhenReady: Boolean = true,
    ended: Boolean = false,
    /** Play is asked for but the system holds it (audio focus lost, output gone): nothing plays. */
    suppressed: Boolean = false,
): ProgressReport? {
    if (!firstFrame || positionMs < 0 || durationMs <= 0) return null
    // Buffering with play asked for is not a pause; held by the system is.
    return ProgressReport(positionMs.coerceAtMost(durationMs), durationMs, !playWhenReady || ended || suppressed)
}

/**
 * How far one D-pad seek press jumps. A held key grows the step as it repeats: 10 s for the first
 * presses, then 20, 30 and 60, so a two hour film can be crossed without hundreds of presses.
 *
 * @param repeat how many repeats of the held key have come in (0 for the first press).
 */
fun seekStepMs(repeat: Int): Long =
    when {
        repeat < 3 -> 10_000L
        repeat < 6 -> 20_000L
        repeat < 10 -> 30_000L
        else -> 60_000L
    }

/**
 * The position a touch at [x] on a bar [width] px wide stands for, or null while there is no length to
 * seek in (the first moments, or while a source is set again), where 0 would throw the resume point away.
 */
fun scrubTo(
    x: Float,
    width: Int,
    durationMs: Long,
): Long? = if (width <= 0 || durationMs <= 0) null else (x.coerceIn(0f, width.toFloat()) / width * durationMs).toLong()

/** How long before the end the Up Next card appears when the core does not know where the credits start. */
const val NEXT_AT_MS = 20_000L

/** A video this short (a trailer) never offers a next episode. */
const val NEXT_MIN_LENGTH_MS = 60_000L

/** The skip segment under the playhead: segments are active on `[startMs, endMs)`. */
fun activeSegment(
    segments: List<Segment>,
    positionMs: Long,
): Segment? = segments.firstOrNull { positionMs >= it.startMs && positionMs < it.endMs }

/** "Skip intro", "Skip recap", "Skip credits", "Skip preview". */
fun skipLabel(kind: SegmentKind): String =
    when (kind) {
        SegmentKind.Intro -> "Skip intro"

        SegmentKind.Recap -> "Skip recap"

        SegmentKind.Credits -> "Skip credits"

        SegmentKind.Preview -> "Skip preview"

        // A segment kind from a newer core.
        SegmentKind.Unknown -> "Skip"
    }

/** Credits that run to the end of the file lead into Up Next (or the end) instead of a seek. */
fun creditsToEnd(
    segment: Segment,
    durationMs: Long,
): Boolean = segment.kind == SegmentKind.Credits && durationMs > 0 && segment.endMs >= durationMs - 1_000L

/**
 * Whether the Up Next card shows. It needs a next episode and a video longer than a trailer; then it is due
 * from the credits start the core matched to this release ([nextAtMs]), else for the last [NEXT_AT_MS], and
 * at once after Skip credits was pressed at [skippedAtMs] (until the viewer seeks back before it).
 */
fun upNextDue(
    hasNext: Boolean,
    nextAtMs: Long?,
    positionMs: Long,
    durationMs: Long,
    skippedAtMs: Long? = null,
): Boolean {
    if (!hasNext || durationMs <= NEXT_MIN_LENGTH_MS) return false
    if (skippedAtMs != null && positionMs >= skippedAtMs) return true
    return if (nextAtMs != null) positionMs >= nextAtMs else durationMs - positionMs <= NEXT_AT_MS
}

/** How long a source that has what a first frame needs may go without one before it counts as one this device cannot decode. */
const val NO_PICTURE_MS = 4_000L

/** Media held ahead of the playhead that is more than a start needs ([PlayerHost] starts at 1 s and rebuffers to 3 s). */
const val HAS_DATA_MS = 3_000L

/**
 * The picture safety net, as on Apple: whether the player waits for a picture it has what it needs for. A
 * decoder that takes the video and gives no frame back raises no error, so the player would wait forever.
 * Counts before the first frame of a source with a video track while the viewer wants it playing, when the
 * player is ready (sound over black: the video is not decoded) or still buffering with [HAS_DATA_MS] held.
 */
fun waitsForPicture(
    firstFrame: Boolean,
    hasVideo: Boolean,
    wantPlay: Boolean,
    ready: Boolean,
    bufferedAheadMs: Long,
): Boolean = !firstFrame && hasVideo && wantPlay && (ready || bufferedAheadMs >= HAS_DATA_MS)

/** Times [waitsForPicture]: whether it has held for [NO_PICTURE_MS] without a break. */
class PictureWait {
    private var since = -1L

    fun outOfTime(
        waiting: Boolean,
        nowMs: Long,
    ): Boolean {
        if (!waiting) {
            since = -1L
            return false
        }
        if (since < 0) since = nowMs
        return nowMs - since >= NO_PICTURE_MS
    }

    fun reset() {
        since = -1L
    }
}

/**
 * The failure card's headline: when the core offers another version, only this one failed; otherwise every
 * source did. The same words on the web and Apple.
 */
fun failedHeadline(
    offered: Boolean,
    empty: EmptyKind? = null,
): String =
    when {
        // The core names why nothing played ("This didn't start" when its 90 s ran out): its words.
        empty != null -> StreamChoiceCopy.emptyLine(empty)

        offered -> "This version could not be played"

        else -> "None of the sources could be played"
    }

/** The headline when the core could not open the version at all (no longer listed), as on Apple. */
const val CANNOT_OPEN = "This version could not be opened"

/** What the player says while the core plays the same versions again (Try again). */
const val TRYING_AGAIN = "Trying again"

/** Whether the core took a Try again since [seen] (the count of the last snapshot; null before the first one). */
fun retriedSince(
    seen: Int?,
    retries: Int,
): Boolean = seen != null && retries > seen

/** What failed, as `failOver` reports it: the core words it for the attempt list. */
enum class FailureKind(
    val wire: String,
) {
    Http("http"),
    Network("network"),
    Timeout("timeout"),
    Unreadable("unreadable"),
    Decode("decode"),
    Audio("audio"),
    Other("other"),
}

/** A playback error: what failed, the HTTP status when there was one, and a sentence for the player's own status line. */
data class Failure(
    val kind: FailureKind,
    val message: String,
    val status: Int? = null,
)

/**
 * A source's error status in the core's words (mq-server `status_words`), so the line shown while the
 * next source opens matches the failure history the core keeps.
 */
fun statusWords(status: Int): String {
    val words =
        when (status) {
            404, 410 -> "The link no longer exists"
            401, 403 -> "The source refused access"
            429 -> "The source is busy, try again soon"
            in 500..599 -> "The source had an error"
            else -> "The source answered with an error"
        }
    return "$words ($status)"
}

/**
 * Sorts a Media3 error code into what failed, with a sentence a viewer can read. [formatSupport] is the
 * renderer's support for the file when a renderer raised it (`ExoPlaybackException.rendererFormatSupport`):
 * a decoder that did not start counts against the file only when the file is past this decoder.
 */
fun classifyFailure(
    code: Int,
    httpStatus: Int?,
    formatSupport: Int? = null,
): Failure =
    when (code) {
        PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS -> {
            Failure(
                FailureKind.Http,
                httpStatus?.let(::statusWords) ?: "The source refused the request",
                httpStatus,
            )
        }

        PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND,
        PlaybackException.ERROR_CODE_IO_NO_PERMISSION,
        PlaybackException.ERROR_CODE_IO_INVALID_HTTP_CONTENT_TYPE,
        PlaybackException.ERROR_CODE_IO_CLEARTEXT_NOT_PERMITTED,
        -> {
            Failure(FailureKind.Unreadable, "The source could not be opened")
        }

        PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED -> {
            Failure(FailureKind.Network, "The connection to the source failed")
        }

        PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT,
        PlaybackException.ERROR_CODE_TIMEOUT,
        -> {
            Failure(FailureKind.Timeout, "The connection to the source failed")
        }

        PlaybackException.ERROR_CODE_PARSING_CONTAINER_MALFORMED,
        PlaybackException.ERROR_CODE_PARSING_MANIFEST_MALFORMED,
        PlaybackException.ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED,
        PlaybackException.ERROR_CODE_PARSING_MANIFEST_UNSUPPORTED,
        -> {
            Failure(FailureKind.Unreadable, "The file could not be read")
        }

        // Only a decoder that cannot decode this file is decode, which the core keeps against the version
        // (plans/reviews/stream-runtime-2026-10-20-r1.md P2-2).
        PlaybackException.ERROR_CODE_DECODING_FAILED,
        PlaybackException.ERROR_CODE_DECODING_FORMAT_EXCEEDS_CAPABILITIES,
        PlaybackException.ERROR_CODE_DECODING_FORMAT_UNSUPPORTED,
        -> {
            Failure(FailureKind.Decode, "This device cannot decode the video")
        }

        // A decoder that did not start for a file past it (Sintel's trailer as 8K H.264, walk NV-W1) is decode;
        // for a file it handles, the decoder was busy (plans/reviews/stream-runtime-2026-10-20-r2.md P1-1).
        PlaybackException.ERROR_CODE_DECODER_INIT_FAILED -> {
            if (formatSupport != null && formatSupport != C.FORMAT_HANDLED) {
                Failure(FailureKind.Decode, "This device cannot decode the video")
            } else {
                Failure(FailureKind.Other, "The video decoder is busy or could not start")
            }
        }

        // A decoder that could not be asked or was taken back by the system (another app holds it): the
        // moment, not the file.
        PlaybackException.ERROR_CODE_DECODER_QUERY_FAILED,
        PlaybackException.ERROR_CODE_DECODING_RESOURCES_RECLAIMED,
        -> {
            Failure(FailureKind.Other, "The video decoder is busy or could not start")
        }

        PlaybackException.ERROR_CODE_AUDIO_TRACK_INIT_FAILED,
        PlaybackException.ERROR_CODE_AUDIO_TRACK_WRITE_FAILED,
        PlaybackException.ERROR_CODE_AUDIO_TRACK_OFFLOAD_INIT_FAILED,
        PlaybackException.ERROR_CODE_AUDIO_TRACK_OFFLOAD_WRITE_FAILED,
        -> {
            Failure(FailureKind.Audio, "The audio output failed")
        }

        else -> {
            Failure(FailureKind.Other, "Playback stopped unexpectedly")
        }
    }

/** The subtitle MIME type from the end of the URL's path; addon URLs often have none, which means SRT. */
fun subtitleMime(url: String): String {
    val path = url.substringBefore('?').substringBefore('#').lowercase()
    return when {
        path.endsWith(".vtt") -> "text/vtt"
        path.endsWith(".ass") || path.endsWith(".ssa") -> "text/x-ssa"
        path.endsWith(".ttml") || path.endsWith(".dfxp") || path.endsWith(".xml") -> "application/ttml+xml"
        else -> "application/x-subrip"
    }
}

/**
 * The addon file a new source carries the viewer's addon pick over to: its own file, else the best one in its
 * language; null when the new source has neither.
 */
fun carriedSubtitle(
    subs: List<SubtitleTrack>,
    pickLanguage: String?,
    pickUrl: String?,
): SubtitleTrack? = subs.firstOrNull { pickUrl != null && it.url == pickUrl } ?: bestInLanguage(subs, pickLanguage)

/**
 * The best track in [language]: the core lists each language's tracks in its own ranking (addon order),
 * and a full regular file beats a hearing-impaired one. A forced file (only the lines in other languages) is
 * never picked for the viewer. Ties keep the core's order.
 */
fun bestInLanguage(
    subs: List<SubtitleTrack>,
    language: String?,
): SubtitleTrack? {
    if (language.isNullOrEmpty()) return null
    return subs
        .filter { sameLanguage(it.lang, language) && !it.forced }
        .minByOrNull { if (it.hearingImpaired) 1 else 0 }
}

/** A subtitle the viewer chose, as [findPick] looks for it again when the tracks change. */
data class TextPick(
    val language: String,
    val external: Boolean,
    /** The addon file's address. */
    val url: String? = null,
    /**
     * The track group's id: the same item keeps it, so the one track chosen stays on even when another
     * in its language comes first (English captions in the video and an English WebVTT rendition).
     */
    val group: String? = null,
    /** The track's type (`text/vtt`, `application/cea-608`), to tell such tracks apart in a new item. */
    val kind: String? = null,
)

/** A text track of the playing item, for [findPick]. */
data class TextCandidate(
    val external: Boolean,
    val url: String?,
    val group: String,
    val kind: String?,
    val language: String?,
)

/**
 * Which of [found] carries the viewer's [pick] after the tracks change: the addon file they chose, else
 * the very track (same group), else one in their language of the same type and origin, else one of the
 * same origin, else any in their language. Null when none fits.
 */
fun findPick(
    pick: TextPick,
    found: List<TextCandidate>,
): Int? {
    val same = { t: TextCandidate ->
        pick.language.isNotEmpty() &&
            t.language?.let { sameLanguage(it, pick.language) } == true
    }
    val rules =
        listOf<(TextCandidate) -> Boolean>(
            { pick.url != null && it.url == pick.url },
            { pick.group != null && it.group == pick.group },
            { same(it) && it.external == pick.external && it.kind == pick.kind },
            { same(it) && it.external == pick.external },
            same,
        )
    for (rule in rules) {
        val at = found.indexOfFirst(rule)
        if (at >= 0) return at
    }
    return null
}

/** "eng" and "en" are the same language. */
fun sameLanguage(
    a: String,
    b: String,
): Boolean = Util.normalizeLanguageCode(a) == Util.normalizeLanguageCode(b)

/** Sources that failed the same way, from one addon, said once. */
data class FailedSources(
    val via: String,
    val error: String,
    val count: Int,
) {
    /** "The link no longer exists (404) · 8 sources". */
    val words: String get() = if (count > 1) "$error · $count sources" else error
}

/** The failure history with each addon and reason once, in the order they first failed. */
fun groupFailures(attempts: List<Attempt>): List<FailedSources> =
    attempts
        .groupBy { it.via to it.error }
        .map { (reason, all) -> FailedSources(reason.first, reason.second, all.size) }
