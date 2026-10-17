package app.marquee.player

import androidx.media3.common.PlaybackException

/** A position the core can store: both numbers are known and the position is inside the video. */
data class ProgressReport(
    val positionMs: Long,
    val durationMs: Long,
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
): ProgressReport? {
    if (!firstFrame || positionMs < 0 || durationMs <= 0) return null
    return ProgressReport(positionMs.coerceAtMost(durationMs), durationMs)
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

enum class FailureKind { Network, Source, Decoder, Audio }

/** A playback error in a plain sentence, and which part failed (the core only needs the sentence). */
data class Failure(
    val kind: FailureKind,
    val message: String,
)

/** Sorts a Media3 error code into the part that failed, with a sentence a viewer can read. */
fun classifyFailure(
    code: Int,
    httpStatus: Int?,
): Failure =
    when (code) {
        PlaybackException.ERROR_CODE_IO_BAD_HTTP_STATUS -> {
            Failure(
                FailureKind.Source,
                httpStatus?.let { "The source answered with HTTP $it" } ?: "The source refused the request",
            )
        }

        PlaybackException.ERROR_CODE_IO_FILE_NOT_FOUND,
        PlaybackException.ERROR_CODE_IO_NO_PERMISSION,
        PlaybackException.ERROR_CODE_IO_INVALID_HTTP_CONTENT_TYPE,
        PlaybackException.ERROR_CODE_IO_CLEARTEXT_NOT_PERMITTED,
        -> {
            Failure(FailureKind.Source, "The source could not be opened")
        }

        PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_FAILED,
        PlaybackException.ERROR_CODE_IO_NETWORK_CONNECTION_TIMEOUT,
        PlaybackException.ERROR_CODE_TIMEOUT,
        -> {
            Failure(FailureKind.Network, "The connection to the source failed")
        }

        PlaybackException.ERROR_CODE_PARSING_CONTAINER_MALFORMED,
        PlaybackException.ERROR_CODE_PARSING_MANIFEST_MALFORMED,
        PlaybackException.ERROR_CODE_PARSING_CONTAINER_UNSUPPORTED,
        PlaybackException.ERROR_CODE_PARSING_MANIFEST_UNSUPPORTED,
        -> {
            Failure(FailureKind.Source, "The file could not be read")
        }

        PlaybackException.ERROR_CODE_DECODER_INIT_FAILED,
        PlaybackException.ERROR_CODE_DECODER_QUERY_FAILED,
        PlaybackException.ERROR_CODE_DECODING_FAILED,
        PlaybackException.ERROR_CODE_DECODING_FORMAT_EXCEEDS_CAPABILITIES,
        PlaybackException.ERROR_CODE_DECODING_FORMAT_UNSUPPORTED,
        -> {
            Failure(FailureKind.Decoder, "This device cannot decode the video")
        }

        PlaybackException.ERROR_CODE_AUDIO_TRACK_INIT_FAILED,
        PlaybackException.ERROR_CODE_AUDIO_TRACK_WRITE_FAILED,
        PlaybackException.ERROR_CODE_AUDIO_TRACK_OFFLOAD_INIT_FAILED,
        PlaybackException.ERROR_CODE_AUDIO_TRACK_OFFLOAD_WRITE_FAILED,
        -> {
            Failure(FailureKind.Audio, "The audio output failed")
        }

        else -> {
            Failure(FailureKind.Source, "Playback stopped unexpectedly")
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
