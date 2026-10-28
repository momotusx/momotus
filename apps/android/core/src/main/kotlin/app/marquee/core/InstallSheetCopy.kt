package app.marquee.core

import java.util.Locale

/**
 * The addon install sheet (plans/addon-install-sheet.md): its words and the Adds line, checked against
 * `apps/shared/install-sheet.json` as the web and Apple are.
 */
object InstallSheetCopy {
    const val TITLE = "Install {name}?"
    const val FROM = "From {host}"
    const val ADDS = "Adds {what}."
    const val ADDS_FOR = "Adds {what} for {kinds}."
    const val NOTE = "It comes from a third party. Momotus does not check it."
    const val INSTALL = "Install"
    const val CANCEL = "Cancel"
    const val CHECKING = "Checking"

    /** A manifest's resources in plain words; any other resource is left out. */
    val RESOURCES =
        mapOf(
            "stream" to "sources",
            "subtitles" to "subtitles",
            "catalog" to "catalogs",
            "meta" to "title details",
            "addon_catalog" to "addon lists",
        )

    /** A manifest's types in plain words; any other type is lowercased. */
    val KINDS =
        mapOf(
            "movie" to "movies",
            "series" to "series",
            "channel" to "channels",
            "tv" to "live TV",
            "anime" to "anime",
        )

    // String.replace is literal: a "$'" or "{host}" in a name stays plain text.
    fun title(name: String) = TITLE.replace("{name}", name)

    fun from(host: String) = FROM.replace("{host}", host)

    /**
     * "Adds sources and subtitles for movies and series.", each word once in manifest order; empty when
     * no resource is one of [RESOURCES]. Types are trimmed and lowercased first; only addon lists, or no types,
     * says no kinds.
     */
    fun adds(
        provides: List<String>,
        types: List<String>,
    ): String {
        val what = provides.mapNotNull { RESOURCES[it] }.distinct()
        if (what.isEmpty()) return ""
        val kinds =
            types
                .map { it.trim().lowercase(Locale.ROOT) }
                .filter { it.isNotEmpty() }
                .map { KINDS[it] ?: it }
                .distinct()
        if (kinds.isEmpty() || what == listOf(RESOURCES.getValue("addon_catalog"))) {
            return ADDS.replace("{what}", joined(what))
        }
        return ADDS_FOR.replace("{what}", joined(what)).replace("{kinds}", joined(kinds))
    }

    /** The sheet's lines under its title: where it comes from, what it adds (when it says), the note. */
    fun lines(
        host: String,
        provides: List<String>,
        types: List<String>,
    ): List<String> = listOfNotNull(from(host), adds(provides, types).ifEmpty { null }, NOTE)

    /** "a", "a and b", "a, b and c". */
    private fun joined(words: List<String>): String =
        if (words.size == 1) words[0] else words.dropLast(1).joinToString(", ") + " and " + words.last()
}
