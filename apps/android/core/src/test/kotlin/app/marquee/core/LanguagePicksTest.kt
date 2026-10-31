package app.marquee.core

import app.marquee.core.model.LanguageChoice
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** The language pickers over the core's full list. */
class LanguagePicksTest {
    private val all =
        listOf(
            LanguageChoice("en", "English"),
            LanguageChoice("es", "Spanish"),
            LanguageChoice("pl", "Polish"),
            LanguageChoice("pt", "Portuguese"),
            LanguageChoice("pt-BR", "Portuguese (Brazil)"),
            LanguageChoice("ko", "Korean"),
            LanguageChoice("zh-Hans", "Chinese (Simplified)"),
            LanguageChoice("zh-Hant", "Chinese (Traditional)"),
            LanguageChoice("ar", "Arabic"),
        )

    private fun names(list: List<LanguageChoice>) = list.map { it.name }

    @Test
    fun `typing finds any language, the names that start with it first`() {
        assertEquals(listOf("Polish"), names(LanguagePicks.search(all, "pol", emptyList())))
        assertEquals(listOf("Portuguese", "Portuguese (Brazil)"), names(LanguagePicks.search(all, "Port", emptyList())))
        // Starts before contains: "an" starts nothing here but is inside Spanish and Korean is not.
        assertEquals(listOf("Spanish"), names(LanguagePicks.search(all, "anish", emptyList())))
        assertEquals(listOf("Korean"), names(LanguagePicks.search(all, "ko", emptyList())))
    }

    @Test
    fun `a chosen language is not offered again, also when chosen under an old code`() {
        assertTrue(names(LanguagePicks.search(all, "pol", listOf("pl"))).isEmpty())
        // An "eng" from before the full list keeps English off.
        assertTrue("English" !in names(LanguagePicks.search(all, "en", listOf("eng"))))
        assertEquals("English", LanguagePicks.name("eng", all))
    }

    @Test
    fun `before anything is typed the common ones are offered`() {
        val offered = LanguagePicks.search(all, "", listOf("en")).map { it.code }
        assertEquals(LanguagePicks.COMMON - "en", offered)
    }

    @Test
    fun `the device's own language is offered first`() {
        assertEquals("pl", LanguagePicks.search(all, "", emptyList(), device = "pl").first().code)
        // Chosen already: not offered again.
        assertEquals("en", LanguagePicks.search(all, "", listOf("pl"), device = "pl").first().code)
    }

    @Test
    fun `audio takes the base codes, one per language`() {
        val audio = LanguagePicks.audio(all)
        assertEquals(listOf("en", "es", "pl", "pt", "ko", "zh", "ar"), audio.map { it.code })
        assertEquals("Chinese", audio.first { it.code == "zh" }.name)
    }

    @Test
    fun `the Second line cannot take the first subtitle language, in any spelling`() {
        assertTrue(LanguagePicks.onFirstLine("en", "eng"))
        assertTrue(LanguagePicks.onFirstLine("en", "en"))
        assertTrue(LanguagePicks.onFirstLine("pt", "pt-BR"))
        assertTrue(LanguagePicks.onFirstLine("pt-BR", "por"))
        assertFalse(LanguagePicks.onFirstLine("tr", "eng"))
        assertFalse(LanguagePicks.onFirstLine("en", null))
    }
}
