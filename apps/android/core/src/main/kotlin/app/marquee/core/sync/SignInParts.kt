package app.marquee.core.sync

import app.marquee.core.AccountRules
import app.marquee.core.model.SignInSnapshot
import app.marquee.core.views.SignInView
import kotlinx.coroutines.CompletableDeferred
import java.net.URI

/** The platform's pieces of a sign-in (plans/accounts.md 4.4), apart so the flows can be tested without them. */
interface SignInPlatform {
    /** Opens the provider's page: an Auth Tab where Chrome has one, else a Custom Tab. The return comes to [BrokeredRun.returned]. */
    fun openBrowser(url: String)

    /** A passkey assertion ([request]: [Passkeys.get]); [immediate]: only a passkey already on the device, no other sheet. */
    suspend fun passkeyGet(
        request: String,
        immediate: Boolean,
    ): PasskeyResult

    /** A new passkey ([request]: [Passkeys.create]). */
    suspend fun passkeyCreate(request: String): PasskeyResult

    /** Google through Credential Manager with a passkey in the same sheet (4.1.2); null in a build without Google's client. */
    val google: GoogleSheet?

    /** The link host, the passkeys' relying party; empty in a build without one (no passkeys then). */
    val rpId: String
}

/** One Credential Manager sheet: Google's ID token for [nonce], or the account passkey picked instead. */
interface GoogleSheet {
    suspend fun signIn(
        nonce: String,
        passkeyRequest: String?,
    ): GoogleAnswer
}

sealed interface GoogleAnswer {
    data class IdToken(
        val token: String,
    ) : GoogleAnswer {
        override fun toString() = "IdToken(hidden)"
    }

    data class Passkey(
        val used: PasskeyResult.Used,
    ) : GoogleAnswer

    data object Cancelled : GoogleAnswer

    /** No Google account (and no passkey) on the device to offer. */
    data object NoCredential : GoogleAnswer

    data class Failed(
        val reason: String,
    ) : GoogleAnswer
}

/**
 * One brokered sign-in (or Apple's revoke) in the core's `signIn` view: the provider page opens once, and the
 * redirect is handed over only while this run waits, only from `marquee://auth`, and only when it names this
 * relay. The core takes only its own flow's return as well, so a stray link ends nothing.
 */
class BrokeredRun(
    private val server: String,
    private val open: (String) -> Unit,
    view: ((SignInSnapshot) -> Unit) -> SignInView,
) {
    /** How it ended: `found`, `new`, `receipt` or `failed` (with `unreachable` when the view could not open). */
    val end = CompletableDeferred<SignInSnapshot>()

    @Volatile
    private var shown = false

    /** The provider page was opened and no return came yet: "Didn't finish signing in?" may show. */
    val waitingForReturn: Boolean get() = shown && !returnedOnce && !end.isCompleted

    @Volatile
    private var returnedOnce = false

    private val signIn: SignInView =
        view { phase ->
            when (phase) {
                is SignInSnapshot.Browser -> {
                    if (!shown) {
                        shown = true
                        open(phase.url)
                    }
                }

                SignInSnapshot.Starting, SignInSnapshot.Waiting -> {
                    Unit
                }

                else -> {
                    finish(phase)
                }
            }
        }

    init {
        signIn.opened.invokeOnCompletion { cause -> if (cause != null) finish(SignInSnapshot.Failed("unreachable")) }
    }

    /**
     * The app's auth session came back with [uri] (`marquee://auth#...`). True when it was this run's to take:
     * it waits, the link is the app's own return, and it names this relay.
     */
    fun returned(uri: String): Boolean {
        if (end.isCompleted || !shown) return false
        val parsed = runCatching { URI(uri) }.getOrNull() ?: return false
        if (parsed.scheme != "marquee" || parsed.host != "auth") return false
        val fragment = parsed.rawFragment ?: return false
        if (!AccountRules.sameRelay(fragment, server)) return false
        returnedOnce = true
        signIn.returned(fragment)
        return true
    }

    /** The provider page was left without a return (an Auth Tab reports it): the run ends as cancelled. */
    fun cancel() = finish(SignInSnapshot.Failed("cancelled"))

    private fun finish(phase: SignInSnapshot) {
        if (end.complete(phase)) signIn.close()
    }
}
