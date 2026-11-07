package app.marquee.core

import app.marquee.core.model.AddonInfo

/** Settings' addon words (plans/m13-turkcealtyazi.md 6.1), as the web's `addonHost`. */
object AddonsCopy {
    /** In place of the host on a row the core runs itself (TurkceAltyazi.org). */
    const val BUILT_IN = "Built into Momotus, runs on this device"

    /** Opens a row's host: the row itself names the addon by its own name, never a raw host. */
    const val DETAILS = "Details"

    /** A row's second line: where a built-in addon runs and the version; never a host (the URL may hold keys). */
    fun caption(addon: AddonInfo): String = if (addon.builtIn) "$BUILT_IN · ${addon.version}" else addon.version

    /**
     * Settings' Addons row (A1): how many are installed and how many of the ones on find streams, with no addon's
     * name (no addon is named where a viewer browses).
     */
    fun summary(addons: List<AddonInfo>): String {
        if (addons.isEmpty()) return "None installed yet."
        val streams = addons.count { it.enabled && "stream" in it.provides }
        val finds =
            when (streams) {
                0 -> "None of them finds streams yet."
                1 -> "1 finds streams."
                else -> "$streams find streams."
            }
        return "${addons.size} installed. $finds"
    }
}
