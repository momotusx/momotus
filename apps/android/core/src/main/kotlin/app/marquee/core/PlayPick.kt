package app.marquee.core

import app.marquee.core.model.BlockedReason
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

    /**
     * The version chosen by hand is not in a list that every addon answered: open it by its key all the same,
     * and the core says whether it is gone ("that version is no longer listed"), as the web does.
     */
    data class Open(
        val key: String,
    ) : PlayPick

    /** Nothing can be played; [blocked] when versions were found and none plays here (3.11). */
    data class None(
        val reason: String,
        val blocked: BlockedReason? = null,
    ) : PlayPick
}

/**
 * Chooses the version to play, like the web player does. A version [chosen] on Versions or the
 * title page plays as soon as it is in the list, and once every addon answered without it, it is
 * opened by its key all the same ([PlayPick.Open]); it never turns into the recommendation (a new
 * view after the app restarted lists it later, review 2026-10-20 r3 P3-8). Otherwise the
 * recommendation plays once it has settled: every addon answered, or the viewer pinned it, or
 * [settled] (it stood unchanged for a few seconds), so a late better source does not start the film
 * twice.
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
    if (chosen != null) {
        known.firstOrNull { it.key == chosen }?.let { return PlayPick.Use(it) }
        // Not in this list (a new view after the app restarted): wait for every addon, then let the core say.
        if (snapshot.done) return PlayPick.Open(chosen)
        return PlayPick.Wait(asking(snapshot) ?: "Finding this version")
    }
    val best = snapshot.recommended
    val ready = snapshot.done || snapshot.frozen || settled
    return when {
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
                snapshot.blocked?.reason,
            )
        }

        else -> {
            PlayPick.Wait(asking(snapshot) ?: "Choosing the best version")
        }
    }
}

/** "Asking Torrentio, Local for versions" while addons still answer. */
private fun asking(snapshot: VersionsSnapshot): String? =
    snapshot.pendingAddons
        .takeIf { it.isNotEmpty() }
        ?.let { "Asking ${it.joinToString(", ")} for versions" }
