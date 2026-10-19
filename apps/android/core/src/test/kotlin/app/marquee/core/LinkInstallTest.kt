package app.marquee.core

import app.marquee.core.client.CoreError
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class LinkInstallTest {
    @Test
    fun `an add outlives the page that asked, says how it went and clears the field once`() =
        runTest {
            val link = LinkInstall(backgroundScope)
            val reply = CompletableDeferred<String>()
            var cleared = 0
            link.run({ reply.await() }, added = { cleared++ })
            runCurrent()
            assertTrue(link.busy)
            assertEquals(LinkInstall.Said("Adding the addon", failed = false, pending = true), link.said)
            // A second add while one runs does nothing.
            assertNull(link.run({ "B" }))
            reply.complete("OpenSubtitles v3")
            runCurrent()
            assertEquals(LinkInstall.Said("Added OpenSubtitles v3", failed = false), link.said)
            assertEquals(1, cleared)
        }

    @Test
    fun `a failure keeps the field and says why in the core's words`() =
        runTest {
            val link = LinkInstall(backgroundScope)
            var cleared = 0
            link.run({ throw CoreError("the manifest is not valid JSON") }, added = { cleared++ })
            runCurrent()
            assertEquals(
                LinkInstall.Said("Adding the addon did not work: the manifest is not valid JSON", failed = true),
                link.said,
            )
            assertEquals(0, cleared)
        }

    @Test
    fun `an add from before a profile switch lands nowhere`() =
        runTest {
            val link = LinkInstall(backgroundScope)
            val reply = CompletableDeferred<String>()
            var cleared = 0
            link.run({ reply.await() }, added = { cleared++ })
            runCurrent()
            link.forget()
            reply.complete("Old profile's addon")
            runCurrent()
            assertNull(link.said)
            assertEquals(0, cleared)
        }
}
