package app.marquee.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * No free space or storage warning anywhere a viewer reads (the owner, plans/no-space-warnings.md): the string
 * literals of every module's main sources, the same list as the web's and Apple's banned-word tests.
 */
class NoSpaceWordsTest {
    private val space =
        listOf(
            "free space",
            "{free}",
            "nearly full",
            "device is full",
            "storage full",
            "storage is full",
            "no room",
            "for this storage",
            "not enough space",
            "not enough room",
            "not enough storage",
            "out of space",
            "low on storage",
            "free some storage",
        )

    private val literal = Regex("\"((?:[^\"\\\\]|\\\\.)*)\"")

    /** The string literals of [source], comment lines left out. */
    private fun literals(source: String): List<String> =
        source
            .lines()
            .map { it.trim() }
            .filterNot { it.startsWith("//") || it.startsWith("*") || it.startsWith("/*") }
            .flatMap { line -> literal.findAll(line).map { it.groupValues[1] } }

    @Test
    fun `no main source says a space word`() {
        val root = File("..")
        val files =
            listOf("app", "core", "design", "player", "ui-phone", "ui-tv")
                .flatMap { m -> File(root, "$m/src/main").walkTopDown().filter { it.extension == "kt" }.toList() }
        assertTrue("found ${files.size} sources", files.size > 50)
        val hits =
            files.flatMap { f ->
                literals(f.readText()).filter { t -> space.any { t.lowercase().contains(it) } }.map { "${f.path}: $it" }
            }
        assertEquals(emptyList<String>(), hits)
    }

    @Test
    fun `the scanner finds a literal and leaves a comment alone`() {
        assertEquals(listOf("free space"), literals("val a = \"free space\"\n// \"no room\"\n * \"nearly full\""))
    }
}
