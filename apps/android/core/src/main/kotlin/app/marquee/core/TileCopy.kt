package app.marquee.core

import app.marquee.core.model.Hero
import app.marquee.core.model.HomeCatalog
import app.marquee.core.model.Ratings
import app.marquee.core.model.Tile

/** The words under a tile and in a hero, the same as the web family's `lib/words.ts`. */
object TileCopy {
    /** Id of the Continue Watching row: its tiles are 16:9 stills with the logo on them. */
    const val CONTINUE_ROW = "continue-watching"

    /** The hold menu of a Continue Watching tile (Next Up tiles live in the same row). */
    const val REMOVE_CONTINUE = "Remove from Continue Watching"

    /** Whether [rowId] is Continue Watching: 16:9 cards with the core's line and no scores. */
    fun isContinue(rowId: String): Boolean = rowId == CONTINUE_ROW

    /** Whether a hold on a tile of [rowId] opens the menu with [REMOVE_CONTINUE]: Continue Watching's. */
    fun removable(rowId: String): Boolean = isContinue(rowId)

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
     * No addon name on Home, in Search or Discover, nor in a row's state, not even for two rows of one title
     * (the owner, 2026-10-22; design/DECISION.md, Shelves). Customize Home is where twins are told apart:
     * [item]'s addon only when another of [all] has the same [title].
     */
    fun <T> twinSource(
        item: T,
        all: List<T>,
        title: (T) -> String,
        source: (T) -> String,
    ): String? = source(item).takeIf { all.count { title(it) == title(item) } > 1 }

    /**
     * The line under a catalog's name on the Home rows page: its addon only beside another row of that
     * title, then [hidden] ("Hidden") when it is; null when neither.
     */
    fun homeRowLine(
        row: HomeCatalog,
        rows: List<HomeCatalog>,
        hidden: String,
    ): String? =
        listOfNotNull(twinSource(row, rows, { it.title }, { it.source }), hidden.takeIf { row.hidden })
            .joinToString(" · ")
            .ifEmpty { null }

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
     * A title's score as plain text where text shows one (the hero, the title page, what a screen reader hears):
     * IMDb's number alone, "IMDb 8.8", as Stremio shows it; the covers carry the rest in RPDB's band (the owner,
     * 2026-10-23). A score the core left null has no slot, and no brand's logo appears.
     */
    fun ratings(ratings: Ratings): List<String> = listOfNotNull(ratings.imdb?.let { "IMDb $it" })

    /**
     * A tile's picture and what shows when it does not load. A cover is the core's poster, RPDB's with the ratings
     * band drawn in for an IMDb title, then the addon's own (an id RPDB does not know answers 404). A 16:9 card and
     * the hero ([wide]) show the backdrop, else the addon's own poster: never a cover with ratings drawn in.
     */
    fun art(
        tile: Tile,
        wide: Boolean,
    ): Pair<String?, String?> {
        if (!wide) return tile.poster to tile.posterFallback
        val plain = tile.posterFallback ?: tile.poster
        return (tile.background ?: plain) to plain.takeIf { tile.background != null }
    }

    /** A hero's genre line: the kind, then its genres ("Series · Drama · Thriller"). */
    fun heroGenres(hero: Hero): String = (listOf(kind(hero.tile.kind)) + hero.genres).joinToString(" · ")

    /** A hero's facts before its ratings ("2022– · 46 min"); null when the addon gave neither. */
    fun heroFacts(hero: Hero): String? =
        listOfNotNull(hero.tile.releaseInfo, hero.runtime).joinToString(" · ").ifEmpty { null }
}
