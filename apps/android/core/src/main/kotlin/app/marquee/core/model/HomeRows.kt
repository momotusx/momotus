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
 * arrived keeps showing them.
 */
fun HomeRow.notice(): RowNotice? =
    when (val s = state) {
        RowState.Loading -> {
            if (tiles.isEmpty()) RowNotice.Waiting(slow = false, detail = null) else null
        }

        RowState.Slow -> {
            if (tiles.isEmpty()) RowNotice.Waiting(slow = true, detail = "$source is slow to answer") else null
        }

        RowState.Ready -> {
            null
        }

        RowState.Empty -> {
            if (tiles.isEmpty()) {
                RowNotice.Explained("$title is empty", "$source has nothing to list here.", failed = false)
            } else {
                null
            }
        }

        is RowState.Failed -> {
            if (tiles.isEmpty()) {
                RowNotice.Explained(
                    "$title could not load",
                    "$source did not give a list: ${s.reason}. Check the network, then check the add-on.",
                    failed = true,
                )
            } else {
                null
            }
        }
    }
