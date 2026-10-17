@file:OptIn(kotlinx.serialization.ExperimentalSerializationApi::class)

package app.marquee.core.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonClassDiscriminator
import kotlinx.serialization.json.JsonObject

// Types the web family does not get from ts-rs yet (pairing, sync): written by hand until the
// core exports them (request C6). The keystore record is opaque JSON that the app stores and hands
// back verbatim, so a change in its shape never needs an app change.

/** The sync keystore record (server, device token, keys, roster state). Secret: stored by `SecretStore`, never logged. */
typealias SyncRecord = JsonObject

/** Reply of `installAddon`. */
@Serializable
data class InstalledAddon(
    val id: String,
    val name: String,
)

/** Reply of `syncNow` (plans/core-protocol.md "Ops"). */
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
    /** Records waiting for a key this device does not hold yet. */
    val parked: Int = 0,
    /** Above 0: the sync server may be hiding device changes; suggest comparing fingerprints. */
    val oldEpochWrites: Int = 0,
    /** This device's clock minus the sync server's, in milliseconds. */
    val clockGapMs: Long = 0,
    /** Pushes wait this long after a sync server restore. */
    val rotationWaitMs: Long = 0,
    /** 12 characters to compare between devices. */
    val fingerprint: String = "",
    /** A changed record: store it, then sync again at once. */
    val record: SyncRecord? = null,
    val again: Boolean = false,
) {
    // The record holds keys and a token.
    override fun toString() = "SyncReport(pushed=$pushed, pulled=$pulled, applied=$applied, again=$again)"
}

/** Reply of `approvePairing`: show [code] big, the new device must show the same; store [record]. */
@Serializable
data class ApprovedPairing(
    val code: String,
    val record: SyncRecord,
) {
    override fun toString() = "ApprovedPairing(code=$code)"
}

/** Reply of `createRecoveryKit`: show [kit] once and keep no copy; store [record]. */
@Serializable
data class RecoveryKitResult(
    val kit: String,
    val record: SyncRecord,
) {
    override fun toString() = "RecoveryKitResult(record)"
}

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

    /** Store [record] in the keystore; sync can start. */
    @Serializable
    @SerialName("joined")
    data class Joined(
        val record: SyncRecord,
    ) : PairingSnapshot {
        override fun toString() = "Joined(record)"
    }

    @Serializable
    @SerialName("expired")
    data object Expired : PairingSnapshot

    @Serializable
    @SerialName("failed")
    data class Failed(
        val reason: String,
    ) : PairingSnapshot
}
