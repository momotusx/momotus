package app.marquee.player

import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.util.Util
import app.marquee.core.model.SubtitleNote
import app.marquee.core.model.SubtitleProblem
import java.util.Locale

/** One row of the audio or subtitles menu. */
data class TrackChoice(
    val id: String,
    val name: String,
    /** Under the name: codec, channels, source, or why choosing it takes a moment. */
    val detail: String? = null,
    val selected: Boolean = false,
)

const val TRACK_OFF = "off"

/** What the Subtitles action says under its name: the chosen language, or how many were found ("None found" for zero). */
fun PlaybackController.subtitlesDetail(): String {
    val found = text.size - 1
    return text.firstOrNull { it.selected && it.id != TRACK_OFF }?.name
        ?: if (subtitlesPending) {
            "Searching"
        } else if (found == 0) {
            "None found"
        } else {
            "Off, $found found"
        }
}

/** "Still asking subtitle addons": under the subtitle list while the core still waits for some. */
const val SUBTITLES_PENDING = "Still asking subtitle addons"

/**
 * What a subtitle note says (plans/m13-turkcealtyazi.md 4.2, the web's `subtitleNote`): a subtitle
 * source that gave nothing this time, and why. A problem from a newer core gets the general line.
 */
fun subtitleNoteWords(note: SubtitleNote): String =
    when (note.problem) {
        SubtitleProblem.Unreachable -> {
            "${note.source} did not answer. Try its subtitles again in a moment."
        }

        SubtitleProblem.Blocked -> {
            "${note.source} is turning requests away. Marquee will try again in a few minutes."
        }

        SubtitleProblem.Changed -> {
            "${note.source} changed its pages, so Marquee cannot read them. An update will fix this."
        }

        SubtitleProblem.TrackFailed -> {
            "A subtitle file from ${note.source} could not be read. Pick another one."
        }

        SubtitleProblem.Unknown -> {
            "Subtitles from ${note.source} did not load."
        }
    }

/** Under the subtitle list: the pending line while it shows, then one line per note in the core's order. */
fun subtitlePanelNote(
    pending: Boolean,
    notes: List<SubtitleNote>,
): String? =
    (listOfNotNull(SUBTITLES_PENDING.takeIf { pending }) + notes.map(::subtitleNoteWords))
        .joinToString("\n")
        .ifEmpty { null }

/** The notes in [now] that [before] did not have: what just happened. */
fun newSubtitleNotes(
    before: List<SubtitleNote>,
    now: List<SubtitleNote>,
): List<SubtitleNote> = now.filter { it !in before }

/** What the Audio action says under its name: the chosen track ("English, E-AC-3, 5.1"), or null while there is none. */
fun PlaybackController.audioDetail(): String? =
    audio.firstOrNull { it.selected }?.let { row ->
        if (row.name == "Audio") row.detail else listOfNotNull(row.name, row.detail).joinToString(" ")
    }

/** "English" for "en" or "eng"; the code itself when the platform has no name for it. */
fun languageName(code: String?): String? {
    if (code.isNullOrBlank() || code == C.LANGUAGE_UNDETERMINED) return null
    val normalized = Util.normalizeLanguageCode(code)
    val name = Locale.forLanguageTag(normalized).getDisplayLanguage(Locale.ENGLISH)
    return name.takeIf { it.isNotBlank() && it != normalized } ?: code
}

/** "AC-3" for `audio/ac3`, "SRT" for `application/x-subrip`; null for a type nobody would name. */
fun codecName(mime: String?): String? =
    when (mime) {
        "audio/ac3" -> "AC-3"
        "audio/eac3" -> "E-AC-3"
        "audio/eac3-joc" -> "E-AC-3 Atmos"
        "audio/mp4a-latm" -> "AAC"
        "audio/vnd.dts" -> "DTS"
        "audio/vnd.dts.hd" -> "DTS-HD"
        "audio/vnd.dts.hd;profile=lbr" -> "DTS Express"
        "audio/true-hd" -> "TrueHD"
        "audio/opus" -> "Opus"
        "audio/vorbis" -> "Vorbis"
        "audio/flac" -> "FLAC"
        "audio/mpeg", "audio/mpeg-L2" -> "MP3"
        "audio/raw" -> "PCM"
        "application/x-subrip" -> "SRT"
        "text/vtt" -> "WebVTT"
        "text/x-ssa" -> "ASS"
        "application/pgs" -> "PGS"
        "application/dvbsubs" -> "DVB"
        "application/vobsub" -> "VobSub"
        "application/ttml+xml" -> "TTML"
        "application/cea-608", "application/cea-708" -> "Closed captions"
        else -> null
    }

/** 2 -> "stereo", 6 -> "5.1". */
fun channelLayout(count: Int): String? =
    when (count) {
        Format.NO_VALUE, 0 -> null
        1 -> "mono"
        2 -> "stereo"
        6 -> "5.1"
        7 -> "6.1"
        8 -> "7.1"
        else -> "$count channels"
    }

/** The menu row of an audio track: "English" over "AC-3, 5.1". */
fun audioChoice(
    id: String,
    format: Format,
    selected: Boolean,
): TrackChoice {
    val name = format.label ?: languageName(format.language) ?: "Audio"
    val detail = listOfNotNull(codecName(format.sampleMimeType), channelLayout(format.channelCount)).joinToString(", ")
    return TrackChoice(id, name, detail.ifBlank { null }, selected)
}

/** The menu row of a subtitle track that is already loaded: "English" over "SRT, forced". */
fun textChoice(
    id: String,
    format: Format,
    selected: Boolean,
): TrackChoice {
    val name = languageName(format.language) ?: format.label ?: "Subtitles"
    val detail =
        listOfNotNull(
            format.label?.takeIf { it != name },
            // Media3 parses subtitles while extracting and then says "cues": the real type is in `codecs`.
            codecName(format.sampleMimeType) ?: codecName(format.codecs),
            "forced".takeIf { format.selectionFlags and C.SELECTION_FLAG_FORCED != 0 },
            "for the hard of hearing".takeIf { format.roleFlags and C.ROLE_FLAG_DESCRIBES_MUSIC_AND_SOUND != 0 },
        ).joinToString(", ")
    return TrackChoice(id, name, detail.ifBlank { null }, selected)
}
