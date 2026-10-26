package app.marquee.core.model

/** What a row shows instead of tiles: skeletons while it loads, text when it has nothing to list. */
sealed interface RowNotice {
    /** Tiles are on their way (loading, or slow: [slow] adds a line saying so). */
    data class Waiting(
        val slow: Boolean,
        val detail: String?,
    ) : RowNotice

    /** The row ended with nothing to show; [title] and [body] explain why. */
    data class Explained(
        val title: String,
        val body: String,
        val failed: Boolean,
    ) : RowNotice
}

/** Whether a Play on this tile can start at once: a known video, or the movie itself (a series has no single video). */
fun Tile.playable(): Boolean = videoId != null || kind == "movie"

/** The video a Play on this tile starts. */
fun Tile.playVideoId(): String = videoId ?: id

/** A hero page's Play is the core's (`play`, the title page's target): none while a series' episodes are unread. */
fun Hero.playable(): Boolean = play != null

/** The tile a hero page's Play opens the player with: its title, at the core's video. */
fun Hero.playTile(): Tile? = play?.let { tile.copy(videoId = it.videoId) }

/**
 * Stable keys for a row's tiles, `tile:<rowId>:<tileId>`, with `#2`, `#3` for an id the row
 * repeats (a list rejects duplicate keys). Focus memory and lists both use them, never positions.
 */
fun HomeRow.tileKeys(): List<String> {
    val seen = HashMap<String, Int>()
    return tiles.map { tile ->
        val base = "tile:$rowId:${tile.id}"
        val count = (seen[base] ?: 0) + 1
        seen[base] = count
        if (count == 1) base else "$base#$count"
    }
}

/**
 * The notice for a row, or null when it has tiles to show. A row that failed after some tiles
 * arrived keeps showing them. It never names the row's addon (design/DECISION.md, Shelves).
 */
fun HomeRow.notice(): RowNotice? =
    when (val s = state) {
        RowState.Loading -> {
            if (tiles.isEmpty()) RowNotice.Waiting(slow = false, detail = null) else null
        }

        RowState.Slow -> {
            if (tiles.isEmpty()) RowNotice.Waiting(slow = true, detail = STILL_LOADING) else null
        }

        RowState.Ready -> {
            null
        }

        RowState.Empty -> {
            if (tiles.isEmpty()) {
                RowNotice.Explained("Nothing here right now", "This row has no titles to show.", failed = false)
            } else {
                null
            }
        }

        is RowState.Failed -> {
            if (tiles.isEmpty()) {
                RowNotice.Explained(
                    ROW_FAILED,
                    // The reason goes to the log (HomeView logs it once), never on screen: it can name the addon's host.
                    "Check the network, then try again.",
                    failed = true,
                )
            } else {
                null
            }
        }
    }

/**
 * Every lane has ended and none has a tile, and at least one failed (most often no network): the
 * page has nothing to focus or open, so it offers one Try again instead of a column of failures.
 */
fun List<HomeRow>.allFailed(): Boolean =
    isNotEmpty() &&
        none { it.tiles.isNotEmpty() || it.state == RowState.Loading || it.state == RowState.Slow } &&
        any { it.state is RowState.Failed }

/**
 * The rows without the Continue Watching tile of [id] (a removal waiting for its Undo); a Continue Watching
 * row left with no tile goes too.
 */
fun List<HomeRow>.withoutRemoved(id: String?): List<HomeRow> {
    if (id == null) return this
    return mapNotNull { row ->
        if (row.rowId != app.marquee.core.TileCopy.CONTINUE_ROW) return@mapNotNull row
        val left = row.tiles.filterNot { it.id == id }
        if (left.isEmpty()) null else row.copy(tiles = left)
    }
}

/** A slow row's line under its skeletons. */
const val STILL_LOADING = "Still loading"

/** A failed row's title. */
const val ROW_FAILED = "This row did not load"
