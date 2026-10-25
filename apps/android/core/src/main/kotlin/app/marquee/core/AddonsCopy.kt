package app.marquee.core

import app.marquee.core.model.AddonInfo

/** Settings' addon words (plans/m13-turkcealtyazi.md 6.1), as the web's `addonHost` and `turkceAltyaziCredit`. */
object AddonsCopy {
    /** In place of the host on a row the core runs itself (TurkceAltyazi.org). */
    const val BUILT_IN = "Built into Momotus, runs on this device"

    /** Settings, About: always shown. */
    const val TURKCEALTYAZI_CREDIT =
        "Turkish subtitles from TurkceAltyazi.org, fetched by this device when you play."

    /** A row's second line: where the addon runs and its version; the host stands in for the URL, which may hold keys. */
    fun caption(addon: AddonInfo): String = "${if (addon.builtIn) BUILT_IN else addon.host} · ${addon.version}"
}
