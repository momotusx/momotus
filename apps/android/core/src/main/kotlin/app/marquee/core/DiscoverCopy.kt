package app.marquee.core

import app.marquee.core.model.DiscoverSnapshot
import app.marquee.core.model.Tile

/** Discover's words (plans/discover-v2.md 6.1), equal to the web's `scopeLine` and `emptyLine`. */
object DiscoverCopy {
    /** The footer: the list's size, its scope beside its sort, how far a decade looked, what did not answer. */
    fun scope(s: DiscoverSnapshot): String {
        val parts = mutableListOf<String>()
        val titles = s.scope.titles ?: s.items.size
        parts += "$titles ${if (titles == 1) "title" else "titles"}"
        val sort = s.sorts.firstOrNull { it.id == s.sort }
        if (s.scope.note != null && sort != null) parts += "${sort.label}, ${s.scope.note}"
        s.scope.looked?.let { parts += "Looked through the first $it" }
        if (s.scope.pending > 0) parts += "${s.scope.pending} ${services(s.scope.pending)} still loading"
        if (s.scope.failed > 0) parts += "${s.scope.failed} ${services(s.scope.failed)} did not answer"
        return parts.joinToString(" · ")
    }

    /** The empty line: what was asked, never a cause the data does not give. */
    fun empty(s: DiscoverSnapshot): String {
        val service = s.platforms.firstOrNull { it.id == s.platform }?.name
        val decade = s.decade?.let { " from the $it" } ?: ""
        return "No ${s.genre ?: "titles"}$decade${service?.let { " on $it" } ?: ""} right now."
    }

    /**
     * On the poster's corner: the value the list is ordered by, the IMDb score for the IMDb sort (even with the cover's
     * band, which can be weeks old) and the year for the date sorts; nothing for the others.
     */
    fun corner(
        sort: String?,
        tile: Tile,
    ): String? =
        when (sort) {
            "imdb" -> tile.ratings.imdb?.let { "IMDb $it" }
            "newest", "oldest" -> tile.releaseInfo
            else -> null
        }

    /** The line under a poster: its year, unless the corner already shows it. */
    fun caption(
        sort: String?,
        tile: Tile,
    ): String? = tile.releaseInfo?.takeIf { corner(sort, tile) != it }

    /** In place of the core's reasons, which can name a host or an addon (the no-addon-names rule). */
    const val OPEN_FAILED = "Discover could not open."
    const val FAILED = "This list did not load."
    const val MORE_FAILED = "More titles did not load."

    /** The heading over the other catalogs in the Sort menu, so a catalog called "Top" does not read as a sort. */
    const val MORE_LISTS = "More lists"

    private fun services(n: Int) = if (n == 1) "service" else "services"
}
