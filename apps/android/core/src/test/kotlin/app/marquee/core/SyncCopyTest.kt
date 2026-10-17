package app.marquee.core

import app.marquee.core.model.Blocked
import app.marquee.core.model.SyncReport
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
        assertEquals("something else", SyncCopy.problem(null, "something else"))
    }

    private fun report(
        clockGapMs: Long = 0,
        waiting: Int = 0,
        rotationWaitMs: Long = 0,
        blocked: Blocked? = null,
    ) = SyncReport(
        pushed = 1,
        stale = 0,
        refused = 0,
        waiting = waiting,
        pulled = 0,
        applied = 0,
        unreadable = 0,
        newer = 0,
        parked = 0,
        oldEpochWrites = 0,
        clockGapMs = clockGapMs,
        rotationWaitMs = rotationWaitMs,
        blocked = blocked,
        fingerprint = "f",
        again = false,
    )

    @Test
    fun `a clean round says nothing, and each trouble gets one line`() {
        assertEquals(emptyList(), SyncCopy.notes(report()))
        assertEquals(
            listOf(
                "The sync server's clock and this device's disagree by 15 minutes, so edits sync in the order they are sent.",
                "2 edits wait for the sync server's clock to catch up.",
                "Changes wait 1 minute after a sync server restore.",
                "Edits wait until a key change finishes.",
            ),
            SyncCopy.notes(
                report(clockGapMs = -900_000, waiting = 2, rotationWaitMs = 30_000, blocked = Blocked.RotationPending),
            ),
        )
    }
}
