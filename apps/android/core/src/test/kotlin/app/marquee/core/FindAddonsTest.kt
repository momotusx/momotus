package app.marquee.core

import app.marquee.core.FindAddons.Action
import app.marquee.core.FindAddons.Filter
import app.marquee.core.client.CoreJson
import app.marquee.core.client.CoreTimeouts
import app.marquee.core.model.AddonCatalogSnapshot
import app.marquee.core.model.Configuration
import app.marquee.core.model.Installed
import app.marquee.core.model.ListState
import app.marquee.core.model.ListedAddon
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class FindAddonsTest {
    private fun entry(
        installed: Installed = Installed.Absent,
        configuration: Configuration = Configuration.None,
        name: String = "Example",
        host: String = "addon.example",
        types: List<String> = listOf("movie"),
        provides: List<String> = listOf("subtitles"),
        copyKey: String? = null,
    ) = ListedAddon(
        key = "https://addon.example/manifest.json",
        id = "org.example",
        host = host,
        name = name,
        types = types,
        provides = provides,
        configuration = configuration,
        configureUrl = "https://addon.example/configure",
        installed = installed,
        copyKey = copyKey,
    )

    @Test
    fun `each installed state gives its one action`() {
        assertEquals(Action.None("Installed"), FindAddons.action(entry(Installed.On)))
        assertEquals(Action.None("Set up"), FindAddons.action(entry(Installed.CopyOn)))
        assertEquals(
            Action.TurnOn("https://addon.example/manifest.json", "Off"),
            FindAddons.action(entry(Installed.Off)),
        )
        val copy = entry(Installed.CopyOff, copyKey = "https://addon.example/abc/manifest.json")
        assertEquals(Action.TurnOn("https://addon.example/abc/manifest.json", "Set up, off"), FindAddons.action(copy))
        // A copy whose link was too long to send: the words, no press.
        assertEquals(Action.None("Set up, off"), FindAddons.action(entry(Installed.CopyOff)))
    }

    @Test
    fun `setup decides between Install, Install and Configure, and Configure`() {
        assertEquals(Action.Install(configure = false), FindAddons.action(entry()))
        assertEquals(Action.Install(configure = true), FindAddons.action(entry(configuration = Configuration.Optional)))
        assertEquals(Action.Configure, FindAddons.action(entry(configuration = Configuration.Required)))
        // An installed state wins over setup.
        assertEquals(
            Action.None("Installed"),
            FindAddons.action(entry(Installed.On, configuration = Configuration.Required)),
        )
        assertEquals(true, FindAddons.pressable(Action.Install(true)))
        assertEquals(false, FindAddons.pressable(Action.Configure))
        assertEquals(false, FindAddons.pressable(Action.None("Installed")))
    }

    @Test
    fun `an installed state wins over required setup`() {
        assertEquals(
            Action.TurnOn("https://addon.example/manifest.json", "Off"),
            FindAddons.action(entry(Installed.Off, configuration = Configuration.Required)),
        )
        val copy =
            entry(
                Installed.CopyOff,
                configuration = Configuration.Required,
                copyKey = "https://addon.example/abc/manifest.json",
            )
        assertEquals(Action.TurnOn("https://addon.example/abc/manifest.json", "Set up, off"), FindAddons.action(copy))
        assertEquals(
            Action.None("Set up"),
            FindAddons.action(entry(Installed.CopyOn, configuration = Configuration.Required)),
        )
    }

    @Test
    fun `a card's whole description carries the busy word, the setup line and the failure`() {
        val subs = entry(name = "OpenSubtitles v3")
        assertEquals("Install OpenSubtitles v3, subtitles", FindAddons.described(subs, FindAddons.action(subs), false))
        assertEquals(
            "OpenSubtitles v3, subtitles, installing",
            FindAddons.described(subs, FindAddons.action(subs), false, busyWord = "Installing"),
        )
        val plexio = entry(name = "Plexio", provides = listOf("stream"), configuration = Configuration.Required)
        assertEquals(
            "Plexio, streams. Set it up on its website, then add the link in Momotus on your phone or computer. " +
                "With sync on, it follows you here.",
            FindAddons.described(plexio, Action.None(""), false, help = FindAddonsCopy.CONFIGURE_TV),
        )
        assertEquals(
            "Install OpenSubtitles v3, subtitles. This addon's server did not answer in time.",
            FindAddons.described(
                subs,
                FindAddons.action(subs),
                false,
                failure = "This addon's server did not answer in time",
            ),
        )
    }

    @Test
    fun `an install refused as needing setup asks for Configure until the addon is installed another way`() {
        assertEquals(Action.Configure, FindAddons.cardAction(entry(), "needsSetup"))
        assertEquals(Action.Install(configure = false), FindAddons.cardAction(entry(), "timeout"))
        assertEquals(Action.Install(configure = false), FindAddons.cardAction(entry(), null))
        assertEquals(Action.None("Set up"), FindAddons.cardAction(entry(Installed.CopyOn), "needsSetup"))
    }

    @Test
    fun `a listed install may wait for the network`() {
        // The core's own fetch ends within 10 s; a local op's 8 s would cut it short.
        assertEquals(CoreTimeouts.network, CoreTimeouts.forOp("installListed"))
    }

    @Test
    fun `filters read what an addon provides`() {
        val subs = entry(provides = listOf("subtitles"))
        assertEquals(true, FindAddons.shows(subs, Filter.All))
        assertEquals(true, FindAddons.shows(subs, Filter.Subtitles))
        assertEquals(false, FindAddons.shows(subs, Filter.Sources))
        assertEquals(false, FindAddons.shows(subs, Filter.Catalogs))
        assertEquals(true, FindAddons.shows(entry(provides = listOf("catalog", "meta", "stream")), Filter.Sources))
        assertEquals(true, FindAddons.shows(entry(provides = listOf("catalog", "meta")), Filter.Catalogs))
        assertEquals("No subtitle addons in this list.", FindAddons.filterEmpty(Filter.Subtitles))
        assertEquals("No source addons in this list.", FindAddons.filterEmpty(Filter.Sources))
        assertEquals("No catalog addons in this list.", FindAddons.filterEmpty(Filter.Catalogs))
        assertNull(FindAddons.filterEmpty(Filter.All))
        assertEquals(listOf("All", "Sources", "Subtitles", "Catalogs"), Filter.entries.map { it.label })
    }

    @Test
    fun `the mark is the first letter or digit`() {
        assertEquals("O", FindAddons.initial("OpenSubtitles v3"))
        assertEquals("4", FindAddons.initial("✨ 4K Streams"))
        assertEquals("ש", FindAddons.initial("שלום"))
        assertEquals("N", FindAddons.initial("[NEW] TurkceAltyazi.Org"))
        assertEquals("C", FindAddons.initial("🕒 Clockrr"))
        assertEquals("", FindAddons.initial("!!!"))
    }

    @Test
    fun `a card's spoken label names the action, then the host only when names repeat`() {
        val subs = entry(name = "OpenSubtitles v3")
        assertEquals("Install OpenSubtitles v3, subtitles", FindAddons.spoken(subs, FindAddons.action(subs), false))
        val trakt = entry(name = "Trakt Tv", host = "trakt.example", provides = listOf("catalog"))
        assertEquals(
            "Install Trakt Tv, from trakt.example, catalogs",
            FindAddons.spoken(trakt, FindAddons.action(trakt), true),
        )
        val on = entry(Installed.On)
        assertEquals("Example, subtitles, installed", FindAddons.spoken(on, FindAddons.action(on), false))
        val off = entry(Installed.Off)
        assertEquals("Turn on Example, subtitles", FindAddons.spoken(off, FindAddons.action(off), false))
        assertEquals("Configure Example, subtitles", FindAddons.spoken(entry(), Action.Configure, false))
        // A TV card with nothing to press says what it is, without a verb.
        assertEquals("Example, subtitles", FindAddons.spoken(entry(), Action.None(""), false))
    }

    @Test
    fun `what a card provides reads as a sentence`() {
        assertEquals(
            "Subtitles for movies and series",
            FindAddons.providesLine(entry(provides = listOf("subtitles"), types = listOf("movie", "series"))),
        )
        assertEquals(
            "Catalogs, Details for channels",
            FindAddons.providesLine(entry(provides = listOf("catalog", "meta"), types = listOf("channel"))),
        )
        assertEquals(
            "Streams for movies, series and other",
            FindAddons.providesLine(entry(provides = listOf("stream"), types = listOf("movie", "series", "other"))),
        )
        assertEquals("Streams", FindAddons.providesLine(entry(provides = listOf("stream"), types = emptyList())))
        assertEquals(
            "Addon lists for live TV",
            FindAddons.providesLine(entry(provides = listOf("addon_catalog"), types = listOf("tv"))),
        )
    }

    @Test
    fun `only names shown twice in a list are marked`() {
        val names =
            FindAddons.repeatedNames(
                listOf(entry(name = "Trakt Tv"), entry(name = "Trakt Tv"), entry(name = "Kitsu")),
            )
        assertEquals(setOf("Trakt Tv"), names)
    }

    /**
     * A snapshot in the core's serde shape, its entries taken from the core's own fixture of the
     * live lists (crates/mq-runtime/tests/fixtures/addon_catalog): every installed state, every
     * setup, a Hebrew name, a null `copyKey`. Strict: an unknown key fails.
     */
    @Test
    fun `a snapshot of both lists decodes, and each entry gets its action`() {
        val strict = Json(CoreJson) { ignoreUnknownKeys = false }
        val text = javaClass.getResource("/addon-catalog.json")!!.readText()
        val snapshot = strict.decodeFromString(AddonCatalogSnapshot.serializer(), text)
        assertEquals(listOf("official", "community"), snapshot.lists.map { it.id })
        assertEquals(listOf(ListState.Ready, ListState.Ready), snapshot.lists.map { it.state })
        val actions = snapshot.lists.flatMap { it.entries }.associate { it.name to FindAddons.action(it) }
        assertEquals(Action.None("Installed"), actions["Cinemeta"])
        assertEquals(Action.Install(configure = false), actions["YouTube"])
        assertEquals(Action.TurnOn("https://watchhub.strem.io/manifest.json", "Off"), actions["WatchHub"])
        assertEquals(
            Action.TurnOn(
                "https://caching.stremio.net/publicdomainmovies.now.sh/configured/manifest.json",
                "Set up, off",
            ),
            actions["Public Domain Movies"],
        )
        assertEquals(Action.None("Set up"), actions["OpenSubtitles v3"])
        assertEquals(Action.Install(configure = true), actions["Cast Search"])
        assertEquals(Action.Configure, actions["Plexio"])
        assertEquals(Action.None("Set up, off"), actions["Statusio"])
        val hebrew = snapshot.lists[1].entries.first { it.host == "sub.scary.network" }
        assertEquals("כ", FindAddons.initial(hebrew.name))
        assertEquals("Subtitles for movies and series", FindAddons.providesLine(hebrew))
        val subtitles = snapshot.lists[0].entries.filter { FindAddons.shows(it, Filter.Subtitles) }
        assertEquals(listOf("OpenSubtitles v3"), subtitles.map { it.name })
    }

    @Test
    fun `a loading and a failed list decode with the failure's words`() {
        val strict = Json(CoreJson) { ignoreUnknownKeys = false }
        val snapshot =
            strict.decodeFromString(
                AddonCatalogSnapshot.serializer(),
                """{"lists":[{"id":"official","name":"Official","state":{"state":"loading"},"entries":[]},
                    {"id":"community","name":"Community","state":{"state":"failed","words":"Cinemeta did not answer."},
                     "entries":[]}]}""",
            )
        assertEquals(ListState.Loading, snapshot.lists[0].state)
        assertEquals(ListState.Failed("Cinemeta did not answer."), snapshot.lists[1].state)
    }

    /**
     * The snapshot fixture above is written by hand from the core's fixture of the live lists: each of
     * its entries must still be in that file, in the same list, with the same id, name and setup.
     */
    @Test
    fun `the snapshot fixture matches the core's fixture of the live lists`() {
        val core = File("../../../crates/mq-runtime/tests/fixtures/addon_catalog")
        val live =
            listOf("official", "community")
                .flatMap { list ->
                    val addons = CoreJson.parseToJsonElement(File(core, "$list.json").readText()).jsonObject
                    addons.getValue("addons").jsonArray.map { a ->
                        val o = a.jsonObject
                        val manifest = o.getValue("manifest").jsonObject
                        val hints = manifest["behaviorHints"] as? JsonObject

                        fun hint(name: String) = hints?.get(name)?.jsonPrimitive?.booleanOrNull == true
                        val setup =
                            when {
                                hint("configurationRequired") -> Configuration.Required
                                hint("configurable") -> Configuration.Optional
                                else -> Configuration.None
                            }
                        o.getValue("transportUrl").jsonPrimitive.content to
                            listOf(
                                list,
                                manifest.getValue("id").jsonPrimitive.content,
                                manifest.getValue("name").jsonPrimitive.content,
                                setup.name,
                            )
                    }
                }.toMap()
        val text = javaClass.getResource("/addon-catalog.json")!!.readText()
        val snapshot =
            Json(
                CoreJson,
            ) { ignoreUnknownKeys = false }.decodeFromString(AddonCatalogSnapshot.serializer(), text)
        for (list in snapshot.lists) {
            for (e in list.entries) {
                assertEquals(live[e.key], listOf(list.id, e.id, e.name, e.configuration.name), e.name)
            }
        }
    }

    @Test
    fun `row ids hold no link and stay with their entry when one is added before it`() {
        val a =
            entry(
                name = "A",
            ).copy(key = "https://a.example/secret-key/manifest.json", id = "org.a", host = "a.example")
        val b = entry(name = "B").copy(key = "https://b.example/manifest.json", id = "org.b", host = "b.example")
        val copy = b.copy(key = "https://b.example/other/manifest.json")
        val before = FindAddons.stableIds(listOf(a, b))
        val after =
            FindAddons.stableIds(
                listOf(b.copy(key = "https://new.example/manifest.json", id = "org.new"), a, b, copy),
            )
        assertEquals(before[a.key], after[a.key])
        assertEquals(before[b.key], after[b.key])
        assertEquals("org.b@b.example#2", after[copy.key])
        assertTrue(after.values.none { "secret" in it || "manifest" in it })
    }
}
