package app.marquee.core

/**
 * Find addons copy (plans/m12-addon-discovery.md 3.4), word for word the web's `findAddons` and
 * Apple's `FindAddonsWords`; all three are checked against `apps/shared/find-addons-words.json`.
 * `{kind}` and `{host}` are filled in by [filterEmpty] and [fromHost].
 */
object FindAddonsCopy {
    const val EYEBROW = "Addons"
    const val TITLE = "Find addons"
    const val LEDE =
        "Most of these add subtitles and catalogs. Sources usually come as a link from the addon's own website."
    const val COMMUNITY =
        "Community addons are made by other people. They see what you open in them, and their catalogs show on " +
            "Home on all your devices."
    const val CONFIGURE_HELP = "Set it up on its website, copy the link it gives you, then paste it here."
    const val CONFIGURE_TV =
        "Set it up on its website, then add the link in Marquee on your phone or computer. With sync on, it " +
            "follows you here."
    const val PASTE_LINK = "Paste a link"
    const val LIST_LABEL = "List"
    const val OFFICIAL = "Official"
    const val COMMUNITY_LIST = "Community"
    const val SHOW_LABEL = "Show"
    const val ALL = "All"
    const val SOURCES = "Sources"
    const val SUBTITLES = "Subtitles"
    const val CATALOGS = "Catalogs"
    const val LOADING = "Asking Cinemeta"
    const val FAILED = "Cinemeta did not answer."
    const val TRY_AGAIN = "Try again"
    const val REFRESH = "Refresh"
    const val EMPTY = "Nothing to show from Cinemeta here."
    const val FILTER_EMPTY = "No {kind} addons in this list."
    const val KIND_SOURCES = "source"
    const val KIND_SUBTITLES = "subtitle"
    const val KIND_CATALOGS = "catalog"
    const val INSTALLING = "Installing"
    const val TURNING_ON = "Turning on"
    const val INSTALLED = "Installed"
    const val OFF = "Off"
    const val SET_UP = "Set up"
    const val SET_UP_OFF = "Set up, off"
    const val INSTALL = "Install"
    const val CONFIGURE = "Configure"
    const val TURN_ON = "Turn on"
    const val FROM_HOST = "from {host}"

    /** Android only, not in the shared words: Configure where no app opens web pages. */
    const val NO_BROWSER = "No app here opens web pages. Open the addon's website on another device."

    /** "No subtitle addons in this list." */
    fun filterEmpty(kind: String) = FILTER_EMPTY.replace("{kind}", kind)

    fun fromHost(host: String) = FROM_HOST.replace("{host}", host)
}
