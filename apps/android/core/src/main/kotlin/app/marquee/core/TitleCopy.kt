package app.marquee.core

import app.marquee.core.model.BlockedReason
import app.marquee.core.model.Episode
import app.marquee.core.model.PlayTarget
import app.marquee.core.model.TitleDetails
import app.marquee.core.model.VersionsSnapshot
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import kotlin.math.roundToInt

/** What a title page's primary button is for. */
sealed interface PrimaryAction {
    /** The words on the button. */
    val label: String

    /** Play (or Resume): [label] says which. */
    data class Play(
        override val label: String,
    ) : PrimaryAction

    /** Every addon answered and some versions exist, but none is ready to play: pick one. */
    data object ChooseVersion : PrimaryAction {
        override val label = "Choose a version"
    }

    /** Every addon answered and none gave a version: the only useful step is adding a source. */
    data object AddSource : PrimaryAction {
        override val label = "Add a source"
    }

    /** Versions were found, and this build plays them only through a streaming service account (3.11). */
    data object AddAccount : PrimaryAction {
        override val label = StreamChoiceCopy.BLOCKED.getValue(BlockedReason.NeedsAccount).second
    }
}

/** The words of the title page, shared by both UI trees and equal to the web family's. */
object TitleCopy {
    private const val NBSP = ' '

    /**
     * "Play", "Play S1:E1", "Resume S1:E2 · 36 min left" (from what the file has left, the figure
     * Continue Watching uses, else from the runtime when the metadata has one), else
     * "Resume S1:E2 from 41:12".
     */
    fun playLabel(
        target: PlayTarget,
        runtime: String? = null,
    ): String {
        val resumed = target.resumeMs?.takeIf { it > 0 } ?: return target.label
        val total = Format.runtimeMinutes(runtime)
        return when {
            target.leftMs != null && target.leftMs > 0 -> "${target.label} · ${minutesLeft(target.leftMs)}"
            total != null -> "${target.label} · ${minutesLeft(total, resumed / 60_000.0)}"
            else -> "${target.label} from ${Format.clock(resumed / 1000)}"
        }
    }

    private fun minutesLeft(
        total: Int,
        playedMinutes: Double,
    ) = "${(total - playedMinutes).roundToInt().coerceAtLeast(1)} min left"

    /** Whole minutes rounded up, as the core words Continue Watching. */
    private fun minutesLeft(leftMs: Long) = "${((leftMs + 59_999) / 60_000).coerceAtLeast(1)} min left"

    /**
     * What the primary button does. Play stays while versions are still arriving (the line under it
     * says so); once every addon has answered and nothing plays, it is "Add a source" (no versions at
     * all) or "Choose a version" (some, none ready), never a Play that can only fail.
     */
    fun primary(
        target: PlayTarget?,
        versions: VersionsSnapshot?,
        runtime: String? = null,
    ): PrimaryAction =
        when {
            versions != null && versions.done && versions.recommended == null -> {
                when {
                    versions.blocked?.reason == BlockedReason.NeedsAccount -> PrimaryAction.AddAccount
                    versions.rows.isEmpty() -> PrimaryAction.AddSource
                    else -> PrimaryAction.ChooseVersion
                }
            }

            else -> {
                PrimaryAction.Play(target?.let { playLabel(it, runtime) } ?: "Play")
            }
        }

    /** How many real seasons there are (Specials do not count). */
    fun seasonCount(seasons: List<Int>): Int = seasons.count { it > 0 }

    /** The seasons in the order they are shown: 1, 2, 3 and so on, then Specials (season 0) last. */
    fun seasonOrder(seasons: List<Int>): List<Int> = seasons.filter { it > 0 }.sorted() + seasons.filter { it == 0 }

    /** With more than five seasons the selector shows bare numbers under a "Season" label. */
    fun compactSeasons(seasons: List<Int>): Boolean = seasons.size > 5

    fun seasonLabel(
        season: Int,
        compact: Boolean,
    ): String =
        when {
            season == 0 -> "Specials"
            compact -> "$season"
            else -> "Season $season"
        }

    /**
     * "2010 · Animation, Short · 52 min" (a series says "2 seasons" instead of a runtime), after the
     * star rating: what the metadata gives, nothing invented. Each item is one unit that never breaks inside, so a
     * long line wraps only after a dot.
     */
    fun meta(
        details: TitleDetails,
        seasons: Int = 0,
    ): String =
        listOfNotNull(
            details.releaseInfo,
            details.genres
                .take(3)
                .takeIf { it.isNotEmpty() }
                ?.joinToString(", "),
            seasons.takeIf { it > 0 }?.let { "$it ${if (it == 1) "season" else "seasons"}" }
                ?: details.runtime.takeIf { details.kind == "movie" },
        ).joinToString("$NBSP· ") { it.replace(' ', NBSP) }

    /** The facts table: label and value, in the web's words, left out when empty. */
    fun facts(
        details: TitleDetails,
        seasons: Int = 0,
    ): List<Pair<String, String>> =
        listOf(
            (if (details.kind == "movie") "Released" else "Aired") to (details.releaseInfo ?: ""),
            "Seasons" to (seasons.takeIf { it > 0 }?.toString() ?: ""),
            "Runtime" to (details.runtime?.let { if (details.kind == "movie") it else "$it an episode" } ?: ""),
            "Directed by" to details.directors.take(2).joinToString(", "),
            "Starring" to details.cast.take(3).joinToString(", "),
            "Details from" to details.source,
        ).filter { it.second.isNotBlank() }

    /** "Watched, S1:E2, The Kingsroad" (and "Airs Nov 3, 2026" when it is not out): what a screen reader says for an episode. */
    fun episode(
        episode: Episode,
        now: Long = System.currentTimeMillis(),
    ): String =
        listOfNotNull(
            "Watched".takeIf { episode.watched },
            Format.episodeLabel(episode.season, episode.episode),
            episode.title.takeIf { it.isNotBlank() },
            airs(episode, now),
        ).joinToString(", ")

    /**
     * False while the episode's release lies ahead, as on the web: its card says when it airs, and a
     * press says so instead of opening a player that can only fail. No date, or one that does not read: out.
     */
    fun aired(
        episode: Episode,
        now: Long = System.currentTimeMillis(),
    ): Boolean = releaseMillis(episode.released)?.let { it <= now } ?: true

    /** "Airs Nov 3, 2026" for an episode that is not out yet, else null. */
    fun airs(
        episode: Episode,
        now: Long = System.currentTimeMillis(),
    ): String? {
        val at = releaseMillis(episode.released)?.takeIf { it > now } ?: return null
        // A release is a date, not an instant: read in UTC, it stays the day it names west of UTC too.
        return "Airs " + SimpleDateFormat("MMM d, yyyy", Locale.US).apply { timeZone = UTC }.format(Date(at))
    }

    private val UTC = TimeZone.getTimeZone("UTC")

    // The core passes the addon's release as given: an ISO instant ("2026-11-03T00:00:00.000Z"), sometimes a bare date.
    private fun releaseMillis(released: String?): Long? {
        if (released.isNullOrBlank()) return null
        for (pattern in listOf("yyyy-MM-dd'T'HH:mm:ss.SSSX", "yyyy-MM-dd'T'HH:mm:ssX", "yyyy-MM-dd")) {
            val format = SimpleDateFormat(pattern, Locale.ROOT).apply { timeZone = UTC }
            runCatching { format.parse(released) }.getOrNull()?.let { return it.time }
        }
        return null
    }

    /**
     * "Watched", "36 min left", "Airs Nov 3, 2026", "52 min" (the runtime is the episode's), or null when
     * nothing is known.
     */
    fun episodeNote(
        episode: Episode,
        runtime: String?,
        now: Long = System.currentTimeMillis(),
    ): String? {
        val total = Format.runtimeMinutes(runtime)
        val airs = airs(episode, now)
        return when {
            episode.watched -> {
                "Watched"
            }

            episode.leftMs != null && episode.leftMs > 0 -> {
                minutesLeft(episode.leftMs)
            }

            episode.progress != null -> {
                total?.let { minutesLeft(it, it * episode.progress.toDouble()) }
                    ?: "${(episode.progress * 100).roundToInt()}% watched"
            }

            airs != null -> {
                airs
            }

            else -> {
                total?.let { "$it min" }
            }
        }
    }

    /** The line under an episode card's title: "S1:E2 · 36 min left". */
    fun episodeCaption(
        episode: Episode,
        runtime: String?,
    ): String =
        listOfNotNull(
            Format.episodeLabel(episode.season, episode.episode),
            episodeNote(episode, runtime),
        ).joinToString(" · ")
}
