package app.marquee.core

import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * Android compiles patterns with ICU, which refuses a `}` the JVM takes as a literal: `\{(\w+)}` passed every unit
 * test here and crashed the email code screen on the phone. So every pattern literal in the app's sources escapes
 * each `}` that does not close a `{n}` or `{n,m}` count, and [AccountRules.fill] needs no pattern at all.
 */
class RegexPortabilityTest {
    @Test
    fun `fill replaces named fields and leaves the rest as written`() {
        val f = { text: String -> AccountRules.fill(text, mapOf("email" to "a@b.org", "seconds" to 42)) }
        assertEquals("We sent a code to a@b.org.", f("We sent a code to {email}."))
        assertEquals("in 42 seconds, a@b.org", f("in {seconds} seconds, {email}"))
        assertEquals("{missing} and {} and {a b} stay", f("{missing} and {} and {a b} stay"))
        assertEquals("{x a@b.org} }{ {", f("{x {email}} }{ {"))
        assertEquals("no fields", f("no fields"))
    }

    @Test
    fun `every pattern literal in the app escapes its closing braces`() {
        val sources = File("..").walkTopDown()
            .filter { it.isFile && it.extension == "kt" && "/src/main/" in it.path && "/build/" !in it.path }
            .toList()
        assertTrue(sources.size > 50, "the app's sources were not found from ${File("..").absolutePath}")
        val literal = Regex("""(?:Regex|toRegex|Pattern\.compile)\(\s*("{3}(.*?)"{3}|"((?:[^"\\]|\\.)*)")""")
        val bad = sources.flatMap { file ->
            literal.findAll(file.readText()).mapNotNull { m ->
                // A raw string as written; a plain one with Kotlin's escapes undone.
                val pattern = m.groups[2]?.value ?: m.groupValues[3].replace("\\\\", "\\")
                if (strayBrace(pattern)) "${file.path}: $pattern" else null
            }.toList()
        }
        assertEquals(emptyList(), bad, "patterns with an unescaped } (ICU refuses them)")
    }

    /**
     * A `}` that is neither escaped, nor the end of a `{n}`, `{n,}` or `{n,m}` count, nor the end of an escape's
     * own braces (`\p{M}`, `\x{2028}`).
     */
    private fun strayBrace(pattern: String): Boolean {
        var i = 0
        var count = false
        while (i < pattern.length) {
            when (val c = pattern[i]) {
                '\\' -> {
                    i++
                    if (pattern.getOrNull(i) in listOf('p', 'P', 'x', 'N') && pattern.getOrNull(i + 1) == '{') {
                        i = pattern.indexOf('}', i).takeIf { it > 0 } ?: return true
                    }
                }
                '{' -> count = Regex("""\{\d+(,\d*)?\}""").find(pattern, i)?.range?.first == i
                '}' -> if (!count) return true else count = false
                else -> if (count && !c.isDigit() && c != ',') count = false
            }
            i++
        }
        return false
    }
}
