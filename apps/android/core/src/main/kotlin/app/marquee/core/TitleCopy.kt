package app.marquee.core

import app.marquee.core.model.Episode
import app.marquee.core.model.PlayTarget
import app.marquee.core.model.TitleDetails
import app.marquee.core.model.VersionsSnapshot
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
                if (versions.rows.isEmpty()) PrimaryAction.AddSource else PrimaryAction.ChooseVersion
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

    /** "Watched, S1:E2, The Kingsroad": what a screen reader says for an episode. */
    fun episode(episode: Episode): String =
        listOfNotNull(
            "Watched".takeIf { episode.watched },
            Format.episodeLabel(episode.season, episode.episode),
            episode.title.takeIf { it.isNotBlank() },
        ).joinToString(", ")

    /** "Watched", "36 min left", "52 min" (the runtime is the episode's), or null when nothing is known. */
    fun episodeNote(
        episode: Episode,
        runtime: String?,
    ): String? {
        val total = Format.runtimeMinutes(runtime)
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
