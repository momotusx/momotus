package app.marquee.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The install sheet's words and Adds line against the shared file the web and Apple read too. */
class InstallSheetCopyTest {
    private val shared = Json.parseToJsonElement(File("../../shared/install-sheet.json").readText()).jsonObject

    private fun words(key: String) = shared.getValue(key).jsonObject.mapValues { it.value.jsonPrimitive.content }

    private fun strings(
        o: JsonObject,
        key: String,
    ) = o.getValue(key).jsonArray.map { it.jsonPrimitive.content }

    @Test
    fun `the words are the shared ones, every key and no more`() {
        val ours =
            with(InstallSheetCopy) {
                mapOf(
                    "title" to TITLE,
                    "from" to FROM,
                    "adds" to ADDS,
                    "addsFor" to ADDS_FOR,
                    "note" to NOTE,
                    "install" to INSTALL,
                    "cancel" to CANCEL,
                    "checking" to CHECKING,
                )
            }
        assertEquals(words("words"), ours)
        assertEquals(words("resources"), InstallSheetCopy.RESOURCES)
        assertEquals(words("kinds"), InstallSheetCopy.KINDS)
    }

    @Test
    fun `the Adds line follows every shared vector`() {
        val vectors = shared.getValue("vectors").jsonArray.map { it.jsonObject }
        assertTrue(vectors.size >= 13, "${vectors.size} vectors")
        for (v in vectors) {
            val provides = strings(v, "provides")
            val types = strings(v, "types")
            val line = v.getValue("line").jsonPrimitive.content
            assertEquals(line, InstallSheetCopy.adds(provides, types), "$provides $types")
        }
    }

    @Test
    fun `a name goes into the title as plain text`() {
        val vectors = shared.getValue("titleVectors").jsonArray.map { it.jsonObject }
        assertTrue(vectors.isNotEmpty())
        for (v in vectors) {
            val name = v.getValue("name").jsonPrimitive.content
            assertEquals(v.getValue("title").jsonPrimitive.content, InstallSheetCopy.title(name), name)
        }
    }

    @Test
    fun `the lines leave out an empty Adds line`() {
        assertEquals(
            listOf("From a.example", "Adds subtitles.", InstallSheetCopy.NOTE),
            InstallSheetCopy.lines("a.example", listOf("subtitles"), emptyList()),
        )
        assertEquals(
            listOf("From a.example", InstallSheetCopy.NOTE),
            InstallSheetCopy.lines("a.example", listOf("something_new"), listOf("movie")),
        )
        assertEquals("Install Torrentio?", InstallSheetCopy.title("Torrentio"))
    }
}
