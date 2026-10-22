package app.marquee.core

import app.marquee.core.model.LanguageChoice

/**
 * The language pickers (Settings' subtitles, stream choice's audio) over every language the core knows
 * (`languages`, 113): the chosen ones first, then a search by name or code. Before anything is typed
 * the search offers the common ones, so a TV remote does not walk past a hundred chips.
 */
object LanguagePicks {
    /** Offered before anything is typed: the languages the pickers had before the full list. */
    val COMMON = listOf("en", "tr", "es", "fr", "de", "it", "pt", "ja")

    /** How many matches the search shows at once. */
    const val SHOWN = 12

    /** The audio list: one language per base code (the part before "-"), as `streams.languages` takes it. */
    fun audio(all: List<LanguageChoice>): List<LanguageChoice> =
        all
            .groupBy { it.code.substringBefore('-') }
            .map { (code, same) -> LanguageChoice(code, same.first().name.substringBefore(" (")) }

    /** The English name of [code]: the core's, else what the platform calls it (a code from before the full list, "eng"). */
    fun name(
        code: String,
        all: List<LanguageChoice>,
    ): String = all.firstOrNull { it.code == code }?.name ?: VersionsCopy.languageName(code)

    /**
     * What the search offers for [query]: names or codes starting with it first, then the ones containing it,
     * none already [chosen] (by code or by name, so an old "eng" keeps English off the list), at most [SHOWN].
     * An empty query offers the [device]'s own language first, then [COMMON].
     */
    fun search(
        all: List<LanguageChoice>,
        query: String,
        chosen: List<String>,
        device: String? = null,
    ): List<LanguageChoice> {
        val taken = chosen.map { name(it, all).lowercase() }.toSet()
        val open = { choice: LanguageChoice -> choice.code !in chosen && choice.name.lowercase() !in taken }
        val q = query.trim().lowercase()
        if (q.isEmpty()) {
            return (listOfNotNull(device?.takeIf { it.isNotBlank() }) + COMMON)
                .distinct()
                .map { code ->
                    all.firstOrNull { it.code == code } ?: LanguageChoice(code, name(code, all))
                }.filter(open)
        }
        val starts = all.filter { it.name.lowercase().startsWith(q) || it.code.lowercase() == q }
        val contains = all.filter { it.name.lowercase().contains(q) } - starts.toSet()
        return (starts + contains).filter(open).take(SHOWN)
    }
}
