package app.marquee.core

import app.marquee.core.client.CoreError
import app.marquee.core.model.AddonInfo
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

    private fun info(name: String) =
        AddonInfo(
            url = "https://a.example/manifest.json",
            host = "a.example",
            name = name,
            version = "1.0.0",
            enabled = false,
            provides = listOf("subtitles"),
            types = listOf("movie"),
            configurable = false,
        )

    @Test
    fun `a pasted link is checked first, and only the sheet's Install adds it, once`() =
        runTest {
            val link = LinkInstall(backgroundScope)
            val reply = CompletableDeferred<AddonInfo>()
            val installs = mutableListOf<Pair<String, Boolean>>()
            var cleared = 0
            link.check("https://a.example/manifest.json") { _, _ -> reply.await() }
            runCurrent()
            assertEquals(LinkInstall.Said(InstallSheetCopy.CHECKING, failed = false, pending = true), link.said)
            assertTrue(link.busy)
            reply.complete(info("OpenSubtitles v3"))
            runCurrent()
            assertNull(link.said)
            assertEquals("OpenSubtitles v3", link.checked?.addon?.name)
            // A second check while the sheet is open does nothing.
            assertNull(link.check("https://b.example/manifest.json") { _, _ -> info("B") })

            // Cancel: nothing added, the field keeps its link.
            link.cancel()
            assertNull(link.checked)
            assertNull(link.confirm({ u, ack -> "never".also { installs += u to ack } }))
            assertEquals(0, cleared)

            link.check("https://a.example/manifest.json") { _, _ -> info("OpenSubtitles v3") }
            runCurrent()
            link.confirm({ u, ack -> "OpenSubtitles v3".also { installs += u to ack } }, added = { cleared++ })
            runCurrent()
            // A second Install on the closed sheet adds nothing more.
            assertNull(link.confirm({ u, ack -> "again".also { installs += u to ack } }))
            assertEquals(listOf("https://a.example/manifest.json" to false), installs)
            assertEquals(LinkInstall.Said("Added OpenSubtitles v3", failed = false), link.said)
            assertEquals(1, cleared)
        }

    @Test
    fun `a link with a sign-in is warned before the sheet, and installs acknowledged after it`() =
        runTest {
            val link = LinkInstall(backgroundScope)
            val previews = mutableListOf<Pair<String, Boolean>>()
            suspend fun preview(
                url: String,
                ack: Boolean,
            ): AddonInfo {
                previews += url to ack
                if (!ack) throw CoreError("this link carries a sign-in", code = LinkInstall.TOKEN_IN_LINK)
                return info("Trakt Tv")
            }
            link.check("https://a.example/token=1/manifest.json", preview = ::preview)
            runCurrent()
            assertEquals(LinkInstall.Said(LinkInstall.TOKEN_WARNING, failed = true, token = true), link.said)
            assertNull(link.checked)
            link.check("https://a.example/token=1/manifest.json", acknowledgeToken = true, preview = ::preview)
            runCurrent()
            assertEquals(listOf(false, true), previews.map { it.second })
            var sent: Boolean? = null
            link.confirm({ _, ack -> "Trakt Tv".also { sent = ack } })
            runCurrent()
            assertEquals(true, sent)
        }

    @Test
    fun `a check for a link the field no longer holds, or from a field that left, opens no sheet`() =
        runTest {
            val link = LinkInstall(backgroundScope)
            val reply = CompletableDeferred<AddonInfo>()
            link.check("https://a.example/manifest.json") { _, _ -> reply.await() }
            runCurrent()
            // The field changed while Checking: that answer lands nowhere.
            link.edited()
            assertNull(link.said)
            assertTrue(!link.busy)
            reply.complete(info("Old"))
            runCurrent()
            assertNull(link.checked)
            assertNull(link.said)

            // The field left its page with the sheet open: the sheet goes, nothing installs.
            link.check("https://b.example/manifest.json") { _, _ -> info("B") }
            runCurrent()
            assertEquals("B", link.checked?.addon?.name)
            link.left()
            assertNull(link.checked)
            assertNull(link.confirm({ _, _ -> "never" }))
        }

    @Test
    fun `a link the core cannot read stays under the field with no sheet`() =
        runTest {
            val link = LinkInstall(backgroundScope)
            link.check("https://a.example/manifest.json") { _, _ -> throw CoreError("the manifest is not valid JSON") }
            runCurrent()
            assertEquals(
                LinkInstall.Said("Adding the addon did not work: the manifest is not valid JSON", failed = true),
                link.said,
            )
            assertNull(link.checked)
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
