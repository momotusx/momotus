package app.marquee.core

import app.marquee.core.client.CoreError
import app.marquee.core.model.PairingCode
import app.marquee.core.sync.PairingApproval
import app.marquee.core.sync.PairingApproval.Step
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.advanceUntilIdle
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
    }

    private fun TestScope.approval(core: Core) =
        PairingApproval(
            this,
            read = {
                core.reads++
                if (it.startsWith("https://")) PairingCode("https://sync", "Den", "tv") else error("not a code")
            },
            approve = { _, scanned, unlocked ->
                core.approvals += scanned to unlocked
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
            sheet.offer(link.replace("n=Den", "n=Hall"), scanned = false)
            advanceUntilIdle()
            assertEquals(Step.Sheet("Sign in \"Den\"?", "tv"), sheet.step, "the stub reads every link as Den")
            core.listed = listOf("m1")
            advanceTimeBy(5_000)
            // The first code's wait ended with it: nothing moves the sheet on.
            assertEquals(Step.Sheet("Sign in \"Den\"?", "tv"), sheet.step)
            sheet.close()
            assertEquals(Step.Idle, sheet.step)
        }
}
