package app.marquee.core

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

    /** "Movie · 2026": the line under a hero's reason. */
    fun heroMeta(tile: Tile): String = listOfNotNull(kind(tile.kind), tile.releaseInfo).joinToString(" · ")
}
