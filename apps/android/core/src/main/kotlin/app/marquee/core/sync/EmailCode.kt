package app.marquee.core.sync

import androidx.compose.runtime.getValue
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

    private var flow: EmailStart? = null
    private var again: (() -> Unit)? = null

    /** The address stage, the address kept (Back, Use another address). */
    fun open() {
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
        call {
            flow = ops.emailSignInStart(server(), typed)
            sentTo = typed
            sentAt = now()
            this.resent = resent
            toCode()
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

    /** The code, at the sixth digit or by Go; a call under way or a code short of six sends nothing. */
    fun submit() {
        val f = flow ?: return
        if (busy || ended || code.length != CODE_LENGTH) return
        val typed = code
        call { ops.emailSignIn(f.flow, typed).also { flow = null } }
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
    fun resendIn(): Int = ((sentAt + RESEND_MS - now()).coerceAtLeast(0) + 999).toInt() / 1000

    private fun call(work: suspend () -> SignInSnapshot?) {
        if (busy) return
        again = { call(work) }
        failure = null
        fieldError = null
        busy = true
        run {
            val got =
                try {
                    work()
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    failed((e as? CoreError)?.code)
                    null
                } finally {
                    busy = false
                }
            got?.let { taken(it) }
        }
    }

    private fun failed(code: String?) {
        CoreLog.w("email sign-in: ${code ?: "no answer"}")
        val line = AccountRules.emailFieldError(code)
        if (line != null) {
            fieldError = line
            if (stage == Stage.Code) this.code = ""
            return
        }
        failure = AccountRules.emailFailure(code)
        if (stage == Stage.Code && AccountRules.emailEnds(code)) {
            ended = true
            flow = null
        }
    }

    private companion object {
        const val CODE_LENGTH = 6
        const val RESEND_MS = 60_000L
    }
}
