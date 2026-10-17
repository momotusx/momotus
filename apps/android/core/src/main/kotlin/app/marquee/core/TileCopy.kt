package app.marquee.core

import app.marquee.core.model.Tile
import kotlin.math.roundToInt

/** The words under a tile and in a hero, the same as the web family's `lib/words.ts`. */
object TileCopy {
    /** Id of the Continue Watching row: its tiles are 16:9 stills with the logo on them. */
    const val CONTINUE_ROW = "continue-watching"

    /** What a row's source label says: the library for Continue Watching, else the add-on's name. */
    fun source(
        rowId: String,
        source: String,
    ): String = if (rowId == CONTINUE_ROW) "From your library" else source

    /**
     * Continue Watching says where you are: "Next: S1 E2" for Up Next, else the episode (or
     * "Movie") and how much is watched. Null for a tile with no progress.
     */
    fun caption(tile: Tile): String? {
        tile.releaseInfo?.takeIf { it.startsWith("Next:") }?.let { return it }
        val progress = tile.progress ?: return null
        val ref = tile.videoId?.let(Format::parseVideo)
        val where =
            when {
                ref != null -> Format.episodeLabel(ref.season, ref.episode)
                tile.kind == "movie" -> "Movie"
                else -> null
            }
        val watched = "${(progress * 100).roundToInt()}% watched"
        return if (where != null) "$where, $watched" else watched
    }

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
