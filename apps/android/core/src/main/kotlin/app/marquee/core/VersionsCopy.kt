package app.marquee.core

import app.marquee.core.client.DeviceCapsParams
import app.marquee.core.model.Group
import app.marquee.core.model.HiddenSummary
import app.marquee.core.model.HideReason
import app.marquee.core.model.Reason
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionStatus
import app.marquee.core.model.VersionsSnapshot
import java.util.Locale

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

            // A reason from a newer core.
            Reason.Unknown -> {
                "Recommended"
            }
        }

    /**
     * A group's header. The recommendation sits above the list (not in it), so when there is one the best group
     * reads "Other good choices": "Best quality" over a 720p row would say 720p is the best.
     */
    fun group(
        group: Group,
        pickAbove: Boolean = false,
    ): String =
        when (group) {
            Group.Best -> if (pickAbove) "Other good choices" else "Best quality"
            Group.SmallerFaster -> "Smaller and faster"
            Group.OtherLanguages -> "Other languages"
            Group.NeedsDownload -> "Needs download"
            Group.Unknown -> "Other versions"
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
        when {
            !snapshot.done -> "Finding versions"
            snapshot.rows.isEmpty() -> "No playable version found"
            else -> "Nothing is ready to play yet"
        }

    /**
     * What the card says while there is no recommendation: while looking, who is slow or failed; once
     * done, why nothing is ready first, then any addon that failed.
     */
    fun waitingBody(snapshot: VersionsSnapshot): String {
        if (!snapshot.done) return addons(snapshot) ?: "Your addons are answering."
        val why =
            when {
                snapshot.rows.isNotEmpty() -> "Choose a version to start it."
                snapshot.hidden.count > 0 -> "Every result was hidden: ${hiddenDetail(snapshot.hidden)}."
                else -> "None of your addons returned a stream for this video. A source addon finds playable streams."
            }
        return listOfNotNull(why, addons(snapshot)?.let { "$it." }).joinToString(" ")
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

            // Kids profiles only.
            HideReason.LeavesApp -> "would open outside Marquee"

            HideReason.NotForKids -> if (one) "is not for kids" else "are not for kids"

            // A reason from a newer core: still counted, in general words.
            HideReason.Unknown -> if (one) "is hidden for another reason" else "are hidden for another reason"
        }

    /** "Hidden (5)": the bold half of the hidden row. */
    fun hiddenTitle(summary: HiddenSummary): String = "Hidden (${summary.count})"

    /** "3 are for other titles, 2 are over your size limit": the other half. */
    fun hiddenDetail(summary: HiddenSummary): String =
        summary.byReason.joinToString(", ") { (reason, n) -> "$n ${hideText(reason, n == 1)}" }

    /** The tone of a line drawn with a dot. */
    enum class Tone { Idle, Ok, Warn, Bad }

    /** The header's line: who is still asked, slow or silent, else how many versions there are. */
    fun progress(snapshot: VersionsSnapshot?): Pair<String, Tone> {
        val names = { list: List<String> -> list.joinToString(", ") }
        return when {
            snapshot == null -> {
                "Opening" to Tone.Idle
            }

            snapshot.pendingAddons.isNotEmpty() -> {
                "Asking ${names(snapshot.pendingAddons)}" to Tone.Warn
            }

            snapshot.slowAddons.isNotEmpty() -> {
                "${names(snapshot.slowAddons)} is slow, still waiting" to Tone.Warn
            }

            snapshot.failedAddons.isNotEmpty() -> {
                "${names(snapshot.failedAddons)} did not answer" to Tone.Bad
            }

            else -> {
                val n = snapshot.rows.size + (if (snapshot.recommended != null) 1 else 0)
                (if (n == 1) "1 version" else "$n versions") to Tone.Ok
            }
        }
    }

    /** "This screen: 1080p, HDR, plays H.264, HEVC in MKV, MP4": what the ranking was made for. */
    fun device(caps: DeviceCapsParams): String {
        val screen = if (caps.displayHeight >= 2160) "4K" else "${caps.displayHeight}p"
        val plays = caps.decoders?.takeIf { it.isNotEmpty() }?.joinToString(", ") { CODECS[it] ?: it.uppercase() }
        val boxes =
            caps.containers?.takeIf { it.isNotEmpty() }?.joinToString(", ") {
                if (it ==
                    "webm"
                ) {
                    "WebM"
                } else {
                    it.uppercase()
                }
            }
        return buildString {
            append("This screen: $screen")
            if (caps.hdr.isNotEmpty()) append(", HDR")
            if (plays != null) append(", plays $plays")
            if (plays != null && boxes != null) append(" in $boxes")
        }
    }

    private val CODECS = mapOf("h264" to "H.264", "h265" to "HEVC", "av1" to "AV1", "vp9" to "VP9")

    /** A recommendation's facts, in the web's order: size, status, where from, more features, languages. */
    fun facts(row: VersionRow): List<Pair<String, String>> =
        listOfNotNull(
            "Size" to (row.sizeBytes?.let(Format::bytes) ?: "Unknown"),
            "Status" to status(row.status),
            "Via" to row.via,
            ("Features" to row.features.joinToString(", ")).takeIf { row.features.size > 1 },
            ("Languages" to languages(row)).takeIf { row.languages.isNotEmpty() },
        )

    /** The quality and the first feature, the card's heading: "4K Dolby Vision". */
    fun pickHeading(row: VersionRow): String = listOfNotNull(row.quality, row.features.firstOrNull()).joinToString(" ")

    /** The line under Play: how many sources back this release up. */
    fun failover(row: VersionRow): String =
        if (row.failoverCount > 0) {
            "${row.failoverCount + 1} sources. If one fails, the next starts on its own."
        } else {
            "One source for this release."
        }

    /** What an opened row adds under its release name: "English, 2 sources", or "Language not stated". */
    fun detail(row: VersionRow): String =
        listOfNotNull(
            languages(row).takeIf { it.isNotEmpty() },
            "${row.failoverCount + 1} sources".takeIf { row.failoverCount > 0 },
        ).joinToString(", ").ifEmpty { "Language not stated" }

    /** "English, Turkish" from the release's codes. */
    fun languages(row: VersionRow): String = row.languages.joinToString(", ", transform = ::languageName)

    /** The English name of a language code ("eng", "tr", "pt-BR"); the code in capitals when unknown. */
    fun languageName(code: String): String {
        val tag = ISO1[code.lowercase()] ?: code
        val name = Locale.forLanguageTag(tag).getDisplayName(Locale.ENGLISH)
        return if (name.isEmpty() || name.equals(code, ignoreCase = true)) code.uppercase() else name
    }

    // The bibliographic three-letter codes Locale does not take, as on the web.
    private val ISO1 =
        mapOf(
            "eng" to "en",
            "tur" to "tr",
            "fre" to "fr",
            "fra" to "fr",
            "ger" to "de",
            "deu" to "de",
            "spa" to "es",
            "ita" to "it",
            "por" to "pt",
            "rus" to "ru",
            "jpn" to "ja",
            "kor" to "ko",
            "chi" to "zh",
            "zho" to "zh",
            "ara" to "ar",
            "dut" to "nl",
            "nld" to "nl",
            "pol" to "pl",
            "swe" to "sv",
            "hin" to "hi",
            "gre" to "el",
            "ell" to "el",
            "rum" to "ro",
            "ron" to "ro",
            "hun" to "hu",
            "cze" to "cs",
            "ces" to "cs",
            "heb" to "he",
            "per" to "fa",
            "fas" to "fa",
            "pob" to "pt-BR",
        )

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
