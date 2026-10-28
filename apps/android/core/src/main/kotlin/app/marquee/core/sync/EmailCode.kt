package app.marquee.core.sync

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.AccountRules
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.EmailStart
import app.marquee.core.model.SignInSnapshot
import app.marquee.core.session.CoreOps
import kotlinx.coroutines.CancellationException

/**
 * The email screen's two stages (plans/accounts-email.md 4): the address, then the 6-digit code the relay mailed.
 * One per sign-in and one per Account's Link another. The address and the flow live here, in memory only (E12).
 * Every failure stays on the stage it came from, and Try again ([retry]) repeats that stage's own call. [run] is
 * the owner's launch, so what [taken] does with the code's `found` or `new` fails the owner's way.
 */
class EmailCode(
    private val ops: CoreOps,
    private val server: () -> String,
    private val run: (suspend () -> Unit) -> Unit,
    private val taken: suspend (SignInSnapshot) -> Unit,
    private val now: () -> Long = System::currentTimeMillis,
) {
    enum class Stage { Address, Code }

    var stage by mutableStateOf(Stage.Address)
        private set

    /** The address as typed. */
    var address by mutableStateOf("")

    /** The code field: digits only, at most six ([type]). */
    var code by mutableStateOf("")
        private set

    /** The line under the field (`badAddress`, `wrongCode`), which the field is described by. */
    var fieldError: String? by mutableStateOf(null)
        private set

    /** Any other answer, with its buttons. */
    var failure: AccountRules.Failure? by mutableStateOf(null)
        private set

    var busy by mutableStateOf(false)
        private set

    /** The code's flow ended (`codeExpired`, `emailLimit`): the field stays, read-only, until a new code. */
    var ended by mutableStateOf(false)
        private set

    /** The last Send a new code worked: `codeResent` shows. */
    var resent by mutableStateOf(false)
        private set

    /** The address the newest code went to, for `codeSentTo`. */
    var sentTo by mutableStateOf("")
        private set

    /** When the newest code was mailed: Send a new code waits 60 s from it, time in the background included. */
    var sentAt by mutableLongStateOf(0L)
        private set

    /** Counts the lines under the field, so the field takes the focus again on each, the same words included. */
    var said by mutableIntStateOf(0)
        private set

    private var flow: EmailStart? = null
    private var again: (() -> Unit)? = null

    /**
     * Moves on with each [open] and [close]: an answer to a call made before is not acted on, so Back or Cancel
     * while a code is checked never signs in or links behind the user's back.
     */
    private var turn = 0

    /** The screen left (Back from the address stage, Cancel, Other ways): an answer still on its way is dropped. */
    fun close() {
        turn++
        busy = false
    }

    /** The address stage, the address kept (Back, Use another address). */
    fun open() {
        turn++
        busy = false
        resent = false
        stage = Stage.Address
        code = ""
        fieldError = null
        failure = null
        ended = false
    }

    /** Send code: the same address while its flow is open goes back to the code stage without a new mail. */
    fun send() {
        if (busy) return
        val open = flow?.takeIf { sentTo == address.trim() && it.expiresMs > now() }
        if (open != null) {
            failure = null
            fieldError = null
            toCode()
        } else {
            mail(resent = false)
        }
    }

    /** Send a new code. */
    fun resend() = mail(resent = true)

    /**
     * A held ticket ran out ("Sign in again to finish"): the code stage with the address in memory and a new code;
     * with no address left (the app restarted), the address stage, empty.
     */
    fun reopen() {
        if (address.isBlank()) return open()
        toCode()
        mail(resent = true)
    }

    private fun mail(resent: Boolean) {
        val typed = address.trim()
        this.resent = false
        call(submitting = false) { mine ->
            val started = ops.emailSignInStart(server(), typed)
            // The newest flow is kept even when the screen moved on: its code went out.
            flow = started
            sentTo = typed
            sentAt = now()
            if (mine()) {
                this.resent = resent
                toCode()
            }
            null
        }
    }

    private fun toCode() {
        stage = Stage.Code
        code = ""
        ended = false
    }

    /** What the code field got: digits only (a pasted `123 456` works), cut at 6, sent once at the sixth. */
    fun type(input: String) {
        if (busy || ended) return
        val digits = input.filter { it in '0'..'9' }.take(CODE_LENGTH)
        if (digits == code) return
        code = digits
        if (digits.length == CODE_LENGTH) submit()
    }

    /** The code, at the sixth digit or by Go; a code short of six says `wrongCode`, a used flow `codeExpired`. */
    fun submit() {
        if (busy || ended) return
        resent = false
        if (code.length != CODE_LENGTH) return failed("badCode", submitting = true)
        val f = flow ?: return failed("wrongFlow", submitting = true)
        val typed = code
        call(submitting = true) { ops.emailSignIn(f.flow, typed).also { flow = null } }
    }

    /** Try again: the last call of this screen again, the same address or the same code. */
    fun retry() {
        again?.invoke()
    }

    /** Back: the code stage goes to the address stage, prefilled; false on the address stage (the owner's Back). */
    fun back(): Boolean {
        if (stage != Stage.Code) return false
        open()
        return true
    }

    /** Seconds until Send a new code turns on. */
    fun resendIn(): Int = ((sentAt + RESEND_MS - now()).coerceIn(0, RESEND_MS) + 999).toInt() / 1000

    /**
     * Runs [work] once at a time; its failure, and one from what [taken] does with its answer, stays on this stage.
     * [work] is told whether its answer is still wanted ([turn]).
     */
    private fun call(
        submitting: Boolean,
        work: suspend (mine: () -> Boolean) -> SignInSnapshot?,
    ) {
        if (busy) return
        again = { call(submitting, work) }
        failure = null
        fieldError = null
        busy = true
        val at = turn
        val mine = { turn == at }
        run {
            val got =
                try {
                    work(mine)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    if (mine()) failed((e as? CoreError)?.code, submitting)
                    null
                } finally {
                    if (mine()) busy = false
                }
            if (got == null || !mine()) return@run
            try {
                taken(got)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // What came after the code failed: its words show here, and the owner comes back to this screen.
                failed((e as? CoreError)?.code, submitting)
                throw e
            }
        }
    }

    private fun failed(
        code: String?,
        submitting: Boolean,
    ) {
        CoreLog.w("email sign-in: ${code ?: "no answer"}")
        val line = AccountRules.emailFieldError(code)
        if (line != null) {
            fieldError = line
            said++
            if (stage == Stage.Code) this.code = ""
            return
        }
        failure = AccountRules.emailFailure(code)
        // Only an answer to the code ends its flow: a refused Send a new code leaves the code mailed before.
        if (submitting && stage == Stage.Code && AccountRules.emailEnds(code)) {
            ended = true
            flow = null
        }
    }

    private companion object {
        const val CODE_LENGTH = 6
        const val RESEND_MS = 60_000L
    }
}
