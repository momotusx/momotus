package app.marquee.core

import app.marquee.core.model.CalendarItem
import app.marquee.core.model.CalendarSeries
import java.util.Calendar
import java.util.TimeZone
import kotlin.test.Test
import kotlin.test.assertEquals

class CalendarCopyTest {
    private fun at(
        zone: String,
        y: Int,
        m: Int,
        d: Int,
        h: Int,
        min: Int = 0,
    ) = Calendar.getInstance(TimeZone.getTimeZone(zone)).apply {
        clear()
        set(y, m - 1, d, h, min)
    }

    @Test
    fun `the local day is the date where the device is, not UTC`() {
        // 23:30 on the 17th in Los Angeles is already the 18th in UTC.
        assertEquals("2026-10-17", CalendarCopy.localDay(at("America/Los_Angeles", 2026, 10, 17, 23, 30)))
        assertEquals("2026-01-05", CalendarCopy.localDay(at("Europe/Istanbul", 2026, 1, 5, 0, 10)))
    }

    @Test
    fun `the timer fires one second past the next local midnight`() {
        assertEquals(30 * 60_000L + 1_000, CalendarCopy.millisToMidnight(at("Europe/Istanbul", 2026, 10, 17, 23, 30)))
        // The night the clocks go back in Berlin is 25 hours long.
        assertEquals(25 * 3_600_000L + 1_000, CalendarCopy.millisToMidnight(at("Europe/Berlin", 2026, 10, 25, 0)))
    }

    @Test
    fun `days near today have names and the rest their weekday`() {
        assertEquals("Today", CalendarCopy.dayName("2026-10-17", "2026-10-17"))
        assertEquals("Tomorrow", CalendarCopy.dayName("2026-10-18", "2026-10-17"))
        assertEquals("Yesterday", CalendarCopy.dayName("2026-10-16", "2026-10-17"))
        assertEquals("Friday", CalendarCopy.dayName("2026-10-23", "2026-10-17"))
        assertEquals("Tomorrow", CalendarCopy.dayName("2027-01-01", "2026-12-31"))
        assertEquals("October 17", CalendarCopy.dayDate("2026-10-17"))
    }

    @Test
    fun `the lede spans a week back to a month ahead`() {
        assertEquals(
            "New episodes of the series in your library, Oct 10 to Nov 16.",
            CalendarCopy.lede("2026-10-17"),
        )
    }

    private fun item(
        season: Int?,
        episode: Int?,
    ) = CalendarItem(
        itemId = "tt1",
        videoId = "tt1:1:3",
        name = "Andor",
        season = season,
        episode = episode,
        title = "Reckoning",
        date = "2026-10-17",
        watched = false,
    )

    @Test
    fun `an episode line has its numbers when both are known`() {
        assertEquals("S1:E3 · Reckoning", CalendarCopy.episodeLine(item(1, 3)))
        assertEquals("Reckoning", CalendarCopy.episodeLine(item(1, null)))
    }

    @Test
    fun `failed series carry their reason`() {
        val series = listOf(CalendarSeries("a", "Andor", "timed out"), CalendarSeries("b", "Severance"))
        assertEquals("Could not load Andor (timed out).", CalendarCopy.failed(series[0]))
        assertEquals("Could not load Severance.", CalendarCopy.failed(series[1]))
        assertEquals("Try again for Severance", CalendarCopy.retrySpoken(series[1]))
        assertEquals("No air dates known for Andor, Severance.", CalendarCopy.noSchedule(series))
        assertEquals("No series in your library yet. Series you add show their episodes here.", CalendarCopy.empty(0))
        assertEquals("Nothing airs in these weeks for the series in your library.", CalendarCopy.empty(2))
    }
}
