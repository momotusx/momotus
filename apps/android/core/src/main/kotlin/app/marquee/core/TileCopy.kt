package app.marquee.core

import app.marquee.core.model.CatalogChoice
import app.marquee.core.model.Hero
import app.marquee.core.model.HomeCatalog
import app.marquee.core.model.HomeRow
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
     * What a row's or a search group's source label says: nothing beside a title of its own, and the
     * addon's name only when another row of [rows] has the same title ([twinSource]). Continue Watching
     * has none: it comes from what was played, not from an addon.
     */
    fun source(
        row: HomeRow,
        rows: List<HomeRow>,
    ): String? = if (row.rowId == CONTINUE_ROW) null else twinSource(row, rows, { it.title }, { it.source })

    /**
     * No addon name in the UI (the owner, 2026-10-22: no "Cinemeta" beside a title): [item]'s addon only
     * when another of [all] has the same [title], to tell the two apart (a row, a search group, a catalog).
     * A failed or empty state and the Addons pages still name it.
     */
    fun <T> twinSource(
        item: T,
        all: List<T>,
        title: (T) -> String,
        source: (T) -> String,
    ): String? = source(item).takeIf { all.count { title(it) == title(item) } > 1 }

    /**
     * A Discover catalog in a line about it ("Popular only answers for one genre at a time"): its name, with
     * "from" its addon only beside another catalog of that name. A failed or empty [state] names the addon
     * always, as it says who did not answer.
     */
    fun catalogLabel(
        catalog: CatalogChoice,
        catalogs: List<CatalogChoice>,
        state: Boolean = false,
    ): String {
        val source = if (state) catalog.source else twinSource(catalog, catalogs, { it.name }, { it.source })
        return source?.let { "${catalog.name} from $it" } ?: catalog.name
    }

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
