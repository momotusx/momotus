package app.marquee.core

import app.marquee.core.model.EmptyKind
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

    /**
     * Nothing can be played: [reason] in words, and when the core names why, its [kind] (the failure card takes
     * that state's actions from the shared table) and the version this device keeps for "Play from this device".
     */
    data class None(
        val reason: String,
        val kind: EmptyKind? = null,
        val key: String? = null,
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
    // The Archive's and the plugins' sections too: done can wait for the plugins' pass.
    val known = listOfNotNull(snapshot.recommended) + snapshot.rows + snapshot.archive.rows + snapshot.plugins.rows
    if (chosen != null) {
        known.firstOrNull { it.key == chosen }?.let { return PlayPick.Use(it) }
        // Not in this list (a new view after the app restarted): wait for every addon, then let the core say.
        if (snapshot.done) return PlayPick.Open(chosen)
        return PlayPick.Wait(StreamChoiceCopy.FINDING_CHOSEN)
    }
    val best = snapshot.recommended
    val ready = snapshot.done || snapshot.frozen || settled
    return when {
        best != null && ready -> {
            PlayPick.Use(best)
        }

        // Nothing can play: why, in the core's words (plans/play-every-version.md 2a).
        snapshot.empty != null -> {
            PlayPick.None(StreamChoiceCopy.emptyLine(snapshot.empty.kind), snapshot.empty.kind, snapshot.empty.key)
        }

        // Every addon answered, versions are listed and none is recommended: sources that need a download first.
        best == null && snapshot.done && snapshot.rows.isNotEmpty() -> {
            PlayPick.None("No version is ready to play yet")
        }

        else -> {
            PlayPick.Wait(StreamChoiceCopy.FINDING_BEST)
        }
    }
}
