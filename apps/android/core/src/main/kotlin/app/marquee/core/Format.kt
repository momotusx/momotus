package app.marquee.core

import java.util.Locale

/** Small pure formatters shared by both UI trees (the Kotlin twin of the web's `lib/format.ts`). */
object Format {
    /**
     * A stream's size in the 1024-based units addons print, as the web's `bytes`: 5 GiB is "5 GB" (the
     * size limit reads the same), one decimal under 100 GB without a trailing ".0", MB under 1 GB.
     */
    fun bytes(n: Long): String {
        val gb = n / GIB
        return when {
            gb >= 100 -> "${Math.round(gb)} GB"
            gb >= 1 -> "${String.format(Locale.US, "%.1f", gb).removeSuffix(".0")} GB"
            n >= MIB -> "${Math.round(n / MIB)} MB"
            else -> "${maxOf(1L, Math.round(n / 1024.0))} KB"
        }
    }

    private const val GIB = 1024.0 * 1024 * 1024
    private const val MIB = 1024.0 * 1024

    /** 161 -> "2:41", 3725 -> "1:02:05". */
    fun clock(seconds: Long): String {
        val s = maxOf(0L, seconds)
        val h = s / 3600
        val m = (s % 3600) / 60
        val ss = (s % 60).toString().padStart(2, '0')
        return if (h > 0) "$h:${m.toString().padStart(2, '0')}:$ss" else "$m:$ss"
    }

    data class VideoRef(
        val id: String,
        val season: Int,
        val episode: Int,
    )

    private val videoId = Regex("^(.+):(\\d+):(\\d+)$")

    /** Stremio series video ids are `<id>:<season>:<episode>`; anything else is a movie. */
    fun parseVideo(videoId: String): VideoRef? {
        val m = this.videoId.matchEntire(videoId) ?: return null
        // A very long digit run is not a season or an episode.
        val season = m.groupValues[2].toIntOrNull() ?: return null
        val episode = m.groupValues[3].toIntOrNull() ?: return null
        return VideoRef(m.groupValues[1], season, episode)
    }

    fun episodeLabel(
        season: Int,
        episode: Int,
    ) = "S$season:E$episode"

    fun seasonName(season: Int) = if (season == 0) "Specials" else "Season $season"

    /** Cinemeta runtimes look like "57 min" or "1h 52min". */
    fun runtimeMinutes(runtime: String?): Int? {
        if (runtime.isNullOrBlank()) return null
        val h =
            Regex("(\\d+)\\s*h")
                .find(runtime)
                ?.groupValues
                ?.get(1)
                ?.toInt() ?: 0
        val m =
            Regex("(\\d+)\\s*m")
                .find(runtime)
                ?.groupValues
                ?.get(1)
                ?.toInt() ?: 0
        return (h * 60 + m).takeIf { it > 0 }
    }

    /** 0.0 to 1.0, clamped. */
    fun clamp01(x: Float) = x.coerceIn(0f, 1f)

    /**
     * Release names break between tokens so a focused row never truncates: after a dot unless a
     * digit follows ("5.1", "H.265", "S02E03.2160p" stay whole), and before the release group.
     */
    fun releaseParts(name: String): List<String> =
        name
            .replace(Regex("\\.(?!\\d)"), ".\u0000")
            .replace(Regex("-(?=[^-.]+$)"), "\u0000-")
            .split('\u0000')

    /** [releaseParts] joined with zero-width spaces: one string a text view may break between tokens. */
    fun releaseWrapped(name: String): String = releaseParts(name).joinToString("​")
}
