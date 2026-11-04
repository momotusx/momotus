package app.marquee.android

import android.content.Context
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.AccountRules
import app.marquee.core.sync.AccountActions
import app.marquee.core.sync.AccountSignIn
import app.marquee.core.sync.BrokeredRun
import app.marquee.core.sync.LibraryRemoval
import app.marquee.core.sync.PairingApproval
import app.marquee.core.sync.SyncAccount

/**
 * The account flows for the app's life (plans/accounts.md 4.1 to 4.3): one sign-in at a time (it waits through
 * the pairing screen for "I already use Marquee"), the Account page's actions and D13's removal. The ticket a
 * sign-in brings lives in memory only, in [signIn].
 */
class AccountFlows(
    private val app: MarqueeApp,
) {
    private val platform get() = app.accountPlatform

    /**
     * The sync service: the stored record's while this device is joined (a relay it followed to a new name
     * included); otherwise the build's hosted default, so a relay moved to a new name is signed in to there (a
     * remembered address is only the fallback of a build without one).
     */
    fun server(context: Context): String =
        when {
            app.sync.joined -> {
                app.sync.server
                    .ifEmpty { context.knownServer() }
                    .ifEmpty { BuildConfig.SYNC_SERVER }
            }

            else -> {
                BuildConfig.SYNC_SERVER.ifEmpty { context.knownServer() }
            }
        }

    /** Account passkeys work here: the build has a link host and the system has Credential Manager's passkeys. */
    val passkeys: Boolean get() = platform.rpId.isNotEmpty()

    /** The sign-in under way, if any. */
    var signIn: AccountSignIn? by mutableStateOf(null)
        private set

    /** A brokered run on [server] that the platform hands its return to. */
    private fun brokered(
        server: String,
        provider: String,
        revoke: Boolean,
        open: (String) -> Unit,
    ) = BrokeredRun(server, open) { onPhase -> app.session.signIn(server, provider, revoke, onPhase) }
        .also { platform.run = it }

    /** A new sign-in on this device's sync service, reading its providers at once. */
    fun startSignIn(context: Context): AccountSignIn {
        val server = server(context)
        val next =
            AccountSignIn(
                app.session.ops,
                app.sync,
                app.session.scope,
                platform,
                server,
                context.deviceName(tv = false),
                deviceForm(context, tv = false),
                ownerName = ::ownerName,
                brokered = { provider, open -> brokered(server, provider, false, open) },
                // Google's own sheet only against the publisher's relay; another relay offers Google brokered (4.4).
                nativeGoogle = server.isNotEmpty() && server == BuildConfig.SYNC_SERVER,
            )
        signIn = next
        next.start()
        return next
    }

    fun endSignIn() {
        signIn = null
    }

    /** The approve sheet's one code at a time (plans/tv-qr-login.md 5.6), for the app's life. */
    val approval by lazy {
        PairingApproval(
            app.session.scope,
            read = app.session.ops::readPairingCode,
            approve = app.sync::approvePairing,
            members = {
                app.sync
                    .loadDevices()
                    .members
                    .map { it.id }
            },
            remove = app.sync::removeDevice,
        )
    }

    /** The Account page's sign-in links, passkeys and deletion. */
    val actions by lazy {
        AccountActions(
            app.session.ops,
            app.sync,
            app.session.scope,
            platform,
            server = { server(app) },
            deviceName = app.deviceName(tv = false),
            ownerName = ::ownerName,
            brokered = { provider, revoke, open -> brokered(server(app), provider, revoke, open) },
        )
    }

    /** D13's three steps, resumed at start from the core's `wipePending`. */
    val removal by lazy {
        LibraryRemoval(
            app.session.ops,
            app.sync,
            app.session.scope,
            traktSignOutEvery = { profiles -> app.trakt.signOutEvery(profiles) },
            debridDisconnectAll = {
                for (a in app.session.debrid.snapshot
                    ?.accounts
                    .orEmpty()) {
                    app.session.debridAccounts.store
                        .disconnect(a.service, a.generation)
                        .await()
                }
            },
        )
    }

    /** The owner profile's name, for the passkey's `user.name`. */
    private fun ownerName(): String =
        app.profiles.snapshot
            ?.profiles
            ?.firstOrNull { it.isOwner }
            ?.name ?: "Momotus"

    private fun profileIds(): List<String> =
        app.profiles.snapshot
            ?.profiles
            .orEmpty()
            .map { it.id }

    /** D13 on Sign out: the leave with the removal recorded first, then steps 2 and 3. */
    suspend fun signOutAndRemove(
        force: Boolean,
        debrid: Boolean,
    ) = removal.signOutAndRemove(force, debrid, profileIds())

    /** The account-deleted card's Remove from this device. */
    suspend fun removeHere(debrid: Boolean) = removal.removeHere(debrid, profileIds())

    /** Try again after a removal stopped, with the choice the core holds. */
    suspend fun retryRemoval() =
        removal.retry(
            app.profiles.snapshot
                ?.wipePending
                ?.debrid ?: true,
            profileIds(),
        )

    /** A removal the core holds goes on at start, once, on the owner of a device with no account. */
    fun resumeRemoval() {
        val s = app.profiles.snapshot ?: return
        removal.resume(s.wipePending, AccountRules.activeProfile(s)?.isOwner == true, profileIds())
    }

    /** The sign-in ways [server] offers (email included), none when it cannot say. */
    suspend fun providers(server: String): List<String> =
        if (server.isEmpty()) {
            emptyList()
        } else {
            runCatching {
                app.session.ops
                    .relayInfo(server)
                    .providers
            }.getOrDefault(emptyList())
        }

    /** Home's one account card (4.3), none on a kids profile. */
    fun homeCard(): AccountRules.Card? {
        val sync = app.sync
        val members = sync.devices?.members
        return AccountRules.homeCard(
            AccountRules.CardState(
                deletion = sync.deletion,
                gone = sync.problem?.code == SyncAccount.GONE,
                announcements = sync.announcements,
                members = members,
                profile = AccountRules.activeProfile(app.profiles.snapshot),
                alone = sync.joined && AccountRules.needsSecondWayIn(members),
                date = ::accountDate,
            ),
        )
    }

    /** The profile on screen and whether it is locked, for [AccountRules.rows]. */
    fun rows(tv: Boolean): AccountRules.Rows {
        val s = app.profiles.snapshot
        return AccountRules.rows(AccountRules.activeProfile(s), tv, s?.locked != false)
    }
}

/** `phone`, `tablet` or `tv` (sealed in the roster: only a phone or tablet approves others, D6). */
internal fun deviceForm(
    context: Context,
    tv: Boolean,
): String =
    when {
        tv -> "tv"
        context.resources.configuration.smallestScreenWidthDp >= 600 -> "tablet"
        else -> "phone"
    }
