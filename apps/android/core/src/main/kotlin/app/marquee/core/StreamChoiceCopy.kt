package app.marquee.core

import app.marquee.core.model.BlockedReason
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.LearnedChoice
import app.marquee.core.model.Network
import app.marquee.core.model.Prefs
import app.marquee.core.model.ReasonLine
import app.marquee.core.model.SectionQuality
import app.marquee.core.model.Speed
import app.marquee.core.model.StreamPrefs
import app.marquee.core.model.SubtitleLayout
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionsConditions
import java.util.Locale

/**
 * Smart stream choice words (plans/m13-stream-choice.md 3.7, 3.9, 3.11, 4.4, 4.5), word for word
 * `apps/shared/stream-choice-words.json` (StreamChoiceCopyTest checks every key), as the web's
 * `lib/words.ts` renders them. The rules are the JSON's "about": a line's details are parts joined with
 * ", ", a part shows only when every field it names is known, counts group thousands with a comma,
 * sizes use [Format.bytes], Mbit/s are whole numbers.
 */
object StreamChoiceCopy {
    class LineWords(
        val tone: VersionsCopy.Tone,
        val text: String,
        val details: List<String>,
        /** "Larger than the size you chose": the large line when the user set the limit. */
        val textChosen: String? = null,
        /** "May pause: only a few people are sharing it": the mayPause line when the swarm is small. */
        val textSmallSwarm: String? = null,
        /** The buffer line on mobile data, where it says no minutes. */
        val detailsMobile: List<String>? = null,
    )

    /** A line as shown: its tone, its default words and the details (empty when no part is known). */
    data class Line(
        val tone: VersionsCopy.Tone,
        val text: String,
        val details: String,
    )

    class Choice<T>(
        val label: String,
        val value: T,
    )

    class Setting<T>(
        val label: String,
        val choices: List<Choice<T>>,
        val help: String? = null,
        /** For a value no chip has ("Up to {size}", "{n} people"). */
        val custom: String? = null,
    )

    class UpDown(
        val up: String,
        val down: String,
    ) {
        fun of(up: Boolean) = if (up) this.up else down
    }

    class Answer(
        val label: String,
        val text: String,
        val sizeLimitBytes: Long?,
        val minSeeders: Int,
    )

    private const val GIB = 1L shl 30
    private val ok = VersionsCopy.Tone.Ok
    private val warn = VersionsCopy.Tone.Warn
    private val bad = VersionsCopy.Tone.Bad

    val PLURALS =
        mapOf(
            "playbacks" to ("{n} playback" to "{n} playbacks"),
            "versions" to ("{n} version" to "{n} versions"),
            "people" to ("{n} person" to "{n} people"),
        )

    val LINES =
        mapOf(
            "copy" to LineWords(bad, "Recorded in a cinema, low quality", listOf("{quality} copy")),
            "fetch" to LineWords(bad, "Not ready yet, has to download first", listOf("{account} does not have it yet")),
            "stalled" to
                LineWords(
                    bad,
                    "Did not start here last time",
                    listOf("Almost nothing arrived when this device last tried it"),
                ),
            "noPicture" to
                LineWords(
                    bad,
                    "No picture on this device",
                    listOf("Its HEVC video is stored in a form this device cannot show"),
                ),
            "didNotPlay" to
                LineWords(
                    bad,
                    "Did not play on this device",
                    listOf("This device could not decode it when it last tried"),
                ),
            "veryFew" to LineWords(bad, "May not start", listOf("shared by {people}")),
            "otherYear" to LineWords(warn, "May be a different film or show", listOf("The release says {year}")),
            "mayPause" to
                LineWords(
                    warn,
                    "May pause on this connection",
                    listOf("Needs about {need} Mbit/s; this connection gives about {have} ({speedSource})"),
                    textSmallSwarm = "May pause: only a few people are sharing it",
                ),
            "washedOut" to
                LineWords(warn, "Colors may look washed out on this screen", listOf("HDR file, screen without HDR")),
            "large" to
                LineWords(
                    warn,
                    "Large file",
                    listOf("{size}; Momotus prefers files up to {limit}"),
                    textChosen = "Larger than the size you chose",
                ),
            "few" to LineWords(warn, "May start slowly", listOf("shared by {people}")),
            "ready" to LineWords(ok, "Starts right away", listOf("Ready now", "{size}")),
            "quick" to LineWords(ok, "Starts quickly", listOf("shared by {people}", "{size}")),
            "should" to
                LineWords(
                    ok,
                    "Should start quickly",
                    listOf("shared by {people}", "{size}", "may be ready in {account}"),
                ),
            "buffer" to
                LineWords(
                    ok,
                    "Plays without storing the whole file",
                    listOf("shared by {people}", "{size}", "room for about {minutes} min ahead in {budget}"),
                    detailsMobile =
                        listOf(
                            "shared by {people}",
                            "{size}",
                            "keeps only a short stretch ahead on mobile data",
                        ),
                ),
        )

    const val SPEED_MEASURED = "measured from {playbacks}"

    /**
     * By reason: the words and the one action, if any. Shown nowhere since every page reads [EMPTY]; kept until the core
     * stops sending `blocked` and stream-choice-words.json drops it (plans/play-every-version.md Step 3 item 6).
     */
    val BLOCKED =
        mapOf(
            // Said quietly, with no action: an account is set up in Settings only.
            BlockedReason.NeedsAccount to ("Found {versions}, but none plays on this device." to null),
            BlockedReason.NothingPlaysHere to ("Found {versions}, but none plays on this device." to "Show them"),
        )

    /** Why nothing can play, by kind (plans/play-every-version.md 2a): the words and the one action, if any. */
    val EMPTY =
        mapOf(
            EmptyKind.NoSource to ("Add a source in Settings" to "Settings"),
            EmptyKind.Unreachable to ("Can't reach any source" to "Try again"),
            // Said quietly, with no action: an account is set up in Settings only (the owner, 2026-10-23).
            EmptyKind.NeedsAccount to ("None of these plays on this device" to null),
            EmptyKind.KidsNothing to ("There's nothing here for you to watch" to null),
            EmptyKind.NoResults to ("No source has this yet" to null),
            EmptyKind.NothingPlaysHere to ("None of these plays on this device" to "Choose a version"),
            EmptyKind.NotStarted to ("This didn't start" to "Try again"),
        )

    /** Try again, the unreachable and notStarted states' action. */
    val EMPTY_TRY_AGAIN = EMPTY.getValue(EmptyKind.NotStarted).second!!

    /** What each dimmed row of an empty state says in place of its line: why it is dimmed, never how fast it starts. */
    val EMPTY_ROWS =
        mapOf(
            EmptyKind.NeedsAccount to "Won't play here",
            EmptyKind.NothingPlaysHere to "Won't play here",
        )

    /** A dimmed row's line, in the neutral tone (a reason, not a warning); a newer kind reads as nothingPlaysHere's. */
    fun emptyRowLine(kind: EmptyKind): Line =
        Line(VersionsCopy.Tone.Idle, EMPTY_ROWS[kind] ?: EMPTY_ROWS.getValue(EmptyKind.NothingPlaysHere), "")

    /** The action for `playFromDevice`, with the unreachable kind's words. */
    const val PLAY_FROM_DEVICE = "Play from this device"

    const val SHOW_ROWS = "Show them"

    /** Where a source is added, from a page with none. */
    const val SETTINGS = "Settings"

    /** The Versions page's own words ("page"): its status line while the core asks, the section folds, Back. */
    const val FINDING = "Finding versions"
    const val STILL_LOOKING = "Still looking for more versions"
    const val SHOW_ALL = "Show all {n}"
    const val SHOW_FEWER = "Show fewer"
    const val BACK = "Back"

    /** A row's source in plain words, after its size ("2 GB, shared by 412 people"). */
    const val SOURCE_READY = "ready now"
    const val SOURCE_DIRECT = "direct link"
    const val SOURCE_DOWNLOAD = "downloads first"
    const val SOURCE_SHARED = "shared by {people}"

    /** A row's details: its info button's and its screen-reader action's words, and the sheet's title. */
    const val DETAILS = "Details"

    /** The card's kicker, by form. */
    const val BEST_TV = "Best for this TV"
    const val BEST_PHONE = "Best for this phone"

    /** The shortcuts under the card (plans/play-every-version.md 2f), in the core's order. */
    const val SMALLER = "Smaller file"
    const val SHARPER = "Sharper"

    /** The quality sections' titles, by the core's section. */
    val SECTIONS =
        mapOf(
            SectionQuality.Uhd to "4K",
            SectionQuality.Fhd to "1080p",
            SectionQuality.Hd to "720p and below",
            SectionQuality.Other to "Other",
        )

    /** A sheet's close ("Done"), one word on every platform. */
    const val DONE = "Done"

    /** What the player says while a version starts ("player"), never an addon's name. */
    const val FINDING_BEST = "Finding the best version"
    const val FINDING_CHOSEN = "Finding this version"
    const val CHOOSE_ANOTHER = "Choose another version"
    const val CHOOSE_ANOTHER_SAID = "Still loading. Choose another version is available"

    /** Every start subtitle failed to read (`startSubtitleFailed`): said with no source. */
    const val SUBTITLES_FAILED = "Subtitles could not be loaded"

    /** The subtitle picker's second line (plans/dual-subtitles.md): its switch, rows and options. */
    const val FIRST_LINE = "First line"
    const val SECOND_LINE = "Second line"
    const val SPOKEN_LANGUAGE = "Spoken language"
    const val SPOKEN_NOT_KNOWN = "not known for this video"
    const val ON_FIRST_LINE = "On the first line"
    const val ON_SECOND_LINE = "On the second line"
    const val LAYOUT = "Layout"
    val LAYOUTS =
        mapOf(
            SubtitleLayout.Stacked to "Stacked",
            SubtitleLayout.Split to "Split",
            SubtitleLayout.OneBox to "One box",
        )
    const val ONLY_WHEN_PAUSED = "Only when paused"

    /** "Show all 12". */
    fun showAll(n: Int): String = SHOW_ALL.replace("{n}", "$n")

    val LEARNED =
        mapOf(
            LearnedChoice.Size to
                UpDown(
                    "On {device} you often picked bigger files, so Momotus now picks files up to {limit}.",
                    "On {device} you often picked smaller files, so Momotus now picks files up to {limit}.",
                ),
            LearnedChoice.Height to
                UpDown(
                    "On {device} you often picked sharper versions, so Momotus now picks up to {height}.",
                    "On {device} you often picked lower resolutions, so Momotus now picks up to {height}.",
                ),
            LearnedChoice.Ready to
                UpDown(
                    "On {device} you often picked files ready on your streaming service, so Momotus now prefers them.",
                    "On {device} you often picked torrents over ready files, so Momotus now treats them like the rest.",
                ),
            LearnedChoice.Hdr to
                UpDown(
                    "On {device} you often picked HDR versions, so Momotus now prefers HDR.",
                    "On {device} you often picked versions without HDR, so Momotus now leaves HDR off.",
                ),
        )
    const val UNDO = "Undo"
    val SETTINGS_LINES =
        mapOf(
            LearnedChoice.Size to
                UpDown("On {screens} you often picked bigger files.", "On {screens} you often picked smaller files."),
            LearnedChoice.Height to
                UpDown(
                    "On {screens} you often picked sharper versions.",
                    "On {screens} you often picked lower resolutions.",
                ),
            LearnedChoice.Ready to
                UpDown(
                    "On {screens} you often picked files ready on your streaming service.",
                    "On {screens} you often picked torrents over ready files.",
                ),
            LearnedChoice.Hdr to
                UpDown(
                    "On {screens} you often picked HDR versions.",
                    "On {screens} you often picked versions without HDR.",
                ),
        )
    val LEARNED_SCREENS =
        mapOf(
            2160 to "4K screens",
            1080 to "1080p screens",
            720 to "720p screens",
            480 to "SD screens",
        )

    const val QUESTION_TITLE = "How should Momotus pick what to play?"
    const val QUESTION_LEAD = "You can change this later in Settings, under Stream choice."
    val FAST = Answer("Start fast", "Smaller files that many people share. Starts in seconds.", 5 * GIB, 50)
    val BEST = Answer("Best quality", "The best version your screen and connection can play.", null, 20)
    const val SKIP = "Skip"

    /** What Skip means: the defaults are Start fast's, so Skip writes nothing. */
    const val SKIP_MEANS = "fast"

    const val TITLE = "Stream choice"
    const val LEDE =
        "How Momotus picks the version to play. Every version stays in the list, so you can always pick another."
    const val SUMMARY = "{size}, shared by at least {people}"
    val SIZE =
        Setting(
            "File size",
            listOf(
                Choice("Up to 2 GB", 2 * GIB),
                Choice("Up to 5 GB", 5 * GIB),
                Choice("Up to 10 GB", 10 * GIB),
                Choice("Up to 20 GB", 20 * GIB),
                Choice<Long?>("Any size", null),
            ),
            custom = "Up to {size}",
        )
    val SEEDERS =
        Setting(
            "Shared by at least",
            listOf(
                Choice("10 people", 10),
                Choice("20 people", 20),
                Choice("50 people", 50),
                Choice("100 people", 100),
            ),
            help = "Files shared by more people start faster.",
            custom = "{n} people",
        )
    val RESOLUTION =
        Setting("Resolution", listOf(Choice<Int?>("This screen's", null), Choice("1080p", 1080), Choice("720p", 720)))
    val HDR = Setting("HDR", listOf(Choice("Use it when the screen shows it", true), Choice("Off", false)))
    val READY =
        Setting(
            "Streaming service",
            listOf(
                Choice("Prefer files that are ready on your account", true),
                Choice("Treat them like the rest", false),
            ),
        )

    /** The Streaming service row shows only with an account. */
    const val READY_ONLY_WITH_ACCOUNT = true
    const val AUDIO = "Audio"
    const val AUDIO_HELP = "Listed in the order you turn them on; the first one wins."
    val LEARN = Setting("Learn from my picks", listOf(Choice("On", true), Choice("Off", false)))
    const val FORGET = "Forget what was learned"
    const val DEVICE = "This device"
    const val MEASURE_AGAIN = "Measure again"
    const val MEASURING = "Measuring at the next playback"

    /** The Versions state `nobodySharing`, on the list and the recommended card. */
    const val NOBODY_SHARING = "Almost nobody is sharing this right now"
    const val NOBODY_SHARING_DETAILS = "Every version has one seeder or none at the moment, so it may not start."

    const val HEADER = "{device}: {conditions}"
    val DEVICES = mapOf("tv" to "this TV", "desktop" to "this computer", "phone" to "this phone")
    val SCREENS = mapOf(2160 to "4K", 1080 to "1080p", 720 to "720p", 480 to "SD")
    const val SCREEN_HDR = "{screen} HDR"
    const val SPEED = "about {mbps} Mbit/s on {network}"
    val NETWORKS =
        mapOf(
            Network.Wifi to "Wi-Fi",
            Network.Ethernet to "Ethernet",
            Network.Cellular to "mobile data",
            Network.Other to "this network",
        )

    // The rules

    /** Fills `{name}` fields; null when one is unknown, so the part is left out. */
    private fun part(
        line: String,
        fields: Map<String, Any?>,
    ): String? {
        var known = true
        val text =
            Regex("\\{(\\w+)\\}").replace(line) { m ->
                val v = fields[m.groupValues[1]]
                if (v == null) known = false
                v?.toString().orEmpty()
            }
        return text.takeIf { known }
    }

    private fun capital(s: String) = s.replaceFirstChar { it.uppercaseChar() }

    private fun thousands(n: Int) = String.format(Locale.US, "%,d", n)

    /** "1 person", "1,204 people". */
    fun counted(
        what: String,
        n: Int,
    ): String {
        val (one, many) = PLURALS.getValue(what)
        return (if (n == 1) one else many).replace("{n}", thousands(n))
    }

    private fun whole(mbps: Float) = Math.round(mbps.toDouble())

    /** The viewer's streaming service, said as their account and never by its name, as no addon is named. */
    private fun account(service: String?) = service?.let { "your account" }

    private fun fields(l: ReasonLine): Pair<String, Map<String, Any?>> =
        when (l) {
            is ReasonLine.Copy -> {
                "copy" to mapOf("quality" to l.quality)
            }

            is ReasonLine.Fetch -> {
                "fetch" to mapOf("account" to account(l.service))
            }

            ReasonLine.Stalled -> {
                "stalled" to emptyMap()
            }

            ReasonLine.NoPicture -> {
                "noPicture" to emptyMap()
            }

            ReasonLine.DidNotPlay -> {
                "didNotPlay" to emptyMap()
            }

            is ReasonLine.VeryFew -> {
                "veryFew" to mapOf("people" to counted("people", l.seeders))
            }

            is ReasonLine.OtherYear -> {
                "otherYear" to mapOf("year" to l.year)
            }

            is ReasonLine.MayPause -> {
                // A speed is said only once a playback measured it; before that the details are left out.
                val playbacks = if (l.samples > 0) counted("playbacks", l.samples) else null
                val source = playbacks?.let { SPEED_MEASURED.replace("{playbacks}", it) }
                "mayPause" to mapOf("need" to whole(l.needMbps), "have" to whole(l.haveMbps), "speedSource" to source)
            }

            ReasonLine.WashedOut -> {
                "washedOut" to emptyMap()
            }

            is ReasonLine.Large -> {
                "large" to mapOf("size" to Format.bytes(l.sizeBytes), "limit" to Format.bytes(l.limitBytes))
            }

            is ReasonLine.Few -> {
                "few" to mapOf("people" to counted("people", l.seeders))
            }

            is ReasonLine.Ready -> {
                "ready" to mapOf("size" to l.sizeBytes?.let(Format::bytes))
            }

            is ReasonLine.Quick -> {
                "quick" to
                    mapOf(
                        "people" to l.seeders?.let { counted("people", it) },
                        "size" to l.sizeBytes?.let(Format::bytes),
                    )
            }

            is ReasonLine.Should -> {
                "should" to
                    mapOf(
                        "people" to l.seeders?.let { counted("people", it) },
                        "size" to l.sizeBytes?.let(Format::bytes),
                        "account" to account(l.service),
                    )
            }

            is ReasonLine.Buffer -> {
                "buffer" to
                    mapOf(
                        "people" to l.seeders?.let { counted("people", it) },
                        "size" to l.sizeBytes?.let(Format::bytes),
                        "minutes" to l.aheadMin,
                        "budget" to Format.bytes(l.budgetBytes),
                    )
            }
        }

    /**
     * A version's line: the tone, the default text, and the details shown on focus (TV) or in the opened
     * row (phone). A large file says "Larger than the size you chose" when the user set the limit; a version
     * that may pause because few people share it says so; a window on mobile data says it keeps only a short
     * stretch ahead instead of its minutes.
     */
    fun line(l: ReasonLine): Line {
        val w = LINES.getValue(fields(l).first)
        val text =
            when {
                l is ReasonLine.Large && l.chosen -> w.textChosen ?: w.text
                l is ReasonLine.MayPause && l.smallSwarm -> w.textSmallSwarm ?: w.text
                else -> w.text
            }
        return Line(w.tone, text, capital(parts(l).joinToString(", ")))
    }

    /** A line's details as its parts, each filled, before they are joined: "shared by 89 people", "4 GB". */
    fun parts(l: ReasonLine): List<String> {
        val (kind, f) = fields(l)
        val w = LINES.getValue(kind)
        val details = if (l is ReasonLine.Buffer && l.aheadMin == null) w.detailsMobile ?: w.details else w.details
        return details.mapNotNull { part(it, f) }
    }

    /** Why nothing can play, in the core's words; a kind from a newer core reads as noResults, the plainest. */
    fun emptyLine(kind: EmptyKind): String = (EMPTY[kind] ?: EMPTY.getValue(EmptyKind.NoResults)).first

    /**
     * A version row's line: the core's "On this device" where this device holds the start of the version last
     * played (plans/player-polish.md 2b), with the line's details kept for focus; else its [line].
     */
    fun rowLine(row: VersionRow): Line {
        val line = line(row.line)
        return row.onDevice?.let { Line(VersionsCopy.Tone.Ok, it, line.details) } ?: line
    }

    /** The speed that decides here: HTTP with a streaming service account or without the engine, else the engine's. */
    fun speedFor(
        http: Speed,
        p2p: Speed,
        engine: Boolean,
        account: Boolean,
    ): Speed = if (account || !engine) http else p2p

    private fun screenWord(screenClass: Int) = SCREENS[screenClass] ?: "${screenClass}p"

    /** "4K HDR, about 42 Mbit/s on Wi-Fi": the screen and the connection, never the disk (plans/no-space-warnings.md). */
    fun conditionsLine(
        screenClass: Int,
        hdr: Boolean,
        network: Network,
        speed: Speed,
    ): String {
        val screen = screenWord(screenClass)
        val net = NETWORKS[network] ?: NETWORKS.getValue(Network.Other)
        // A speed only once a playback measured it; before that the network alone.
        val measured = speed.samples > 0
        val said = if (measured) SPEED.replace("{mbps}", "${whole(speed.mbps)}").replace("{network}", net) else net
        return listOf(if (hdr) SCREEN_HDR.replace("{screen}", screen) else screen, said).joinToString(", ")
    }

    /** "this TV" or "this phone". */
    fun deviceName(tv: Boolean) = DEVICES.getValue(if (tv) "tv" else "phone")

    /** The Versions header from the conditions the ranking read: "This TV: 4K HDR, about 42 Mbit/s on Wi-Fi". */
    fun header(
        tv: Boolean,
        c: VersionsConditions,
    ): String =
        capital(
            HEADER.replace("{device}", deviceName(tv)).replace(
                "{conditions}",
                conditionsLine(
                    c.screenClass,
                    c.hdr,
                    c.network,
                    speedFor(c.http, c.p2p, c.engine, c.account),
                ),
            ),
        )

    /**
     * The one-time note on Versions (3.9) with the preferences now in effect: "On this TV you often picked
     * bigger files, so Marquee now picks files up to 10 GB." Null when the change names no number.
     */
    fun learnedNote(
        choice: LearnedChoice,
        up: Boolean,
        tv: Boolean,
        prefs: StreamPrefs,
    ): String? {
        val words = LEARNED[choice] ?: return null
        val fields =
            mapOf(
                "device" to deviceName(tv),
                "limit" to prefs.sizeLimitBytes?.let(Format::bytes),
                "height" to prefs.maxHeight?.let(::screenWord),
            )
        return part(words.of(up), fields)
    }

    /** One thing the picks taught, for Settings, with what Undo sends. */
    data class LearnedRow(
        val screenClass: Int,
        val choice: LearnedChoice,
        val up: Boolean,
        val text: String,
    )

    private val CHOICES = listOf(LearnedChoice.Size, LearnedChoice.Height, LearnedChoice.Ready, LearnedChoice.Hdr)

    // Largest screen first.
    private fun classes(p: Prefs) =
        listOf(
            2160 to p.learned.uhd,
            1080 to p.learned.fhd,
            720 to p.learned.hd,
            480 to p.learned.sd,
        )

    private fun count(
        l: app.marquee.core.model.Learned,
        choice: LearnedChoice,
    ) = when (choice) {
        LearnedChoice.Size -> l.size
        LearnedChoice.Height -> l.height
        LearnedChoice.Ready -> l.ready
        LearnedChoice.Hdr -> l.hdr
        LearnedChoice.Unknown -> 0
    }

    /**
     * Each choice at 3 or more either way, by screen class, unless undone or forgotten (a class stamped
     * with an older epoch reads as zero): largest screens first, then size, height, ready, HDR.
     */
    fun learnedRows(p: Prefs): List<LearnedRow> =
        classes(p).flatMap { (cls, l) ->
            if (l.epoch < p.learnedEpoch) return@flatMap emptyList()
            CHOICES.filter { Math.abs(count(l, it)) >= 3 && it !in l.muted }.map { choice ->
                val up = count(l, choice) > 0
                val text = SETTINGS_LINES.getValue(choice).of(up).replace("{screens}", LEARNED_SCREENS.getValue(cls))
                LearnedRow(cls, choice, up, text)
            }
        }

    /** Anything learned that Forget would clear. */
    fun anyLearned(p: Prefs): Boolean =
        classes(p).any { (_, l) -> l.epoch >= p.learnedEpoch && CHOICES.any { count(l, it) != 0 } }

    /** "Up to 5 GB, shared by at least 50 people": the Settings row's line. */
    fun summary(s: StreamPrefs): String {
        val size =
            SIZE.choices.firstOrNull { it.value == s.sizeLimitBytes }?.label
                ?: SIZE.custom!!.replace("{size}", Format.bytes(s.sizeLimitBytes ?: 0))
        val people =
            SEEDERS.choices.firstOrNull { it.value == s.minSeeders }?.label
                ?: SEEDERS.custom!!.replace("{n}", thousands(s.minSeeders))
        return SUMMARY.replace("{size}", size).replace("{people}", people)
    }

    /** `displayHeight` to the core's screen class (3.3): a 1440 phone is 1080. */
    fun screenClass(lines: Int) =
        when {
            lines >= 2000 -> 2160
            lines >= 1000 -> 1080
            lines >= 700 -> 720
            else -> 480
        }
}
