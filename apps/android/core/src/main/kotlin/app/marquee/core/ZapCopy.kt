package app.marquee.core

import app.marquee.core.model.Dial
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.model.ZapPick
import app.marquee.core.route.Route

/** Zap's words and its route rules (plans/zap.md 2), the same as the web's `zapWords`. */
object ZapCopy {
    const val ZAP = "Zap"
    const val WHAT = "What is this?"
    const val FROM_START = "Start from the beginning"
    const val OPEN_TITLE = "Open title"
    const val AGAIN = "Zap again"
    const val NOTHING_PLAYED = "Nothing played."
    const val NOTHING_CAME_UP = "Nothing came up."

    /** The dial's group name, spoken. */
    const val DIAL = "Zap from"

    /** The core's codes for a zap with nothing to play. */
    const val NO_ADDONS = "noAddons"
    const val NOTHING = "nothingCameUp"

    /** The zaps in a row that did not play before the quiet "Nothing played." screen. */
    const val TRIES = 3

    /** Zap shows once the profiles are known and the profile is not a kids one (plans/zap.md 2.1). */
    fun shown(profiles: ProfilesSnapshot?): Boolean =
        profiles != null && !profiles.kids && AccountRules.activeProfile(profiles)?.kids != true

    /** Whether [tries] zaps in a row that did not play end on "Nothing played." instead of another zap. */
    fun spent(tries: Int): Boolean = tries >= TRIES

    /** The dial's three steps in order, each with its word and what a screen reader says. */
    val DIALS: List<Triple<Dial, String, String>> =
        listOf(
            Triple(Dial.Same, "Same", "Same: what you watch"),
            Triple(Dial.Near, "Near", "Near: titles like yours"),
            Triple(Dial.Far, "Far", "Far: what you never open"),
        )

    /** The panel's second line for an episode: "S2:E5 · Episode name"; null for a film. */
    fun episodeLine(
        season: Int?,
        episode: Int?,
        name: String?,
    ): String? {
        if (season == null || episode == null) return null
        val label = Format.episodeLabel(season, episode)
        return if (name.isNullOrBlank()) label else "$label · $name"
    }

    /** The zap player of a pick: its start and the tries so far ride in the route. */
    fun player(
        pick: ZapPick,
        kind: String?,
        tries: Int,
    ): Route.Player =
        Route.Player(
            pick.kind,
            pick.itemId,
            pick.videoId,
            pick.name,
            zapAt = pick.startMs,
            zapTries = tries,
            zapKind = kind,
        )

    /**
     * The zap after [player]: [auto] when it failed before its first frame, which counts one more try; a press (Zap
     * again, the dial) always starts the count again.
     */
    fun next(
        player: Route.Player,
        auto: Boolean,
    ): Route.Zap = Route.Zap(player.zapKind, if (auto) player.zapTries + 1 else 0)

    /** The tries a zap route deals with: its own, until Zap again on its miss screen deals anew from 0. */
    fun tries(
        route: Route.Zap,
        deals: Int,
    ): Int = if (deals == 0) route.tries else 0

    /** The plain play of a zap's video from its start (Start from the beginning): it counts at once. */
    fun fromStart(player: Route.Player): Route.Player =
        Route.Player(player.kind, player.id, player.videoId, player.title, startOver = true)
}
