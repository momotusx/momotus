package app.marquee.core

import app.marquee.core.model.Hero
import app.marquee.core.model.Ratings
import app.marquee.core.model.Tile

/** The words under a tile and in a hero, the same as the web family's `lib/words.ts`. */
object TileCopy {
    /** Id of the Continue Watching row: its tiles are 16:9 stills with the logo on them. */
    const val CONTINUE_ROW = "continue-watching"

    /** The hold menu of a Continue Watching tile (Next Up tiles live in the same row). */
    const val REMOVE_CONTINUE = "Remove from Continue Watching"

    /** Whether a hold on a tile of [rowId] opens the menu with [REMOVE_CONTINUE]. */
    fun removable(rowId: String): Boolean = rowId == CONTINUE_ROW

    /** TV's hint beside Continue Watching, as the title page has one for its episodes. */
    const val HOLD_TO_REMOVE = "Hold select on a title to remove it"

    /** The note while a removal can still be taken back. */
    fun removed(name: String): String = "$name was removed from Continue Watching."

    /** The note when the core did not take the removal: the title is back. */
    fun removeFailed(
        name: String,
        reason: String,
    ): String = "$name could not be removed from Continue Watching: ${reason.trimEnd('.')}."

    /**
     * What a row's source label says: the addon's name. Continue Watching has none: it comes from
     * what was played, not from an addon or the library.
     */
    fun source(
        rowId: String,
        source: String,
    ): String? = if (rowId == CONTINUE_ROW) null else source

    /**
     * The line under a tile. Only Continue Watching has one, and the core writes it ("S2:E3 · 36 min
     * left", "36 min left", "Next: S1:E2" in `releaseInfo`): it is shown as it is. Other rows have none.
     */
    fun caption(
        rowId: String,
        tile: Tile,
    ): String? = tile.releaseInfo.takeIf { rowId == CONTINUE_ROW }

    fun kind(kind: String): String =
        when (kind) {
            "movie" -> "Movie"
            "series" -> "Series"
            "channel" -> "Channel"
            "tv" -> "Live TV"
            "anime" -> "Anime"
            else -> kind.replaceFirstChar { it.uppercase() }
        }

    /**
     * A title's scores side by side as plain text, the source first: "IMDb 8.8", "RT 87%". A score the
     * core left null has no slot, and the brands' logos and icons never appear (plans/research/ratings.md).
     */
    fun ratings(ratings: Ratings): List<String> =
        listOfNotNull(ratings.imdb?.let { "IMDb $it" }, ratings.rottenTomatoes?.let { "RT $it" })

    /**
     * The facts after a hero's scores: "2026 · 145 min · Action, Adventure". The kind
     * stands in when the addon gave no year, runtime or genre ("Series").
     */
    fun heroMeta(hero: Hero): String {
        val facts = listOfNotNull(hero.tile.releaseInfo, hero.runtime, hero.genres.joinToString(", ").ifEmpty { null })
        return facts.ifEmpty { listOf(kind(hero.tile.kind)) }.joinToString(" · ")
    }
}
