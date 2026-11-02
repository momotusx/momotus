package app.marquee.core

/** The words of Search when nothing came back, equal to the web's. */
object SearchCopy {
    /** No installed addon has a search catalog (the core's `searchable` is 0): nothing could match. */
    const val NONE_CAN_SEARCH = "None of your addons can search."

    /** The way on when nothing came back, from [NONE_CAN_SEARCH] or no match. */
    const val RECOMMENDED = "Recommended addons"

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
