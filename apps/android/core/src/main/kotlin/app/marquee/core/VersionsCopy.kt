package app.marquee.core

import app.marquee.core.model.Group
import app.marquee.core.model.HiddenSummary
import app.marquee.core.model.HideReason
import app.marquee.core.model.Reason
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionStatus
import app.marquee.core.model.VersionsSnapshot

/** What the line under a title's primary button says about the versions. */
sealed interface VersionLine {
    /** Still asking the addons: a spinner and "Finding versions". */
    data object Finding : VersionLine

    /**
     * The recommended version, as the web writes it: the [facts] (quality, features, size), a [status]
     * with a dot ([good] when it can play now), and where it comes from.
     */
    data class Chosen(
        val facts: List<String>,
        val status: String,
        val good: Boolean,
        val via: String,
    ) : VersionLine {
        /** One sentence for a screen reader. */
        val spoken: String get() = (facts + status + "via $via").joinToString(", ")
    }

    /** Why there is nothing to play. */
    data class Problem(
        val text: String,
    ) : VersionLine
}

/**
 * The words of the Versions screen and of the version line on the title page, shared by both UI
 * trees and equal to the web family's `lib/words.ts`: plain sentences, no emoji, no vendor names
 * beyond what the user's own addons report.
 */
object VersionsCopy {
    /** Why a version is recommended; "cached on X" when the status names the service. */
    fun reason(
        reason: Reason,
        status: VersionStatus? = null,
    ): String =
        when (reason) {
            Reason.ReadyNow -> {
                (status as? VersionStatus.Ready)?.service?.let { "Ready now, cached on $it" } ?: "Ready now"
            }

            Reason.PlaysDirectly -> {
                "Plays directly on this device"
            }

            Reason.MatchesScreen -> {
                "Matches your screen"
            }

            Reason.SameReleaseAsLast -> {
                "Same release as the last episode"
            }

            Reason.YourLanguage -> {
                "In your language"
            }

            Reason.SmallerFaster -> {
                "Smaller and faster to start"
            }
        }

    fun group(group: Group): String =
        when (group) {
            Group.Best -> "Best quality"
            Group.SmallerFaster -> "Smaller and faster"
            Group.OtherLanguages -> "Other languages"
            Group.NeedsDownload -> "Needs download"
        }

    fun status(status: VersionStatus): String =
        when (status) {
            is VersionStatus.Ready -> status.service?.let { "Ready on $it" } ?: "Ready"
            is VersionStatus.Torrent -> status.seeds?.let { "Torrent, $it seeds" } ?: "Torrent"
            is VersionStatus.NeedsDownload -> status.service?.let { "Needs download on $it" } ?: "Needs download"
            VersionStatus.Direct -> "Direct link"
        }

    /** A status with a good dot: it can play now (ready on a service, or a direct link). */
    fun good(status: VersionStatus): Boolean = status is VersionStatus.Ready || status == VersionStatus.Direct

    /** "4K, Dolby Vision, Atmos". The core words an unknown quality itself ("WEB-DL", "Quality not stated"). */
    fun headline(row: VersionRow): String = (listOf(row.quality) + row.features).joinToString(", ")

    /** The quality in a narrow cell or a small value: "Unknown", one word that never wraps. */
    fun cell(quality: String): String =
        if (quality == "Quality not stated" ||
            quality == "Unknown"
        ) {
            "Unknown"
        } else {
            quality
        }

    /** "1.3 GB, Real-Debrid" (the size is left out when the source does not say). */
    fun source(row: VersionRow): String = listOfNotNull(row.sizeBytes?.let(Format::bytes), row.via).joinToString(", ")

    /** The line under the primary button: the recommended version, a wait, or why nothing can play. */
    fun line(
        snapshot: VersionsSnapshot?,
        failed: Boolean = false,
    ): VersionLine {
        val best = snapshot?.recommended
        return when {
            failed -> {
                VersionLine.Problem("Sources could not load")
            }

            best != null -> {
                VersionLine.Chosen(
                    facts =
                        listOfNotNull(best.quality) + best.features + listOfNotNull(best.sizeBytes?.let(Format::bytes)),
                    status = status(best.status),
                    good = good(best.status),
                    via = best.via,
                )
            }

            snapshot == null || !snapshot.done -> {
                VersionLine.Finding
            }

            snapshot.rows.isEmpty() -> {
                VersionLine.Problem(
                    "No playable version found. A source is an addon that finds a playable stream for a title.",
                )
            }

            else -> {
                VersionLine.Problem("No version is ready to play yet. Choose one to start it.")
            }
        }
    }

    /** The heading of the card shown while there is no recommendation. */
    fun waitingTitle(snapshot: VersionsSnapshot): String =
        if (snapshot.done) "No playable version found" else "Finding versions"

    /** What the card says while there is no recommendation: who is slow or failed, else a plain line. */
    fun waitingBody(snapshot: VersionsSnapshot): String =
        addons(snapshot)
            ?: if (snapshot.done) {
                "None of your addons returned a stream for this video. A source addon finds playable streams."
            } else {
                "Your addons are answering."
            }

    private fun hideText(
        reason: HideReason,
        one: Boolean,
    ): String =
        when (reason) {
            HideReason.WrongTitle -> if (one) "is for another title" else "are for other titles"
            HideReason.WrongEpisode -> if (one) "is another episode" else "are other episodes"
            HideReason.Excluded -> if (one) "matches your avoid list" else "match your avoid list"
            HideReason.TooLarge -> if (one) "is over your size limit" else "are over your size limit"
            HideReason.Unplayable -> "cannot play on this device"
            HideReason.UnsupportedSource -> "${if (one) "uses a source" else "use a source"} this app cannot open yet"
        }

    /** "Hidden (5)": the bold half of the hidden row. */
    fun hiddenTitle(summary: HiddenSummary): String = "Hidden (${summary.count})"

    /** "3 are for other titles, 2 are over your size limit": the other half. */
    fun hiddenDetail(summary: HiddenSummary): String =
        summary.byReason.joinToString(", ") { (reason, n) -> "$n ${hideText(reason, n == 1)}" }

    /** Addons still answering or that failed, as one line, or null. */
    fun addons(snapshot: VersionsSnapshot): String? {
        val parts =
            listOfNotNull(
                snapshot.pendingAddons.takeIf { it.isNotEmpty() }?.let { "Waiting for ${it.joinToString(", ")}" },
                snapshot.slowAddons.takeIf { it.isNotEmpty() }?.let { "Slow: ${it.joinToString(", ")}" },
                snapshot.failedAddons.takeIf { it.isNotEmpty() }?.let { "No answer from ${it.joinToString(", ")}" },
            )
        return parts.takeIf { it.isNotEmpty() }?.joinToString(". ")
    }
}
