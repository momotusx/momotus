package app.marquee.core

import app.marquee.core.client.CoreError
import app.marquee.core.model.Configuration
import app.marquee.core.model.Installed
import app.marquee.core.model.ListedAddon
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

@OptIn(ExperimentalCoroutinesApi::class)
class InstallCardsTest {
    private val a = "https://a.example/manifest.json"
    private val b = "https://b.example/manifest.json"

    private fun TestScope.cards() = InstallCards(backgroundScope)

    private fun listed(
        key: String,
        installed: Installed,
    ) = ListedAddon(
        key = key,
        id = key,
        host = "a.example",
        name = "A",
        configuration = Configuration.None,
        configureUrl = key.replace("manifest.json", "configure"),
        installed = installed,
    )

    @Test
    fun `a card is busy while its request runs, and a second press does nothing`() =
        runTest {
            val cards = cards()
            val reply = CompletableDeferred<Unit>()
            var calls = 0
            cards.run(a) {
                calls++
                reply.await()
            }
            runCurrent()
            assertEquals(InstallCards.State(busy = true), cards[a])
            assertNull(cards.run(a) { calls++ })
            runCurrent()
            assertEquals(1, calls)
            reply.complete(Unit)
            runCurrent()
            assertEquals(InstallCards.State(busy = false), cards[a])
        }

    @Test
    fun `a failure keeps the core's code and words on its own card only`() =
        runTest {
            val cards = cards()
            cards.run(a) { throw CoreError("This addon now needs setup first", code = "needsSetup") }
            cards.run(b) { }
            runCurrent()
            assertEquals(InstallCards.State(false, "needsSetup", "This addon now needs setup first"), cards[a])
            assertEquals(InstallCards.State(busy = false), cards[b])
        }

    @Test
    fun `a late result after a list switch or a reopened page lands on its card`() =
        runTest {
            // The store outlives the page: nothing here belongs to a screen.
            val cards = cards()
            val reply = CompletableDeferred<Unit>()
            cards.run(a) {
                reply.await()
                throw CoreError("This addon's server did not answer in time", code = "timeout")
            }
            runCurrent()
            // Refresh or Try again forgets finished results, never a running one.
            cards.forgetAll(running = false)
            assertEquals(true, cards[a]?.busy)
            reply.complete(Unit)
            runCurrent()
            assertEquals("timeout", cards[a]?.code)
            assertNull(cards[b])
        }

    @Test
    fun `results from before a profile switch land nowhere`() =
        runTest {
            val cards = cards()
            val reply = CompletableDeferred<Unit>()
            cards.run(a) { reply.await() }
            runCurrent()
            cards.forgetAll()
            assertNull(cards[a])
            reply.complete(Unit)
            runCurrent()
            assertNull(cards[a])
            // The new profile's own presses work.
            cards.run(a) { throw CoreError("This addon is no longer in the list", code = "notListed") }
            runCurrent()
            assertEquals("notListed", cards[a]?.code)
        }

    @Test
    fun `forgetting a card while its request runs keeps it running`() =
        runTest {
            val cards = cards()
            val reply = CompletableDeferred<Unit>()
            cards.run(a) { reply.await() }
            runCurrent()
            cards.forget(a)
            assertEquals(InstallCards.State(busy = true), cards[a])
            reply.complete(Unit)
            runCurrent()
            assertEquals(InstallCards.State(busy = false), cards[a])
        }

    @Test
    fun `a failure reads in the core's words, never as raw exception text`() =
        runTest {
            assertEquals(
                "This addon is not the one the list named",
                InstallCards.wordsOf(CoreError("This addon is not the one the list named", code = "mismatch")),
            )
            assertEquals(
                "The core is not responding.",
                InstallCards.wordsOf(CoreError("installListed did not answer in 1m", timedOut = true)),
            )
            assertEquals("IllegalStateException: boom", InstallCards.wordsOf(IllegalStateException("boom")))
            val cards = cards()
            cards.run(a) { throw java.io.IOException("socket closed") }
            runCurrent()
            assertEquals("IOException: socket closed", cards[a]?.words)
        }

    @Test
    fun `a changed installed state forgets that card's result`() =
        runTest {
            val cards = cards()
            cards.follow(listOf(listed(a, Installed.Absent), listed(b, Installed.Absent)))
            cards.run(a) { throw CoreError("x", code = "needsSetup") }
            cards.run(b) { throw CoreError("y", code = "timeout") }
            runCurrent()
            cards.follow(listOf(listed(a, Installed.CopyOn), listed(b, Installed.Absent)))
            assertNull(cards[a])
            assertEquals("timeout", cards[b]?.code)
        }
}
