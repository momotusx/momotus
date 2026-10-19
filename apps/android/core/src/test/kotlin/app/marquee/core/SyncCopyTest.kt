package app.marquee.core

import app.marquee.core.SyncCopy.Action
import app.marquee.core.model.Blocked
import app.marquee.core.model.SyncReport
import java.util.TimeZone
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class SyncCopyTest {
    @Test
    fun `failures with a fix get their words, a network error never shows its address`() {
        assertTrue(SyncCopy.problem("dropped", "x").startsWith("This device was removed"))
        assertEquals(
            "The sync server did not answer. Your changes wait here and go out on the next sync.",
            SyncCopy.problem(null, "network: error sending request (https://relay.example/<redacted>)"),
        )
        // Anything the core did not word for people is replaced: it can name the server.
        assertEquals(
            "The sync server did not answer. Your changes wait here and go out on the next sync.",
            SyncCopy.problem(null, "something else"),
        )
    }

    @Test
    fun `each action words the same answer for what it means there`() {
        assertEquals(
            "This sync server needs an invite code, or the one entered is wrong.",
            SyncCopy.problem(null, "http status 403", Action.Start),
        )
        assertEquals(SyncCopy.WRONG_KIT, SyncCopy.problem(null, "http status 404", Action.Recover))
        assertEquals(
            "This code expired or was used already. Ask the new device for a new one.",
            SyncCopy.problem(null, "http status 410", Action.Approve),
        )
        // A 404 while making a kit is not a used pairing code.
        assertEquals(
            "The sync server did not answer. Try again in a moment.",
            SyncCopy.problem(null, "http status 404", Action.Kit),
        )
        assertEquals(
            "The sync server did not answer, so this list may be out of date. Try again in a moment.",
            SyncCopy.problem(null, "http status 502", Action.Devices),
        )
        assertEquals(
            "Nothing changed: this device did not confirm it is you.",
            SyncCopy.problem("unconfirmed", "", Action.Remove),
        )
        assertTrue(SyncCopy.problem("noLock", "", Action.Kit).startsWith("Set a screen lock first"))
        assertTrue(SyncCopy.problem("unsent", "", Action.Leave).startsWith("Some changes have not been sent yet"))
        assertEquals(
            "The roster is out of date.",
            SyncCopy.problem(null, "roster: the roster is out of date", Action.Lost),
        )
    }

    @Test
    fun `pairing keeps the core's own reasons and never shows an address`() {
        assertEquals(
            "The code is for another sync server.",
            SyncCopy.pairingProblem("pairing: the code is for another sync server"),
        )
        assertEquals(
            "The sync server address must start with https.",
            SyncCopy.pairingProblem("the relay must use https unless it is on this network"),
        )
        for (raw in listOf(
            "network: error sending request (https://user:secret@relay.example/<redacted>)",
            "invalid url: https://user:secret@relay.example",
            "http status 502",
            "timed out after 10s",
        )) {
            val words = SyncCopy.pairingProblem(raw)
            assertTrue("relay.example" !in words && "secret" !in words, words)
        }
    }

    private fun report(
        clockGapMs: Long = 0,
        waiting: Int = 0,
        parked: Int = 0,
        rotationWaitMs: Long = 0,
        blocked: Blocked? = null,
        refused: Int = 0,
        unreadable: Int = 0,
        newer: Int = 0,
        oldEpochWrites: Int = 0,
        pushed: Int = 1,
        applied: Int = 0,
    ) = SyncReport(
        pushed = pushed,
        stale = 0,
        refused = refused,
        waiting = waiting,
        pulled = 0,
        applied = applied,
        unreadable = unreadable,
        newer = newer,
        parked = parked,
        oldEpochWrites = oldEpochWrites,
        clockGapMs = clockGapMs,
        rotationWaitMs = rotationWaitMs,
        blocked = blocked,
        fingerprint = "f",
        again = false,
    )

    @Test
    fun `one of each reads in the singular`() {
        assertEquals(
            listOf(
                "1 edit waits for the sync server's clock to catch up. Nothing is lost.",
                "1 change from another device waits until this device catches up with a device change.",
                "1 edit was too large or malformed for the sync server, so it stays only on this device.",
                "1 change from another device did not open with this account's key and was skipped.",
                "1 change comes from a newer version of Marquee. Update the app to see it.",
            ),
            SyncCopy.notes(report(waiting = 1, parked = 1, refused = 1, unreadable = 1, newer = 1)),
        )
    }

    @Test
    fun `a clean round says nothing, and every fact of a round gets its line`() {
        assertEquals(emptyList(), SyncCopy.notes(report()))
        assertEquals(
            listOf(
                "The sync server's clock and this device's disagree by 15 minutes, so edits sync in the order they are sent.",
                "2 edits wait for the sync server's clock to catch up. Nothing is lost.",
                "3 changes from other devices wait until this device catches up with a device change.",
                "The sync server may be hiding a device change. Compare the 12 characters below on each device.",
                "Edits wait 1 minute more: the sync server was restored, and other devices get a chance to catch up first.",
                "2 edits were too large or malformed for the sync server, so they stay only on this device.",
                "2 changes from another device did not open with this account's key and were skipped.",
                "4 changes come from a newer version of Marquee. Update the app to see them.",
                "Edits wait until the account finishes a device change.",
            ),
            SyncCopy.notes(
                report(
                    clockGapMs = -900_000,
                    waiting = 2,
                    parked = 3,
                    oldEpochWrites = 1,
                    rotationWaitMs = 30_000,
                    refused = 2,
                    unreadable = 2,
                    newer = 4,
                    blocked = Blocked.RotationPending,
                ),
            ),
        )
        assertEquals(
            listOf("The sync server does not take edits from this device yet, so they were not sent."),
            SyncCopy.notes(report(blocked = Blocked.NotActive)),
        )
        assertTrue(
            SyncCopy.notes(report(blocked = Blocked.RelayDisagrees)).single().startsWith("The sync server disagrees"),
        )
        // A wait rounds up to whole minutes: never "0 minutes", never less than it is.
        assertEquals(
            listOf(
                "Edits wait 2 minutes more: the sync server was restored, and other devices get a chance to catch " +
                    "up first.",
            ),
            SyncCopy.notes(report(rotationWaitMs = 90_000)),
        )
        // Ten minutes exactly is still fine; a minute more is not.
        assertEquals(emptyList(), SyncCopy.notes(report(clockGapMs = 600_000)))
        assertEquals(1, SyncCopy.notes(report(clockGapMs = 660_000)).size)
    }

    @Test
    fun `lines that name devices, kits and counts`() {
        val utc = TimeZone.getTimeZone("UTC")
        // 2026-10-19 12:00 UTC
        val day = 1_792_411_200_000L
        assertEquals("Added Oct 19, 2026, the first device", SyncCopy.addedLine(false, day, null, utc))
        assertEquals("Added Oct 19, 2026 through Phone", SyncCopy.addedLine(false, day, "Phone", utc))
        assertEquals("Made Oct 19, 2026 on Phone", SyncCopy.addedLine(true, day, "Phone", utc))
        assertEquals("Remove 1 device", SyncCopy.lostButton(1))
        assertEquals("Remove 3 devices", SyncCopy.lostButton(3))
        assertEquals(
            "1 recovery kit made on these devices stops working. Make a new one afterwards.",
            SyncCopy.lostKits(1),
        )
        assertEquals(
            "2 recovery kits made on these devices stop working. Make a new one afterwards.",
            SyncCopy.lostKits(2),
        )
        assertEquals("Approve a device from sync.example.org?", SyncCopy.approveAsk("sync.example.org"))
        assertEquals(
            "TV is removed from your account and cannot read anything you change from now on.",
            SyncCopy.named(SyncCopy.LOST_LEDE, "TV"),
        )
        assertEquals(
            "Synced at 3:15 PM: 1 change sent, 2 changes received.",
            SyncCopy.syncedLine("3:15 PM", report(pushed = 1, applied = 2)),
        )
    }
}
