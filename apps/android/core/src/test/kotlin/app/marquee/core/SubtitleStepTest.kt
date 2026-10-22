package app.marquee.core

import app.marquee.core.ScriptedCore.Companion.fail
import app.marquee.core.ScriptedCore.Companion.ok
import kotlinx.coroutines.CompletableDeferred
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** The first run's subtitles step: the order it lists, the device's language chosen, and what Continue writes. */
class SubtitleStepTest {
    private val core = ScriptedCore()

    @AfterTest
    fun close() = core.close()

    private val languages =
        listOf(
            "de" to "German",
            "en" to "English",
            "no" to "Norwegian",
            "pl" to "Polish",
            "pt-BR" to "Portuguese (Brazil)",
            "tr" to "Turkish",
        )

    /** The list as the core answers it: the language the device's code reads as marked ("nb" is Norwegian). */
    private fun answer(device: String?): String {
        val mine = if (device == "nb") "no" else device
        return languages.joinToString(",", "[", "]") { (code, name) ->
            """{"code":"$code","name":"$name","device":${code == mine}}"""
        }
    }

    private val zero = """{"ready":0,"height":0,"hdr":0,"size":0,"picks":0,"epoch":0,"atMs":0}"""
    private val prefs =
        """{"streams":{"quality":"balanced","minSeeders":50,"hdr":true,"preferReady":true,"learn":true},""" +
            """"autoplayNext":true,"homeRows":{},"learned":{"uhd":$zero,"fhd":$zero,"hd":$zero,"sd":$zero},"learnedEpoch":0}"""

    private fun step(device: String?): SubtitleStep {
        core.answers["languages"] = { request -> ok(answer(request["device"]?.jsonPrimitive?.content)) }
        return SubtitleStep(core.ops, core.scope, device).also {
            it.load()
            core.waitFor("the languages") { it.loaded }
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
        assertEquals(listOf("no", "pl", "pt-BR"), step.rest.map { it.code })
    }

    @Test
    fun an_english_device_leads_with_english_and_chooses_no_subtitles() {
        val step = step("en")
        assertNull(step.chosen)
        assertEquals(listOf("en", "tr", "es", "fr", "de", "it", "pt", "ja"), step.common.map { it.code })
    }

    @Test
    fun a_device_language_is_the_one_the_core_lists() {
        core.answers["setPrefs"] = { ok(prefs) }
        // Norway's Bokmål: the core lists Norwegian as "no" and marks it.
        val step = step("nb")
        assertEquals("no", step.chosen)
        assertEquals("Norwegian", step.common.first().name)
        var done = false
        step.save { done = true }
        core.waitFor("the write") { done }
        assertEquals("""["no"]""", written())
        // A code the core does not list is neither offered nor chosen.
        val unknown = step("haw")
        assertNull(unknown.chosen)
        assertEquals("en", unknown.common.first().code)
    }

    @Test
    fun turkish_comes_after_the_device_and_english() {
        assertEquals(listOf("de", "en", "tr"), step("de").common.map { it.code }.take(3))
    }

    @Test
    fun continue_writes_the_choice_and_no_subtitles_writes_none() {
        core.answers["setPrefs"] = { ok(prefs) }
        val step = step("de")
        var done = 0
        step.save { done++ }
        core.waitFor("the write") { done == 1 }
        assertEquals("""["de"]""", written())
        step.choose(null)
        step.save { done++ }
        core.waitFor("the second write") { done == 2 }
        assertEquals("[]", written())
    }

    @Test
    fun continue_waits_for_the_list_and_a_choice_made_before_it_stays() {
        core.answers["setPrefs"] = { ok(prefs) }
        val list = CompletableDeferred<Unit>()
        core.answers["languages"] = { request ->
            list.await()
            ok(answer(request["device"]?.jsonPrimitive?.content))
        }
        val step = SubtitleStep(core.ops, core.scope, "tr").also { it.load() }
        // No subtitles, chosen before the list: Continue does nothing yet.
        step.choose(null)
        var done = false
        step.save { done = true }
        assertEquals(false, step.loaded)
        list.complete(Unit)
        core.waitFor("the languages") { step.loaded }
        // The choice stays over the device's Turkish, and nothing was written before.
        assertNull(step.chosen)
        assertEquals(false, done)
        assertTrue(core.asked.none { it.getValue("op").jsonPrimitive.content == "setPrefs" })
        step.save { done = true }
        core.waitFor("the write") { done }
        assertEquals("[]", written())
    }

    @Test
    fun a_list_that_fails_says_why_and_continue_still_goes_on() {
        core.answers["setPrefs"] = { ok(prefs) }
        core.answers["languages"] = { fail(null, "the core is busy") }
        val step = SubtitleStep(core.ops, core.scope, "tr").also { it.load() }
        core.waitFor("the failure") { step.loaded }
        assertEquals("the core is busy", step.listProblem)
        assertNull(step.chosen)
        var done = false
        step.save { done = true }
        core.waitFor("the write") { done }
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
