@file:OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)

package app.marquee.core.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonClassDiscriminator
import kotlinx.serialization.json.JsonObject

// Types the web family does not get from ts-rs yet (pairing, sync): written by hand until the
// core exports them (request C6). Key bundles are opaque JSON handed back to the core verbatim,
// so a change in their shape never needs an app change.

/** What a device needs to sync with an account. Goes to the secret store, never to a log. */
@Serializable
data class Joined(
    val server: String,
    val deviceToken: String,
    val keys: JsonObject,
)

/** Reply of `createSyncAccount`. */
@Serializable
data class SyncAccount(
    val account: String,
    val deviceToken: String,
    val keys: JsonObject,
)

/** Reply of `syncNow`. */
@Serializable
data class SyncReport(
    val pushed: Int = 0,
    val stale: Int = 0,
    val refused: Int = 0,
    val waiting: Int = 0,
    val pulled: Int = 0,
    val applied: Int = 0,
    val unreadable: Int = 0,
    val newer: Int = 0,
    /** This device's clock minus the relay's, in milliseconds. */
    val clockGapMs: Long = 0,
)

/** Reply of `installAddon`. */
@Serializable
data class InstalledAddon(
    val id: String,
    val name: String,
)

/** The `pairing` view: a new device shows a QR code and a code to compare. */
@Serializable
@JsonClassDiscriminator("phase")
sealed interface PairingSnapshot {
    @Serializable
    @SerialName("starting")
    data object Starting : PairingSnapshot

    @Serializable
    @SerialName("waiting")
    data class Waiting(
        val qr: String,
        val expiresMs: Long,
    ) : PairingSnapshot

    @Serializable
    @SerialName("confirm")
    data class Confirm(
        val code: String,
    ) : PairingSnapshot

    @Serializable
    @SerialName("joined")
    data class JoinedAccount(
        val joined: Joined,
    ) : PairingSnapshot

    @Serializable
    @SerialName("expired")
    data object Expired : PairingSnapshot

    @Serializable
    @SerialName("failed")
    data class Failed(
        val reason: String,
    ) : PairingSnapshot
}
