package app.marquee.core

import app.marquee.core.model.Group
import app.marquee.core.model.HiddenSummary
import app.marquee.core.model.HideReason
import app.marquee.core.model.Reason
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionStatus
import app.marquee.core.model.VersionsSnapshot

/**
 * The words of the Versions screen and of the version line on the title page, shared by both UI
 * trees: plain sentences, no emoji, no vendor names beyond what the user's own add-ons report.
 */
object VersionsCopy {
    fun reason(reason: Reason): String =
        when (reason) {
            Reason.ReadyNow -> "Ready to stream now"
            Reason.PlaysDirectly -> "Plays directly on this device"
            Reason.MatchesScreen -> "Matches your screen"
            Reason.SameReleaseAsLast -> "Same release as your last episode"
            Reason.YourLanguage -> "In your language"
            Reason.SmallerFaster -> "Smaller, so it starts faster"
        }

    fun group(group: Group): String =
        when (group) {
            Group.Best -> "Best matches"
            Group.SmallerFaster -> "Smaller and faster"
            Group.OtherLanguages -> "Other languages"
            Group.NeedsDownload -> "Needs a download first"
        }

    fun status(status: VersionStatus): String =
        when (status) {
            is VersionStatus.Ready -> status.service?.let { "Ready on $it" } ?: "Ready"
            is VersionStatus.Torrent -> status.seeds?.let { "Torrent, $it seeds" } ?: "Torrent"
            is VersionStatus.NeedsDownload -> status.service?.let { "Needs a download on $it" } ?: "Needs a download"
            VersionStatus.Direct -> "Direct link"
        }

    /** "4K, Dolby Vision, Atmos". */
    fun headline(row: VersionRow): String = (listOf(row.quality) + row.features).joinToString(", ")

    /** "1.3 GB, Real-Debrid" (the size is left out when the source does not say). */
    fun source(row: VersionRow): String = listOfNotNull(row.sizeBytes?.let(Format::bytes), row.via).joinToString(", ")

    /** The line under the Play button: what will play, or why nothing is ready yet. */
    fun chosen(snapshot: VersionsSnapshot?): String =
        when {
            snapshot == null -> "Looking for sources"
            snapshot.recommended != null -> "${headline(snapshot.recommended)}, ${source(snapshot.recommended)}"
            snapshot.done -> "No playable source found"
            else -> "Looking for sources"
        }

    fun hiddenReason(reason: HideReason): String =
        when (reason) {
            HideReason.WrongTitle -> "wrong title"
            HideReason.WrongEpisode -> "wrong episode"
            HideReason.Excluded -> "excluded by your filters"
            HideReason.TooLarge -> "too large"
            HideReason.Unplayable -> "cannot play on this device"
            HideReason.UnsupportedSource -> "unsupported source"
        }

    /** "5 hidden: wrong title 3, too large 2", or null when nothing is hidden. */
    fun hidden(summary: HiddenSummary): String? {
        if (summary.count == 0) return null
        val parts = summary.byReason.joinToString(", ") { "${hiddenReason(it.first)} ${it.second}" }
        return "${summary.count} hidden: $parts"
    }

    /** Add-ons still answering or that failed, as one line, or null. */
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
