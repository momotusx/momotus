package app.marquee.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonObject
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** The update feed against the shared vectors (plans/auto-update.md), and the updater's gates. */
class UpdateFeedTest {
    private val vectors =
        Json.parseToJsonElement(File("../../shared/fixtures/update-vectors.json").readText()).jsonObject
    private val key = UpdateFeed.publicKey(vectors.getValue("publicKey").jsonPrimitive.content)
    private val feeds = vectors.getValue("feeds").jsonObject

    private fun feed(name: String) = feeds.getValue(name).jsonPrimitive.content

    @Test
    fun `only envelopes signed by the key verify`() {
        val expected = vectors.getValue("verifies").jsonObject
        assertEquals(expected.keys, feeds.keys)
        for ((name, ok) in expected) {
            assertEquals(ok.jsonPrimitive.boolean, UpdateFeed.verified(feed(name), key) != null, name)
        }
    }

    @Test
    fun `the pinned key is the shared public key file`() {
        assertEquals(File("../../shared/update-feed-key.pub.pem").readText(), UpdateFeed.KEY)
    }

    @Test
    fun `the release key is a P-256 key and refuses the test feed`() {
        val release = UpdateFeed.publicKey(UpdateFeed.KEY)
        assertEquals("EC", release.algorithm)
        assertNull(UpdateFeed.verified(feed("good"), release))
    }

    @Test
    fun `each device takes its own ABI's APK, and only a higher version`() {
        val picks = vectors.getValue("picks").jsonArray.map { it.jsonObject }
        assertTrue(picks.size >= 7)
        for (row in picks) {
            val body = assertNotNull(UpdateFeed.verified(feed(row.getValue("feed").jsonPrimitive.content), key))
            val installed = row.getValue("installed").jsonPrimitive.long
            val offer = UpdateFeed.pick(body, installed)
            val want = row.getValue("pick")
            if (want is JsonNull) {
                assertNull(offer, "installed $installed")
                continue
            }
            val pick = want.jsonObject
            val role = pick.getValue("role").jsonPrimitive.content
            assertNotNull(offer, "installed $installed")
            assertEquals(pick.getValue("version").jsonPrimitive.content, offer.version)
            assertEquals(pick.getValue("versionCode").jsonPrimitive.long, offer.versionCode)
            val entry = body.getValue(role).jsonObject
            assertEquals(entry.getValue("url").jsonPrimitive.content, offer.url)
            assertEquals(entry.getValue("sha256").jsonPrimitive.content, offer.sha256)
            assertEquals(entry.getValue("size").jsonPrimitive.long, offer.size)
        }
    }

    @Test
    fun `a malformed entry offers nothing`() {
        fun entry(
            url: String = "https://github.com/x.apk",
            sha: String = "a".repeat(64),
            size: Long = 10,
            version: String = "1.0.0",
        ) = buildJsonObject {
            putJsonObject("android-arm64") {
                put("version", version)
                put("url", url)
                put("sha256", sha)
                put("size", size)
            }
        }
        assertNotNull(UpdateFeed.pick(entry(), 40002))
        assertNull(UpdateFeed.pick(entry(url = "http://github.com/x.apk"), 40002))
        assertNull(UpdateFeed.pick(entry(sha = "A".repeat(64)), 40002))
        assertNull(UpdateFeed.pick(entry(size = 0), 40002))
        assertNull(UpdateFeed.pick(entry(size = UpdateFeed.MAX_APK_BYTES + 1), 40002))
        assertNull(UpdateFeed.pick(entry(version = "1.0"), 40002))
        assertNull(UpdateFeed.pick(entry(version = "1.1000.0"), 40002))
    }

    @Test
    fun `the versionCode is build gradle's`() {
        assertEquals(40002L, UpdateFeed.versionCode("0.4.0", 2))
        assertEquals(10_020_031L, UpdateFeed.versionCode("1.2.3", 1))
        assertNull(UpdateFeed.versionCode("210.0.0", 2))
    }

    @Test
    fun `an APK with another signer, package or version is refused`() {
        val offer = UpdateFeed.Offer("0.5.0", 50002, "https://x", "a".repeat(64), 10)
        val own = setOf("release")

        fun ok(
            pkg: String? = "app.momotus.android",
            code: Long? = 50002,
            signers: Set<String> = own,
        ) = UpdateFeed.archiveMatches(offer, pkg, code, signers, "app.momotus.android", own)
        assertTrue(ok())
        assertFalse(ok(signers = setOf("debug")))
        assertFalse(ok(signers = setOf("release", "other")))
        assertFalse(ok(signers = emptySet()))
        assertFalse(ok(pkg = "app.momotus.android.walk"))
        assertFalse(ok(pkg = null))
        assertFalse(ok(code = 50001))
    }

    @Test
    fun `silent only with the toggle on, API 31 and no other update owner`() {
        val me = "app.momotus.android"
        assertTrue(UpdateRules.silent(31, true, null, me))
        assertTrue(UpdateRules.silent(36, true, me, me))
        assertFalse(UpdateRules.silent(30, true, null, me))
        assertFalse(UpdateRules.silent(36, false, null, me))
        assertFalse(UpdateRules.silent(36, true, "com.android.vending", me))
    }

    @Test
    fun `nothing installs while something plays or downloads`() {
        assertTrue(UpdateRules.mayCommitInBackground(playing = false, downloading = false))
        assertFalse(UpdateRules.mayCommitInBackground(playing = true, downloading = false))
        assertFalse(UpdateRules.mayCommitInBackground(playing = false, downloading = true))
    }

    @Test
    fun `no download nobody asked for on a metered network or while playing`() {
        assertTrue(UpdateRules.mayDownload(manual = false, metered = false, playing = false))
        assertFalse(UpdateRules.mayDownload(manual = false, metered = true, playing = false))
        assertFalse(UpdateRules.mayDownload(manual = false, metered = false, playing = true))
        assertTrue(UpdateRules.mayDownload(manual = true, metered = true, playing = false))
    }
}
