package app.marquee.core

import app.marquee.core.VersionsCopy.Badge.Look
import app.marquee.core.VersionsCopy.EmptyStep
import app.marquee.core.model.EmptyAction
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.EmptyVersions
import app.marquee.core.model.Group
import app.marquee.core.model.HiddenSummary
import app.marquee.core.model.Reason
import app.marquee.core.model.ReasonLine
import app.marquee.core.model.RowOrigin
import app.marquee.core.model.SectionQuality
import app.marquee.core.model.SourceRows
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionSection
import app.marquee.core.model.VersionStatus
import app.marquee.core.model.VersionsSnapshot
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class VersionsCopyTest {
    private fun row(
        sizeBytes: Long? = 1_300_000_000L,
        features: List<String> = listOf("HDR10"),
    ) = VersionRow(
        key = "k",
        quality = "4K",
        features = features,
        sizeBytes = sizeBytes,
        status = VersionStatus.Ready("Real-Debrid"),
        via = "Real-Debrid",
        releaseName = "A.2160p.mkv",
        languages = emptyList(),
        reasons = listOf(Reason.ReadyNow),
        group = Group.Best,
        failoverCount = 0,
        line = VersionsFixtures.line,
        sourceClass = VersionsFixtures.sourceClass,
        score = VersionsFixtures.score,
    )

    private fun snapshot(
        recommended: VersionRow? = null,
        done: Boolean = false,
        pending: List<String> = emptyList(),
        slow: List<String> = emptyList(),
        failed: List<String> = emptyList(),
    ) = VersionsSnapshot(
        conditions = VersionsFixtures.conditions,
        recommended = recommended,
        rows = emptyList(),
        hidden = HiddenSummary(0, emptyList()),
        pendingAddons = pending,
        slowAddons = slow,
        failedAddons = failed,
        done = done,
        frozen = false,
        nobodySharing = false,
    )

    @Test
    fun `a row reads as quality and features, and no fact names its addon`() {
        assertEquals("4K, HDR10", VersionsCopy.headline(row()))
        assertEquals("4K", VersionsCopy.headline(row(features = emptyList())))
    }

    @Test
    fun `a row in another language than the viewer's carries its language`() {
        assertEquals(
            "Turkish",
            VersionsCopy.languageTag(row().copy(group = Group.OtherLanguages, languages = listOf("tr"))),
        )
        assertEquals(null, VersionsCopy.languageTag(row().copy(group = Group.Best, languages = listOf("tr"))))
        assertEquals(null, VersionsCopy.languageTag(row().copy(group = Group.OtherLanguages, languages = emptyList())))
    }

    @Test
    fun `the line under the primary button says what will play, that it is looking, or why nothing plays`() {
        assertEquals(VersionLine.Finding, VersionsCopy.line(null))
        assertEquals(VersionLine.Finding, VersionsCopy.line(snapshot()))
        assertEquals(VersionLine.Problem("Sources could not load"), VersionsCopy.line(null, failed = true))
        // Every addon answered with nothing while the core checks what this device keeps: still finding.
        assertEquals(VersionLine.Finding, VersionsCopy.line(snapshot(done = true)))
        // Every addon answered, rows exist, none is recommended: say so and let the viewer choose.
        val notReady = VersionsCopy.line(snapshot(done = true).copy(rows = listOf(row())))
        assertEquals(VersionLine.Problem("No version is ready to play yet. Choose one to start it."), notReady)
        val chosen = VersionsCopy.line(snapshot(recommended = row())) as VersionLine.Chosen
        assertEquals(listOf("4K", "HDR10", "1.2 GB"), chosen.facts)
        assertEquals("Ready", chosen.status)
        assertEquals(true, chosen.good)
        assertEquals("4K, HDR10, 1.2 GB, Ready", chosen.spoken)
        val torrent =
            row().copy(
                status = VersionStatus.Torrent(10),
                sizeBytes = null,
                features = emptyList(),
                via = "Torrentio",
            )
        assertEquals(
            VersionLine.Chosen(listOf("4K"), "Torrent, 10 seeds", good = false),
            VersionsCopy.line(snapshot(recommended = torrent)),
        )
    }

    @Test
    fun `an unstated quality is one word in a cell`() {
        assertEquals("Unknown", VersionsCopy.cell("Quality not stated"))
        assertEquals("720p", VersionsCopy.cell("720p"))
        assertEquals("WEB-DL", VersionsCopy.cell("WEB-DL"))
    }

    @Test
    fun `the group words read plainly`() {
        assertEquals("Smaller and faster", VersionsCopy.group(Group.SmallerFaster, pickAbove = true))
    }

    @Test
    fun `the reasons, groups and status words match the web table`() {
        assertEquals("Ready now", VersionsCopy.reason(Reason.ReadyNow))
        // A film replayed and an episode after the last one alike (walk EW-F1).
        assertEquals("Same release as last time", VersionsCopy.reason(Reason.SameReleaseAsLast))
        assertEquals(
            // The four the web names, and the words for a group from a newer core.
            listOf("Best quality", "Smaller and faster", "Other languages", "Needs download", "Other versions"),
            Group.entries.map(VersionsCopy::group),
        )
        assertEquals("Needs download", VersionsCopy.status(VersionStatus.NeedsDownload("Real-Debrid")))
        // With the pick above the list, the best group is not "Best quality".
        assertEquals("Other good choices", VersionsCopy.group(Group.Best, pickAbove = true))
        assertEquals("Smaller and faster", VersionsCopy.group(Group.SmallerFaster, pickAbove = true))
    }

    @Test
    fun `no words name the viewer's streaming service, as none names an addon`() {
        val lines =
            listOf(
                ReasonLine.Ready("TorBox", 4L shl 30),
                ReasonLine.Should(89, 4L shl 30, "TorBox"),
                ReasonLine.Fetch("TorBox"),
            )
        for (l in lines) {
            val r = row().copy(status = VersionStatus.Ready("TorBox"), line = l)
            val words =
                listOf(
                    VersionsCopy.why(r),
                    VersionsCopy.rowWords(r),
                    VersionsCopy.rowSpoken(r),
                    VersionsCopy.cardSpoken("Best for this TV", r),
                    VersionsCopy.details(r),
                    VersionsCopy.status(r.status),
                    StreamChoiceCopy.line(l).text,
                    StreamChoiceCopy.line(l).details,
                )
            for (w in words) assertFalse("TorBox" in w, "\"$w\" names the service")
        }
        assertEquals("Starts right away: ready now, 4 GB", VersionsCopy.why(row().copy(line = lines[0])))
    }

    @Test
    fun `every status, reason and group has words and no emoji`() {
        val words =
            Reason.entries.map { VersionsCopy.reason(it) } +
                Group.entries.map(VersionsCopy::group) +
                EmptyKind.entries.map(StreamChoiceCopy::emptyLine) +
                listOf(
                    VersionsCopy.status(VersionStatus.Ready(null)),
                    VersionsCopy.status(VersionStatus.Torrent(12)),
                    VersionsCopy.status(VersionStatus.NeedsDownload("Real-Debrid")),
                    VersionsCopy.status(VersionStatus.Direct),
                )
        assertTrue(words.all { it.isNotBlank() && it.all { c -> c.code < 0x2000 } })
    }

    @Test
    fun `the header never names an addon - finding, still looking, else the count`() {
        assertEquals("Finding versions" to VersionsCopy.Tone.Idle, VersionsCopy.progress(null))
        assertEquals(
            "Finding versions" to VersionsCopy.Tone.Idle,
            VersionsCopy.progress(snapshot(pending = listOf("Torrentio"))),
        )
        val found = snapshot(recommended = row(), pending = listOf("Torrentio"), slow = listOf("Comet"))
        assertEquals("Still looking for more versions" to VersionsCopy.Tone.Idle, VersionsCopy.progress(found))
        assertEquals(
            "1 version" to VersionsCopy.Tone.Ok,
            VersionsCopy.progress(snapshot(recommended = row(), done = true, failed = listOf("Torrentio"))),
        )
        assertEquals("0 versions" to VersionsCopy.Tone.Ok, VersionsCopy.progress(snapshot(done = true)))
        for (s in listOf(
            found,
            snapshot(slow = listOf("Torrentio")),
            snapshot(done = true, failed = listOf("Torrentio")),
        )) {
            val text = VersionsCopy.progress(s)?.first.orEmpty()
            assertFalse("Torrentio" in text || "Comet" in text, text)
        }
    }

    @Test
    fun `nothing that plays says the core's line under Play, and the header counts what was found`() {
        val empty = { kind: EmptyKind, count: Int ->
            snapshot(done = true).copy(empty = EmptyVersions(kind, EmptyAction.None, count))
        }
        assertEquals(
            VersionLine.Problem("None of these plays on this device"),
            VersionsCopy.line(empty(EmptyKind.NeedsAccount, 12)),
        )
        // Neutral: nothing plays, so no tone says these are good.
        assertEquals("12 versions" to VersionsCopy.Tone.Idle, VersionsCopy.progress(empty(EmptyKind.NeedsAccount, 12)))
        // Nothing found: the empty line says it, so no "0 versions" beside it.
        assertNull(VersionsCopy.progress(empty(EmptyKind.NoResults, 0)))
        // A kind from a newer core still says something.
        assertEquals(VersionLine.Problem("No source has this yet"), VersionsCopy.line(empty(EmptyKind.Unknown, 0)))
    }

    @Test
    fun `each zero-playable state offers its actions in the shared order, Back last`() {
        val steps = { action: EmptyAction -> VersionsCopy.emptySteps(EmptyVersions(EmptyKind.Unknown, action, 0)) }
        assertEquals(
            listOf(EmptyStep.PlayFromDevice, EmptyStep.TryAgain, EmptyStep.Back),
            steps(EmptyAction.PlayFromDevice),
        )
        assertEquals(listOf(EmptyStep.TryAgain, EmptyStep.Back), steps(EmptyAction.TryAgain))
        assertEquals(listOf(EmptyStep.Settings, EmptyStep.Back), steps(EmptyAction.SettingsAddons))
        // Choose a version is the player's and the title's: Versions lists the rows instead.
        assertEquals(listOf(EmptyStep.Back), steps(EmptyAction.ChooseVersion))
        assertEquals(listOf(EmptyStep.Back), steps(EmptyAction.None))
        assertEquals(listOf(EmptyStep.Back), steps(EmptyAction.Unknown))
        assertEquals(
            listOf("Play from this device", "Try again", "Settings", "Back"),
            EmptyStep.entries.map { it.label },
        )
    }

    @Test
    fun `language codes read as English names, an unknown code in capitals`() {
        assertEquals("English", VersionsCopy.languageName("eng"))
        assertEquals("German", VersionsCopy.languageName("ger"))
        assertEquals("Turkish", VersionsCopy.languageName("tr"))
        assertEquals("Portuguese (Brazil)", VersionsCopy.languageName("pob"))
        assertEquals("QQX", VersionsCopy.languageName("qqx"))
    }

    @Test
    fun `the player's starting line gives the quality and size, never where from`() {
        val line = VersionsCopy.starting(row().copy(via = "Torrentio", sizeBytes = 2L * 1024 * 1024 * 1024))
        assertEquals("Starting 4K, 2 GB", line)
        assertFalse("Torrentio" in line || " from " in line, line)
        assertEquals("Starting 4K", VersionsCopy.starting(row(sizeBytes = null)))
    }

    @Test
    fun `the card without a pick says none is ready, with no addon name`() {
        assertEquals("Nothing is ready to play yet", VersionsCopy.NOT_READY_TITLE)
        assertEquals("Choose a version to start it.", VersionsCopy.NOT_READY_BODY)
    }

    @Test
    fun `a title page never names an addon`() {
        // The owner, 2026-10-22 (design/DECISION.md, Shelves): addon names only on the Addons pages (Versions names
        // none either since the Versions spec): not the version's addon, nor where the details or the scores came from.
        val pages =
            listOf(
                "../ui-tv/src/main/kotlin/app/marquee/ui/tv/TvTitle.kt",
                "../ui-phone/src/main/kotlin/app/marquee/ui/phone/PhoneTitle.kt",
                "src/main/kotlin/app/marquee/core/TitleCopy.kt",
            )
        val names = Regex("""\.via\b|\bvia \$|\.source\b|Details from|Ratings from""")
        for (path in pages) {
            assertFalse(names.containsMatchIn(File(path).readText()), "$path names an addon")
        }
    }

    @Test
    fun `the list follows the core's quality sections, and rows they do not cover keep the groups`() {
        val a = row().copy(key = "a", quality = "4K")
        val b = row().copy(key = "b", quality = "1080p")
        val c = row().copy(key = "c", quality = "720p")
        val shown =
            snapshot(done = true).copy(
                rows = listOf(a, b, c),
                sections =
                    listOf(
                        VersionSection(SectionQuality.Fhd, listOf("b")),
                        VersionSection(SectionQuality.Uhd, listOf("a")),
                        VersionSection(SectionQuality.Hd, listOf("c", "gone")),
                        VersionSection(SectionQuality.Unknown, listOf("a")),
                        VersionSection(SectionQuality.Unknown, listOf("b")),
                    ),
            )
        val sections = VersionsCopy.sections(shown, shown.rows)
        // The core's order and titles; a key not in the rows is left out; two newer sections never share a key.
        assertEquals(
            listOf(
                "1080p" to listOf("b"),
                "4K" to listOf("a"),
                "720p and below" to listOf("c"),
                "Other" to listOf("a"),
                "Other" to listOf("b"),
            ),
            sections.map { it.title to it.rows.map(VersionRow::key) },
        )
        assertEquals(sections.size, sections.map { it.id }.toSet().size)
        // A row no section covers still shows, under its group after the sections.
        val extra = row().copy(key = "x", quality = "SD")
        val more = VersionsCopy.sections(shown.copy(rows = shown.rows + extra), shown.rows + extra)
        assertEquals(listOf("x"), more.last().rows.map(VersionRow::key))
        assertEquals("Best quality", more.last().title)
        // An older core sends no sections: the groups as before.
        val older = VersionsCopy.sections(shown.copy(sections = emptyList()), shown.rows)
        assertEquals(listOf("Best quality"), older.map { it.title })
        // The plugins' rows come last, under From plugins, each naming its provider and host.
        val found =
            row().copy(
                key = "plugin:r/p:h1",
                status = VersionStatus.Direct,
                sizeBytes = null,
                origin = RowOrigin.Plugin,
                sourceName = "Dizibox",
                host = "cdn.example.com",
            )
        val withPlugins =
            VersionsCopy.sections(
                shown.copy(plugins = SourceRows(listOf(found), pending = false)),
                shown.rows,
            )
        assertEquals(
            PluginCopy.SECTION to listOf(found.key),
            withPlugins.last().title to withPlugins.last().rows.map(VersionRow::key),
        )
        assertEquals("Dizibox, plays from cdn.example.com", VersionsCopy.quiet(found))
    }

    @Test
    fun `the shortcuts come in the core's order, never the card's own version`() {
        val best = row().copy(key = "best")
        val small = row().copy(key = "small")
        val sharp = row().copy(key = "sharp")
        val shown = snapshot(recommended = best, done = true).copy(smaller = small, sharper = sharp)
        assertEquals(
            listOf("Smaller file" to "small", "Sharper" to "sharp"),
            VersionsCopy.shortcuts(shown).map { it.first to it.second.key },
        )
        assertEquals(
            listOf("Sharper" to "sharp"),
            VersionsCopy.shortcuts(shown.copy(smaller = best)).map {
                it.first to
                    it.second.key
            },
        )
        assertEquals(emptyList(), VersionsCopy.shortcuts(null))
    }

    @Test
    fun `held over the player, rows keep the order they opened in, and a new one goes after them in its section`() {
        val seen = mutableMapOf<String, Int>()
        val r = { key: String -> row().copy(key = key) }
        val keys = { sections: List<VersionsCopy.Section> ->
            VersionsCopy
                .held(sections, seen)
                .single()
                .rows
                .map(VersionRow::key)
        }
        val first = listOf(VersionsCopy.Section("section:fhd", "1080p", listOf(r("a"), r("b"))))
        assertEquals(listOf("a", "b"), keys(first))
        // Recorded once they show; held reads only.
        assertEquals(emptyMap(), seen)
        VersionsCopy.record(VersionsCopy.held(first, seen), seen)
        // The core re-ranks b over a and finds c, ranked first.
        val reranked = listOf(VersionsCopy.Section("section:fhd", "1080p", listOf(r("c"), r("b"), r("a"))))
        assertEquals(listOf("a", "b", "c"), keys(reranked))
        assertEquals(mapOf("a" to 0, "b" to 1), seen, "held changes nothing")
    }

    @Test
    fun `the sheet's details drop only what the face already said, and keep the rest`() {
        val gib = 1L shl 30
        val shared = { line: ReasonLine ->
            row(sizeBytes = 4 * gib).copy(status = VersionStatus.Torrent(89), line = line)
        }
        // The face says "4 GB, shared by 89 people"; the account and the window are said only here.
        val should = shared(ReasonLine.Should(89, 4 * gib, "TorBox"))
        assertEquals("4 GB, shared by 89 people", VersionsCopy.rowWords(should))
        assertEquals("May be ready in your account", VersionsCopy.sheetDetails(should, null))
        val buffer = shared(ReasonLine.Buffer(89, 4 * gib, aheadMin = 7, budgetBytes = (2.5 * gib).toLong()))
        assertEquals("Room for about 7 min ahead in 2.5 GB", VersionsCopy.sheetDetails(buffer, null))
        // Nothing is left when the face said it all.
        assertEquals("", VersionsCopy.sheetDetails(shared(ReasonLine.Quick(89, 4 * gib)), null))
        // A shared file whose count the status does not know: the face says only its size, so the people stay.
        val unknown =
            row(sizeBytes = 4 * gib).copy(
                status = VersionStatus.Torrent(null),
                line =
                    ReasonLine.Should(
                        89,
                        4 * gib,
                        "TorBox",
                    ),
            )
        assertEquals("4 GB", VersionsCopy.rowWords(unknown))
        assertEquals("Shared by 89 people, may be ready in your account", VersionsCopy.sheetDetails(unknown, null))
        // A warning face keeps the details whole: they say what it does not.
        assertEquals("Shared by 3 people", VersionsCopy.sheetDetails(shared(ReasonLine.VeryFew(3)), null))
        // A ready file: the face says "2 GB, ready now", so neither part is said again.
        val ready =
            row(sizeBytes = 2 * gib).copy(
                status = VersionStatus.Ready(null),
                line =
                    ReasonLine.Ready(
                        null,
                        2 * gib,
                    ),
            )
        assertEquals("2 GB, ready now", VersionsCopy.rowWords(ready))
        assertEquals("", VersionsCopy.sheetDetails(ready, null))
        // On this device: the people and the size limit stay, as the face says neither.
        val kept = shared(ReasonLine.Quick(89, 4 * gib)).copy(onDevice = "On this device")
        assertEquals("Shared by 89 people", VersionsCopy.sheetDetails(kept, null))
        val large =
            row(sizeBytes = (7.4 * gib).toLong()).copy(
                onDevice = "On this device",
                line = ReasonLine.Large((7.4 * gib).toLong(), 5 * gib, chosen = false),
            )
        assertEquals("7.4 GB; Momotus prefers files up to 5 GB", VersionsCopy.sheetDetails(large, null))
        // A part is said only whole: "14 GB" and "1.4 GB" do not say "4 GB".
        assertEquals(
            "4 GB",
            VersionsCopy.sheetDetails(
                row(sizeBytes = 14 * gib).copy(
                    status = VersionStatus.Torrent(89),
                    line =
                        ReasonLine.Quick(
                            89,
                            4 * gib,
                        ),
                ),
                null,
            ),
        )
        assertEquals(
            "4 GB",
            VersionsCopy.sheetDetails(
                row(sizeBytes = (1.4 * gib).toLong()).copy(
                    status = VersionStatus.Torrent(89),
                    line =
                        ReasonLine.Quick(
                            89,
                            4 * gib,
                        ),
                ),
                null,
            ),
        )
        // A dimmed row's face says its size and why it is dimmed: the people follow.
        assertEquals(
            "Shared by 89 people",
            VersionsCopy.sheetDetails(shared(ReasonLine.Quick(89, 4 * gib)), EmptyKind.NeedsAccount),
        )
    }

    @Test
    fun `after the colon a word in capitals stays in capitals`() {
        assertEquals(
            "Recorded in a cinema, low quality: CAM copy",
            VersionsCopy.why(row().copy(line = ReasonLine.Copy("CAM"))),
        )
        assertEquals(
            "Colors may look washed out on this screen: HDR file, screen without HDR",
            VersionsCopy.why(row().copy(line = ReasonLine.WashedOut)),
        )
        assertEquals("May not start: shared by 3 people", VersionsCopy.why(row().copy(line = ReasonLine.VeryFew(3))))
    }

    @Test
    fun `badges follow Apple's row - the quality filled, one video and one audio format`() {
        val badges = { features: List<String> ->
            VersionsCopy.badges(row(features = features)).map { it.text to it.look }
        }
        assertEquals(
            listOf("4K" to Look.Filled, "Dolby Vision" to Look.Plain, "Dolby Atmos" to Look.Plain),
            badges(listOf("Dolby Vision", "HDR10", "Atmos", "5.1", "Remux")),
        )
        assertEquals(
            listOf("4K" to Look.Filled, "HDR" to Look.Outlined, "5.1" to Look.Outlined),
            badges(listOf("HDR10", "5.1")),
        )
        assertEquals(listOf("4K" to Look.Filled), badges(emptyList()))
        // A quality the release does not state gets no badge at all: its section says it.
        val unstated = { quality: String, features: List<String> ->
            VersionsCopy.badges(row(features = features).copy(quality = quality)).map { it.text }
        }
        assertEquals(listOf("HDR"), unstated("Quality not stated", listOf("HDR10")))
        assertEquals(emptyList(), unstated("Quality not stated", emptyList()))
        assertEquals(emptyList(), unstated("Unknown", emptyList()))
        // A source the core names in its place is a badge.
        assertEquals(listOf("WEB-DL"), unstated("WEB-DL", emptyList()))
        // Said with no empty part.
        val none = row(features = emptyList()).copy(quality = "Quality not stated")
        assertEquals("Best for this TV. ${VersionsCopy.why(none)}", VersionsCopy.cardSpoken("Best for this TV", none))
    }

    @Test
    fun `the card's why line for the version kept here says that alone, one idea`() {
        val kept = row().copy(line = ReasonLine.Quick(22, 2L * 1024 * 1024 * 1024), onDevice = "On this device")
        assertEquals("On this device", VersionsCopy.why(kept))
        assertEquals(
            "Best for this phone. 4K, HDR. On this device",
            VersionsCopy.cardSpoken("Best for this phone", kept),
        )
    }

    @Test
    fun `the card's why line joins the core's line and details, and says the whole card once`() {
        val one = row().copy(line = ReasonLine.Quick(412, 2L * 1024 * 1024 * 1024))
        val why = VersionsCopy.why(one)
        assertTrue(why.startsWith("Starts quickly: "), why)
        assertEquals("Best for this TV. 4K, HDR. $why", VersionsCopy.cardSpoken("Best for this TV", one))
    }

    @Test
    fun `a row's quiet line is its size and source in plain words, and a warning from the core replaces it`() {
        val gb = 2L * 1024 * 1024 * 1024
        val quick = ReasonLine.Quick(412, gb)
        assertEquals(
            "2 GB, shared by 412 people",
            VersionsCopy.quiet(row(sizeBytes = gb).copy(status = VersionStatus.Torrent(412), line = quick)),
        )
        assertEquals(
            "2 GB, shared by 1 person",
            VersionsCopy.quiet(row(sizeBytes = gb).copy(status = VersionStatus.Torrent(1), line = quick)),
        )
        assertEquals("2 GB, ready now", VersionsCopy.quiet(row(sizeBytes = gb).copy(line = quick)))
        assertEquals(
            "2 GB, direct link",
            VersionsCopy.quiet(row(sizeBytes = gb).copy(status = VersionStatus.Direct, line = quick)),
        )
        assertEquals(
            "2 GB, downloads first",
            VersionsCopy.quiet(row(sizeBytes = gb).copy(status = VersionStatus.NeedsDownload(null), line = quick)),
        )
        assertEquals(
            "2 GB, on this device",
            VersionsCopy.quiet(row(sizeBytes = gb).copy(line = quick, onDevice = "On this device")),
        )
        // The size always comes first, then the source, the core's warning or why the row is dimmed.
        assertEquals(
            "2 GB, may start slowly",
            VersionsCopy.rowWords(row(sizeBytes = gb).copy(line = ReasonLine.Few(31))),
        )
        assertEquals("2 GB, ready now", VersionsCopy.rowWords(row(sizeBytes = gb).copy(line = quick)))
        assertEquals("2 GB, won't play here", VersionsCopy.rowWords(row(sizeBytes = gb), EmptyKind.NeedsAccount))
        // With no size first, the words start with a capital.
        assertEquals(
            "On this device",
            VersionsCopy.rowWords(row(sizeBytes = null).copy(line = quick, onDevice = "On this device")),
        )
        assertEquals("Ready now", VersionsCopy.rowWords(row(sizeBytes = null).copy(line = quick)))
        assertEquals("May start slowly", VersionsCopy.rowWords(row(sizeBytes = null).copy(line = ReasonLine.Few(31))))
        assertEquals("Won't play here", VersionsCopy.rowWords(row(sizeBytes = null), EmptyKind.NeedsAccount))
        // A warning shows in its place.
        assertNull(VersionsCopy.quiet(row(sizeBytes = gb).copy(line = ReasonLine.Few(31))))
        // Said as its badges, then the quiet line.
        assertEquals("4K, HDR, 2 GB, ready now", VersionsCopy.rowSpoken(row(sizeBytes = gb).copy(line = quick)))
    }
}
