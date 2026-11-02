package app.marquee.core

import app.marquee.core.model.ListedRepo
import app.marquee.core.model.PluginRepoInfo

/**
 * Nuvio plugin repositories' words (plans/plugins-and-marketplace.md 7.7, 7.8): the same as
 * apps/shared/plugin-words.json, which the web and Apple read too (PluginCopyTest holds them equal).
 */
object PluginCopy {
    const val GROUP = "Plugins"
    const val SECTION = "From plugins"
    const val LOOKING = "Plugins are still looking."
    const val CHOOSE = "Choose a version to play."
    const val TITLE = "Plugins"
    const val LEDE =
        "Nuvio plugin repositories. Their providers find direct links when you open Versions, and their rows play only when you choose them."
    const val FIELD_LABEL = "Repository link"
    const val ADD = "Add"
    const val CHECKING = "Reading the repository"
    const val ADDING = "Adding"
    const val ADDED = "Added {name}"
    const val COULD_NOT_ADD = "Could not add: {reason}"
    const val SHEET_TITLE = "Add {name}?"
    const val SHEET_FROM = "From {host}."
    const val SHEET_ADDS = "Adds {what}."
    const val SHEET_NOTE =
        "Plugins are made by other people. They run on this device when you open Versions, get the name and year of what you open, and can contact any public website. Their links play only when you choose them. They cannot read your files, accounts or keys."
    const val PROVIDERS = "Providers"
    const val ON = "On"
    const val OFF = "Off"
    const val REMOVE = "Remove"
    const val REMOVE_CONFIRM = "Press again to remove"
    const val EMPTY = "No repositories yet. Paste a Nuvio repository link above."
    const val UNREAD = "Could not read this repository: {problem}"
    const val LOADING = "Reading repositories"
    const val KIDS = "Kids profiles never run plugins."
    const val NOT_AVAILABLE = "Plugins are not available in this version."
    const val SUMMARY_NONE = "Add Nuvio repositories for direct links."
    const val DIRECTORY_TITLE = "Nuvio plugins"
    const val DIRECTORY_FROM = "From {source}"
    const val DIRECTORY_SEARCH = "Search plugins"
    const val INSTALLED = "Installed"
    const val DIRECTORY_NO_MATCH = "No repository matches."
    const val FOR_KINDS = "for {kinds}"
    const val LANGUAGES = "Languages: {languages}."

    /** Every word by its key in the shared file. */
    val ALL: Map<String, String> =
        mapOf(
            "group" to GROUP,
            "section" to SECTION,
            "looking" to LOOKING,
            "choose" to CHOOSE,
            "title" to TITLE,
            "lede" to LEDE,
            "fieldLabel" to FIELD_LABEL,
            "add" to ADD,
            "checking" to CHECKING,
            "adding" to ADDING,
            "added" to ADDED,
            "couldNotAdd" to COULD_NOT_ADD,
            "sheetTitle" to SHEET_TITLE,
            "sheetFrom" to SHEET_FROM,
            "sheetAdds" to SHEET_ADDS,
            "sheetNote" to SHEET_NOTE,
            "providers" to PROVIDERS,
            "on" to ON,
            "off" to OFF,
            "remove" to REMOVE,
            "removeConfirm" to REMOVE_CONFIRM,
            "empty" to EMPTY,
            "unread" to UNREAD,
            "loading" to LOADING,
            "kids" to KIDS,
            "notAvailable" to NOT_AVAILABLE,
            "summaryNone" to SUMMARY_NONE,
            "directoryTitle" to DIRECTORY_TITLE,
            "directoryFrom" to DIRECTORY_FROM,
            "directorySearch" to DIRECTORY_SEARCH,
            "installed" to INSTALLED,
            "directoryNoMatch" to DIRECTORY_NO_MATCH,
            "forKinds" to FOR_KINDS,
            "languages" to LANGUAGES,
        )

    /** `template` with each `{name}` replaced by its value as plain text. */
    fun fill(
        template: String,
        vararg values: Pair<String, Any>,
    ): String = values.fold(template) { s, (k, v) -> s.replace("{$k}", v.toString()) }

    /** "Adds 8 providers: A, B, C, D, E and 3 more." */
    fun adds(names: List<String>): String {
        val n = names.size
        val shown = names.take(5)
        val rest = n - shown.size
        val list =
            when {
                rest > 0 -> "${shown.joinToString(", ")} and $rest more"
                shown.size < 2 -> shown.joinToString("")
                else -> "${shown.dropLast(1).joinToString(", ")} and ${shown.last()}"
            }
        return fill(SHEET_ADDS, "what" to "$n ${if (n == 1) "provider" else "providers"}: $list")
    }

    /** "8 providers for films and series. Languages: tr, en." for a listed repository's card. */
    fun directoryLine(r: ListedRepo): String {
        val n = r.providers.size
        val kinds = r.types.map { if (it == "movie") "films" else it }
        val forKinds = if (kinds.isEmpty()) "" else " " + fill(FOR_KINDS, "kinds" to kinds.joinToString(" and "))
        val head = "$n ${if (n == 1) "provider" else "providers"}$forKinds."
        return if (r.languages.isEmpty()) {
            head
        } else {
            "$head " +
                fill(LANGUAGES, "languages" to r.languages.joinToString(", "))
        }
    }

    /** Lower case, accents dropped: "Türkçe" finds "turkce". */
    private fun fold(s: String) =
        java.text.Normalizer
            .normalize(s, java.text.Normalizer.Form.NFD)
            .replace(Regex("\\p{M}+"), "")
            .lowercase()

    /** Whether every word of [query] is in the repository's name, its providers' names or its languages. */
    fun matches(
        r: ListedRepo,
        query: String,
    ): Boolean {
        val text = fold((listOf(r.name) + r.providers + r.languages).joinToString(" "))
        return fold(query).split(Regex("\\s+")).filter { it.isNotEmpty() }.all { it in text }
    }

    /** The Settings row's line: repositories and the providers on in those that are on. */
    fun summary(repos: List<PluginRepoInfo>): String {
        if (repos.isEmpty()) return SUMMARY_NONE
        val on = repos.filter { it.enabled }.sumOf { r -> r.providers.count { it.enabled } }
        val n = repos.size
        return "$n ${if (n == 1) "repository" else "repositories"}, $on ${if (on == 1) "provider" else "providers"} on"
    }
}
