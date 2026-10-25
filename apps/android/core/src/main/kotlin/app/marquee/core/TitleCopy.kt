package app.marquee.core

import app.marquee.core.model.EmptyAction
import app.marquee.core.model.Episode
import app.marquee.core.model.PlayTarget
import app.marquee.core.model.Slot
import app.marquee.core.model.TitleDetails
import app.marquee.core.model.TitleScores
import app.marquee.core.model.VersionsSnapshot
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import kotlin.math.roundToInt

/**
 * The title page's one primary: Play (or Resume), always, never a Settings or account step (the owner, through
 * the lead, 2026-10-23; plans/play-every-version.md, the title-page row). [label] says Play or Resume.
 */
data class PrimaryAction(
    val label: String,
    /** A state whose only way on is Back (kidsNothing, noResults): in place, dimmed, not focusable, no action. */
    val inert: Boolean = false,
    /** No addon answered and this device keeps the version last played (unreachable): Play plays it. */
    val keptKey: String? = null,
)

/** Where the title's Play takes the viewer. */
sealed interface PlayTo {
    /** The player, with this version, or with none ([key] null): it opens at once and starts the core's pick. */
    data class Player(
        val key: String?,
    ) : PlayTo

    /** Versions, which holds why nothing plays, its action and the rows that can still start. */
    data object Versions : PlayTo

    /** Nowhere: there is no video, or Play is inert. */
    data object Nowhere : PlayTo
}

/** One item of the title page's ratings row: the service's [mark], its number as shown ([text] null while it is
 * pending, keeping its place), what a screen reader says, and the title's page there ([url], when the core gave a
 * checked one: the phone links it, TV never). */
data class ScoreItem(
    val mark: Mark,
    val text: String?,
    val spoken: String,
    val url: String? = null,
)

/** The services whose marks the title page's ratings row shows. */
enum class Mark { Imdb, Tmdb, Trakt }

/** The words of the title page, shared by both UI trees and equal to the web family's. */
object TitleCopy {
    /**
     * The title page's ratings row (plans/title-page-ratings-genres-similar.md): IMDb as x/10 (the addon's score),
     * TMDB and Trakt as percentages, in that order. A pending slot keeps its place; a none slot and a missing IMDb
     * score have none.
     */
    fun scoreItems(
        imdb: String?,
        scores: TitleScores?,
        imdbUrl: String? = null,
    ): List<ScoreItem> =
        buildList {
            imdb?.let { add(ScoreItem(Mark.Imdb, "$it/10", "IMDb $it out of 10", outLink(imdbUrl))) }

            fun slot(
                mark: Mark,
                name: String,
                slot: Slot?,
            ) {
                when (slot) {
                    is Slot.Shown -> {
                        val spoken = "$name ${slot.text.replace("%", " percent")}"
                        add(ScoreItem(mark, slot.text, spoken, outLink(slot.url)))
                    }

                    Slot.Pending -> {
                        add(ScoreItem(mark, null, ""))
                    }

                    else -> {
                        Unit
                    }
                }
            }
            slot(Mark.Tmdb, "TMDB", scores?.tmdb)
            slot(Mark.Trakt, "Trakt", scores?.trakt)
        }

    private val HOSTS = setOf("imdb.com", "www.imdb.com", "themoviedb.org", "www.themoviedb.org", "trakt.tv")

    /** A badge's link: only an https page on IMDb, TMDB or Trakt; anything else is no link. */
    fun outLink(url: String?): String? =
        url?.takeIf {
            runCatching { java.net.URI(it) }.getOrNull()?.let { u -> u.scheme == "https" && u.host in HOSTS } == true
        }

    /**
     * The people on the title page, each name a link to Search: at most four of the cast and two directors and
     * writers, so the TV column stays inside the safe area (plans/title-page-ratings-genres-similar.md, 4). Without
     * [cast] (the phone, which shows Starring as headshots of its own): the directors and writers only.
     */
    fun people(
        details: TitleDetails,
        cast: Boolean = true,
    ): List<Pair<String, List<String>>> =
        listOf(
            "Starring" to if (cast) details.cast.take(4).map { it.name } else emptyList(),
            "Directed by" to details.directors.take(2),
            "Written by" to details.writers.take(2),
        ).filter { it.second.isNotEmpty() }

    /** A person's initials, for a headshot TMDB has none of: the first letter of each of the first two words. */
    fun initials(name: String): String =
        name
            .split(' ')
            .filter { it.isNotEmpty() }
            .take(2)
            .joinToString("") { it.substring(0, it.offsetByCodePoints(0, 1)).uppercase() }

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
     * What Play does (plans/play-every-version.md, the title-page row in the shared table). While versions arrive
     * and once one is recommended it plays it. When nothing can play the core's line sits under it, and: unreachable
     * with a kept version plays that version; needsAccount, kidsNothing and noResults leave it inert (a Play that opens a
     * page holding only Back is a dead end); every other state opens Versions, which holds the line, its action
     * and the rows that can still start.
     */
    fun primary(
        target: PlayTarget?,
        versions: VersionsSnapshot?,
        runtime: String? = null,
    ): PrimaryAction {
        val label = target?.let { playLabel(it, runtime) } ?: "Play"
        val empty = versions?.empty?.takeIf { versions.recommended == null }
        return when (empty?.action) {
            EmptyAction.PlayFromDevice -> PrimaryAction(label, keptKey = empty.key)
            EmptyAction.None -> PrimaryAction(label, inert = true)
            else -> PrimaryAction(label)
        }
    }

    /**
     * Where Play goes: the recommended or the kept version; while versions are still being searched the player
     * at once, in the loading pattern, its pick starting when it arrives (plans/versions-android-spec.md, section
     * 6); Versions once the search is done with nothing recommended or a state says why nothing plays.
     */
    fun playTo(
        primary: PrimaryAction,
        target: PlayTarget?,
        versions: VersionsSnapshot?,
    ): PlayTo {
        val best = versions?.recommended
        val kept = primary.keptKey
        return when {
            !live(primary, target) -> PlayTo.Nowhere
            kept != null -> PlayTo.Player(kept)
            best != null -> PlayTo.Player(best.key)
            versions != null && (versions.done || versions.empty != null) -> PlayTo.Versions
            else -> PlayTo.Player(null)
        }
    }

    /** Whether Play does something: it needs the video and is never [PrimaryAction.inert]. */
    fun live(
        primary: PrimaryAction,
        target: PlayTarget?,
    ): Boolean = target != null && !primary.inert

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
        /** False on a title page, whose genres have a row of their own (design/DECISION.md, Title page). */
        genres: Boolean = true,
    ): String =
        listOfNotNull(
            details.releaseInfo,
            details.genres
                .take(3)
                .takeIf { genres && it.isNotEmpty() }
                ?.joinToString(", "),
            seasons.takeIf { it > 0 }?.let { "$it ${if (it == 1) "season" else "seasons"}" }
                ?: details.runtime.takeIf { details.kind == "movie" },
        ).joinToString("$NBSP· ") { it.replace(' ', NBSP) }

    /**
     * The facts table: label and value, in the web's words, left out when empty. Not where the details or the
     * scores came from: no addon's name on a title page (design/DECISION.md, Shelves). The people are [people],
     * links of their own.
     */
    fun facts(
        details: TitleDetails,
        seasons: Int = 0,
    ): List<Pair<String, String>> =
        listOf(
            (if (details.kind == "movie") "Released" else "Aired") to (details.releaseInfo ?: ""),
            "Seasons" to (seasons.takeIf { it > 0 }?.toString() ?: ""),
            "Runtime" to (details.runtime?.let { if (details.kind == "movie") it else "$it an episode" } ?: ""),
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
        return "Airs " + day(at)
    }

    /**
     * The line where Play would be when there is nothing to play: "Next episode airs Nov 3, 2026" while the
     * next episode is not out (the core's `nextAirs`), else "Nothing to play yet".
     */
    fun nothingToPlay(nextAirs: String?): String =
        releaseMillis(nextAirs)?.let { "Next episode airs ${day(it)}" } ?: "Nothing to play yet"

    // A release is a date, not an instant: read in UTC, it stays the day it names west of UTC too.
    private fun day(at: Long): String =
        SimpleDateFormat("MMM d, yyyy", Locale.US).apply { timeZone = UTC }.format(Date(at))

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
