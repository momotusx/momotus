package app.marquee.core

import app.marquee.core.model.Configuration
import app.marquee.core.model.Installed
import app.marquee.core.model.ListedAddon
import java.util.Locale

/**
 * What a Find addons card shows and does, from the core's entry. The core sends facts
 * (`installed`, `configuration`); these rules turn them into one action, the same on every app
 * (plans/m12-addon-discovery.md 3.2; the web's `lib/find-addons.ts`).
 */
object FindAddons {
    enum class Filter(
        val label: String,
        /** What an entry must provide to show; null shows every entry. */
        val provides: String?,
        /** The word in "No subtitle addons in this list." */
        val kind: String?,
    ) {
        All(FindAddonsCopy.ALL, null, null),
        Sources(FindAddonsCopy.SOURCES, "stream", FindAddonsCopy.KIND_SOURCES),
        Subtitles(FindAddonsCopy.SUBTITLES, "subtitles", FindAddonsCopy.KIND_SUBTITLES),
        Catalogs(FindAddonsCopy.CATALOGS, "catalog", FindAddonsCopy.KIND_CATALOGS),
    }

    fun shows(
        entry: ListedAddon,
        filter: Filter,
    ): Boolean = filter.provides == null || filter.provides in entry.provides

    /** "No subtitle addons in this list."; null for All, which only an empty list leaves empty. */
    fun filterEmpty(filter: Filter): String? = filter.kind?.let(FindAddonsCopy::filterEmpty)

    /** The card's action. A TV shows no Configure (its card draws the help line instead). */
    sealed interface Action {
        /** Installed in some form: a word, no press. */
        data class None(
            val word: String,
        ) : Action

        /** `setAddonEnabled` on [url] (the entry's key, or a configured copy's). */
        data class TurnOn(
            val url: String,
            val word: String,
        ) : Action

        data object Configure : Action

        data class Install(
            val configure: Boolean,
        ) : Action
    }

    /**
     * First match: installed and on, none; off, Turn on that link (a configured copy's when only a
     * copy is installed, none when that copy's link was too long to send); setup required,
     * Configure; setup optional, Install and Configure; else Install.
     */
    fun action(entry: ListedAddon): Action =
        when (entry.installed) {
            Installed.On -> {
                Action.None(FindAddonsCopy.INSTALLED)
            }

            Installed.CopyOn -> {
                Action.None(FindAddonsCopy.SET_UP)
            }

            Installed.Off -> {
                Action.TurnOn(entry.key, FindAddonsCopy.OFF)
            }

            Installed.CopyOff -> {
                entry.copyKey?.let { Action.TurnOn(it, FindAddonsCopy.SET_UP_OFF) }
                    ?: Action.None(FindAddonsCopy.SET_UP_OFF)
            }

            Installed.Absent -> {
                if (entry.configuration == Configuration.Required) {
                    Action.Configure
                } else {
                    Action.Install(configure = entry.configuration == Configuration.Optional)
                }
            }
        }

    /**
     * The action with the card's last result: an install refused as needing setup asks for
     * Configure, until the entry is installed some other way (a pasted configured link), which wins.
     */
    fun cardAction(
        entry: ListedAddon,
        lastCode: String?,
    ): Action = if (entry.installed == Installed.Absent && lastCode == "needsSetup") Action.Configure else action(entry)

    /** Whether a press does something here: Install or Turn on (Configure is a page, not a press on TV). */
    fun pressable(action: Action) = action is Action.Install || action is Action.TurnOn

    /** What an addon provides, in words (the web's `providesText`). */
    fun providesText(p: String): String =
        when (p) {
            "catalog" -> "Catalogs"
            "meta" -> "Details"
            "stream" -> "Streams"
            "subtitles" -> "Subtitles"
            "addon_catalog" -> "Addon lists"
            else -> p
        }

    private fun kindWords(kind: String): String =
        when (kind) {
            "movie" -> "movies"
            "series" -> "series"
            "channel" -> "channels"
            "tv" -> "live TV"
            "anime" -> "anime"
            else -> TileCopy.kind(kind).lowercase()
        }

    /** "Catalogs, Details" */
    fun providesWords(entry: ListedAddon): String = entry.provides.joinToString(", ", transform = ::providesText)

    /** "Subtitles for movies and series". */
    fun providesLine(entry: ListedAddon): String {
        val what = providesWords(entry)
        val kinds = entry.types.map(::kindWords)
        if (kinds.isEmpty()) return what
        val head = if (kinds.size > 1) kinds.dropLast(1).joinToString(", ") + " and " + kinds.last() else kinds[0]
        return "$what for $head"
    }

    /** Names shown by more than one card of a list: their cards also say the host. */
    fun repeatedNames(entries: List<ListedAddon>): Set<String> =
        entries
            .groupingBy { it.name }
            .eachCount()
            .filterValues { it > 1 }
            .keys

    /**
     * The mark on the brand surface: the name's first letter or digit, symbols skipped; no remote
     * logo, so opening a list contacts no image host.
     */
    fun initial(name: String): String {
        var at = 0
        while (at < name.length) {
            val cp = name.codePointAt(at)
            if (Character.isLetter(cp) || Character.getType(cp) in NUMBERS) {
                return String(Character.toChars(cp)).uppercase(Locale.ENGLISH)
            }
            at += Character.charCount(cp)
        }
        return ""
    }

    /** Unicode's number categories, as the web's `\p{N}`. */
    private val NUMBERS =
        setOf(
            Character.DECIMAL_DIGIT_NUMBER.toInt(),
            Character.LETTER_NUMBER.toInt(),
            Character.OTHER_NUMBER.toInt(),
        )

    /**
     * The whole description of a card for a screen reader: its spoken label, or while a request runs
     * the name with [busyWord] ("installing") in place of the action; then the setup line and the
     * failure as sentences. A TV card is one stop, so it says all of it; a phone button says the
     * first part, the card's own lines carry the rest.
     */
    fun described(
        entry: ListedAddon,
        action: Action,
        sameName: Boolean,
        busyWord: String? = null,
        help: String? = null,
        failure: String? = null,
    ): String {
        val head = spoken(entry, busyWord?.let { Action.None(it) } ?: action, sameName)
        if (help == null && failure == null) return head
        return listOfNotNull(head, help, failure).joinToString(" ") { if (it.endsWith(".")) it else "$it." }
    }

    /**
     * What the card does, in words, for a screen reader: "Install OpenSubtitles v3, subtitles",
     * with the host when another card in the list has the same name. An installed card says its
     * state instead of an action.
     */
    fun spoken(
        entry: ListedAddon,
        action: Action,
        sameName: Boolean,
    ): String {
        val verb =
            when (action) {
                is Action.None -> null
                is Action.TurnOn -> FindAddonsCopy.TURN_ON
                Action.Configure -> FindAddonsCopy.CONFIGURE
                is Action.Install -> FindAddonsCopy.INSTALL
            }
        return listOfNotNull(
            if (verb != null) "$verb ${entry.name}" else entry.name,
            if (sameName) FindAddonsCopy.fromHost(entry.host) else null,
            providesWords(entry).lowercase(),
            (action as? Action.None)?.word?.lowercase(),
        ).filter { it.isNotEmpty() }
            .joinToString(", ")
    }
}
