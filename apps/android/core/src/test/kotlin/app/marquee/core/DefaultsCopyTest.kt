package app.marquee.core

import app.marquee.core.model.DefaultAddonInfo
import app.marquee.core.model.DefaultAddons
import app.marquee.core.model.DefaultChoice
import app.marquee.core.model.DefaultInstalled
import app.marquee.core.route.DefaultsMode
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class DefaultsCopyTest {
    private fun addon(
        key: String,
        installed: DefaultInstalled,
        group: String = "lists",
    ) = DefaultAddonInfo(key, key.replaceFirstChar { it.uppercase() }, "One line.", group, installed)

    private val list =
        listOf(
            addon("cinemeta", DefaultInstalled.On),
            addon("torrentio", DefaultInstalled.Absent),
            addon("trakt", DefaultInstalled.Off),
            addon("kitsu", DefaultInstalled.Absent),
        )

    @Test
    fun `onboarding names all nine, Review the missing ones, Settings only what was switched`() {
        // First run: every row, Trakt (off before) stays off, Kitsu was switched off.
        assertEquals(
            listOf(
                DefaultChoice("cinemeta", true),
                DefaultChoice("torrentio", true),
                DefaultChoice("trakt", false),
                DefaultChoice("kitsu", false),
            ),
            DefaultsCopy.choices(list, DefaultsMode.FirstRun, mapOf("kitsu" to false)),
        )
        // After an import every key is named too; what Stremio brought has no switch and keeps how it is.
        assertEquals(
            listOf(
                DefaultChoice("cinemeta", true),
                DefaultChoice("torrentio", true),
                DefaultChoice("trakt", false),
                DefaultChoice("kitsu", true),
            ),
            DefaultsCopy.choices(list, DefaultsMode.AfterImport, mapOf("trakt" to true)),
        )
        assertFalse(DefaultsCopy.hasSwitch(list[2], DefaultsMode.AfterImport))
        // Review: the missing ones only.
        assertEquals(
            listOf(DefaultChoice("torrentio", true), DefaultChoice("kitsu", false)),
            DefaultsCopy.choices(list, DefaultsMode.Review, mapOf("kitsu" to false)),
        )
        assertEquals(listOf("torrentio", "kitsu"), DefaultsCopy.rows(list, DefaultsMode.Review).map { it.key })
        // Settings: a missing one starts off; only rows switched away from where they started are sent.
        assertEquals(listOf(true, false, false, false), list.map { DefaultsCopy.initial(it, DefaultsMode.Settings) })
        assertEquals(
            listOf(DefaultChoice("torrentio", true)),
            DefaultsCopy.choices(list, DefaultsMode.Settings, mapOf("torrentio" to true, "trakt" to false)),
        )
        assertEquals(emptyList(), DefaultsCopy.choices(list, DefaultsMode.Settings, emptyMap()))
        assertEquals(listOf(true, true, false, true), list.map { DefaultsCopy.initial(it, DefaultsMode.FirstRun) })
    }

    @Test
    fun `the catalog note shows only when no catalog default stays on`() {
        val catalogs =
            listOf(
                addon("cinemeta", DefaultInstalled.On, "catalogs"),
                addon("kitsu", DefaultInstalled.Absent, "catalogs"),
                addon("trakt", DefaultInstalled.On),
            )
        // Cinemeta off while Anime Kitsu goes in on: Home still has a catalog.
        assertFalse(DefaultsCopy.catalogMissing(catalogs, DefaultsMode.FirstRun, mapOf("cinemeta" to false)))
        assertTrue(
            DefaultsCopy.catalogMissing(catalogs, DefaultsMode.FirstRun, mapOf("cinemeta" to false, "kitsu" to false)),
        )
        // Settings starts the missing Kitsu off: Cinemeta off leaves nothing until Kitsu is switched on.
        assertTrue(DefaultsCopy.catalogMissing(catalogs, DefaultsMode.Settings, mapOf("cinemeta" to false)))
        assertFalse(
            DefaultsCopy.catalogMissing(catalogs, DefaultsMode.Settings, mapOf("cinemeta" to false, "kitsu" to true)),
        )
        // A list addon is not a catalog for this note.
        assertTrue(
            DefaultsCopy.catalogMissing(
                catalogs.take(2),
                DefaultsMode.FirstRun,
                mapOf(
                    "cinemeta" to false,
                    "kitsu" to false,
                ),
            ),
        )
    }

    @Test
    fun `the card shows once per version, never for kids, and only with something missing`() {
        val answer = DefaultAddons(version = 2, addons = list)
        assertTrue(DefaultsCopy.offer(answer, seen = 1, kids = false))
        assertFalse(DefaultsCopy.offer(answer, seen = 2, kids = false))
        assertFalse(DefaultsCopy.offer(answer, seen = 1, kids = true))
        val none = answer.copy(addons = list.map { it.copy(installed = DefaultInstalled.Off) })
        assertFalse(DefaultsCopy.offer(none, seen = 0, kids = false))
    }

    @Test
    fun `words for groups and the card`() {
        assertEquals("Catalogs and details", DefaultsCopy.group("catalogs"))
        assertEquals("Marquee now also recommends Torrentio.", DefaultsCopy.cardLine(listOf("Torrentio")))
        assertEquals(
            "Marquee now also recommends Torrentio, SubSense and Trakt.",
            DefaultsCopy.cardLine(listOf("Torrentio", "SubSense", "Trakt")),
        )
    }
}
