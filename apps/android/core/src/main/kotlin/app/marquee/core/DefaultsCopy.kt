package app.marquee.core

import app.marquee.core.model.DefaultAddonInfo
import app.marquee.core.model.DefaultAddons
import app.marquee.core.model.DefaultChoice
import app.marquee.core.model.DefaultInstalled
import app.marquee.core.route.DefaultsMode

/**
 * The recommended addons step and the Home card (plans/m13-default-addons.md 6 and 7). The core
 * names and describes each addon; these are the words around them, and which switch each row shows.
 */
object DefaultsCopy {
    const val TITLE = "Recommended addons"
    const val LEAD = "Marquee starts with these. Turn off any you do not want; you can change this later in Settings."
    const val REVIEW_TITLE = "New recommended addons"
    const val REVIEW_LEAD = "Marquee now also recommends these. Turn off any you do not want."
    const val SETTINGS_LEAD = "The addons Marquee starts with. Turn any on or off, or add back one you removed."
    const val NO_CATALOG = "Home and search need a catalog addon."
    const val FROM_STREMIO = "From Stremio"
    const val TV_MORE = "Add more later from your phone or computer."
    const val CONTINUE = "Continue"
    const val SAVE = "Save"
    const val WORKING = "Installing"
    const val FAILED = "The addons could not be installed"
    const val NOT_LOADED = "The recommended addons did not load"
    const val REVIEW = "Review"
    const val NOT_NOW = "Not now"
    const val SETTINGS_ROW = "Recommended addons"
    const val SETTINGS_ROW_LINE = "Turn the addons Marquee starts with on or off, or add back one you removed."

    /** The group of the defaults that give Home and search their titles. */
    const val CATALOGS = "catalogs"

    fun group(key: String): String =
        when (key) {
            CATALOGS -> "Catalogs and details"
            "streams" -> "Streams"
            "subtitles" -> "Subtitles"
            "lists" -> "Lists"
            else -> key.replaceFirstChar { it.uppercase() }
        }

    /** The card's line: the defaults this profile does not have yet. */
    fun cardLine(missing: List<String>): String = "Marquee now also recommends ${names(missing)}."

    private fun names(list: List<String>): String =
        when (list.size) {
            0 -> ""
            1 -> list[0]
            2 -> "${list[0]} and ${list[1]}"
            else -> list.dropLast(1).joinToString(", ") + " and " + list.last()
        }

    /**
     * A row's switch before the user touches it. Onboarding and Review offer a missing default on;
     * Settings shows what the profile has, so one removed before is off until switched back on.
     */
    fun initial(
        addon: DefaultAddonInfo,
        mode: DefaultsMode,
    ): Boolean =
        if (mode == DefaultsMode.Settings) {
            addon.installed == DefaultInstalled.On
        } else {
            addon.installed != DefaultInstalled.Off
        }

    /** The rows the step shows: the Home card's Review only the missing ones. */
    fun rows(
        addons: List<DefaultAddonInfo>,
        mode: DefaultsMode,
    ): List<DefaultAddonInfo> =
        if (mode ==
            DefaultsMode.Review
        ) {
            addons.filter { it.installed == DefaultInstalled.Absent }
        } else {
            addons
        }

    /** Whether a shown row has a switch: after an import, the ones it brought keep how they are. */
    fun hasSwitch(
        addon: DefaultAddonInfo,
        mode: DefaultsMode,
    ): Boolean = mode != DefaultsMode.AfterImport || addon.installed == DefaultInstalled.Absent

    /** Whether [addon] ends up on: its switch while the step shows one, else as the profile has it. */
    fun on(
        addon: DefaultAddonInfo,
        mode: DefaultsMode,
        switches: Map<String, Boolean>,
    ): Boolean = switches[addon.key]?.takeIf { hasSwitch(addon, mode) } ?: initial(addon, mode)

    /**
     * The choices for `installDefaults`, which adds or switches only the keys it is given. Onboarding
     * names all nine (an import's own keep how they are), Review the missing ones it shows, and
     * Settings only the rows the user switched ([switches] holds every touched row).
     */
    fun choices(
        addons: List<DefaultAddonInfo>,
        mode: DefaultsMode,
        switches: Map<String, Boolean>,
    ): List<DefaultChoice> =
        when (mode) {
            DefaultsMode.Settings -> addons.filter { a -> switches[a.key]?.let { it != initial(a, mode) } == true }
            else -> rows(addons, mode)
        }.map { DefaultChoice(it.key, on(it, mode, switches)) }

    /** The switches leave no catalog addon on (as the web's `catalogMissing`): Home and search would be empty. */
    fun catalogMissing(
        addons: List<DefaultAddonInfo>,
        mode: DefaultsMode,
        switches: Map<String, Boolean>,
    ): Boolean = addons.none { it.group == CATALOGS && on(it, mode, switches) }

    /** The Home card shows once per list version, never on a kids profile, and only when one is missing. */
    fun offer(
        answer: DefaultAddons,
        seen: Int,
        kids: Boolean,
    ): Boolean = !kids && seen < answer.version && answer.addons.any { it.installed == DefaultInstalled.Absent }
}
