package app.marquee.core

import app.marquee.core.model.EmptyAction
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.EmptyVersions
import app.marquee.core.model.Group
import app.marquee.core.model.Reason
import app.marquee.core.model.SectionQuality
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
    /** Why a version is recommended. No service is named, as no addon is: "Ready now", never "Ready on TorBox". */
    fun reason(reason: Reason): String =
        when (reason) {
            Reason.ReadyNow -> {
                "Ready now"
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
            is VersionStatus.Ready -> "Ready"
            is VersionStatus.Torrent -> status.seeds?.let { "Torrent, $it seeds" } ?: "Torrent"
            is VersionStatus.NeedsDownload -> "Needs download"
            VersionStatus.Direct -> "Direct link"
        }

    /** A status with a good dot: it can play now (ready on a service, or a direct link). */
    fun good(status: VersionStatus): Boolean = status is VersionStatus.Ready || status == VersionStatus.Direct

    /** "4K, Dolby Vision, Atmos". The core words an unknown quality itself ("WEB-DL", "Quality not stated"). */
    fun headline(row: VersionRow): String = (listOf(row.quality) + row.features).joinToString(", ")

    /** The quality in a narrow cell or a small value: "Unknown", one word that never wraps. */
    fun cell(quality: String): String =
        if (quality == "Quality not stated" ||
            quality == UNKNOWN
        ) {
            UNKNOWN
        } else {
            quality
        }

    private const val UNKNOWN = "Unknown"

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

    /** One of Apple's badges: the resolution filled, the Dolby words plain, the short marks outlined. */
    data class Badge(
        val text: String,
        val look: Look,
    ) {
        enum class Look { Filled, Plain, Outlined }
    }

    /**
     * A version's badges as Apple draws its badge row (design/exploration/v3 `badgeRow`): the quality first, then one
     * video format (Dolby Vision, else HDR10+, else HDR) and one audio format (Dolby Atmos, else 7.1, else 5.1).
     */
    fun badges(row: VersionRow): List<Badge> {
        val f = row.features.toSet()
        val video =
            when {
                "Dolby Vision" in f -> Badge("Dolby Vision", Badge.Look.Plain)
                "HDR10+" in f -> Badge("HDR10+", Badge.Look.Outlined)
                "HDR10" in f || "HDR" in f || "HLG" in f -> Badge("HDR", Badge.Look.Outlined)
                else -> null
            }
        val audio =
            when {
                "Atmos" in f -> Badge("Dolby Atmos", Badge.Look.Plain)
                "7.1" in f -> Badge("7.1", Badge.Look.Outlined)
                "5.1" in f -> Badge("5.1", Badge.Look.Outlined)
                else -> null
            }
        // A quality the release does not state gets no badge: its section ("Other") already says so.
        val quality = Badge(row.quality, Badge.Look.Filled).takeIf { cell(row.quality) != UNKNOWN }
        return listOfNotNull(quality, video, audio)
    }

    /**
     * A row's one quiet line (plans/versions-android-spec.md, section 1): its size and its source in plain words
     * ("2 GB, shared by 412 people", "4 GB, ready now"), or "On this device"; null when the core's line warns (it
     * then shows instead: "May start slowly").
     */
    fun quiet(row: VersionRow): String? {
        val line = StreamChoiceCopy.line(row.line)
        val size = row.sizeBytes?.let(Format::bytes)
        if (row.onDevice != null) return sized(size, row.onDevice)
        if (line.tone != Tone.Ok) return null
        val source =
            when (val s = row.status) {
                is VersionStatus.Ready -> {
                    StreamChoiceCopy.SOURCE_READY
                }

                VersionStatus.Direct -> {
                    StreamChoiceCopy.SOURCE_DIRECT
                }

                is VersionStatus.NeedsDownload -> {
                    StreamChoiceCopy.SOURCE_DOWNLOAD
                }

                is VersionStatus.Torrent -> {
                    s.seeds?.let {
                        StreamChoiceCopy.SOURCE_SHARED.replace(
                            "{people}",
                            StreamChoiceCopy.counted("people", it),
                        )
                    }
                }
            }
        return source?.let { sized(size, it) } ?: size ?: line.text
    }

    /**
     * A row's one line as it shows: its size first, then its source in plain words, the core's warning, or why it is
     * dimmed ([why]): "1.5 GB, shared by 1,204 people", "12 GB, may start slowly", "2 GB, needs an account".
     */
    fun rowWords(
        row: VersionRow,
        why: EmptyKind? = null,
    ): String {
        val size = row.sizeBytes?.let(Format::bytes)
        val said =
            when {
                why != null -> StreamChoiceCopy.emptyRowLine(why).text
                else -> return quiet(row) ?: sized(size, StreamChoiceCopy.line(row.line).text)
            }
        return sized(size, said)
    }

    /** [words] after the size ("12 GB, may start slowly"), or on their own with a capital ("Did not play on this device"). */
    private fun sized(
        size: String?,
        words: String,
    ) = size?.let { "$it, ${words.replaceFirstChar(Char::lowercase)}" } ?: words.replaceFirstChar(Char::uppercase)

    /** A row's details, reserved under it and shown on focus: the release name, the line's details, its languages and sources. */
    fun details(row: VersionRow): String =
        listOfNotNull(
            row.releaseName,
            StreamChoiceCopy.line(row.line).details.takeIf { it.isNotEmpty() },
            detail(row),
        ).joinToString(" · ")

    /** A row as a screen reader says it: its badges, its language, then its quiet line or its size. */
    fun rowSpoken(row: VersionRow): String =
        (badges(row).map { it.text } + listOfNotNull(languageTag(row), quiet(row) ?: row.sizeBytes?.let(Format::bytes)))
            .joinToString(", ")

    /**
     * The card's one why line: the core's line and its details joined by a colon ("Starts quickly: 412 seeders, 2 GB"),
     * or, for the version kept on this device, that alone ("On this device"): one idea, as on the web.
     */
    fun why(row: VersionRow): String {
        row.onDevice?.let { return it }
        val line = StreamChoiceCopy.line(row.line)
        return if (line.details.isEmpty()) line.text else "${line.text}: ${line.details}"
    }

    /** The card as a screen reader says it: the kicker, the badges, the whole why sentence. */
    fun cardSpoken(
        kicker: String,
        row: VersionRow,
    ): String =
        listOf(
            kicker,
            badges(row).joinToString(", ") { it.text },
            why(row),
        ).filter { it.isNotEmpty() }.joinToString(". ")

    /** One section of the Versions list: its [id] keys the heading and the fold ("section:fhd"). */
    data class Section(
        val id: String,
        val title: String,
        val rows: List<VersionRow>,
    )

    /**
     * The list's sections (plans/play-every-version.md 2f): the core's quality sections, in its order, each with its
     * rows in the core's order. [rows] the core's sections do not cover (an older core, or the dimmed rows of a state
     * where nothing plays) keep today's groups.
     */
    fun sections(
        snapshot: VersionsSnapshot?,
        rows: List<VersionRow>,
    ): List<Section> {
        val byKey = rows.associateBy { it.key }
        val core =
            snapshot
                ?.sections
                .orEmpty()
                .mapIndexed { i, s ->
                    Section(
                        // A section from a newer core keys by its place, so two never share a key.
                        "section:${s.quality.wire.ifEmpty { "new$i" }}",
                        StreamChoiceCopy.SECTIONS[s.quality] ?: StreamChoiceCopy.SECTIONS.getValue(
                            SectionQuality.Other,
                        ),
                        s.keys.mapNotNull(byKey::get),
                    )
                }.filter { it.rows.isNotEmpty() }
        // The rows those sections do not cover (all of them from an older core) follow under today's groups.
        val covered = core.flatMap { section -> section.rows.map { it.key } }.toSet()
        val groups = rows.filter { it.key !in covered }.groupBy { it.group }
        return core +
            Group.entries.mapNotNull { g ->
                groups[g]?.let {
                    Section(
                        "group:${g.wire.ifEmpty { "new" }}",
                        group(g, snapshot?.recommended != null),
                        it,
                    )
                }
            }
    }

    /**
     * [sections] with each one's rows in the order they were first seen ([seen], filled by [record] after they show):
     * while Versions is open over the player, a re-rank under it never moves a row, and a new version goes after
     * the ones already there in its section, in the core's order. Reads [seen] only.
     */
    fun held(
        sections: List<Section>,
        seen: Map<String, Int>,
    ): List<Section> = sections.map { s -> s.copy(rows = s.rows.sortedBy { seen[it.key] ?: Int.MAX_VALUE }) }

    /** Remembers the order [sections] show their rows in, for [held]: a key seen before keeps its place. */
    fun record(
        sections: List<Section>,
        seen: MutableMap<String, Int>,
    ) {
        for (section in sections) for (row in section.rows) if (row.key !in seen) seen[row.key] = seen.size
    }

    /** A section's heading as a screen reader says it: "1080p, 12 versions". */
    fun sectionSpoken(
        title: String,
        n: Int,
    ): String = "$title, ${StreamChoiceCopy.counted("versions", n)}"

    /** A section's fold as a screen reader says it: "Show all 12 1080p versions", "Show fewer 1080p versions". */
    fun foldSpoken(
        title: String,
        n: Int,
        open: Boolean,
    ): String = (if (open) StreamChoiceCopy.SHOW_FEWER else StreamChoiceCopy.showAll(n)) + " $title versions"

    /** A shortcut's words: "Smaller file: 1080p, 2 GB". */
    fun shortcutLabel(
        label: String,
        row: VersionRow,
    ): String = "$label: " + listOfNotNull(row.quality, row.sizeBytes?.let(Format::bytes)).joinToString(", ")

    /** The shortcuts under the card, in the core's order, each only when it is not the card's own version. */
    fun shortcuts(snapshot: VersionsSnapshot?): List<Pair<String, VersionRow>> {
        val best = snapshot?.recommended?.key
        return listOfNotNull(
            snapshot?.smaller?.takeIf { it.key != best }?.let { StreamChoiceCopy.SMALLER to it },
            snapshot?.sharper?.takeIf { it.key != best }?.let { StreamChoiceCopy.SHARPER to it },
        )
    }

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
     * the first one, "Still looking for more versions" while addons answer, else how many versions there are; null
     * when nothing can play and nothing was found, since the empty state's line says it.
     */
    fun progress(snapshot: VersionsSnapshot?): Pair<String, Tone>? {
        val found = snapshot?.let { it.rows.size + (if (it.recommended != null) 1 else 0) } ?: 0
        return when {
            snapshot == null || (!snapshot.done && found == 0 && snapshot.empty == null) -> {
                StreamChoiceCopy.FINDING to Tone.Idle
            }

            !snapshot.done && snapshot.empty == null -> {
                StreamChoiceCopy.STILL_LOOKING to Tone.Idle
            }

            // What was found, none of it playable here (the card says why), or nothing to count.
            snapshot.empty != null -> {
                snapshot.empty.count
                    .takeIf { it > 0 }
                    ?.let { StreamChoiceCopy.counted("versions", it) to Tone.Idle }
            }

            else -> {
                StreamChoiceCopy.counted("versions", found) to Tone.Ok
            }
        }
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
