package app.marquee.core

import app.marquee.core.model.CalendarItem
import app.marquee.core.model.CalendarSeries
import java.util.Calendar
import java.util.Locale

/**
 * The words and dates of the Calendar screen, equal to the web's. Days are `YYYY-MM-DD` local
 * dates, never shifted to UTC; java.util.Calendar, since java.time needs API 26.
 */
object CalendarCopy {
    /** The local date of [now]. */
    fun localDay(now: Calendar = Calendar.getInstance()): String =
        "%04d-%02d-%02d".format(
            Locale.ROOT,
            now.get(Calendar.YEAR),
            now.get(Calendar.MONTH) + 1,
            now.get(Calendar.DAY_OF_MONTH),
        )

    /** How long until one second past the next local midnight. */
    fun millisToMidnight(now: Calendar = Calendar.getInstance()): Long {
        val next = now.clone() as Calendar
        next.add(Calendar.DAY_OF_MONTH, 1)
        next.set(Calendar.HOUR_OF_DAY, 0)
        next.set(Calendar.MINUTE, 0)
        next.set(Calendar.SECOND, 1)
        next.set(Calendar.MILLISECOND, 0)
        return next.timeInMillis - now.timeInMillis
    }

    /** "Today", "Tomorrow", "Yesterday", else the weekday. */
    fun dayName(
        day: String,
        today: String,
    ): String =
        when (daysBetween(today, day)) {
            0 -> "Today"
            1 -> "Tomorrow"
            -1 -> "Yesterday"
            else -> name(day, Calendar.DAY_OF_WEEK, Calendar.LONG)
        }

    /** "October 17". */
    fun dayDate(day: String): String =
        "${name(day, Calendar.MONTH, Calendar.LONG)} ${parse(day).get(Calendar.DAY_OF_MONTH)}"

    /** "New episodes of the series in your library, Oct 10 to Nov 16." */
    fun lede(today: String): String =
        "New episodes of the series in your library, ${short(today, -7)} to ${short(today, 30)}."

    /** "S1:E3 · Pilot", or the title alone when the numbers are unknown. */
    fun episodeLine(item: CalendarItem): String {
        val label = item.season?.let { season -> item.episode?.let { Format.episodeLabel(season, it) } }
        return label?.let { "$it · ${item.title}" } ?: item.title
    }

    /** "Could not load Severance (timed out), Andor." */
    fun failed(series: List<CalendarSeries>): String =
        "Could not load ${series.joinToString(", ") { s -> s.reason?.let { "${s.name} ($it)" } ?: s.name }}."

    /** "No air dates known for Severance, Andor." */
    fun noSchedule(series: List<CalendarSeries>): String =
        "No air dates known for ${series.joinToString(", ") { it.name }}."

    private fun parse(day: String): Calendar {
        val (y, m, d) = day.split("-").map { it.toIntOrNull() ?: 1 } + listOf(1, 1, 1)
        return Calendar.getInstance().apply {
            clear()
            set(y, m - 1, d)
        }
    }

    // Noon on both days: a daylight saving change between them never rounds to the wrong day.
    private fun daysBetween(
        from: String,
        to: String,
    ): Int {
        val a = parse(from).apply { set(Calendar.HOUR_OF_DAY, 12) }
        val b = parse(to).apply { set(Calendar.HOUR_OF_DAY, 12) }
        return Math.round((b.timeInMillis - a.timeInMillis) / 86_400_000.0).toInt()
    }

    private fun name(
        day: String,
        field: Int,
        style: Int,
    ): String = parse(day).getDisplayName(field, style, Locale.ENGLISH).orEmpty()

    private fun short(
        today: String,
        offset: Int,
    ): String {
        val day = parse(today).apply { add(Calendar.DAY_OF_MONTH, offset) }
        return "${day.getDisplayName(Calendar.MONTH, Calendar.SHORT, Locale.ENGLISH)} ${day.get(Calendar.DAY_OF_MONTH)}"
    }
}
