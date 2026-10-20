package app.marquee.core

import app.marquee.core.model.Tile
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** Remove from Continue Watching with five seconds for Undo (journey review r2 P3-7). */
@OptIn(ExperimentalCoroutinesApi::class)
class PendingRemovalTest {
    private val scope = TestScope(StandardTestDispatcher())
    private val sent = mutableListOf<String>()
    private var failNext = false
    private val removal =
        PendingRemoval(scope) {
            if (failNext) error("timed out")
            sent += it.id
        }

    private fun tile(id: String) = Tile(id = id, kind = "series", name = "Title $id", posterShape = "poster")

    @Test
    fun `nothing goes to the core until the five seconds pass, and Home leaves the title out meanwhile`() {
        removal.remove(tile("a"))
        assertEquals("a", removal.removing?.id)
        scope.advanceTimeBy(4_900)
        scope.runCurrent()
        assertEquals(emptyList(), sent)
        scope.advanceTimeBy(200)
        scope.runCurrent()
        assertEquals(listOf("a"), sent)
        assertNull(removal.removing)
    }

    @Test
    fun `Undo takes it back and nothing is sent`() {
        removal.remove(tile("a"))
        removal.undo()
        scope.advanceTimeBy(10_000)
        scope.runCurrent()
        assertEquals(emptyList(), sent)
        assertNull(removal.removing)
    }

    @Test
    fun `a second removal sends the first at once`() {
        removal.remove(tile("a"))
        removal.remove(tile("b"))
        scope.runCurrent()
        assertEquals(listOf("a"), sent)
        assertEquals("b", removal.removing?.id)
    }

    @Test
    fun `a failure is said and the title comes back`() {
        failNext = true
        removal.remove(tile("a"))
        scope.advanceTimeBy(5_100)
        scope.runCurrent()
        assertNull(removal.removing)
        val said = removal.failed.orEmpty()
        assertTrue(said.startsWith("Title a could not be removed from Continue Watching: "), said)
        assertTrue("timed out" in said, said)
    }

    @Test
    fun `a new profile drops a removal still waiting`() {
        removal.remove(tile("a"))
        removal.forget()
        scope.advanceTimeBy(10_000)
        scope.runCurrent()
        assertEquals(emptyList(), sent)
    }
}
