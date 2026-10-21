package app.marquee.core

import app.marquee.core.ScriptedCore.Companion.fail
import app.marquee.core.ScriptedCore.Companion.ok
import kotlinx.serialization.json.jsonObject
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** The first run's subtitles step: the order it lists, the device's language chosen, and what Continue writes. */
class SubtitleStepTest {
    private val core = ScriptedCore()

    @AfterTest
    fun close() = core.close()

    private val languages =
        """[{"code":"de","name":"German"},{"code":"en","name":"English"},{"code":"pl","name":"Polish"},""" +
            """{"code":"pt-BR","name":"Portuguese (Brazil)"},{"code":"tr","name":"Turkish"}]"""
    private val zero = """{"ready":0,"height":0,"hdr":0,"size":0,"picks":0,"epoch":0,"atMs":0}"""
    private val prefs =
        """{"streams":{"quality":"balanced","minSeeders":50,"hdr":true,"preferReady":true,"learn":true},""" +
            """"autoplayNext":true,"homeRows":{},"learned":{"uhd":$zero,"fhd":$zero,"hd":$zero,"sd":$zero},"learnedEpoch":0}"""

    private fun step(device: String?): SubtitleStep {
        core.answers["languages"] = { ok(languages) }
        return SubtitleStep(core.ops, core.scope, device).also {
            it.load()
            core.waitFor("the languages") { it.all.isNotEmpty() }
        }
    }

    private fun written() =
        core
            .last("setPrefs")
            .getValue("patch")
            .jsonObject
            .getValue("subtitleLanguages")
            .toString()

    @Test
    fun the_devices_language_is_first_and_chosen() {
        val step = step("tr")
        assertEquals("tr", step.chosen)
        assertEquals(listOf("tr", "en", "es", "fr", "de", "it", "pt", "ja"), step.common.map { it.code })
        assertEquals("Turkish", step.common.first().name)
        // The rest, by name, none of them twice.
        assertEquals(listOf("pl", "pt-BR"), step.rest.map { it.code })
    }

    @Test
    fun continue_writes_the_choice_and_no_subtitles_writes_none() {
        core.answers["setPrefs"] = { ok(prefs) }
        val step = step("de")
        var done = 0
        step.save { done++ }
        core.waitFor("the write") { done == 1 }
        assertEquals("""["de"]""", written())
        step.chosen = null
        step.save { done++ }
        core.waitFor("the second write") { done == 2 }
        assertEquals("[]", written())
    }

    @Test
    fun a_refused_write_stays_on_the_step_and_says_why() {
        core.answers["setPrefs"] = { fail(null, "the profile is locked") }
        val step = step(null)
        assertNull(step.chosen)
        var done = false
        step.save { done = true }
        core.waitFor("the answer") { step.problem != null }
        assertEquals("the profile is locked", step.problem)
        assertEquals(false, done)
    }
}
