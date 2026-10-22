package app.marquee.core

import app.marquee.core.model.TraktImporting
import app.marquee.core.model.TraktLoginSnapshot
import app.marquee.core.model.TraktPlayback
import app.marquee.core.model.TraktStatus
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class TraktCopyTest {
    private val time = { ms: Long -> "T$ms" }

    private fun status(
        state: String,
        user: String? = "tester",
        sinceMs: Long? = null,
        reason: String? = null,
    ) = TraktStatus(state = state, sinceMs = sinceMs, reason = reason, user = user, pendingWrites = 0)

    @Test
    fun `the words are the shared ones, every key and no more`() {
        val shared =
            Json
                .parseToJsonElement(File("../../shared/trakt-words.json").readText())
                .jsonObject
                .mapValues { it.value.jsonPrimitive.content }
                // Android shows no eyebrow (design v3); the web and Apple keep theirs until their own screens drop it.
                .filterKeys { it != "eyebrow" }
        val ours =
            with(TraktCopy) {
                mapOf(
                    "title" to TITLE,
                    "lede" to LEDE,
                    "rowOff" to ROW_OFF,
                    "connected" to CONNECTED,
                    "connectedNoUser" to CONNECTED_NO_USER,
                    "failing" to FAILING,
                    "reconnect" to RECONNECT,
                    "off" to OFF,
                    "thisDeviceOff" to THIS_DEVICE_OFF,
                    "notConfigured" to NOT_CONFIGURED,
                    "connect" to CONNECT,
                    "signInAgain" to SIGN_IN_AGAIN,
                    "signInHere" to SIGN_IN_HERE,
                    "starting" to STARTING,
                    "open" to OPEN,
                    "scanOrOpen" to SCAN_OR_OPEN,
                    "enterCode" to ENTER_CODE,
                    "goodFor" to GOOD_FOR,
                    "openPage" to OPEN_PAGE,
                    "saving" to SAVING,
                    "expired" to EXPIRED,
                    "denied" to DENIED,
                    "failed" to FAILED,
                    "newCode" to NEW_CODE,
                    "cancel" to CANCEL,
                    "close" to CLOSE,
                    "lastSynced" to LAST_SYNCED,
                    "neverSynced" to NEVER_SYNCED,
                    "pendingNone" to PENDING_NONE,
                    "pendingOne" to PENDING_ONE,
                    "pendingMany" to PENDING_MANY,
                    "importing" to IMPORTING,
                    "lastError" to LAST_ERROR,
                    "syncNow" to SYNC_NOW,
                    "syncing" to SYNCING,
                    "synced" to SYNCED,
                    "syncFailed" to SYNC_FAILED,
                    "signOut" to SIGN_OUT,
                    "signOutAsk" to SIGN_OUT_ASK,
                    "signedOut" to SIGNED_OUT,
                    "signOutFailed" to SIGN_OUT_FAILED,
                    "memoryOnly" to MEMORY_ONLY,
                    "unmappedTitle" to UNMAPPED_TITLE,
                    "unmappedLede" to UNMAPPED_LEDE,
                    "playerOn" to PLAYER_ON,
                    "playerUnmapped" to PLAYER_UNMAPPED,
                    "playerThisDeviceOff" to PLAYER_THIS_DEVICE_OFF,
                )
            }
        assertEquals(shared.keys, ours.keys)
        for ((key, value) in shared) assertEquals(value, ours[key], key)
    }

    @Test
    fun `each state has its own words`() {
        assertEquals("Connected as tester", TraktCopy.state(status("connected"), time))
        assertEquals(TraktCopy.CONNECTED_NO_USER, TraktCopy.state(status("connected", user = null), time))
        assertEquals(
            "Trakt has not answered since T5: http 503. Marquee keeps trying, and nothing waiting here is lost.",
            TraktCopy.state(status("failing", sinceMs = 5, reason = "http 503"), time),
        )
        assertEquals(
            "Trakt needs you to sign in again (Trakt refused a fresh token). Until then nothing is sent or brought in.",
            TraktCopy.state(status("reconnect", reason = "Trakt refused a fresh token"), time),
        )
        assertEquals(TraktCopy.OFF, TraktCopy.state(status("off", user = null), time))
        assertEquals(TraktCopy.THIS_DEVICE_OFF, TraktCopy.state(status("thisDeviceOff", user = null), time))
        assertEquals(TraktCopy.NOT_CONFIGURED, TraktCopy.state(status("notConfigured", user = null), time))
        // A state from a newer core promises nothing.
        assertEquals(TraktCopy.OFF, TraktCopy.state(status("later", user = null), time))
        // The Settings row says what Trakt does while it is off.
        assertEquals(TraktCopy.ROW_OFF, TraktCopy.row(status("off"), time))
        assertEquals(TraktCopy.ROW_OFF, TraktCopy.row(null, time))
        assertEquals("Connected as tester", TraktCopy.row(status("connected"), time))
    }

    @Test
    fun `each state offers its own sign-in, or none`() {
        assertEquals(TraktCopy.CONNECT, TraktCopy.signIn("off"))
        assertEquals(TraktCopy.SIGN_IN_AGAIN, TraktCopy.signIn("reconnect"))
        assertEquals(TraktCopy.SIGN_IN_HERE, TraktCopy.signIn("thisDeviceOff"))
        assertNull(TraktCopy.signIn("connected"))
        assertNull(TraktCopy.signIn("failing"))
        assertNull(TraktCopy.signIn("notConfigured"))
        // Sync now and Sign out only where this device holds an account.
        assertTrue(TraktCopy.signedIn("connected") && TraktCopy.signedIn("failing") && TraktCopy.signedIn("reconnect"))
        assertFalse(
            TraktCopy.signedIn("off") || TraktCopy.signedIn("thisDeviceOff") || TraktCopy.signedIn("notConfigured"),
        )
    }

    @Test
    fun `counts, times and sign-in phases read in full`() {
        assertEquals(TraktCopy.PENDING_NONE, TraktCopy.pending(0))
        assertEquals("1 change waiting to go to Trakt", TraktCopy.pending(1))
        assertEquals("3 changes waiting to go to Trakt", TraktCopy.pending(3))
        assertEquals("Last synced T9", TraktCopy.lastSynced(9, time))
        assertEquals(TraktCopy.NEVER_SYNCED, TraktCopy.lastSynced(null, time))
        assertEquals("Bringing in your Trakt history: 2 of 7 shows", TraktCopy.importing(TraktImporting(2, 7)))
        assertEquals(TraktCopy.EXPIRED, TraktCopy.login(TraktLoginSnapshot.Expired))
        assertEquals(TraktCopy.DENIED, TraktCopy.login(TraktLoginSnapshot.Denied))
        assertEquals(TraktCopy.NOT_CONFIGURED, TraktCopy.login(TraktLoginSnapshot.NotConfigured))
        assertEquals(
            "The sign-in did not finish: the code was already used.",
            TraktCopy.login(TraktLoginSnapshot.Failed("the code was already used")),
        )
    }

    @Test
    fun `the player's mark speaks for each state and is absent without Trakt`() {
        assertEquals(TraktCopy.PLAYER_ON, TraktCopy.player(TraktPlayback.On))
        assertEquals(TraktCopy.PLAYER_UNMAPPED, TraktCopy.player(TraktPlayback.Unmapped))
        assertEquals(TraktCopy.PLAYER_THIS_DEVICE_OFF, TraktCopy.player(TraktPlayback.ThisDeviceOff))
        assertNull(TraktCopy.player(TraktPlayback.Off))
        assertNull(TraktCopy.player(null))
    }
}
