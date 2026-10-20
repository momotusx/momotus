package app.marquee.core

import kotlin.test.Test
import kotlin.test.assertEquals

/** Search with nothing to show, as the web words it (551362d4). */
class SearchCopyTest {
    @Test
    fun `with no addon that can search it says so, not No matches`() {
        assertEquals("None of your addons can search.", SearchCopy.empty("journey", searchable = 0))
        assertEquals("No matches for \"zqxj\" in your catalogs.", SearchCopy.empty("zqxj", searchable = 2))
    }
}
