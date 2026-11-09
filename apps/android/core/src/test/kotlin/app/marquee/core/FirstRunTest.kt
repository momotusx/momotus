package app.marquee.core

import app.marquee.core.FirstRun.After
import app.marquee.core.FirstRun.Step
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** First run's order and setup chips against the shared vectors (plans/first-screens.md 4.3), all three parts. */
class FirstRunTest {
    private val vectors =
        Json.parseToJsonElement(File("../../shared/fixtures/first-run-vectors.json").readText()).jsonObject

    private fun JsonObject.flag(key: String) = getValue(key).jsonPrimitive.boolean

    private fun after(name: String) = After.entries.single { it.name.equals(name, ignoreCase = true) }

    private fun step(name: String) = Step.entries.single { it.name.equals(name, ignoreCase = true) }

    @Test
    fun `every entry row gives its step`() {
        val rows = vectors.getValue("entry").jsonArray
        assertTrue(rows.size >= 20)
        for (row in rows.map { it.jsonObject }) {
            val state =
                FirstRun.State(
                    fresh = row.flag("fresh"),
                    kidsSession = row.flag("kidsSession"),
                    server = row.flag("server"),
                    inAccount = row.flag("inAccount"),
                    profileStepDone = row.flag("profileStepDone"),
                    profiles = row.getValue("profiles").jsonPrimitive.int,
                    pin = row.flag("pin"),
                    locked = row.flag("locked"),
                )
            val after = after(row.getValue("after").jsonPrimitive.content)
            assertEquals(step(row.getValue("step").jsonPrimitive.content), FirstRun.next(state, after), "$row")
        }
    }

    /**
     * One path driven through the rule by its presses: each press is the event it stands for, the other device's join
     * comes after the last press of a join path, and every step the rule opens is a screen until Home.
     */
    private fun walk(
        path: String,
        presses: List<String>,
    ): List<String> {
        var s =
            FirstRun.State(
                fresh = true,
                kidsSession = false,
                server = path != "noServer",
                inAccount = false,
                profileStepDone = false,
                profiles = 1,
                pin = false,
                locked = false,
            )
        val screens = mutableListOf<String>()
        var bring = false

        fun open(step: Step) {
            if (step != Step.Home && step != Step.None) screens += step.name.lowercase()
        }
        open(FirstRun.next(s, After.Start))
        for (press in presses) {
            when (press) {
                "getStarted" -> {
                    open(FirstRun.next(s, After.GetStarted))
                }

                "alreadyUse" -> {
                    open(FirstRun.next(s, After.AlreadyUse))
                }

                "notNow" -> {
                    open(FirstRun.next(s, After.NotNow))
                }

                "continueWithGoogle" -> {
                    s = s.copy(inAccount = true)
                    open(FirstRun.next(s, After.Created))
                }

                "continue" -> {
                    s = s.copy(profileStepDone = true)
                    open(FirstRun.next(s, After.ProfileDone))
                }

                "bringLibrary" -> {
                    bring = true
                }

                "startWatching" -> {
                    if (bring) screens += "import"
                }

                // The pairing code shows in the sheet; a profile picked on Who's watching? opens Home.
                "useAnotherDevice", "profileTile" -> {
                    Unit
                }

                else -> {
                    error("unknown press $press")
                }
            }
        }
        // A join path: the other device approves after this device's presses.
        if (path.startsWith("join")) {
            val many = path == "joinManyProfilesOrPin"
            val joined = s.copy(fresh = false, inAccount = true, profiles = if (many) 3 else 1)
            val next = FirstRun.next(joined, After.Joined)
            open(next)
            // Who's watching? comes before the press that answers it.
            if (next == Step.Who) assertEquals("profileTile", presses.last())
        }
        return screens
    }

    @Test
    fun `every path shows its screens for its presses on every form`() {
        val paths = vectors.getValue("paths").jsonArray.map { it.jsonObject }
        assertTrue(paths.size >= 6)
        for (path in paths) {
            val name = path.getValue("path").jsonPrimitive.content
            // The web's own row (its recovery kit) is the web's; the other forms are this app's phone and TV.
            for (form in listOf("phone", "wide", "tv")) {
                val row = path[form]?.jsonObject ?: continue
                val screens = row.getValue("screens").jsonArray.map { it.jsonPrimitive.content }
                val presses = row.getValue("presses").jsonArray.map { it.jsonPrimitive.content }
                assertEquals(screens, walk(name, presses), "$name on $form")
                // Four presses or fewer on every new path (9), plus the code on the Stremio path.
                assertTrue(presses.size <= 5, "$name on $form")
            }
        }
    }

    @Test
    fun `every chip row gives its chips in order and the one chosen`() {
        for (row in vectors.getValue("chips").jsonArray.map { it.jsonObject }) {
            val device =
                row
                    .getValue("device")
                    .takeUnless { it is JsonNull }
                    ?.jsonPrimitive
                    ?.content
            val chosen = row.getValue("chosen").jsonArray.map { it.jsonPrimitive.content }
            val chips = FirstRun.chips(device, chosen)
            assertEquals(row.getValue("chips").jsonArray.map { it.jsonPrimitive.content }, chips.chips, "$row")
            assertEquals(row.getValue("selected").jsonPrimitive.content, chips.selected, "$row")
        }
    }
}
