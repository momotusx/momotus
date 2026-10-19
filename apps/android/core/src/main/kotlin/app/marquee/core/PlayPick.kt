package app.marquee.core

import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionsSnapshot

/** What the player does with the ranked versions it has so far. */
sealed interface PlayPick {
    /** Play this one. */
    data class Use(
        val row: VersionRow,
    ) : PlayPick

    /** Nothing yet: say [line] and wait for the next snapshot (or for the settle time). */
    data class Wait(
        val line: String,
    ) : PlayPick

    /** Nothing can be played. */
    data class None(
        val reason: String,
    ) : PlayPick
}

/**
 * Chooses the version to play, like the web player does. A version [chosen] on Versions or the
 * title page plays as soon as it is in the list. Otherwise the recommendation plays once it has
 * settled: every addon answered, or the viewer pinned it, or [settled] (it stood unchanged for a
 * few seconds), so a late better source does not start the film twice.
 *
 * @param snapshot the versions so far; null while the view is opening.
 * @param settled the recommendation has not changed for the settle time.
 */
fun pickVersion(
    snapshot: VersionsSnapshot?,
    chosen: String?,
    settled: Boolean,
): PlayPick {
    if (snapshot == null) return PlayPick.Wait("Opening")
    val known = listOfNotNull(snapshot.recommended) + snapshot.rows
    chosen?.let { key -> known.firstOrNull { it.key == key }?.let { return PlayPick.Use(it) } }
    val best = snapshot.recommended
    val ready = snapshot.done || snapshot.frozen || settled
    return when {
        // The chosen one is not in this list (a new view after the app restarted): wait for the addons, then the best.
        best != null && ready -> {
            PlayPick.Use(best)
        }

        // Every addon answered and nothing is recommended: sources that need a download first, or none at all.
        best == null && snapshot.done -> {
            PlayPick.None(
                when {
                    // Versions were found and none plays here: say why (3.11).
                    snapshot.blocked != null -> VersionsCopy.blockedText(snapshot) ?: "No playable version found"

                    snapshot.rows.isEmpty() -> "No playable version found"

                    else -> "No version is ready to play yet"
                },
            )
        }

        snapshot.pendingAddons.isNotEmpty() -> {
            PlayPick.Wait(
                "Asking ${snapshot.pendingAddons.joinToString(", ")} for versions",
            )
        }

        else -> {
            PlayPick.Wait("Choosing the best version")
        }
    }
}
