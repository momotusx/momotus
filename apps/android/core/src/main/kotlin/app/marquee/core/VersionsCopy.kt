package app.marquee.core

import app.marquee.core.model.EmptyAction
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.EmptyVersions
import app.marquee.core.model.Group
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
     * The recommended version: the [facts] (quality, features, size) and a [status] with a dot ([good] when it
     * can play now). Not the addon it comes from: no addon's name on a title page (design/DECISION.md, Shelves);
     * the Versions screens name it.
     */
    data class Chosen(
        val facts: List<String>,
        val status: String,
        val good: Boolean,
    ) : VersionLine {
        /** One sentence for a screen reader. */
        val spoken: String get() = (facts + status).joinToString(", ")
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
                "Same release as last time"
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
                )
            }

            snapshot == null || !snapshot.done -> {
                VersionLine.Finding
            }

            // Nothing can play, in the core's words (plans/play-every-version.md 2a).
            snapshot.empty != null -> {
                VersionLine.Problem(StreamChoiceCopy.emptyLine(snapshot.empty.kind))
            }

            // Every addon answered with nothing listed while the core still checks what this device keeps.
            snapshot.rows.isEmpty() -> {
                VersionLine.Finding
            }

            else -> {
                VersionLine.Problem("No version is ready to play yet. Choose one to start it.")
            }
        }
    }

    /**
     * The player's line while a chosen version starts: "Starting 1080p, 2 GB", the quality and size alone, never an
     * addon's name (plans/versions-android-spec.md, section 6).
     */
    fun starting(row: VersionRow): String =
        "Starting " + listOfNotNull(row.quality, row.sizeBytes?.let(Format::bytes)).joinToString(", ")

    /** The card when every addon answered, versions are listed and none is recommended. */
    const val NOT_READY_TITLE = "Nothing is ready to play yet"
    const val NOT_READY_BODY = "Choose a version to start it."

    /** What a zero-playable state offers on the Versions page. */
    enum class EmptyStep(
        val label: String,
    ) {
        PlayFromDevice(StreamChoiceCopy.PLAY_FROM_DEVICE),
        TryAgain(StreamChoiceCopy.EMPTY.getValue(EmptyKind.Unreachable).second!!),
        Settings(StreamChoiceCopy.SETTINGS),
        Back(StreamChoiceCopy.BACK),
    }

    /**
     * A zero-playable state's actions on the Versions page, in order (plans/play-every-version.md, "Shared Step 3
     * decisions"): Back always last. "Choose a version" is the player's and the title's action; here the rows are
     * listed.
     */
    fun emptySteps(empty: EmptyVersions): List<EmptyStep> =
        when (empty.action) {
            EmptyAction.PlayFromDevice -> listOf(EmptyStep.PlayFromDevice, EmptyStep.TryAgain)
            EmptyAction.TryAgain -> listOf(EmptyStep.TryAgain)
            EmptyAction.SettingsAddons, EmptyAction.SettingsAccounts -> listOf(EmptyStep.Settings)
            else -> emptyList()
        } + EmptyStep.Back

    /** The tone of a line drawn with a dot. */
    enum class Tone { Idle, Ok, Warn, Bad }

    /**
     * The header's line, never an addon's name (plans/versions-android-spec.md, section 1): "Finding versions" until
     * the first one, "Still looking for more versions" while addons answer, else how many versions there are.
     */
    fun progress(snapshot: VersionsSnapshot?): Pair<String, Tone> {
        val found = snapshot?.let { it.rows.size + (if (it.recommended != null) 1 else 0) } ?: 0
        return when {
            snapshot == null || (!snapshot.done && found == 0 && snapshot.empty == null) -> {
                StreamChoiceCopy.FINDING to Tone.Idle
            }

            !snapshot.done && snapshot.empty == null -> {
                StreamChoiceCopy.STILL_LOOKING to Tone.Idle
            }

            // What was found, none of it playable here (the card says why).
            snapshot.empty != null && snapshot.empty.count > 0 -> {
                StreamChoiceCopy.counted("versions", snapshot.empty.count) to Tone.Warn
            }

            else -> {
                StreamChoiceCopy.counted("versions", found) to Tone.Ok
            }
        }
    }

    /** A recommendation's facts, in the web's order: size, status, where from, more features, languages. */
    fun facts(row: VersionRow): List<Pair<String, String>> =
        listOfNotNull(
            "Size" to (row.sizeBytes?.let(Format::bytes) ?: "Unknown"),
            "Status" to status(row.status),
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

    /**
     * The row's language tag when it is in another language than the viewer's (the core's `otherLanguages` group):
     * "Turkish", or null when the release names none (plans/versions-android-spec.md, section 1).
     */
    fun languageTag(row: VersionRow): String? =
        languages(row).takeIf { row.group == Group.OtherLanguages && it.isNotEmpty() }

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
}
