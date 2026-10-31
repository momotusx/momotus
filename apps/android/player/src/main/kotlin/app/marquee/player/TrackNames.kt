package app.marquee.player

import androidx.media3.common.C
import androidx.media3.common.Format
import androidx.media3.common.util.Util
import app.marquee.core.StreamChoiceCopy
import app.marquee.core.model.SubtitleNote
import app.marquee.core.model.SubtitleProblem
import app.marquee.core.model.SubtitleTrack
import java.util.Locale

/** One row of the audio or subtitles menu. */
data class TrackChoice(
    val id: String,
    val name: String,
    /** Under the name: codec, channels, source, or where the track is from. */
    val detail: String? = null,
    val selected: Boolean = false,
    /** A subtitle's language by its English name, the picker's heading for it; null for Off and audio. */
    val section: String? = null,
    /** A subtitle's language as one key ([languageKey]): the picker groups by it. Null for Off and audio. */
    val language: String? = null,
    /** The language's own name ("Türkçe") when it differs from [section]. */
    val nativeName: String? = null,
    /** A track inside the video file: it leads its language, as it always fits the file. */
    val inFile: Boolean = false,
    val forced: Boolean = false,
    /** The core's match: the track names the playing release's group ("Same release group"). */
    val fits: Boolean = false,
    /** Made for this very file, by its hash ("Matches this file"). */
    val hashMatch: Boolean = false,
    /** The frame rate it was made for, when one of it and the file's is PAL and the other not ("Made for 25 fps"). */
    val madeForFps: Float? = null,
    /** The release text the addon names for the track, for the detail line. */
    val release: String? = null,
)

const val TRACK_OFF = "off"

/** The detail of a subtitle track inside the video file. */
const val IN_THE_FILE = "In the file"

/** The picker's heading for a track in the file that names no language. */
const val UNKNOWN_LANGUAGE = "Unknown language"

/**
 * What the Subtitles action says under its name: the chosen language, "Turkish + English" with a second line drawn,
 * or how many the picker shows ("None found" for zero).
 */
fun PlaybackController.subtitlesDetail(): String =
    subtitlesDetail(
        text,
        subtitlesPending,
        secondText.takeIf {
            secondOn
        },
    )

/** The core still lists, but tracks are in: the count shows with a small searching mark until the listing is done. */
fun PlaybackController.subtitlesStillListing(): Boolean = subtitlesStillListing(text, subtitlesPending)

internal fun subtitlesStillListing(
    rows: List<TrackChoice>,
    pending: Boolean,
): Boolean = pending && rows.size > 1

/**
 * The chosen track's language ("Portuguese (Brazil)"), else the languages the picker shows ("Off, 35 languages",
 * plans/player-polish.md 2.2). "Searching" only while nothing is in: the viewer does not wait for the slowest source.
 */
internal fun subtitlesDetail(
    rows: List<TrackChoice>,
    pending: Boolean,
    /** The second line's rows while it is drawn: its language follows the first's. */
    second: List<TrackChoice>? = null,
): String {
    val found = subtitleLanguages(rows).size
    val chosen = rows.firstOrNull { it.selected && it.id != TRACK_OFF }
    val two = second?.firstOrNull { it.selected && it.id != TRACK_OFF }?.section
    return chosen?.let { it.section ?: it.name }?.let { if (two != null) "$it + $two" else it }
        ?: if (pending && found == 0) {
            "Searching"
        } else if (found == 0) {
            "None found"
        } else {
            "Off, $found ${if (found == 1) "language" else "languages"}"
        }
}

/**
 * The subtitles menu (plans/m13-subtitle-languages.md 2, plans/player-polish.md 2.4): Off, then the core's tracks
 * in the snapshot's order (the profile's languages first), each with its language and named by what tells it from
 * the others; then the tracks inside the file, which [subtitleLanguages] puts first in their language. A side-loaded
 * track takes its player row's id and state from [loaded] (by URL); one not loaded yet loads when chosen; one in
 * [hidden] is left out.
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
            // An older core sends no name or heading: the code's language stands in, never the addon's name
            // or its label (a label can carry a provider and a file name, "OpenSubtitles · [SRT] · x.srt").
            val section = t.languageName.ifEmpty { languageName(t.lang) ?: t.lang }
            val name = t.name.ifEmpty { section }
            TrackChoice(
                row?.id ?: "$late${t.url}",
                name,
                trackDetail(t).ifEmpty { null },
                row?.selected == true,
                section,
                language = languageKey(t.language.ifEmpty { t.lang }),
                nativeName = t.nativeName.takeIf { it.isNotEmpty() && it != section },
                forced = t.forced,
                fits = t.fits,
                hashMatch = t.hashMatch,
                madeForFps = t.madeForFps,
                release = t.release?.takeIf { it != name },
            )
        }
    val rows = coreRows + inFile
    return listOf(TrackChoice(TRACK_OFF, "Off", selected = rows.none { it.selected })) + rows
}

/**
 * Under a core track's name: what kind of file it is, never where it came from (no addon or service is named where a
 * viewer reads it, plans/player-polish.md 2.4).
 */
internal fun trackDetail(t: SubtitleTrack): String =
    listOfNotNull(
        "Forced".takeIf { t.forced },
        "For the hard of hearing".takeIf { t.hearingImpaired },
    ).joinToString(DOT)

/** Between the parts of a subtitle's detail line. */
internal const val DOT = " · "

/** One key for every spelling of a language, so the file's "eng" and the core's "en" meet: "en", "pt-br". */
fun languageKey(code: String): String = Util.normalizeLanguageCode(code).lowercase(Locale.ROOT)

/**
 * Whether two codes or tags name one language by their base language ("en" and "en-GB", "eng" and "en"), as the web's
 * `sameLanguage` (this package's [sameLanguage] matches whole tags): two lines never show one language. An unknown
 * or empty one never matches.
 */
fun samePrimaryLanguage(
    a: String?,
    b: String?,
): Boolean {
    val base = { c: String -> languageKey(c).substringBefore('-') }
    if (a.isNullOrBlank() || b.isNullOrBlank()) return false
    val x = base(a)
    return x.isNotEmpty() && x != C.LANGUAGE_UNDETERMINED && x == base(b)
}

/**
 * The second line's track (plans/dual-subtitles.md 6.2): the core's [auto] (`autoSecondSubtitle`) as a level, which
 * may change or go null with the first line, until the viewer [chose] one in this play; then the [current] one.
 */
internal fun followSecond(
    auto: String?,
    chose: Boolean,
    current: String?,
): String? = if (chose) current else auto

/** "Türkçe" for "tr": the language's name in itself, from the platform; null when it has none. */
internal fun ownName(code: String): String? {
    val locale = Locale.forLanguageTag(code)
    val name = locale.getDisplayLanguage(locale)
    return name.takeIf { it.isNotBlank() && !it.equals(locale.language, ignoreCase = true) }?.replaceFirstChar {
        it.titlecase(locale)
    }
}

/** "Still asking subtitle addons": under the subtitle list while the core still waits for some. */
const val SUBTITLES_PENDING = "Still asking subtitle addons"

/**
 * What a subtitle note says (plans/m13-turkcealtyazi.md 4.2, `player.subtitleNotes` in the shared words): a
 * subtitle source that gave nothing this time, and why, never the source's name (the owner's rule). A problem
 * from a newer core gets the general line.
 */
fun subtitleNoteWords(note: SubtitleNote): String {
    val key =
        when (note.problem) {
            SubtitleProblem.Unreachable -> "unreachable"
            SubtitleProblem.Blocked -> "blocked"
            SubtitleProblem.Changed -> "changed"
            SubtitleProblem.TrackFailed -> "trackFailed"
            SubtitleProblem.Unknown -> "other"
        }
    return StreamChoiceCopy.SUBTITLE_NOTES.getValue(key)
}

/**
 * Under the subtitle list: the pending line while it shows, then one line per note in the core's order, each line
 * once (two sources with one problem read alike).
 */
fun subtitlePanelNote(
    pending: Boolean,
    notes: List<SubtitleNote>,
): String? =
    (listOfNotNull(SUBTITLES_PENDING.takeIf { pending }) + notes.map(::subtitleNoteWords).distinct())
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

/**
 * An audio track that names no language, in the shared player words (apps/shared/stream-choice-words.json): "Audio"
 * when it is the file's only one, else "Audio 2" by its place. Never "Original": a dub is not.
 */
fun audioPlace(
    index: Int,
    count: Int,
): String =
    if (count == 1) StreamChoiceCopy.AUDIO_ONE else StreamChoiceCopy.AUDIO_NUMBERED.replace("{n}", "${index + 1}")

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

/**
 * The menu row of an audio track: "English" over "AC-3, 5.1". One that names no language reads by its place among
 * the file's [count] audio tracks ([audioPlace]).
 */
fun audioChoice(
    id: String,
    format: Format,
    selected: Boolean,
    index: Int = 0,
    count: Int = 1,
): TrackChoice {
    val name = format.label ?: languageName(format.language) ?: audioPlace(index, count)
    val detail = listOfNotNull(codecName(format.sampleMimeType), channelLayout(format.channelCount)).joinToString(", ")
    return TrackChoice(id, name, detail.ifBlank { null }, selected)
}

/** The menu row of a subtitle track inside the file: "English" over "In the file · SRT · Forced", under its language. */
fun textChoice(
    id: String,
    format: Format,
    selected: Boolean,
): TrackChoice {
    val language = languageName(format.language)
    val name = language ?: format.label ?: "Subtitles"
    val forced = format.selectionFlags and C.SELECTION_FLAG_FORCED != 0
    val detail =
        listOfNotNull(
            IN_THE_FILE,
            format.label?.takeIf { it != name },
            // Media3 parses subtitles while extracting and then says "cues": the real type is in `codecs`.
            codecName(format.sampleMimeType) ?: codecName(format.codecs),
            "Forced".takeIf { forced },
            "For the hard of hearing".takeIf { format.roleFlags and C.ROLE_FLAG_DESCRIBES_MUSIC_AND_SOUND != 0 },
        ).joinToString(DOT)
    val code = format.language?.takeIf { language != null }
    return TrackChoice(
        id,
        name,
        detail,
        selected,
        section = language ?: UNKNOWN_LANGUAGE,
        language = code?.let(::languageKey) ?: "",
        nativeName = code?.let(::ownName)?.takeIf { it != language },
        inFile = true,
        forced = forced,
    )
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
