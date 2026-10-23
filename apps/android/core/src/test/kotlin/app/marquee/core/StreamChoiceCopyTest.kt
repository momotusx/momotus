package app.marquee.core

import app.marquee.core.StreamChoiceCopy.Setting
import app.marquee.core.StreamChoiceCopy.UpDown
import app.marquee.core.model.BlockedReason
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.Learned
import app.marquee.core.model.LearnedByClass
import app.marquee.core.model.LearnedChoice
import app.marquee.core.model.Network
import app.marquee.core.model.Prefs
import app.marquee.core.model.ReasonLine
import app.marquee.core.model.Speed
import app.marquee.core.model.StreamPrefs
import app.marquee.core.model.VersionsConditions
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class StreamChoiceCopyTest {
    private val gib = 1L shl 30
    private val mib = 1L shl 20

    private fun value(v: Any?): JsonElement =
        when (v) {
            null -> JsonNull
            is String -> JsonPrimitive(v)
            is Number -> JsonPrimitive(v)
            is Boolean -> JsonPrimitive(v)
            else -> error("no JSON for $v")
        }

    private fun JsonObjectBuilder.upDown(
        key: String,
        w: UpDown,
    ) = put(
        key,
        buildJsonObject {
            put("up", w.up)
            put("down", w.down)
        },
    )

    private fun <T> setting(
        w: Setting<T>,
        extra: JsonObjectBuilder.() -> Unit = {},
    ) = buildJsonObject {
        put("label", w.label)
        w.help?.let { put("help", it) }
        w.custom?.let { put("custom", it) }
        extra()
        put(
            "choices",
            JsonArray(
                w.choices.map { c ->
                    buildJsonObject {
                        put("label", c.label)
                        put("value", value(c.value))
                    }
                },
            ),
        )
    }

    private fun answer(a: StreamChoiceCopy.Answer) =
        buildJsonObject {
            put("label", a.label)
            put("text", a.text)
            put(
                "patch",
                buildJsonObject {
                    put("sizeLimitBytes", value(a.sizeLimitBytes))
                    put("minSeeders", a.minSeeders)
                },
            )
        }

    /** Our words in the shared file's shape. */
    private fun ours(): JsonObject =
        with(StreamChoiceCopy) {
            buildJsonObject {
                put(
                    "plurals",
                    buildJsonObject {
                        for ((k, v) in PLURALS) {
                            put(
                                k,
                                JsonArray(listOf(JsonPrimitive(v.first), JsonPrimitive(v.second))),
                            )
                        }
                    },
                )
                put(
                    "lines",
                    buildJsonObject {
                        for ((k, w) in LINES) {
                            put(
                                k,
                                buildJsonObject {
                                    put("tone", w.tone.name.lowercase())
                                    put("text", w.text)
                                    w.textChosen?.let { put("textChosen", it) }
                                    w.textSmallSwarm?.let { put("textSmallSwarm", it) }
                                    put("details", JsonArray(w.details.map(::JsonPrimitive)))
                                    w.detailsMobile?.let { put("detailsMobile", JsonArray(it.map(::JsonPrimitive))) }
                                },
                            )
                        }
                    },
                )
                put(
                    "speedSource",
                    buildJsonObject {
                        put("measured", SPEED_MEASURED)
                        put("estimated", SPEED_ESTIMATED)
                    },
                )
                put(
                    "blocked",
                    buildJsonObject {
                        for ((reason, w) in BLOCKED) {
                            put(
                                reason.wire,
                                buildJsonObject {
                                    put("text", w.first)
                                    if (reason == BlockedReason.NoSpace) put("textNoWindow", NO_SPACE_NO_WINDOW)
                                    put("action", w.second)
                                },
                            )
                        }
                        put("showRows", SHOW_ROWS)
                    },
                )
                put(
                    "empty",
                    buildJsonObject {
                        for ((kind, w) in EMPTY) {
                            put(
                                kind.wire,
                                buildJsonObject {
                                    put("text", w.first)
                                    w.second?.let { put("action", it) }
                                    if (kind == EmptyKind.Unreachable) put("playFromDevice", PLAY_FROM_DEVICE)
                                },
                            )
                        }
                    },
                )
                put(
                    "learned",
                    buildJsonObject {
                        for ((choice, w) in LEARNED) upDown(choice.wire, w)
                        put("undo", UNDO)
                        put(
                            "settingsLines",
                            buildJsonObject { for ((choice, w) in SETTINGS_LINES) upDown(choice.wire, w) },
                        )
                        put("screens", buildJsonObject { for ((cls, w) in LEARNED_SCREENS) put("$cls", w) })
                    },
                )
                put(
                    "question",
                    buildJsonObject {
                        put("title", QUESTION_TITLE)
                        put("lead", QUESTION_LEAD)
                        put("fast", answer(FAST))
                        put("best", answer(BEST))
                        put(
                            "skip",
                            buildJsonObject {
                                put("label", SKIP)
                                put("means", SKIP_MEANS)
                            },
                        )
                    },
                )
                put(
                    "settings",
                    buildJsonObject {
                        put("title", TITLE)
                        put("lede", LEDE)
                        put("summary", SUMMARY)
                        put("size", setting(SIZE))
                        put("seeders", setting(SEEDERS))
                        put("resolution", setting(RESOLUTION))
                        put("hdr", setting(HDR))
                        put("ready", setting(READY) { put("onlyWithAccount", READY_ONLY_WITH_ACCOUNT) })
                        put(
                            "audio",
                            buildJsonObject {
                                put("label", AUDIO)
                                put("help", AUDIO_HELP)
                            },
                        )
                        put("learn", setting(LEARN) { put("forget", FORGET) })
                        put(
                            "device",
                            buildJsonObject {
                                put("label", DEVICE)
                                put("measureAgain", MEASURE_AGAIN)
                                put("measuring", MEASURING)
                            },
                        )
                    },
                )
                put(
                    "conditions",
                    buildJsonObject {
                        put("header", HEADER)
                        put("devices", buildJsonObject { for ((k, v) in DEVICES) put(k, v) })
                        put("screens", buildJsonObject { for ((cls, v) in SCREENS) put("$cls", v) })
                        put("hdr", SCREEN_HDR)
                        put("speed", SPEED)
                        put("speedUnknown", SPEED_UNKNOWN)
                        put("free", FREE)
                        put("networks", buildJsonObject { for ((n, v) in NETWORKS) put(n.wire, v) })
                    },
                )
                put(
                    "nobodySharing",
                    buildJsonObject {
                        put("text", NOBODY_SHARING)
                        put("details", NOBODY_SHARING_DETAILS)
                    },
                )
            }
        }

    @Test
    fun `the words are the shared ones, every key and no more`() {
        val shared =
            JsonObject(
                Json
                    .parseToJsonElement(File("../../shared/stream-choice-words.json").readText())
                    .jsonObject - "about",
            )
        assertEquals(shared, ours())
    }

    private fun line(l: ReasonLine) = StreamChoiceCopy.line(l)

    private val ok = VersionsCopy.Tone.Ok
    private val warn = VersionsCopy.Tone.Warn
    private val bad = VersionsCopy.Tone.Bad

    @Test
    fun `every line has its tone, its plain text and its numbers in the details`() {
        val line = StreamChoiceCopy::line
        assertEquals(
            StreamChoiceCopy.Line(bad, "Recorded in a cinema, low quality", "Telesync copy"),
            line(ReasonLine.Copy("telesync")),
        )
        assertEquals(
            StreamChoiceCopy.Line(bad, "Not ready yet, has to download first", "Real-Debrid does not have it yet"),
            line(ReasonLine.Fetch("Real-Debrid")),
        )
        assertEquals("1 seeder", line(ReasonLine.VeryFew(1)).details)
        assertEquals(StreamChoiceCopy.Line(bad, "May not start", "3 seeders"), line(ReasonLine.VeryFew(3)))
        assertEquals(
            StreamChoiceCopy.Line(warn, "May be a different film or show", "The release says 2004"),
            line(ReasonLine.OtherYear(2004)),
        )
        assertEquals(
            StreamChoiceCopy.Line(warn, "Colors may look washed out on this screen", "HDR file, screen without HDR"),
            line(ReasonLine.WashedOut),
        )
        assertEquals(
            StreamChoiceCopy.Line(
                bad,
                "Did not start here last time",
                "Almost nothing arrived when this device last tried it",
            ),
            line(ReasonLine.Stalled),
        )
        assertEquals(
            StreamChoiceCopy.Line(
                bad,
                "No picture on this device",
                "Its HEVC video is stored in a form this device cannot show",
            ),
            line(ReasonLine.NoPicture),
        )
        assertEquals(
            StreamChoiceCopy.Line(
                bad,
                "Did not play on this device",
                "This device could not decode it when it last tried",
            ),
            line(ReasonLine.DidNotPlay),
        )
        assertEquals(StreamChoiceCopy.Line(warn, "May start slowly", "31 seeders"), line(ReasonLine.Few(31)))
        assertEquals(
            StreamChoiceCopy.Line(ok, "Starts right away", "Ready on TorBox, 7.4 GB"),
            line(ReasonLine.Ready("TorBox", (7.4 * gib).toLong())),
        )
        assertEquals(
            StreamChoiceCopy.Line(ok, "Starts quickly", "1,204 seeders, 2.2 GB"),
            line(ReasonLine.Quick(1204, (2.2 * gib).toLong())),
        )
        assertEquals(
            StreamChoiceCopy.Line(ok, "Should start quickly", "89 seeders, 4 GB, may be ready on Real-Debrid"),
            line(ReasonLine.Should(89, 4 * gib, "Real-Debrid")),
        )
    }

    @Test
    fun `a detail with an unknown number is left out, never shown with a gap`() {
        assertEquals("", line(ReasonLine.Should(null, null, null)).details)
        assertEquals("3 GB", line(ReasonLine.Quick(null, 3 * gib)).details)
        assertEquals("3 GB", line(ReasonLine.Ready(null, 3 * gib)).details)
        assertEquals("", line(ReasonLine.Fetch(null)).details)
    }

    @Test
    fun `a large file says whether the size was the user's choice`() {
        val large = ReasonLine.Large((7.4 * gib).toLong(), 5 * gib, chosen = false)
        assertEquals(StreamChoiceCopy.Line(warn, "Large file", "7.4 GB; Marquee prefers files up to 5 GB"), line(large))
        assertEquals("Larger than the size you chose", line(large.copy(chosen = true)).text)
    }

    @Test
    fun `may pause says where its speed comes from`() {
        assertEquals(
            "Needs about 32 Mbit/s; this connection gives about 18 (estimated)",
            line(ReasonLine.MayPause(31.6f, 17.5f, 0)).details,
        )
        assertEquals(
            "Needs about 79 Mbit/s; this connection gives about 25 (measured from 1 playback)",
            line(ReasonLine.MayPause(79f, 25f, 1)).details,
        )
        assertTrue(line(ReasonLine.MayPause(79f, 25f, 6)).details.contains("(measured from 6 playbacks)"))
    }

    @Test
    fun `may pause on a small swarm says few people share it, and keeps the speed`() {
        val small = line(ReasonLine.MayPause(31.6f, 17.5f, 0, smallSwarm = true))
        assertEquals("May pause: only a few people are sharing it", small.text)
        assertEquals("Needs about 32 Mbit/s; this connection gives about 18 (estimated)", small.details)
        assertEquals("May pause on this connection", line(ReasonLine.MayPause(31.6f, 17.5f, 0)).text)
    }

    @Test
    fun `a window says its minutes ahead, or on mobile data that it keeps a short stretch`() {
        val buffer = ReasonLine.Buffer(214, 40 * gib, aheadMin = 7, budgetBytes = (2.5 * gib).toLong())
        assertEquals(
            StreamChoiceCopy.Line(
                ok,
                "Plays without storing the whole file",
                "214 seeders, 40 GB, room for about 7 min ahead in 2.5 GB",
            ),
            line(buffer),
        )
        assertEquals(
            "214 seeders, 40 GB, keeps only a short stretch ahead on mobile data",
            line(buffer.copy(aheadMin = null)).details,
        )
        assertEquals(
            "40 GB, room for about 10 min ahead in 2.5 GB",
            line(buffer.copy(seeders = null, aheadMin = 10)).details,
        )
    }

    @Test
    fun `the recommended card shows the window's line, and no other line`() {
        val buffer = ReasonLine.Buffer(53, 545 * mib, aheadMin = 8, budgetBytes = 508 * mib)
        assertEquals(
            StreamChoiceCopy.Line(
                ok,
                "Plays without storing the whole file",
                "53 seeders, 545 MB, room for about 8 min ahead in 508 MB",
            ),
            StreamChoiceCopy.cardLine(VersionsFixtures.row(line = buffer)),
        )
        assertNull(StreamChoiceCopy.cardLine(VersionsFixtures.row(line = ReasonLine.Quick(53, gib))))
    }

    @Test
    fun `a version this device holds the start of says so as its line, on its row and on the card`() {
        val quick = ReasonLine.Quick(53, gib)
        val held = VersionsFixtures.row(line = quick).copy(onDevice = "On this device")
        val line = StreamChoiceCopy.rowLine(held)
        assertEquals("On this device", line.text)
        assertEquals(StreamChoiceCopy.line(quick).details, line.details, "the reason's details stay for focus")
        assertEquals(line, StreamChoiceCopy.cardLine(held))
        assertEquals(StreamChoiceCopy.line(quick), StreamChoiceCopy.rowLine(held.copy(onDevice = null)))
    }

    @Test
    fun `the empty line is the core's words, and a newer kind reads as noResults`() {
        assertEquals("These play through a streaming account", StreamChoiceCopy.emptyLine(EmptyKind.NeedsAccount))
        assertEquals("No source has this yet", StreamChoiceCopy.emptyLine(EmptyKind.Unknown))
        assertEquals("2,000 versions", StreamChoiceCopy.counted("versions", 2000))
    }

    @Test
    fun `the device line reads as one sentence, leaving out what is not known`() {
        val measured = Speed(42.2f, 3)
        val prior = Speed(15f, 0)
        assertEquals(
            "4K HDR, about 42 Mbit/s on Wi-Fi, 61 GB free",
            StreamChoiceCopy.conditionsLine(2160, true, Network.Wifi, measured, 61 * gib),
        )
        assertEquals(
            "1080p, mobile data, speed not measured yet",
            StreamChoiceCopy.conditionsLine(1080, false, Network.Cellular, prior, null),
        )
        val v =
            VersionsConditions(
                screenClass = 480,
                hdr = false,
                network = Network.Other,
                http = prior,
                p2p = measured,
                runtimeMin = 52,
                sizeLimitBytes = 5 * gib,
                minSeeders = 50,
                engine = true,
                account = false,
            )
        // No account and the engine: the torrent speed decides.
        assertEquals("This TV: SD, about 42 Mbit/s on this network", StreamChoiceCopy.header(tv = true, v))
        // With an account, or without the engine, the HTTP one does.
        assertEquals(
            "This phone: SD, this network, speed not measured yet",
            StreamChoiceCopy.header(tv = false, v.copy(account = true)),
        )
        assertEquals(prior, StreamChoiceCopy.speedFor(prior, measured, engine = false, account = false))
        // A network type from a newer core reads as "this network".
        assertEquals(
            "1080p, this network, speed not measured yet",
            StreamChoiceCopy.conditionsLine(1080, false, Network.Unknown, prior, null),
        )
    }

    private fun streams(
        sizeLimitBytes: Long? = null,
        maxHeight: Int? = null,
        minSeeders: Int = 50,
    ) = StreamPrefs(
        quality = app.marquee.core.model.QualityPref.Balanced,
        maxHeight = maxHeight,
        sizeLimitBytes = sizeLimitBytes,
        minSeeders = minSeeders,
        hdr = true,
        preferReady = true,
        learn = true,
    )

    @Test
    fun `a learned note names the device and the preferences now in effect`() {
        assertEquals(
            "On this TV you often picked bigger files, so Marquee now picks files up to 10 GB.",
            StreamChoiceCopy.learnedNote(LearnedChoice.Size, true, tv = true, streams(sizeLimitBytes = 10 * gib)),
        )
        assertEquals(
            "On this phone you often picked lower resolutions, so Marquee now picks up to 720p.",
            StreamChoiceCopy.learnedNote(LearnedChoice.Height, false, tv = false, streams(maxHeight = 720)),
        )
        assertEquals(
            "On this TV you often picked HDR versions, so Marquee now prefers HDR.",
            StreamChoiceCopy.learnedNote(LearnedChoice.Hdr, true, tv = true, streams()),
        )
        // No number to name: no note rather than a gap.
        assertNull(StreamChoiceCopy.learnedNote(LearnedChoice.Size, true, tv = true, streams()))
        assertNull(StreamChoiceCopy.learnedNote(LearnedChoice.Unknown, true, tv = true, streams()))
    }

    private val zero = Learned(ready = 0, height = 0, hdr = 0, size = 0, picks = 0, epoch = 0, atMs = 0)

    private fun prefs(
        uhd: Learned,
        fhd: Learned,
        epoch: Int = 0,
    ) = Prefs(
        streams = streams(),
        autoplayNext = true,
        homeRows =
            app.marquee.core.model
                .HomeRows(),
        learned = LearnedByClass(uhd, fhd, zero, zero),
        learnedEpoch = epoch,
    )

    @Test
    fun `Settings lists what was learned at 3 or more, unless undone or forgotten`() {
        val p =
            prefs(
                zero.copy(size = 3, height = -4, hdr = 2, picks = 9),
                zero.copy(ready = 5, hdr = 3, muted = listOf(LearnedChoice.Ready)),
            )
        // Largest screens first, then size, height, ready, HDR.
        assertEquals(
            listOf(
                StreamChoiceCopy.LearnedRow(
                    2160,
                    LearnedChoice.Size,
                    true,
                    "On 4K screens you often picked bigger files.",
                ),
                StreamChoiceCopy.LearnedRow(
                    2160,
                    LearnedChoice.Height,
                    false,
                    "On 4K screens you often picked lower resolutions.",
                ),
                StreamChoiceCopy.LearnedRow(
                    1080,
                    LearnedChoice.Hdr,
                    true,
                    "On 1080p screens you often picked HDR versions.",
                ),
            ),
            StreamChoiceCopy.learnedRows(p),
        )
        // A class stamped before the last Forget reads as zero.
        assertEquals(emptyList(), StreamChoiceCopy.learnedRows(p.copy(learnedEpoch = 1)))
        assertTrue(StreamChoiceCopy.anyLearned(p))
        assertFalse(StreamChoiceCopy.anyLearned(p.copy(learnedEpoch = 1)))
    }

    @Test
    fun `the onboarding patches are the plan's, and Skip is Start fast`() {
        assertEquals(5 * gib, StreamChoiceCopy.FAST.sizeLimitBytes)
        assertEquals(50, StreamChoiceCopy.FAST.minSeeders)
        assertNull(StreamChoiceCopy.BEST.sizeLimitBytes)
        assertEquals(20, StreamChoiceCopy.BEST.minSeeders)
        assertEquals("fast", StreamChoiceCopy.SKIP_MEANS)
    }

    @Test
    fun `the Settings row sums up the size and the seeders as the chips say them`() {
        assertEquals(
            "Up to 5 GB, shared by at least 50 people",
            StreamChoiceCopy.summary(streams(5 * gib, minSeeders = 50)),
        )
        assertEquals("Any size, shared by at least 20 people", StreamChoiceCopy.summary(streams(null, minSeeders = 20)))
        assertEquals(
            "Up to 2.5 GB, shared by at least 35 people",
            StreamChoiceCopy.summary(streams((2.5 * gib).toLong(), minSeeders = 35)),
        )
    }

    @Test
    fun `a display's lines fall into the core's screen classes`() {
        assertEquals(
            listOf(2160, 2160, 1080, 1080, 1080, 720, 720, 480),
            listOf(2160, 2000, 1440, 1179, 1000, 720, 700, 576).map(StreamChoiceCopy::screenClass),
        )
    }
}
