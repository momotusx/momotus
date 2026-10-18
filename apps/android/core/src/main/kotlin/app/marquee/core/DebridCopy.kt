package app.marquee.core

import app.marquee.core.model.AccountState
import app.marquee.core.model.DebridLoginSnapshot

/** The debrid services Settings offers, equal to Apple's `DebridKeys`, and the words around them. */
object DebridCopy {
    data class Service(
        val id: String,
        val name: String,
        val keyPage: String,
    )

    val services =
        listOf(
            Service("realdebrid", "Real-Debrid", "https://real-debrid.com/apitoken"),
            Service("torbox", "TorBox", "https://torbox.app/settings"),
            Service("alldebrid", "AllDebrid", "https://alldebrid.com/apikeys"),
            Service("premiumize", "Premiumize", "https://www.premiumize.me/account"),
        )

    /** Services that sign in with a code; the others take a key only. */
    val codeServices = setOf("realdebrid", "alldebrid", "torbox")

    /**
     * A service card's state in words, null when it is not connected. [pending] is a record the
     * keystore has not stored yet; [durable] is false when the keystore keeps things in memory only.
     */
    fun state(
        account: AccountState?,
        pending: Boolean,
        durable: Boolean,
    ): String? =
        when {
            account == null -> null
            pending && !durable -> "Kept for this session"
            account == AccountState.Reconnect -> "Sign in again: the service asked for it"
            account == AccountState.Failing -> "Not answering, trying again"
            pending -> "Saving"
            else -> "Connected"
        }

    /** What a sign-in by code shows, for every phase but the code itself. */
    fun login(phase: DebridLoginSnapshot): String =
        when (phase) {
            DebridLoginSnapshot.Starting -> "Getting a code"
            is DebridLoginSnapshot.Code -> "Enter the code"
            DebridLoginSnapshot.Done -> "Connected"
            DebridLoginSnapshot.Expired -> "The code ran out before it was entered."
            DebridLoginSnapshot.Denied -> "The sign-in was turned down on the service's page."
            DebridLoginSnapshot.Used -> "Another sign-in or key took its place."
            is DebridLoginSnapshot.Failed -> phase.reason
        }

    /** How many services are connected, for Settings. */
    fun inUse(count: Int) =
        when (count) {
            0 -> "No debrid service in use."
            1 -> "1 debrid service in use."
            else -> "$count debrid services in use."
        }

    /** "real-debrid.com/device": the sign-in page as a person types it. */
    fun address(url: String) =
        url
            .removePrefix("https://")
            .removePrefix("http://")
            .removePrefix("www.")
            .trimEnd('/')

    /** "real-debrid.com/apitoken": where to get a key, to type on another device. */
    fun page(service: Service) = service.keyPage.removePrefix("https://").removePrefix("www.")
}
