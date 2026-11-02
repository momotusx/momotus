package app.marquee.core

/** The words of Search when nothing came back, equal to the web's. */
object SearchCopy {
    /** No installed addon has a search catalog (the core's `searchable` is 0): nothing could match. */
    const val NONE_CAN_SEARCH = "None of your addons can search."

    /** The way on when nothing came back, from [NONE_CAN_SEARCH] or no match. */
    const val RECOMMENDED = "Recommended addons"

    /** Under the field before a query, where nothing else shows. */
    const val HINT = "Type a title. Results come from every addon with a search catalog."

    /** A kids session searches its own list. */
    const val KIDS_HINT = "Type a title from your list."

    /** The clear control of the recent searches. */
    const val CLEAR_RECENT = "Clear recent searches"

    /**
     * The one line before a query, and only where nothing else shows: no recent searches and no shelf of titles
     * to start from (owner, 2026-11-02).
     */
    fun idleHint(
        recent: List<String>,
        hasStart: Boolean,
        kids: Boolean = false,
    ): String? =
        when {
            recent.isNotEmpty() || hasStart -> null
            kids -> KIDS_HINT
            else -> HINT
        }

    /**
     * Nothing came back for [query]: no addon can search ([searchable] 0), or none of them matched. A kids session
     * searches its own list, and never hears of addons ([KidsCopy.SEARCH_NONE]).
     */
    fun empty(
        query: String,
        searchable: Int,
        kids: Boolean = false,
    ): String =
        when {
            kids -> KidsCopy.SEARCH_NONE
            searchable == 0 -> NONE_CAN_SEARCH
            else -> "No matches for \"$query\" in your catalogs."
        }
}
