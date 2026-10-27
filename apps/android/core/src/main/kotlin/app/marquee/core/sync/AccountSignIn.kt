package app.marquee.core.sync

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.AccountRules
import app.marquee.core.AccountRules.Act
import app.marquee.core.AccountRules.EMAIL
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.NativeStart
import app.marquee.core.model.SignInSnapshot
import app.marquee.core.session.CoreOps
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * A phone's sign-in (plans/accounts.md 4.1, 5): the providers the relay offers, Google through one Credential
 * Manager sheet with the account passkey in it, the others through the relay's brokered flow, then `found` or
 * `new`. A found account opens with a passkey whose `user.id` is that account, else the pairing QR, a recovery
 * kit or Start over; a new one is created with the ticket and gets a passkey, or the step that says why one
 * matters. Email (plans/accounts-email.md 4) is its own row and screen, [email], whose code ends in the same
 * `found` or `new`. The ticket stays in memory only.
 */
class AccountSignIn(
    private val ops: CoreOps,
    private val sync: SyncAccount,
    private val scope: CoroutineScope,
    private val platform: SignInPlatform,
    private val server: String,
    private val deviceName: String,
    /** `phone` or `tablet`: sealed in the roster, so this device can approve others. */
    private val form: String,
    /** The owner profile's name, for the passkey's `user.name`. */
    private val ownerName: suspend () -> String,
    /** Opens the core's `signIn` view for a brokered run. */
    private val brokered: (provider: String, open: (String) -> Unit) -> BrokeredRun,
    /** Google's own sheet only against the publisher's relay; another relay offers Google brokered (4.4). */
    private val nativeGoogle: Boolean,
    private val now: () -> Long = System::currentTimeMillis,
) {
    sealed interface Step {
        data object Loading : Step

        /**
         * The sign-in screen: Continue with each provider, Continue with email when the relay offers it (primary
         * when it is the only way), Other ways, Not now; with neither, today's doors.
         */
        data class Start(
            val providers: List<String>,
            val email: Boolean = false,
        ) : Step

        /** The email screen: [email]'s address and code stages. */
        data object Email : Step

        data class Working(
            val provider: String?,
        ) : Step

        /** The sign-in has an account and no passkey here opened it: the QR, a recovery kit, or Start over. */
        data class Found(
            val account: String,
        ) : Step

        /** No account yet: Create your Marquee account, or I already use Marquee on another device. */
        data object New : Step

        /** "Your old library can't be opened..." before Start over moves the sign-in to a new account. */
        data object StartOverAsk : Step

        /** Created without a passkey: Try again, Use a recovery kit instead, Later (4.1.6). */
        data object SecondWay : Step

        /** In: [created] goes on to the first-run steps, else Home after the first pull. */
        data class Done(
            val created: Boolean,
        ) : Step
    }

    /** A sign-in's result, held for the next screen in memory only. */
    data class Held(
        val provider: String,
        val ticket: String,
        val account: String?,
    ) {
        override fun toString() = "Held(provider=$provider, ticket=hidden)"
    }

    var step: Step by mutableStateOf(Step.Loading)
        private set

    /** What the last attempt said (section 5), with its buttons; null when nothing needs saying. */
    var failure: AccountRules.Failure? by mutableStateOf(null)
        private set

    var held: Held? by mutableStateOf(null)
        private set

    /** The brokered run waiting for its return, if any: the app hands it the auth session's redirect. */
    @Volatile
    var run: BrokeredRun? = null
        private set

    private var native: NativeStart? = null
    private var provider: String? = null
    private var emailOffered = false

    /** The email screen's stages; the address stays here, in memory, for a held ticket's Continue. */
    val email = EmailCode(ops, { server }, { work -> launch(work) }, { end(EMAIL, it) }, now)

    /** What to do again once a new ticket came (`ticketExpired`'s Continue). */
    private var pending: (suspend (Held) -> Unit)? = null
    private var job: Job? = null

    /** Reads the relay's providers; a native Google flow is fetched at once, so its sheet opens without a wait. */
    fun start() {
        // A new step each time, so Try again after an unreachable relay is seen as one.
        step = Step.Loading
        launch {
            val offered = ops.relayInfo(server).providers
            providers = AccountRules.providerOrder(offered)
            emailOffered = EMAIL in offered
            step = startStep()
            if ("google" in providers && useNative("google")) runCatching { nativeFlow() }
        }
    }

    private fun useNative(p: String) = p == "google" && nativeGoogle && platform.google != null

    /** A fresh native flow (the relay takes one 10 minutes; a minute is left for the sheet). */
    private suspend fun nativeFlow(): NativeStart {
        native?.takeIf { it.expiresMs - now() > 60_000 }?.let { return it }
        return ops.nativeSignInStart(server, "google").also { native = it }
    }

    /** Continue with [p], or with email. */
    fun choose(p: String) {
        pending = null
        if (p == EMAIL) openEmail(again = false) else signIn(p)
    }

    /** The email screen; [again] (a held ticket's Continue) at the code stage with a new code. */
    private fun openEmail(again: Boolean) {
        provider = EMAIL
        failure = null
        step = Step.Email
        if (again) email.reopen() else email.open()
    }

    /** Back on the email screen: the code stage to the address stage, the address stage to Sign in. */
    fun back() {
        if (step != Step.Email) return
        failure = null
        if (!email.back()) leaveEmail()
    }

    /** Sign in again, out of the email screen: an answer still on its way is dropped, and Try again is not email's. */
    private fun leaveEmail() {
        email.close()
        provider = null
        step = startStep()
    }

    private fun signIn(p: String) {
        provider = p
        failure = null
        launch { end(p, if (useNative(p)) nativeSignIn() else brokeredSignIn(p)) }
    }

    private suspend fun nativeSignIn(): SignInSnapshot? {
        step = Step.Working("google")
        val flow = nativeFlow()
        val passkey = platform.rpId.takeIf { it.isNotEmpty() }?.let { Passkeys.get(it) }
        return when (val answer = checkNotNull(platform.google).signIn(flow.nonce, passkey)) {
            is GoogleAnswer.IdToken -> {
                native = null
                ops.nativeSignIn(flow.flow, answer.token)
            }

            // The account passkey, picked in Google's sheet: one sheet, one fingerprint, Home (4.1.2).
            is GoogleAnswer.Passkey -> {
                recoverWith(answer.used, account = null)
                null
            }

            GoogleAnswer.Cancelled -> {
                SignInSnapshot.Failed("cancelled")
            }

            // No Google account on this phone: the browser signs in to one.
            GoogleAnswer.NoCredential -> {
                brokeredSignIn("google")
            }

            is GoogleAnswer.Failed -> {
                SignInSnapshot.Failed("signInRefused")
            }
        }
    }

    private suspend fun brokeredSignIn(p: String): SignInSnapshot {
        step = Step.Working(p)
        val r = brokered(p, platform::openBrowser)
        run = r
        return try {
            r.end.await()
        } finally {
            if (run === r) run = null
        }
    }

    /** The app is in front again: a provider page left without a return says so, and the run keeps waiting. */
    fun resumed() {
        val r = run ?: return
        scope.launch {
            delay(STALL_MS)
            if (run === r && r.waitingForReturn) failure = AccountRules.signInFailure("noRedirect", provider.orEmpty())
        }
    }

    private suspend fun end(
        p: String,
        result: SignInSnapshot?,
    ) {
        when (result) {
            null -> {
                Unit
            }

            is SignInSnapshot.Found -> {
                val h = Held(p, result.ticket, result.account)
                held = h
                pending?.let { again ->
                    pending = null
                    return again(h)
                }
                foundHere(h)
            }

            is SignInSnapshot.New -> {
                val h = Held(p, result.ticket, null)
                held = h
                pending?.let { again ->
                    pending = null
                    return again(h)
                }
                newHere(h)
            }

            is SignInSnapshot.Failed -> {
                step = startStep()
                failure = AccountRules.signInFailure(result.code, p)
            }

            else -> {
                step = startStep()
            }
        }
    }

    private fun startStep(): Step = Step.Start(providers, emailOffered)

    private var providers: List<String> = emptyList()

    /** `found`: a passkey on this device opens it when its `user.id` is that account (4.1.3). */
    private suspend fun foundHere(h: Held) {
        val account = checkNotNull(h.account)
        if (platform.rpId.isEmpty()) {
            step = Step.Found(account)
            failure = AccountRules.signInFailure("kitUnknown")
            return
        }
        step = Step.Working(h.provider)
        when (val got = platform.passkeyGet(Passkeys.get(platform.rpId), immediate = true)) {
            is PasskeyResult.Used -> {
                recoverWith(got, account)
            }

            PasskeyResult.Cancelled -> {
                step = Step.Found(account)
                failure = AccountRules.signInFailure("passkeyCancelled")
            }

            else -> {
                step = Step.Found(account)
                failure = AccountRules.signInFailure("kitUnknown")
            }
        }
    }

    /**
     * A passkey's PRF output opens an account: only the [account] the sign-in found, when one did (a passkey of
     * another account is the QR's case, never a recovery into the wrong account). One that opens nothing goes to
     * the found account's QR; with no sign-in held (Use a passkey, Google's sheet) to the first step, since there
     * is no ticket to join or start over with.
     */
    private suspend fun recoverWith(
        used: PasskeyResult.Used,
        account: String?,
    ) {
        val prf = used.prf
        if (prf != null && (account == null || Passkeys.sameAccount(used.userHandle, account))) {
            try {
                sync.recover(server, kit = null, name = deviceName, prf = prf, form = form)
                step = Step.Done(created = false)
                return
            } catch (e: CoreError) {
                if (e.code != "kitUnknown") throw e
            }
        }
        if (account != null) {
            step = Step.Found(account)
            failure = AccountRules.signInFailure("kitUnknown")
        } else {
            step = startStep()
            failure = unknownPasskey()
        }
    }

    /**
     * Other ways: Use a passkey. It signs in only (D10): any of the link host's passkeys, picked in the sheet,
     * opens the account it belongs to; one no account knows is `kitUnknown`, a cancelled sheet says so.
     */
    fun usePasskey() =
        launch {
            failure = null
            if (platform.rpId.isEmpty()) return@launch
            step = Step.Working(null)
            when (val got = platform.passkeyGet(Passkeys.get(platform.rpId), immediate = false)) {
                is PasskeyResult.Used -> {
                    recoverWith(got, account = null)
                }

                PasskeyResult.Cancelled -> {
                    step = startStep()
                    failure = AccountRules.signInFailure("passkeyCancelled")
                }

                else -> {
                    step = startStep()
                    failure = unknownPasskey()
                }
            }
        }

    /** No held sign-in to start over from: a recovery kit, or the QR from a signed-in device. */
    private fun unknownPasskey() =
        AccountRules.signInFailure("kitUnknown")?.let { f ->
            f.copy(
                actions =
                    f.actions.filter { it.act != Act.CantUse } +
                        AccountRules.Action(app.marquee.core.AccountCopy.SCAN_FROM_DEVICE, Act.Scan),
            )
        }

    /** `new`: a Marquee passkey already on this device signs in, and the identity is linked to it (4.1.4). */
    private suspend fun newHere(h: Held) {
        if (platform.rpId.isNotEmpty()) {
            step = Step.Working(h.provider)
            val got = platform.passkeyGet(Passkeys.get(platform.rpId), immediate = true)
            if (got is PasskeyResult.Used && got.prf != null) {
                try {
                    sync.recover(server, kit = null, name = deviceName, prf = got.prf, form = form)
                } catch (e: CoreError) {
                    if (e.code != "kitUnknown") throw e
                    step = Step.New
                    return
                }
                linkHeld(h)
                step = Step.Done(created = false)
                return
            }
        }
        step = Step.New
    }

    /**
     * Links a held sign-in to the account this device just joined. The join stands whatever the link answers: one
     * that another account holds waits on Account with "Use it for this account instead" ([SyncAccount.heldLink]);
     * anything else is Account's Link another.
     */
    private suspend fun linkHeld(h: Held) {
        try {
            sync.linkIdentity(h.ticket)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            val code = (e as? CoreError)?.code
            CoreLog.w("sign-in: the link after the join: ${code ?: e.javaClass.simpleName}")
            if (code == "linkedElsewhere") sync.heldLink = SyncAccount.HeldLink(h.provider, h.ticket)
        }
    }

    /**
     * Continue on a sync service that lists no provider (plans/tv-qr-login.md 5.4): a passkey on this device
     * opens its account; none, or one no account knows, is `new`, whose Create needs no sign-in there.
     */
    fun continueHere() =
        launch {
            failure = null
            if (platform.rpId.isNotEmpty()) {
                step = Step.Working(null)
                val got = platform.passkeyGet(Passkeys.get(platform.rpId), immediate = true)
                if (got is PasskeyResult.Used && got.prf != null) {
                    try {
                        sync.recover(server, kit = null, name = deviceName, prf = got.prf, form = form)
                        step = Step.Done(created = false)
                        return@launch
                    } catch (e: CoreError) {
                        if (e.code != "kitUnknown") throw e
                    }
                }
            }
            step = Step.New
        }

    /** Create your Momotus account: with the ticket a sign-in brought, else the network's sign-up; then its passkey. */
    fun create() =
        if (held != null) {
            withTicket { h -> createWith(h, move = false) }
        } else {
            launch {
                step = Step.Working(null)
                sync.createAccount(server, "", deviceName, form = form)
                savePasskey()
            }
        }

    private suspend fun createWith(
        h: Held,
        move: Boolean,
    ) {
        step = Step.Working(h.provider)
        try {
            sync.createAccount(server, "", deviceName, ticket = h.ticket, move = move, form = form)
        } catch (e: CoreError) {
            // The sign-in has an account by now: the found screen, whose Start over warns first.
            if (e.code == "linkedElsewhere" && !move) {
                step = Step.Found(h.account.orEmpty())
                failure = AccountRules.signInFailure("kitUnknown")
                return
            }
            throw e
        }
        held = null
        savePasskey()
    }

    /** The new account's passkey (3.1): create without `excludeCredentials`; no PRF result, one get for that id. */
    fun savePasskeyAgain() = launch { savePasskey() }

    private suspend fun savePasskey() {
        if (platform.rpId.isEmpty()) {
            step = Step.SecondWay
            return
        }
        step = Step.Working(null)
        val account = sync.loadDevices().account
        val made = platform.passkeyCreate(Passkeys.create(platform.rpId, account, ownerName()))
        val prf =
            (made as? PasskeyResult.Used)?.let { used ->
                used.prf
                    ?: (
                        platform.passkeyGet(
                            Passkeys.get(platform.rpId, listOf(used.credentialId)),
                            false,
                        ) as? PasskeyResult.Used
                    )?.prf
            }
        if (prf == null) {
            step = Step.SecondWay
            return
        }
        sync.savePasskey(
            prf,
            AccountRules.fill(app.marquee.core.AccountCopy.PASSKEY_LABEL, mapOf("device" to deviceName)),
        )
        step = Step.Done(created = true)
    }

    /** Later, on the second-way step: in, with the nudge on Home and Account until a second way exists. */
    fun later() {
        step = Step.Done(created = true)
    }

    /**
     * I already use Marquee on another device: the app opens the pairing QR and calls [joined] once this device
     * joined, which links the identity with the ticket held. Joined with no sign-in held (a passkey no account
     * knew, then the QR), it is in all the same.
     */
    fun joined() =
        launch {
            val h = held
            held = null
            // A `found` sign-in already leads to the account the QR joined; a `new` one is linked to it now.
            if (h != null && h.account == null) linkHeld(h)
            step = Step.Done(created = false)
        }

    /** I can't use any of these: the warning first. */
    fun startOver() {
        failure = null
        step = Step.StartOverAsk
    }

    fun confirmStartOver() = withTicket { h -> createWith(h, move = true) }

    /** A button under [failure]; the screen itself opens what Other, Not now, Scan and Kit lead to. */
    fun act(a: Act) {
        failure = null
        when (a) {
            // An email stage repeats its own call; after an email sign-in (a held ticket's Continue, a failed step
            // after the code) the email screen again, with a new code when work waits for a ticket; no provider
            // chosen yet: the relay did not answer, so it is asked again.
            Act.Retry -> {
                when {
                    step == Step.Email -> email.retry()
                    provider == EMAIL -> openEmail(again = pending != null)
                    else -> provider?.let(::signIn) ?: start()
                }
            }

            Act.Resend -> {
                email.resend()
            }

            Act.OtherAddress -> {
                email.open()
            }

            // Other ways to sign in, from the email screen: back to Sign in.
            Act.Other -> {
                if (step == Step.Email) leaveEmail()
            }

            Act.Move -> {
                withTicket { h -> createWith(h, move = true) }
            }

            Act.CantUse -> {
                startOver()
            }

            // Cancel on the Start over warning: back to the found account's QR and its buttons.
            Act.Cancel -> {
                val h = held
                if (step == Step.StartOverAsk && h != null) {
                    step = Step.Found(h.account.orEmpty())
                    failure = AccountRules.signInFailure("kitUnknown")
                }
            }

            Act.NotNow, Act.Anyway, Act.Scan, Act.Kit -> {
                Unit
            }
        }
    }

    /**
     * Runs [use] with the held ticket; a ticket past its 10 minutes says "Sign in again to finish." and its
     * Continue signs in again, then does [use] with the new one.
     */
    private fun withTicket(use: suspend (Held) -> Unit) =
        launch {
            val h = held ?: return@launch
            try {
                use(h)
            } catch (e: CoreError) {
                if (e.code != "ticketExpired") throw e
                pending = use
                step = startStep()
                failure = AccountRules.signInFailure("ticketExpired", h.provider)
            }
        }

    private fun launch(block: suspend () -> Unit) {
        val from = step
        job =
            scope.launch {
                try {
                    block()
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    val code = (e as? CoreError)?.code
                    CoreLog.w("sign-in: ${code ?: e.javaClass.simpleName}")
                    if (step is Step.Loading) {
                        step = Step.Start(emptyList())
                        failure = AccountRules.signInFailure("unreachable")
                    } else {
                        // An email sign-in goes back to the step it came from, never to the provider list; back on
                        // the email screen, the words are under its field already.
                        if (step is Step.Working) step = if (provider == EMAIL && from.settled()) from else startStep()
                        if (step != Step.Email) {
                            failure = AccountRules.signInFailure(code ?: "signInRefused", provider.orEmpty())
                        }
                    }
                }
            }
    }

    private fun Step.settled() = this !is Step.Loading && this !is Step.Working

    private companion object {
        /** Back in front this long with no return: "Didn't finish signing in?" (the web's 1.5 s). */
        const val STALL_MS = 1_500L
    }
}
