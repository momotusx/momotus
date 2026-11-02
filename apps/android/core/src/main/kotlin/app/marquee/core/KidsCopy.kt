package app.marquee.core

import app.marquee.core.model.TitleHeader
import app.marquee.core.model.TitleState

/** What a kids session reads (plans/m11-kids.md 6.4): never an addon, a source or a way to add one. */
object KidsCopy {
    /** Home with nothing allowed yet. */
    const val HOME_EMPTY_TITLE = "Nothing here yet"
    const val HOME_EMPTY = "Ask a grown-up to pick some shows for you."

    /** Search looks only through the kid's own list. */
    const val SEARCH_FIELD = "Search your shows"
    const val SEARCH_NONE = "That isn't on your list. Ask a grown-up."

    /** A title off the list (the core's notAllowed): these words and Back, nothing else. */
    const val TITLE_REFUSED = "That isn't on your list"
    const val ASK = "Ask a grown-up."

    /** The words a title page shows in place of its details, or null when it may show (QA round 1, Q7 row 28). */
    fun titleRefusal(header: TitleHeader?): Pair<String, String>? =
        (TITLE_REFUSED to ASK).takeIf { header?.state is TitleState.NotAllowed }
}
