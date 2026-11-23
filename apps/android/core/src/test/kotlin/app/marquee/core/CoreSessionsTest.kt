package app.marquee.core

import app.marquee.core.session.CoreSessions
import org.junit.Test
import org.junit.runner.RunWith
import org.junit.runners.model.Statement
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import java.io.File
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * A test's sessions end with the test: each one's state thread is gone after the rule, so the app's tests keep one
 * JVM's thread count flat instead of leaving a looping thread per test (107 at the end of the app's tests on CI).
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [36])
class CoreSessionsTest {
    private fun stateThreads() = Thread.getAllStackTraces().keys.count { it.name == "marquee-state" && it.isAlive }

    @Test
    fun `the sessions a test opens leave no state thread behind`() {
        val before = stateThreads()
        val rule = CoreSessions()
        rule
            .apply(
                object : Statement() {
                    override fun evaluate() {
                        repeat(3) { rule.open(RuntimeEnvironment.getApplication()) }
                        assertEquals(before + 3, stateThreads(), "each session runs its own state thread")
                    }
                },
                null,
            ).evaluate()
        assertEquals(before, stateThreads(), "a state thread outlived its test")
    }

    @Test
    fun `no test builds a session but through the rule`() {
        val files =
            listOf("app", "core", "player", "ui-phone", "ui-tv")
                .flatMap { m -> File("..", "$m/src/test").walkTopDown().filter { it.extension == "kt" }.toList() }
        assertTrue(files.size > 100, "found ${files.size} tests")
        val built = Regex("""(?<![.\w])CoreSession\(""")
        val hits =
            files.flatMap { f ->
                f.readLines().filter { built.containsMatchIn(it) }.map { "${f.path}: ${it.trim()}" }
            }
        assertEquals(emptyList(), hits)
    }
}
