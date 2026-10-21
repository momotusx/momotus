package app.marquee.player

import java.text.Normalizer
import java.util.Locale

/**
 * One language of the subtitle picker (plans/player-polish.md 2.4): its English [name], its own name when it
 * differs, and its options, the file's own tracks first (plain before forced), then the core's in its order.
 */
data class SubtitleLanguage(
    /** [languageKey] of the language; empty for tracks in the file that name none. */
    val key: String,
    val name: String,
    val nativeName: String?,
    val options: List<TrackChoice>,
) {
    val chosen: Boolean get() = options.any { it.selected }

    /**
     * What picking the language turns on: its first option that is not forced (the file's own track, which
     * always fits the file, else the core's best match), else its first.
     */
    val best: TrackChoice get() = options.firstOrNull { !it.forced } ?: options.first()
}

/** The subtitle rows by language, in the order the languages first come in [rows] (the core's, then the file's). */
fun subtitleLanguages(rows: List<TrackChoice>): List<SubtitleLanguage> =
    rows
        .filter { it.language != null }
        .groupBy { it.language!! }
        .map { (key, options) ->
            SubtitleLanguage(
                key = key,
                // The core's name for it when it lists the language, which it does first.
                name = options.first().section ?: key,
                nativeName = options.firstNotNullOfOrNull { it.nativeName },
                options =
                    options.sortedBy {
                        if (!it.inFile) {
                            2
                        } else if (it.forced) {
                            1
                        } else {
                            0
                        }
                    },
            )
        }

/** The languages whose English name, own name or code starts a word with [query]; all of them for a blank one. */
fun filterLanguages(
    languages: List<SubtitleLanguage>,
    query: String,
): List<SubtitleLanguage> {
    val q = folded(query.trim())
    if (q.isEmpty()) return languages
    return languages.filter { lang ->
        val names = listOfNotNull(lang.name, lang.nativeName).map(::folded)
        names.any { name -> name.startsWith(q) || name.contains(" $q") || name.contains("($q") } ||
            codes(lang.key).any { it.startsWith(q) }
    }
}

/** "en" and "eng" for English: the key and the three letters addons send. */
private fun codes(key: String): List<String> {
    if (key.isEmpty()) return emptyList()
    val three = runCatching { Locale.forLanguageTag(key).isO3Language }.getOrNull()
    return listOfNotNull(key, three?.takeIf { it.isNotEmpty() })
}

/** Lower case with the accents gone, so "turk" finds "Türkçe". */
private fun folded(text: String): String =
    Normalizer
        .normalize(text.lowercase(Locale.ROOT), Normalizer.Form.NFD)
        .replace(MARKS, "")

private val MARKS = Regex("\\p{Mn}+")

/** The circle-flags drawable for a language key: its own, else its language's ("pt-br", then "pt"); null for none. */
fun flagFor(key: String): Int? = FLAGS[key] ?: FLAGS[key.substringBefore('-')]

/**
 * An option's quiet line: where it is from and what it is ([TrackChoice.detail]), "Same release group" where the
 * core's match fits, then the release text cut in the middle to [room] characters ([middleCut]).
 */
fun optionLine(
    option: TrackChoice,
    room: Int,
): String {
    val head = listOfNotNull(option.detail, SAME_GROUP.takeIf { option.fits }).joinToString(DOT)
    val release = option.release ?: return head
    // ponytail: a character measure, as the frames use; a measured cut if widths vary too much.
    val left = room - head.length - DOT.length
    val cut = middleCut(release, if (left >= MIN_RELEASE) left else room)
    return if (head.isEmpty()) cut else head + DOT + cut
}

/** The core's caption for a track that names the playing release's group (plans/player-polish.md 2.2). */
const val SAME_GROUP = "Same release group"

/** Below this many characters left on the line, the release text takes a whole line's measure (it wraps). */
private const val MIN_RELEASE = 24

/**
 * A long release name cut in its middle at a dot or space, so the group at its end stays in view and no dot
 * sits against the ellipsis: the tail is taken first, back to the boundary before it (so "x265-MeGusta[EZTVx.to]"
 * stays whole), then the head fills what is left. The frames' `middle` (design/exploration/v3/src/v2/common.ts).
 */
fun middleCut(
    text: String,
    max: Int,
): String {
    if (text.length <= max) return text

    fun at(j: Int) = text[j] == '.' || text[j] == ' '
    var from = text.length - maxOf(1, max - 1 - ((max - 1) * 0.45).toInt())
    while (from > 0 && !at(from - 1)) from--
    // A tail with no boundary near enough keeps its share and starts at the next boundary instead.
    if (text.length - from > max - 5) {
        from = text.length - (max - 5)
        while (from < text.length && !at(from - 1)) from++
    }
    val tail = text.substring(from).trimStart('.', ' ')
    var cut = max - 1 - tail.length
    while (cut > 0 && !at(cut)) cut--
    val head = text.substring(0, if (cut > 0) cut else (max - 1 - tail.length).coerceAtLeast(0)).trimEnd('.', ' ')
    return "$head…$tail"
}

/**
 * The track inside the file that takes over from the core's start subtitle (plans/player-polish.md 2.2): once the
 * file has opened, its own track in that language, if one is not forced, wins, as it always fits the file. The
 * start pick alone: a track the viewer chose stays.
 */
internal fun fileTakesOver(
    language: String,
    rows: List<TrackChoice>,
): TrackChoice? = rows.firstOrNull { it.inFile && !it.forced && it.language == language && language.isNotEmpty() }
