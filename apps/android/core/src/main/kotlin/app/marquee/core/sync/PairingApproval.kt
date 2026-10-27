package app.marquee.core.sync

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.AccountCopy
import app.marquee.core.SyncCopy
import app.marquee.core.client.CoreError
import app.marquee.core.model.PairingCode
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * The phone's approve sheet (plans/tv-qr-login.md 5.6): one pairing code at a time, from a link (the system
 * camera, the link host's page) or this app's camera scanner. Sign in sends it; the core says first whether the
 * screen is on this phone's network. Not near, a link asks for a scan here ([Step.ScanHere]) and a scan for the
 * screen lock ([Step.Unlock]), and nothing is sent before. Then it waits until the device list shows the new
 * device ([Step.Done], with Not you? Remove it) or says it is slow ([Step.Slow]). Only the camera analyzer may call
 * [offer] with `scanned`: that is the app's word to the core.
 */
class PairingApproval(
    private val scope: CoroutineScope,
    private val read: suspend (qr: String) -> PairingCode,
    private val approve: suspend (qr: String, scanned: Boolean, unlocked: Boolean) -> String,
    /** The account's member ids, as the device list shows them. */
    private val members: suspend () -> List<String>,
    private val remove: suspend (member: String) -> Unit,
    private val pollEveryMs: Long = 2_000,
    private val waitMs: Long = 60_000,
) {
    sealed interface Step {
        data object Idle : Step

        /** "Sign in "<name>"?" with Sign in. */
        data class Sheet(
            val ask: String,
            val form: String?,
        ) : Step

        /** Not on that screen's network and the code came by a link: scan it here. */
        data object ScanHere : Step

        /** Not on that screen's network: the screen lock, then [unlocked]. */
        data object Unlock : Step

        data class SigningIn(
            val device: String,
        ) : Step

        data class Done(
            val device: String,
        ) : Step

        data class Slow(
            val device: String,
        ) : Step

        /** Not you? Remove it ran. */
        data object Removed : Step

        data class Failed(
            val words: String,
        ) : Step
    }

    var step: Step by mutableStateOf(Step.Idle)
        private set

    var busy by mutableStateOf(false)
        private set

    private var code: String? = null
    private var scanned = false
    private var device = ""
    private var member: String? = null

    /**
     * The read or approval under way, and the wait after it: [close] (and so every new code) cancels them, and a
     * cancelled coroutine never resumes past its suspended core call (the core's calls cancel promptly), so a late
     * answer writes nothing.
     */
    private var work: Job? = null
    private var waiting: Job? = null

    /** A code arrived: it replaces any other, and its wait. */
    fun offer(
        qr: String,
        scanned: Boolean,
    ) {
        close()
        code = qr
        this.scanned = scanned
        work =
            scope.launch {
                step =
                    try {
                        val read = read(qr)
                        device = read.name ?: AccountCopy.APPROVE_UNNAMED
                        Step.Sheet(ask(read), read.form)
                    } catch (e: CancellationException) {
                        throw e
                    } catch (_: Exception) {
                        Step.Failed(SyncCopy.NOT_A_CODE)
                    }
            }
    }

    /**
     * A code scanned here after [Step.ScanHere]. The same code goes on at once: the user already said Sign in
     * to it. Another one is a new sheet, so the name the user answers is always the scanned screen's.
     */
    fun scannedHere(qr: String) {
        if (qr != code) return offer(qr, scanned = true)
        scanned = true
        signIn()
    }

    fun signIn() = send(unlocked = false)

    /** The screen lock just passed. */
    fun unlocked() = send(unlocked = true)

    /** The undo after [Step.Done]: the device that was just signed in leaves the account. */
    fun notYou() {
        val m = member ?: return
        if (busy) return
        busy = true
        scope.launch {
            try {
                remove(m)
                step = Step.Removed
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                step = Step.Failed(SyncAccount.problemOf(e, SyncCopy.Action.Remove).words)
            } finally {
                busy = false
            }
        }
    }

    /** The sheet closed: nothing under way writes to it again, and no wait goes on. */
    fun close() {
        work?.cancel()
        work = null
        waiting?.cancel()
        waiting = null
        member = null
        busy = false
        step = Step.Idle
    }

    private fun send(unlocked: Boolean) {
        val qr = code ?: return
        if (busy) return
        busy = true
        work =
            scope.launch {
                try {
                    val joined = approve(qr, scanned, unlocked)
                    member = joined
                    step = Step.SigningIn(device)
                    waitFor(joined)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    step = failed(e)
                } finally {
                    busy = false
                }
            }
    }

    private fun waitFor(joined: String) {
        waiting?.cancel()
        waiting =
            scope.launch {
                var waited = 0L
                while (waited < waitMs) {
                    delay(pollEveryMs)
                    waited += pollEveryMs
                    val listed = runCatching { members() }.getOrNull()
                    if (listed?.contains(joined) == true) {
                        step = Step.Done(device)
                        return@launch
                    }
                }
                step = Step.Slow(device)
            }
    }

    private fun failed(e: Exception): Step =
        when ((e as? CoreError)?.code) {
            "scanRequired" -> Step.ScanHere
            "unlockRequired" -> Step.Unlock
            "codeAnswered" -> Step.Failed(AccountCopy.APPROVE_ANSWERED)
            "codeExpired" -> Step.Failed(AccountCopy.APPROVE_EXPIRED)
            else -> Step.Failed(SyncAccount.problemOf(e, SyncCopy.Action.Approve).words)
        }

    private companion object {
        fun ask(code: PairingCode): String =
            when {
                code.name != null -> AccountCopy.APPROVE_ASK.replace("{device}", code.name)
                code.form == "tv" -> AccountCopy.APPROVE_ASK_TV
                else -> AccountCopy.APPROVE_ASK_OTHER
            }
    }
}
