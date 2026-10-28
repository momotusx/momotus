package app.marquee.core

import app.marquee.core.client.CoreError
import app.marquee.core.model.PairingCode
import app.marquee.core.sync.PairingApproval
import app.marquee.core.sync.PairingApproval.Step
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals

/** The phone's approve sheet (plans/tv-qr-login.md 5.6), against a core that answers as told. */
@OptIn(ExperimentalCoroutinesApi::class)
class PairingApprovalTest {
    private class Core {
        /** Each approve call: (scanned, unlocked). */
        val approvals = mutableListOf<Pair<Boolean, Boolean>>()
        var answer: (Boolean, Boolean) -> String = { _, _ -> "m1" }
        var listed = listOf<String>()
        val removed = mutableListOf<String>()
        var reads = 0

        /** How long a read or an approval takes, in virtual ms. */
        var readMs = 0L
        var approveMs = 0L
    }

    private fun TestScope.approval(core: Core) =
        PairingApproval(
            this,
            read = {
                core.reads++
                delay(core.readMs)
                // Named by the code's own n=, as the core reads it.
                val name = Regex("[#&]n=([^&]*)").find(it)?.groupValues?.get(1)
                if (it.startsWith("https://")) PairingCode("https://sync", name, "tv") else error("not a code")
            },
            approve = { _, scanned, unlocked ->
                core.approvals += scanned to unlocked
                delay(core.approveMs)
                core.answer(scanned, unlocked)
            },
            members = { core.listed },
            remove = { core.removed += it },
        )

    private fun refused(code: String): Nothing = throw CoreError("refused", code = code)

    private val link = "https://link.example/pair#v=3&s=x&p=1&k=a&g=b&n=Den&f=tv"

    @Test
    fun `near, a link signs the screen in with one tap and the list shows it`() =
        runTest {
            val core = Core()
            val sheet = approval(core)
            sheet.offer(link, scanned = false)
            advanceUntilIdle()
            assertEquals(Step.Sheet("Sign in \"Den\"?", "tv"), sheet.step)
            sheet.signIn()
            advanceTimeBy(100)
            assertEquals(Step.SigningIn("Den"), sheet.step)
            core.listed = listOf("m1")
            advanceTimeBy(2_100)
            assertEquals(Step.Done("Den"), sheet.step)
            assertEquals(listOf(false to false), core.approvals)
            sheet.notYou()
            advanceUntilIdle()
            assertEquals(listOf("m1"), core.removed)
            assertEquals(Step.Removed, sheet.step)
        }

    @Test
    fun `not near, a link asks for a scan here, and the scan for the lock`() =
        runTest {
            val core = Core()
            core.answer = { scanned, unlocked ->
                when {
                    !scanned -> refused("scanRequired")
                    !unlocked -> refused("unlockRequired")
                    else -> "m2"
                }
            }
            val sheet = approval(core)
            sheet.offer(link, scanned = false)
            advanceUntilIdle()
            sheet.signIn()
            advanceUntilIdle()
            assertEquals(Step.ScanHere, sheet.step)
            sheet.scannedHere(link)
            advanceUntilIdle()
            assertEquals(Step.Unlock, sheet.step)
            sheet.unlocked()
            advanceTimeBy(100)
            assertEquals(Step.SigningIn("Den"), sheet.step)
            assertEquals(listOf(false to false, true to false, true to true), core.approvals)
            sheet.close()
        }

    @Test
    fun `a slow join, an answered or expired code, and a text that is no code say so`() =
        runTest {
            val core = Core()
            val sheet = approval(core)
            sheet.offer(link, scanned = true)
            advanceUntilIdle()
            sheet.signIn()
            advanceTimeBy(61_000)
            assertEquals(Step.Slow("Den"), sheet.step)
            for ((code, words) in listOf(
                "codeAnswered" to AccountCopy.APPROVE_ANSWERED,
                "codeExpired" to AccountCopy.APPROVE_EXPIRED,
            )) {
                core.answer = { _, _ -> refused(code) }
                sheet.offer(link, scanned = true)
                advanceUntilIdle()
                sheet.signIn()
                advanceUntilIdle()
                assertEquals(Step.Failed(words), sheet.step)
            }
            sheet.offer("hello", scanned = true)
            advanceUntilIdle()
            assertEquals(Step.Failed(SyncCopy.NOT_A_CODE), sheet.step)
        }

    @Test
    fun `a second code replaces the first, and closing ends the wait`() =
        runTest {
            val core = Core()
            val sheet = approval(core)
            sheet.offer(link, scanned = false)
            advanceUntilIdle()
            sheet.signIn()
            advanceTimeBy(100)
            sheet.offer(hall, scanned = false)
            advanceUntilIdle()
            assertEquals(Step.Sheet("Sign in \"Hall\"?", "tv"), sheet.step)
            core.listed = listOf("m1")
            advanceTimeBy(5_000)
            // The first code's wait ended with it: nothing moves the sheet on.
            assertEquals(Step.Sheet("Sign in \"Hall\"?", "tv"), sheet.step)
            sheet.close()
            assertEquals(Step.Idle, sheet.step)
        }

    private val hall = link.replace("n=Den", "n=Hall")

    @Test
    fun `a slow read of the first code never names the second`() =
        runTest {
            val core = Core()
            val sheet = approval(core)
            core.readMs = 1_000
            sheet.offer(link, scanned = false)
            advanceTimeBy(100)
            core.readMs = 0
            sheet.offer(hall, scanned = false)
            advanceUntilIdle()
            assertEquals(Step.Sheet("Sign in \"Hall\"?", "tv"), sheet.step)
        }

    @Test
    fun `an approval that answers after close or a new code writes nothing and waits for nothing`() =
        runTest {
            val core = Core()
            val sheet = approval(core)
            core.approveMs = 1_000
            sheet.offer(link, scanned = false)
            advanceUntilIdle()
            sheet.signIn()
            advanceTimeBy(100)
            sheet.close()
            core.listed = listOf("m1")
            advanceTimeBy(70_000)
            assertEquals(Step.Idle, sheet.step)
            // A new code while the old approval is out: its sheet stays, and its Sign in works.
            sheet.offer(link, scanned = false)
            advanceUntilIdle()
            sheet.signIn()
            advanceTimeBy(100)
            sheet.offer(hall, scanned = false)
            advanceTimeBy(70_000)
            assertEquals(Step.Sheet("Sign in \"Hall\"?", "tv"), sheet.step)
            core.approveMs = 0
            sheet.signIn()
            advanceTimeBy(100)
            assertEquals(Step.SigningIn("Hall"), sheet.step)
            sheet.close()
        }

    @Test
    fun `a second code replaces the sheet in place, never through Idle, and its Sign in stays busy`() =
        runTest {
            val core = Core()
            val sheet = approval(core)
            sheet.offer(link, scanned = false)
            advanceUntilIdle()
            core.readMs = 500
            sheet.offer(hall, scanned = false)
            assertEquals(Step.Sheet("Sign in \"Den\"?", "tv"), sheet.step, "no Idle between the two sheets")
            advanceTimeBy(100)
            assertEquals(
                Step.Sheet("Sign in \"Den\"?", "tv"),
                sheet.step,
                "the old sheet stays while the new code is read",
            )
            advanceUntilIdle()
            assertEquals(Step.Sheet("Sign in \"Hall\"?", "tv"), sheet.step)
            // Sign in while the next code is read sends nothing: the sheet still names the old screen.
            core.approveMs = 1_000
            sheet.offer(link, scanned = false)
            sheet.signIn()
            advanceTimeBy(100)
            assertEquals(true, sheet.busy, "Sign in waits for the read")
            assertEquals(emptyList(), core.approvals, "nothing sent under the old name")
            advanceTimeBy(500)
            assertEquals(Step.Sheet("Sign in \"Den\"?", "tv"), sheet.step)
            assertEquals(false, sheet.busy)
            // An approval cancelled by a new code never clears the next one's busy.
            core.readMs = 0
            sheet.signIn()
            advanceTimeBy(100)
            sheet.offer(hall, scanned = false)
            runCurrent()
            sheet.signIn()
            advanceTimeBy(100)
            assertEquals(true, sheet.busy, "the second approval is still out")
            assertEquals(2, core.approvals.size)
            sheet.close()
        }

    @Test
    fun `a scan here of another code asks again under that screen's name`() =
        runTest {
            val core = Core()
            core.answer = { scanned, _ -> if (scanned) "m3" else refused("scanRequired") }
            val sheet = approval(core)
            sheet.offer(link, scanned = false)
            advanceUntilIdle()
            sheet.signIn()
            advanceUntilIdle()
            assertEquals(Step.ScanHere, sheet.step)
            sheet.scannedHere(hall)
            advanceUntilIdle()
            assertEquals(Step.Sheet("Sign in \"Hall\"?", "tv"), sheet.step)
            assertEquals(listOf(false to false), core.approvals, "nothing sent for the scanned code yet")
            sheet.close()
        }
}
