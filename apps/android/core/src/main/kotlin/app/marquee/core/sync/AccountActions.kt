package app.marquee.core.sync

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.AccountCopy
import app.marquee.core.AccountRules
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.model.IdentityLink
import app.marquee.core.model.SignInSnapshot
import app.marquee.core.session.CoreOps
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

/**
 * The Account page's account rows (plans/accounts.md 4.3, D15 to D17): Signed in with (link another, unlink,
 * with Apple's revoke first for an Apple link), Save a passkey here, and Delete account with Apple's revoke
 * receipt or Delete anyway. The rows show only where [AccountRules.rows] says; the core answers `notOwner`
 * otherwise. The OS authentication prompt comes before [unlink] and [delete] (the screen's).
 */
class AccountActions(
    private val ops: CoreOps,
    private val sync: SyncAccount,
    private val scope: CoroutineScope,
    private val platform: SignInPlatform,
    private val server: () -> String,
    private val deviceName: String,
    private val ownerName: suspend () -> String,
    /** Opens the core's `signIn` view (or `appleRevoke` with revoke) for a brokered run. */
    private val brokered: (provider: String, revoke: Boolean, open: (String) -> Unit) -> BrokeredRun,
) {
    var links: List<IdentityLink>? by mutableStateOf(null)
        private set

    var providers: List<String> by mutableStateOf(emptyList())
        private set

    var busy by mutableStateOf(false)
        private set

    /**
     * A line the last action left (why not), with its buttons, and the step its Anyway or Move runs: one state, so
     * the button never shows before its step is there.
     */
    private class Asked(
        val failure: AccountRules.Failure,
        val then: (suspend () -> Unit)?,
    )

    private var asked: Asked? by mutableStateOf(null)

    val failure: AccountRules.Failure? get() = asked?.failure

    var said: String? by mutableStateOf(null)
        private set

    /** The last action was Delete account: its [failure] and [said] show under it, the others under Signed in with. */
    var deleting by mutableStateOf(false)
        private set

    @Volatile
    var run: BrokeredRun? = null
        private set

    fun load() =
        go {
            providers = AccountRules.providerOrder(ops.relayInfo(server()).providers)
            links = sync.identities()
        }

    /** Link another: the provider's sign-in, then its identity is linked here; one linked elsewhere offers to move it. */
    fun link(provider: String) =
        go {
            val end = brokeredRun(provider, revoke = false)
            val ticket =
                when (end) {
                    is SignInSnapshot.Found -> end.ticket
                    is SignInSnapshot.New -> end.ticket
                    is SignInSnapshot.Failed -> return@go fail(end.code, provider)
                    else -> return@go
                }
            try {
                sync.linkIdentity(ticket)
            } catch (e: CoreError) {
                if (e.code != "linkedElsewhere") throw e
                ask(AccountRules.signInFailure("linkedElsewhere", provider)) { sync.linkIdentity(ticket, move = true) }
                return@go
            }
            links = sync.identities()
        }

    /**
     * Unlinks [link]; an Apple link runs Apple's revoke first and sends its receipt, and without one goes on only
     * on Unlink anyway. Unlinking the last link says how sign-in works then.
     */
    fun unlink(link: IdentityLink) =
        go {
            val last = links?.size == 1
            if (link.provider == "apple") {
                val receipt = revoke(AccountCopy.UNLINK_ANYWAY) { finishUnlink(link, null, last) } ?: return@go
                finishUnlink(link, receipt, last)
            } else {
                finishUnlink(link, null, last)
            }
        }

    private suspend fun finishUnlink(
        link: IdentityLink,
        receipt: String?,
        last: Boolean,
    ) {
        sync.unlinkIdentity(link.link, receipt)
        links = sync.identities()
        if (last) said = AccountCopy.UNLINK_LAST
    }

    /**
     * Deletes the account for every device; with an Apple link, Apple's revoke first and its receipt, else on
     * Delete anyway. True once the sync server confirmed.
     */
    fun delete(onDone: () -> Unit) =
        go(deleting = true) {
            if (links.orEmpty().any { it.provider == "apple" }) {
                val receipt =
                    revoke(AccountCopy.DELETE_ANYWAY) {
                        sync.deleteAccount(null)
                        onDone()
                    } ?: return@go
                sync.deleteAccount(receipt)
            } else {
                sync.deleteAccount(null)
            }
            onDone()
        }

    /**
     * Apple's revoke: its receipt, or null with [failure] set and [anywayStep] waiting for its [anyway] button
     * (nothing for a cancelled page). Both come in one state ([Asked]), so the button always has its step.
     */
    private suspend fun revoke(
        anyway: String,
        anywayStep: suspend () -> Unit,
    ): String? {
        val end = brokeredRun("apple", revoke = true)
        if (end is SignInSnapshot.Receipt) return end.receipt
        val code = (end as? SignInSnapshot.Failed)?.code ?: "signInRefused"
        AccountRules.revokeFailure(code, anyway, CANCEL)?.let { ask(it, anywayStep) }
        return null
    }

    /** The Anyway or Move under [failure]. */
    fun goOn() {
        val next = asked?.then ?: return
        asked = null
        go(deleting) { next() }
    }

    fun dismiss() {
        asked = null
        said = null
    }

    /**
     * Save a passkey here (3.1): an immediate get first, and when that passkey already opens the account the
     * core says `kitHere` and nothing is saved; else create, and with no PRF result one get for the new id.
     * [probe] false after a passkey was dropped (`kitDropped`): straight to create.
     */
    fun savePasskeyHere(probe: Boolean = true) =
        go {
            if (platform.rpId.isEmpty()) return@go
            if (probe) {
                val got = platform.passkeyGet(Passkeys.get(platform.rpId), immediate = true)
                if (got is PasskeyResult.Used && got.prf != null) {
                    if (save(got.prf)) return@go
                }
            }
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
                ask(
                    if (made is PasskeyResult.Cancelled) {
                        AccountRules.signInFailure("passkeyCancelled")
                    } else {
                        AccountRules.Failure(AccountCopy.NO_SECOND_WAY_IN, emptyList())
                    },
                )
                return@go
            }
            save(prf)
        }

    /** True when the save is settled (saved, or the passkey already works here). */
    private suspend fun save(prf: String): Boolean =
        try {
            sync.savePasskey(prf, AccountRules.fill(AccountCopy.PASSKEY_LABEL, mapOf("device" to deviceName)))
            true
        } catch (e: CoreError) {
            when (e.code) {
                "kitHere" -> {
                    said = AccountCopy.KIT_HERE
                    true
                }

                "kitDropped", "kitTaken", "tooManyKits", "plainLink", "notOwner" -> {
                    said = KIT_WORDS.getValue(e.code!!)
                    e.code != "kitDropped"
                }

                else -> {
                    throw e
                }
            }
        }

    private suspend fun brokeredRun(
        provider: String,
        revoke: Boolean,
    ): SignInSnapshot {
        val r = brokered(provider, revoke, platform::openBrowser)
        run = r
        return try {
            r.end.await()
        } finally {
            if (run === r) run = null
        }
    }

    private fun fail(
        code: String,
        provider: String,
    ) = ask(AccountRules.signInFailure(code, provider))

    private fun ask(
        failure: AccountRules.Failure?,
        then: (suspend () -> Unit)? = null,
    ) {
        asked = failure?.let { Asked(it, then) }
    }

    private fun go(
        deleting: Boolean = false,
        block: suspend () -> Unit,
    ) {
        this.deleting = deleting
        scope.launch {
            busy = true
            said = null
            try {
                block()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                val code = (e as? CoreError)?.code
                CoreLog.w("account: ${code ?: e.javaClass.simpleName}")
                said = code?.let(KIT_WORDS::get) ?: AccountRules.signInFailure(code ?: "signInRefused")?.text
            } finally {
                busy = false
            }
        }
    }

    private companion object {
        /** The shared words' "Cancel" (`linkedElsewhereActions`). */
        val CANCEL = AccountCopy.LINKED_ELSEWHERE_ACTIONS[1]

        val KIT_WORDS =
            mapOf(
                "kitDropped" to AccountCopy.KIT_DROPPED,
                "kitTaken" to AccountCopy.KIT_TAKEN,
                "tooManyKits" to AccountCopy.TOO_MANY_KITS,
                "plainLink" to AccountCopy.PLAIN_LINK,
                "notOwner" to AccountCopy.NOT_OWNER,
            )
    }
}
