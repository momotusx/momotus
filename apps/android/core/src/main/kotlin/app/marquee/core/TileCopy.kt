package app.marquee.core

import app.marquee.core.model.Hero
import app.marquee.core.model.Tile

/** The words under a tile and in a hero, the same as the web family's `lib/words.ts`. */
object TileCopy {
    /** Id of the Continue Watching row: its tiles are 16:9 stills with the logo on them. */
    const val CONTINUE_ROW = "continue-watching"

    /** What a row's source label says: the library for Continue Watching, else the addon's name. */
    fun source(
        rowId: String,
        source: String,
    ): String = if (rowId == CONTINUE_ROW) "From your library" else source

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
     * The facts after a hero's star rating: "2026 · 145 min · Action, Adventure". The kind
     * stands in when the addon gave no year, runtime or genre ("Series").
     */
    fun heroMeta(hero: Hero): String {
        val facts = listOfNotNull(hero.tile.releaseInfo, hero.runtime, hero.genres.joinToString(", ").ifEmpty { null })
        return facts.ifEmpty { listOf(kind(hero.tile.kind)) }.joinToString(" · ")
    }
}
