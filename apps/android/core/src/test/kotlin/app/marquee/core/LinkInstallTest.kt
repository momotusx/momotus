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
    fun `a link with a sign-in warns first and goes in only when sent again acknowledged`() =
        runTest {
            val link = LinkInstall(backgroundScope)
            val sent = mutableListOf<Pair<String, Boolean>>()
            var cleared = 0

            // The field's link at the moment of each press: what the page hands to run.
            fun press(
                url: String,
                acknowledged: Boolean,
            ) = link.run({ ack ->
                sent += url to ack
                if (!ack) throw CoreError("this link carries a sign-in", code = LinkInstall.TOKEN_IN_LINK)
                "Trakt Tv"
            }, added = { cleared++ }, acknowledgeToken = acknowledged)
            press("https://a.example/token=1/manifest.json", acknowledged = false)
            runCurrent()
            assertEquals(LinkInstall.Said(LinkInstall.TOKEN_WARNING, failed = true, token = true), link.said)
            assertEquals(0, cleared)
            press("https://a.example/token=1/manifest.json", acknowledged = true)
            runCurrent()
            assertEquals(
                listOf(
                    "https://a.example/token=1/manifest.json" to false,
                    "https://a.example/token=1/manifest.json" to true,
                ),
                sent,
            )
            assertEquals(LinkInstall.Said("Added Trakt Tv", failed = false), link.said)
            assertEquals(1, cleared)
        }

    @Test
    fun `an edit drops the sign-in warning, so a confirm never adds a link nobody was warned about`() =
        runTest {
            val link = LinkInstall(backgroundScope)
            link.run({ throw CoreError("this link carries a sign-in", code = LinkInstall.TOKEN_IN_LINK) })
            runCurrent()
            assertTrue(link.said?.token == true)
            link.edited()
            assertNull(link.said)
            // Any other line stays through an edit: it says how the last add went.
            link.run({ throw CoreError("the manifest is not valid JSON") })
            runCurrent()
            link.edited()
            assertEquals(
                LinkInstall.Said("Adding the addon did not work: the manifest is not valid JSON", failed = true),
                link.said,
            )
            // A profile switch drops a warning too.
            link.run({ throw CoreError("this link carries a sign-in", code = LinkInstall.TOKEN_IN_LINK) })
            runCurrent()
            link.forget()
            assertNull(link.said)
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
