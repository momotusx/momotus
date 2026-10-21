package app.marquee.core.sync

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.AccountRules
import app.marquee.core.AccountRules.Act
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
 * matters. The ticket stays in memory only.
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

        /** The sign-in screen: Continue with each provider (none: today's doors), Other ways, Not now. */
        data class Start(
            val providers: List<String>,
        ) : Step

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

    /** What to do again once a new ticket came (`ticketExpired`'s Continue). */
    private var pending: (suspend (Held) -> Unit)? = null
    private var job: Job? = null

    /** Reads the relay's providers; a native Google flow is fetched at once, so its sheet opens without a wait. */
    fun start() {
        launch {
            providers = AccountRules.providerOrder(ops.relayInfo(server).providers)
            step = Step.Start(providers)
            if ("google" in providers && useNative("google")) runCatching { nativeFlow() }
        }
    }

    private fun useNative(p: String) = p == "google" && nativeGoogle && platform.google != null

    /** A fresh native flow (the relay takes one 10 minutes; a minute is left for the sheet). */
    private suspend fun nativeFlow(): NativeStart {
        native?.takeIf { it.expiresMs - now() > 60_000 }?.let { return it }
        return ops.nativeSignInStart(server, "google").also { native = it }
    }

    /** Continue with [p]. */
    fun choose(p: String) {
        pending = null
        signIn(p)
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

            GoogleAnswer.NoCredential, is GoogleAnswer.Failed -> {
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

    private fun startStep(): Step = Step.Start(providers)

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
     * another account is the QR's case, never a recovery into the wrong account).
     */
    private suspend fun recoverWith(
        used: PasskeyResult.Used,
        account: String?,
    ) {
        val prf = used.prf
        if (prf == null || (account != null && !Passkeys.sameAccount(used.userHandle, account))) {
            step = Step.Found(account ?: used.userHandle.orEmpty())
            failure = AccountRules.signInFailure("kitUnknown")
            return
        }
        try {
            sync.recover(server, kit = null, name = deviceName, prf = prf, form = form)
            step = Step.Done(created = false)
        } catch (e: CoreError) {
            if (e.code != "kitUnknown") throw e
            step = Step.Found(account ?: used.userHandle.orEmpty())
            failure = AccountRules.signInFailure("kitUnknown")
        }
    }

    /** `new`: a Marquee passkey already on this device signs in, and the identity is linked to it (4.1.4). */
    private suspend fun newHere(h: Held) {
        if (platform.rpId.isNotEmpty()) {
            step = Step.Working(h.provider)
            val got = platform.passkeyGet(Passkeys.get(platform.rpId), immediate = true)
            if (got is PasskeyResult.Used && got.prf != null) {
                try {
                    sync.recover(server, kit = null, name = deviceName, prf = got.prf, form = form)
                    sync.linkIdentity(h.ticket)
                    step = Step.Done(created = false)
                    return
                } catch (e: CoreError) {
                    if (e.code != "kitUnknown") throw e
                }
            }
        }
        step = Step.New
    }

    /** Create your Marquee account: the account with the ticket, then its passkey. */
    fun create() = withTicket { h -> createWith(h, move = false) }

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
     * joined, which links the identity with the ticket held.
     */
    fun joined() =
        withTicket { h ->
            // A `found` sign-in already leads to the account the QR joined; a `new` one is linked to it now.
            if (h.account == null) sync.linkIdentity(h.ticket)
            held = null
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
            Act.Retry -> provider?.let(::signIn)
            Act.Move -> withTicket { h -> createWith(h, move = true) }
            Act.CantUse -> startOver()
            Act.Cancel, Act.Other, Act.NotNow, Act.Anyway, Act.Scan, Act.Kit -> Unit
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
                        if (step is Step.Working) step = startStep()
                        failure = AccountRules.signInFailure(code ?: "signInRefused", provider.orEmpty())
                    }
                }
            }
    }

    private companion object {
        /** Back in front this long with no return: "Didn't finish signing in?" (the web's 1.5 s). */
        const val STALL_MS = 1_500L
    }
}
