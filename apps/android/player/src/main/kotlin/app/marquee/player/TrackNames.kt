package app.marquee.player

import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.util.Util
import app.marquee.core.model.SubtitleNote
import app.marquee.core.model.SubtitleProblem
import app.marquee.core.model.SubtitleTrack
import java.util.Locale

/** One row of the audio or subtitles menu. */
data class TrackChoice(
    val id: String,
    val name: String,
    /** Under the name: codec, channels, source, or why choosing it takes a moment. */
    val detail: String? = null,
    val selected: Boolean = false,
    /** The heading of the run of rows it belongs to: a core track's language, or [IN_THE_FILE]; null for Off and audio. */
    val section: String? = null,
)

const val TRACK_OFF = "off"

/** The heading over the subtitle tracks inside the video file, after the core's languages. */
const val IN_THE_FILE = "In the file"

/** What the Subtitles action says under its name: the chosen language, or how many were found ("None found" for zero). */
fun PlaybackController.subtitlesDetail(): String = subtitlesDetail(text, subtitlesPending)

/** The core still lists, but tracks are in: the count shows with a small searching mark until the listing is done. */
fun PlaybackController.subtitlesStillListing(): Boolean = subtitlesStillListing(text, subtitlesPending)

internal fun subtitlesStillListing(
    rows: List<TrackChoice>,
    pending: Boolean,
): Boolean = pending && rows.size > 1

/** "Searching" only while nothing is in: the viewer does not wait for the slowest source. */
internal fun subtitlesDetail(
    rows: List<TrackChoice>,
    pending: Boolean,
): String {
    val found = rows.size - 1
    val chosen = rows.firstOrNull { it.selected && it.id != TRACK_OFF }
    // A core track says its language ("Portuguese (Brazil)"), a track in the file its own name.
    return chosen?.let { it.section?.takeIf { s -> s != IN_THE_FILE } ?: it.name }
        ?: if (pending && found == 0) {
            "Searching"
        } else if (found == 0) {
            "None found"
        } else {
            "Off, $found found"
        }
}

/**
 * The subtitles menu (plans/m13-subtitle-languages.md 2): Off, then the core's tracks in the snapshot's
 * order (the profile's languages first), each under its language and named by what tells it from the
 * others; then the tracks inside the file. A side-loaded track takes its player row's id and state from
 * [loaded] (by URL); one not loaded yet loads when chosen; one in [hidden] is left out.
 */
internal fun subtitleRows(
    core: List<SubtitleTrack>,
    loaded: Map<String, TrackChoice>,
    hidden: Set<String>,
    inFile: List<TrackChoice>,
    late: String,
): List<TrackChoice> {
    val coreRows =
        core.mapNotNull { t ->
            val row = loaded[t.url]
            if (row == null && t.url in hidden) return@mapNotNull null
            val name = t.name.ifEmpty { t.label ?: t.source }
            // An older core sends no name or heading: the code's language stands in.
            val section = t.languageName.ifEmpty { languageName(t.lang) ?: t.lang }
            val detail = listOfNotNull(trackDetail(t, name), "loads when chosen".takeIf { row == null })
            TrackChoice(
                row?.id ?: "$late${t.url}",
                name,
                detail.filter { it.isNotEmpty() }.joinToString(", ").ifEmpty { null },
                row?.selected == true,
                section,
            )
        }
    val files = inFile.map { it.copy(section = IN_THE_FILE) }
    val rows = coreRows + files
    return listOf(TrackChoice(TRACK_OFF, "Off", selected = rows.none { it.selected })) + rows
}

/** Under a core track's name: its source when the name does not already say it, then what kind of file it is. */
internal fun trackDetail(
    t: SubtitleTrack,
    name: String,
): String {
    val source = t.source.takeIf { name != it && !name.startsWith("$it ") }
    return listOfNotNull(
        source,
        "Forced".takeIf { t.forced },
        "For the hard of hearing".takeIf { t.hearingImpaired },
    ).joinToString(", ")
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

/**
 * The start subtitle to turn on (plans/m13-subtitle-languages.md 6): the core's [auto] track, once, unless
 * the viewer [chose] one (or Off) in this play, or a pick carried from the episode before is on.
 */
internal fun startSubtitle(
    auto: String?,
    chose: Boolean,
    picked: Boolean,
    started: String?,
): String? = auto?.takeIf { !chose && !picked && it != started }
