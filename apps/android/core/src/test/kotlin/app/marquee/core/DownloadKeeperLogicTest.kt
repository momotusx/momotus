package app.marquee.core

import app.marquee.core.KeeperEffect.Allocate
import app.marquee.core.KeeperEffect.Allowed
import app.marquee.core.KeeperEffect.Failed
import app.marquee.core.KeeperEffect.Finished
import app.marquee.core.KeeperEffect.Keep
import app.marquee.core.KeeperEffect.Progress
import app.marquee.core.KeeperEffect.Release
import app.marquee.core.KeeperEffect.SettleLater
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
    ) = KeeperEvent.Facts(KeeperFacts(left, bytesDone, bytesTotal, "Severance", done, failed, needed))

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
        assertEquals(listOf<KeeperEffect>(Allowed(false)), keeper.on(KeeperEvent.Stopped))
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
}
