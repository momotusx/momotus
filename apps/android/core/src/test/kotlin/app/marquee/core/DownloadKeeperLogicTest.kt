package app.marquee.core

import app.marquee.core.KeeperEffect.Allocate
import app.marquee.core.KeeperEffect.Allowed
import app.marquee.core.KeeperEffect.ClearNotices
import app.marquee.core.KeeperEffect.Failed
import app.marquee.core.KeeperEffect.Finished
import app.marquee.core.KeeperEffect.HideProgress
import app.marquee.core.KeeperEffect.Keep
import app.marquee.core.KeeperEffect.Progress
import app.marquee.core.KeeperEffect.Release
import app.marquee.core.KeeperEffect.SettleLater
import app.marquee.core.model.DownloadActivity
import app.marquee.core.model.DownloadSettings
import app.marquee.core.model.DownloadStorage
import app.marquee.core.model.DownloadsSnapshot
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class DownloadKeeperLogicTest {
    private fun facts(
        left: Int,
        done: Map<String, String> = emptyMap(),
        failed: Map<String, String> = emptyMap(),
        bytesDone: Long = 0,
        bytesTotal: Long = 0,
        needed: Long? = null,
        unmetered: Boolean = false,
        locked: Boolean = false,
        running: Int = left,
        profile: String = "owner",
    ) = KeeperEvent.Facts(
        KeeperFacts(
            left,
            running,
            bytesDone,
            bytesTotal,
            "Severance",
            done,
            failed,
            needed,
            unmetered,
            locked = locked,
            profile = profile,
        ),
    )

    /** A locked session's view: counts and bytes, no names, nothing done or failed. */
    private fun locked(
        left: Int,
        bytesDone: Long = 0,
        bytesTotal: Long = 0,
    ) = KeeperEvent.Facts(
        KeeperFacts(left, left, bytesDone, bytesTotal, null, emptyMap(), emptyMap(), null, locked = true, profile = "owner"),
    )

    private fun KeeperEffect.isProgress() = this is Progress

    @Test
    fun `downloads in front hold the job once, and an empty queue that settles lets it go`() {
        val keeper = DownloadKeeperLogic()
        assertEquals(listOf<KeeperEffect>(Allowed(true)), keeper.on(KeeperEvent.Visible(true)))
        assertEquals(emptyList(), keeper.on(facts(0)))
        val first = keeper.on(facts(3, bytesTotal = 3_000))
        assertEquals(Keep(3_000), first[0])
        assertEquals(Progress("Downloading 1 of 3", "Severance · 0%", 0f), first[1])
        // Further snapshots only move the notification: one job per queue.
        val next = keeper.on(facts(2, done = mapOf("a" to "Severance"), bytesDone = 1_500, bytesTotal = 3_000))
        assertEquals(listOf<KeeperEffect>(Progress("Downloading 2 of 3", "Severance · 50%", 0.5f)), next)
        // Leaving the app keeps downloads allowed while the job holds it.
        assertEquals(emptyList(), keeper.on(KeeperEvent.Visible(false)))
        // An empty queue waits to settle: a blink between two downloads keeps the job.
        assertEquals(listOf<KeeperEffect>(SettleLater), keeper.on(facts(0, done = mapOf("a" to "Severance"))))
        assertEquals(1, keeper.on(facts(1, done = mapOf("a" to "Severance"))).count { it.isProgress() })
        assertEquals(emptyList(), keeper.on(KeeperEvent.Settled))
        keeper.on(facts(0, done = mapOf("a" to "Severance", "b" to "Severance", "c" to "Severance")))
        assertEquals(
            listOf(Release, Finished("Downloads finished", "3 episodes of Severance"), Allowed(false)),
            keeper.on(KeeperEvent.Settled),
        )
    }

    @Test
    fun `out of sight nothing is held, and the front takes up the queue`() {
        val keeper = DownloadKeeperLogic()
        assertEquals(listOf<KeeperEffect>(Allowed(false)), keeper.on(facts(2)))
        val front = keeper.on(KeeperEvent.Visible(true))
        assertTrue(front[0] is Keep)
        assertTrue(front[1].isProgress())
        assertEquals(Allowed(true), front.last())
        // The system stopped the job out of sight: downloads wait for the app until it starts it again.
        keeper.on(KeeperEvent.Visible(false))
        assertEquals(listOf(HideProgress, Allowed(false)), keeper.on(KeeperEvent.Stopped))
        assertEquals(listOf<KeeperEffect>(Allowed(true)), keeper.on(KeeperEvent.Started))
    }

    @Test
    fun `a minute sends the say again while downloads run, and the front always does`() {
        val keeper = DownloadKeeperLogic()
        keeper.on(KeeperEvent.Visible(true))
        assertEquals(emptyList(), keeper.on(KeeperEvent.Minute))
        keeper.on(facts(1))
        assertEquals(listOf<KeeperEffect>(Allowed(true)), keeper.on(KeeperEvent.Minute))
        assertEquals(listOf<KeeperEffect>(Allowed(true)), keeper.on(KeeperEvent.Visible(true)))
    }

    @Test
    fun `a new failure is told once, an old one never, and a space wait asks the system once`() {
        val keeper = DownloadKeeperLogic()
        keeper.on(KeeperEvent.Visible(true))
        assertEquals(emptyList(), keeper.on(facts(0, failed = mapOf("old" to "Sintel"))))
        assertEquals(
            listOf<KeeperEffect>(Failed("new", "Severance S1:E4")),
            keeper.on(facts(0, failed = mapOf("old" to "Sintel", "new" to "Severance S1:E4"))),
        )
        assertEquals(emptyList(), keeper.on(facts(0, failed = mapOf("old" to "Sintel", "new" to "Severance S1:E4"))))
        assertEquals(listOf<KeeperEffect>(Allocate(500)), keeper.on(facts(0, needed = 500)))
        assertEquals(emptyList(), keeper.on(facts(0, needed = 500)))
    }

    /**
     * A download waiting for Wi-Fi is still left to do: the job stays scheduled, with Wi-Fi as its network, so the
     * system starts it when the phone joins Wi-Fi with the app away, and the core hears it may run (code review r1
     * C1).
     */
    @Test
    fun `a download waiting for Wi-Fi keeps the job, which starts on Wi-Fi with the app away`() {
        val keeper = DownloadKeeperLogic()
        keeper.on(KeeperEvent.Visible(true))
        val held = keeper.on(facts(1, bytesTotal = 2_000, unmetered = true))
        assertEquals(Keep(2_000, unmetered = true), held[0])
        // The app leaves; nothing settles while the download waits.
        assertEquals(emptyList(), keeper.on(KeeperEvent.Visible(false)))
        assertEquals(
            emptyList(),
            keeper.on(facts(1, bytesTotal = 2_000, unmetered = true)).filterNot { it.isProgress() },
        )
        // Wi-Fi: the system starts the job, and downloads may run.
        keeper.on(KeeperEvent.Stopped)
        assertEquals(listOf<KeeperEffect>(Allowed(true)), keeper.on(KeeperEvent.Started))
    }

    /** The system starting the job again with nothing left lets it go after the settle (code review r1 M1). */
    @Test
    fun `a job started again with nothing left settles and lets go`() {
        val keeper = DownloadKeeperLogic()
        keeper.on(KeeperEvent.Visible(true))
        keeper.on(facts(1))
        keeper.on(facts(0))
        keeper.on(KeeperEvent.Settled)
        keeper.on(KeeperEvent.Visible(false))
        assertTrue(SettleLater in keeper.on(KeeperEvent.Started))
        assertTrue(Release in keeper.on(KeeperEvent.Settled))
    }

    /**
     * The core counts what is left for the keeper, waiting downloads included, also for a locked profile with no row
     * to read; a download let onto mobile data needs any network.
     */
    @Test
    fun `the facts count running, queued and waiting, and Wi-Fi only asks for Wi-Fi unless one goes on mobile data`() {
        fun snapshot(
            waiting: Int,
            mobileData: Boolean = false,
            wifiOnly: Boolean = true,
        ) = DownloadsSnapshot(
            locked = false,
            storage = DownloadStorage(0),
            settings = DownloadSettings(wifiOnly = wifiOnly, smart = true),
            active = DownloadActivity(1, 1, 0, 0, waiting = waiting, mobileData = mobileData),
        )
        assertEquals(3, KeeperFacts.of(snapshot(waiting = 1)).left)
        assertTrue(KeeperFacts.of(snapshot(waiting = 1)).unmetered)
        assertEquals(false, KeeperFacts.of(snapshot(waiting = 1, mobileData = true)).unmetered)
        assertEquals(false, KeeperFacts.of(snapshot(waiting = 0, wifiOnly = false)).unmetered)
        // A locked session's view says so, and the keeper keeps its names (code review r2 N1).
        assertTrue(KeeperFacts.of(snapshot(waiting = 1).copy(locked = true)).locked)
    }

    /**
     * The core keeps the keeper's view across a lock (code review r2 N1): a locked snapshot lists nothing, so the
     * failures it does not show are not gone, and they are not news when the unlock lists them again.
     */
    @Test
    fun `a lock hides the failures without forgetting them, and the unlock raises nothing old`() {
        val keeper = DownloadKeeperLogic()
        keeper.on(KeeperEvent.Visible(true))
        keeper.on(facts(0, failed = mapOf("old" to "Sintel")))
        val both = mapOf("old" to "Sintel", "new" to "Severance S1:E4")
        assertEquals(listOf<KeeperEffect>(Failed("new", "Severance S1:E4")), keeper.on(facts(0, failed = both)))
        assertEquals(emptyList(), keeper.on(locked(0)))
        assertEquals(emptyList(), keeper.on(facts(0, failed = both)))
        // A failure that came while locked is news once its name is known.
        assertEquals(
            listOf<KeeperEffect>(Failed("later", "Sintel")),
            keeper.on(facts(0, failed = mapOf("old" to "Sintel", "new" to "Severance S1:E4", "later" to "Sintel"))),
        )
    }

    /** A batch begun while locked counts from what was done before it, not from nothing (code review r2 N1). */
    @Test
    fun `a batch begun while locked counts only its own downloads after the unlock`() {
        val keeper = DownloadKeeperLogic()
        keeper.on(KeeperEvent.Visible(true))
        val before = mapOf("a" to "Sintel", "b" to "Sintel")
        keeper.on(facts(0, done = before))
        // Locked: the view lists nothing done, then the batch begins.
        keeper.on(locked(0))
        assertTrue(keeper.on(locked(2, bytesTotal = 2_000)).any { it is Keep })
        // Unlocked with one of the two done: the second of two, and the finish names only this batch.
        assertEquals(
            Progress("Downloading 2 of 2", "Severance · 50%", 0.5f),
            keeper.on(facts(1, done = before + ("c" to "Severance"), bytesDone = 1_000, bytesTotal = 2_000)).last(),
        )
        keeper.on(facts(0, done = before + ("c" to "Severance") + ("d" to "Severance")))
        assertEquals(
            listOf(Release, Finished("Downloads finished", "2 episodes of Severance")),
            keeper.on(KeeperEvent.Settled),
        )
    }

    /** Begun locked before any name was known: it counts from the first names it sees, never everything ever done. */
    @Test
    fun `a batch begun locked at the start counts from the first names`() {
        val keeper = DownloadKeeperLogic()
        keeper.on(KeeperEvent.Visible(true))
        keeper.on(locked(1, bytesTotal = 1_000))
        assertEquals(
            Progress("Downloading", "Severance · 0%", 0f),
            keeper.on(facts(1, done = mapOf("a" to "Sintel", "b" to "Sintel"), bytesTotal = 1_000)).last(),
        )
    }

    /**
     * Wi-Fi only, then "Download now on mobile data": the held job asks for any network, so the system keeps it with
     * the app away (code review r2 N2).
     */
    @Test
    fun `a held job asks again when its network changes`() {
        val keeper = DownloadKeeperLogic()
        keeper.on(KeeperEvent.Visible(true))
        assertEquals(Keep(2_000, unmetered = true), keeper.on(facts(1, bytesTotal = 2_000, unmetered = true))[0])
        assertEquals(Keep(2_000, unmetered = false), keeper.on(facts(1, bytesTotal = 2_000))[0])
        assertTrue(keeper.on(facts(1, bytesTotal = 2_000)).none { it is Keep })
        // Out of sight the system takes no job: the change waits for the front.
        keeper.on(KeeperEvent.Visible(false))
        assertTrue(keeper.on(facts(1, bytesTotal = 2_000, unmetered = true)).none { it is Keep })
    }

    /** A refused job is asked for again once a minute, not at every snapshot (code review r2 n3). */
    @Test
    fun `a refused job waits for the minute`() {
        val keeper = DownloadKeeperLogic()
        keeper.on(KeeperEvent.Visible(true))
        assertTrue(keeper.on(facts(1)).any { it is Keep })
        keeper.on(KeeperEvent.Refused)
        assertTrue(keeper.on(facts(1)).none { it is Keep })
        assertTrue(keeper.on(facts(1)).none { it is Keep })
        assertTrue(keeper.on(KeeperEvent.Minute).any { it is Keep })
        assertTrue(keeper.on(facts(1)).none { it is Keep })
    }

    /**
     * A switch with no lock (owner to kids) takes the owner's finished and failure notices down, and the next
     * profile's failures and finishes are not news to it, nor the owner's again on the way back (code review r3 M1).
     */
    @Test
    fun `a profile switch clears the notices and raises nothing of either profile`() {
        val keeper = DownloadKeeperLogic()
        keeper.on(KeeperEvent.Visible(true))
        val owner = mapOf("a" to "Sintel", "b" to "Severance S1:E4")
        assertEquals(emptyList(), keeper.on(facts(0, failed = mapOf("a" to "Sintel"))))
        assertEquals(listOf<KeeperEffect>(Failed("b", "Severance S1:E4")), keeper.on(facts(0, failed = owner)))
        assertEquals(emptyList(), keeper.on(facts(0, failed = owner)))
        assertEquals(
            listOf<KeeperEffect>(ClearNotices),
            keeper.on(facts(0, failed = mapOf("a" to "Sintel"), profile = "kid")),
        )
        assertEquals(listOf<KeeperEffect>(ClearNotices), keeper.on(facts(0, failed = owner)))
        assertEquals(emptyList(), keeper.on(facts(0, failed = owner)))
    }

    /**
     * The new profile's first snapshot carries failures the last profile never saw: they are its past, not news, so
     * no "Couldn't download" goes up for them and the last profile's notices come down (code review r4 m5).
     */
    @Test
    fun `the new profile's first snapshot raises none of its old failures`() {
        val keeper = DownloadKeeperLogic()
        keeper.on(KeeperEvent.Visible(true))
        keeper.on(facts(0, profile = "kid"))
        val owner = mapOf("a" to "Sintel", "b" to "Severance S1:E4")
        assertEquals(listOf<KeeperEffect>(ClearNotices), keeper.on(facts(0, failed = owner)))
        // A failure that comes after is news.
        assertEquals(
            listOf<KeeperEffect>(Failed("c", "Sintel")),
            keeper.on(facts(0, failed = owner + ("c" to "Sintel"))),
        )
    }

    /** A batch that begins after a switch counts from the new profile's done downloads, not the old one's. */
    @Test
    fun `a batch after a switch counts only the new profile's own finishes`() {
        val keeper = DownloadKeeperLogic()
        keeper.on(KeeperEvent.Visible(true))
        keeper.on(facts(0, profile = "kid"))
        val before = mapOf("a" to "Sintel", "b" to "Sintel")
        assertEquals(
            Progress("Downloading", "Severance · 0%", 0f),
            keeper.on(facts(1, done = before, bytesTotal = 1_000)).last(),
        )
        keeper.on(facts(0, done = before + ("c" to "Severance")))
        assertEquals(listOf(Release, Finished("Downloads finished", "Severance")), keeper.on(KeeperEvent.Settled))
    }

    /** A refusal as the app leaves is asked again at the next front, not a minute later (code review r3 m1). */
    @Test
    fun `a refused job is asked again when the app comes back`() {
        val keeper = DownloadKeeperLogic()
        keeper.on(KeeperEvent.Visible(true))
        keeper.on(facts(1))
        keeper.on(KeeperEvent.Visible(false))
        assertEquals(listOf(HideProgress, Allowed(false)), keeper.on(KeeperEvent.Refused))
        assertTrue(keeper.on(KeeperEvent.Visible(true)).any { it is Keep })
    }

    /** Every download waits (Wi-Fi, a connection, room): the notification says so, never "Downloading" (r3 m2). */
    @Test
    fun `downloads that only wait read as waiting`() {
        val keeper = DownloadKeeperLogic()
        keeper.on(KeeperEvent.Visible(true))
        assertEquals(
            Progress("Waiting", "Severance", null, waiting = true),
            keeper.on(facts(2, running = 0, unmetered = true))[1],
        )
        assertEquals(Progress("Downloading 1 of 2", "Severance", null), keeper.on(facts(2, running = 1)).last())
    }
}
