package app.marquee.core

import app.marquee.core.SyncCopy.Action
import app.marquee.core.model.Blocked
import app.marquee.core.model.SyncReport
import java.text.DateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class SyncCopyTest {
    @Test
    fun `failures with a fix get their words, a network error never shows its address`() {
        assertTrue(SyncCopy.problem("dropped", "x").startsWith("This device was removed"))
        // A kit or a wipe asked off the main profile (D13, D17), not a server that did not answer.
        assertEquals("Only the main profile can do that.", SyncCopy.problem("notOwner", "x", Action.Kit))
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
        assertEquals(SyncCopy.WRONG_KIT, SyncCopy.problem("kitUnknown", "opens no account", Action.Recover))
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
    fun `a local leave with a record not this device's own changed nothing`() {
        assertEquals(
            "This device's sync key does not match the account stored here, so nothing changed.",
            SyncCopy.problem("wrongRecord", "", Action.Leave),
        )
    }

    @Test
    fun `approving a code of a device already in the account says so`() {
        assertEquals(
            "This code belongs to a device already in the account.",
            SyncCopy.problem(null, "pairing: this code belongs to a device already in the account", Action.Approve),
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
        again: Boolean = false,
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
        again = again,
    )

    @Test
    fun `one of each reads in the singular`() {
        assertEquals(
            listOf(
                "1 edit from this device could not be sent: the sync server does not take it, so it stays on this " +
                    "device only.",
                "1 change from another device could not be read and was skipped.",
                "1 change comes from a newer version of Momotus. Update Momotus here to see it.",
                "1 edit waits for the sync server's clock to catch up. Nothing is lost.",
                "1 change from another device waits until this device catches up with a device change.",
            ),
            SyncCopy.notes(report(waiting = 1, parked = 1, refused = 1, unreadable = 1, newer = 1)),
        )
    }

    @Test
    fun `a clean round says nothing, and every fact of a round gets its line`() {
        assertEquals(emptyList(), SyncCopy.notes(report()))
        assertEquals(
            listOf(
                "2 edits from this device could not be sent: the sync server does not take them, so they stay on " +
                    "this device only.",
                "2 changes from other devices could not be read and were skipped.",
                "4 changes come from a newer version of Momotus. Update Momotus here to see them.",
                "The sync server's clock and this device's disagree by 15 minutes, so edits sync in the order they are sent.",
                "2 edits wait for the sync server's clock to catch up. Nothing is lost.",
                "3 changes from other devices wait until this device catches up with a device change.",
                "The sync server may be hiding a device change. Compare the 12 characters below on each device.",
                "Edits wait 1 minute more: the sync server was restored, and other devices get a chance to catch up first.",
                "Edits wait until the account finishes a device change.",
                "The last sync stopped before it finished. It goes on with the next one.",
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
                    again = true,
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

        // Day and time, in the device's own format (the JDK writes a narrow space before AM).
        fun plain(line: String) = line.replace('\u202f', ' ')
        assertEquals(
            "Added Oct 19, 2026, 12:00 PM, the first device",
            plain(SyncCopy.addedLine(false, day, null, utc, Locale.US)),
        )
        assertEquals(
            "Added Oct 19, 2026, 12:00 PM through Phone",
            plain(SyncCopy.addedLine(false, day, "Phone", utc, Locale.US)),
        )
        assertEquals(
            "Made Oct 19, 2026, 12:00 PM on Phone",
            plain(SyncCopy.addedLine(true, day, "Phone", utc, Locale.US)),
        )
        // The device's locale decides the words; JDKs differ in the short month ("okt" in 21, "okt." in 17).
        val dutch = SyncCopy.addedLine(false, day, "Phone", utc, Locale("nl"))
        val dutchDay =
            DateFormat
                .getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT, Locale("nl"))
                .apply { timeZone = utc }
                .format(Date(day))
        assertEquals("Added $dutchDay through Phone", dutch)
        assertTrue(dutch.startsWith("Added 19 okt") && dutch.endsWith(" 2026 12:00 through Phone"), dutch)
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
        assertTrue(SyncCopy.removeDeviceAsk("Tablet").endsWith("Devices it added stay."))
        assertTrue(SyncCopy.removeDeviceAsk("Tablet", 1).endsWith(" The recovery kit made on it stops working too."))
        assertTrue(SyncCopy.removeDeviceAsk("Tablet", 2).endsWith(" The 2 recovery kits made on it stop working too."))
        assertEquals(
            "TV is removed from your account and cannot read anything you change from now on.",
            SyncCopy.named(SyncCopy.LOST_LEDE, "TV"),
        )
        assertEquals(
            "Synced at 3:15 PM: 1 change sent, 2 changes received.",
            SyncCopy.syncedLine("3:15 PM", report(pushed = 1, applied = 2)),
        )
    }

    @Test
    fun `the compare page names what continuing removes, and why it cannot go on`() {
        assertEquals("Continue with the server's list", SyncCopy.forkButton(0))
        assertEquals("Continue and remove 1 device", SyncCopy.forkButton(1))
        assertEquals("Continue and remove 2 devices and 1 recovery kit", SyncCopy.forkButton(2, 1))
        assertEquals("Continue and remove 1 device and 3 recovery kits", SyncCopy.forkButton(1, 3))
        assertEquals("Its recovery kit goes with it.", SyncCopy.goesWith(1))
        assertEquals("Its 2 recovery kits go with it.", SyncCopy.goesWith(2))
        assertEquals(SyncCopy.FORK_CHANGED, SyncCopy.problem("rosterChanged", "", Action.ServerList))
        // A server that lost this device's token: retrying cannot help, so it says so.
        assertEquals(SyncCopy.FORK_TOKEN_LOST, SyncCopy.problem(null, "http status 401", Action.Compare))
        assertEquals(
            "The sync server did not answer, so the lists could not be compared. Try again in a moment.",
            SyncCopy.problem(null, "http status 502", Action.Compare),
        )
        assertTrue(SyncCopy.problem("fork", "").endsWith("Compare the lists to keep syncing."))
    }

    @Test
    fun `a round that failed or waits never reads as synced`() {
        assertEquals(SyncCopy.Outcome.Complete, SyncCopy.outcome(report()))
        assertEquals(SyncCopy.Outcome.Failed, SyncCopy.outcome(report(refused = 1)))
        assertEquals(SyncCopy.Outcome.Failed, SyncCopy.outcome(report(unreadable = 1)))
        assertEquals(SyncCopy.Outcome.Pending, SyncCopy.outcome(report(blocked = Blocked.NotActive)))
        assertEquals(SyncCopy.Outcome.Pending, SyncCopy.outcome(report(again = true)))
        assertEquals(SyncCopy.Outcome.Pending, SyncCopy.outcome(report(newer = 1)))
        assertEquals(
            "Synced at 3:15 PM. Some changes did not sync.",
            SyncCopy.stateLine("3:15 PM", report(refused = 1)),
        )
        assertEquals("Synced at 3:15 PM. Some changes still wait.", SyncCopy.stateLine("3:15 PM", report(waiting = 1)))
        assertEquals(
            "Synced at 3:15 PM: 1 change sent, 0 changes received.",
            SyncCopy.stateLine("3:15 PM", report()),
        )
        // Settings: a problem, then failures kept since joining, then what waits, then the time.
        assertEquals(SyncCopy.OFF_LINE, SyncCopy.summary(false, null, null, null, null))
        assertEquals("Nope.", SyncCopy.summary(true, "Nope.", "Since...", report(), "3:15 PM"))
        val failed = SyncCopy.failedSince(2, 1, 0)
        assertEquals(
            "Since this device joined, 2 edits from this device could not be sent and stay here only, and 1 change " +
                "from other devices could not be read.",
            failed,
        )
        assertEquals(failed, SyncCopy.summary(true, null, failed, report(), "3:15 PM"))
        assertEquals(
            "The sync server does not take edits from this device yet, so they were not sent.",
            SyncCopy.summary(true, null, null, report(blocked = Blocked.NotActive), "3:15 PM"),
        )
        assertEquals("On. Synced at 3:15 PM.", SyncCopy.summary(true, null, null, report(), "3:15 PM"))
        assertEquals(null, SyncCopy.failedSince(0, 0, 0))
        assertEquals(
            "Since this device joined, 1 change from a newer version of Momotus waits for an update here.",
            SyncCopy.failedSince(0, 0, 1),
        )
    }
}
