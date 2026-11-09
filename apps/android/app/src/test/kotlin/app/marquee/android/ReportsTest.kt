package app.marquee.android

import kotlin.test.Test
import kotlin.test.assertEquals

/** Device fields in the relay's charset (plans/error-reports.md 3.1): a model the relay would refuse is cleaned. */
class ReportsTest {
    @Test
    fun `device words keep the relay's charset`() {
        assertEquals("Google Pixel 9 Pro", Reports.plain("Google Pixel 9 Pro", 60))
        assertEquals("Amazon AFTKA", Reports.plain("Amazon AFTKA", 60))
        assertEquals("Xiaomi  23013RK75C", Reports.plain("Xiaomi /23013RK75C", 60))
        assertEquals("Android 15", Reports.plain("Android 15", 40))
        assertEquals("abc", Reports.plain("abcdef", 3))
        assertEquals("", Reports.plain("Robin’s", 0))
    }
}
