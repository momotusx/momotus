package app.marquee.core

import app.marquee.core.model.ListedRepo
import app.marquee.core.model.PluginInfo
import app.marquee.core.model.PluginRepoInfo
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals

/** The plugin words against the shared file the web and Apple read too, and their lines. */
class PluginCopyTest {
    private val shared =
        Json
            .parseToJsonElement(File("../../shared/plugin-words.json").readText())
            .jsonObject
            .mapValues { it.value.jsonPrimitive.content }

    @Test
    fun `the words are the shared ones, every key and no more`() {
        assertEquals(shared, PluginCopy.ALL)
    }

    @Test
    fun `a listed repository says its providers, kinds and languages, and is found by any of them`() {
        val r =
            ListedRepo(
                url = "u",
                host = "raw.githubusercontent.com",
                name = "Türk Plugins",
                description = "",
                providers = listOf("Dizibox", "HDFilmCehennemi"),
                languages = listOf("tr"),
                types = listOf("movie", "series"),
                installed = false,
            )
        assertEquals("2 providers for films and series. Languages: tr.", PluginCopy.directoryLine(r))
        assertEquals(
            "1 provider.",
            PluginCopy.directoryLine(r.copy(providers = listOf("One"), types = emptyList(), languages = emptyList())),
        )
        assertEquals(true, PluginCopy.matches(r, "turk"))
        assertEquals(true, PluginCopy.matches(r, "dizibox TR"))
        assertEquals(false, PluginCopy.matches(r, "anime"))
        assertEquals(true, PluginCopy.matches(r, "  "))
    }

    @Test
    fun `the sheet names five providers and counts the rest`() {
        assertEquals("Adds 1 provider: One.", PluginCopy.adds(listOf("One")))
        assertEquals("Adds 2 providers: A and B.", PluginCopy.adds(listOf("A", "B")))
        assertEquals("Adds 8 providers: A, B, C, D, E and 3 more.", PluginCopy.adds("ABCDEFGH".map { it.toString() }))
    }

    @Test
    fun `the Settings line counts providers on in repositories that are on`() {
        fun repo(
            enabled: Boolean,
            on: List<Boolean>,
        ) = PluginRepoInfo(
            url = "u",
            host = "h",
            name = null,
            description = "",
            enabled = enabled,
            defaultKey = null,
            problem = null,
            providers = on.map { PluginInfo("i", "n", "", "", emptyList(), emptyList(), it) },
        )
        assertEquals(PluginCopy.SUMMARY_NONE, PluginCopy.summary(emptyList()))
        assertEquals(
            "2 repositories, 2 providers on",
            PluginCopy.summary(listOf(repo(true, listOf(true, false, true)), repo(false, listOf(true)))),
        )
        assertEquals("1 repository, 1 provider on", PluginCopy.summary(listOf(repo(true, listOf(true)))))
    }
}
