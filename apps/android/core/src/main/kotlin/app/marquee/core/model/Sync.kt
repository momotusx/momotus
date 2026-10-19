package app.marquee.core.model

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject

// Replies the core sends as plain `json!()` objects, so ts-rs does not export them; everything
// else of the sync and pairing protocol (SyncReport, PairingSnapshot, Devices, ...) is generated
// in Generated.kt. The keystore record is opaque JSON that the app stores and hands back verbatim,
// so a change in its shape never needs an app change.

/** The sync keystore record (server, device token, keys, roster state). Secret: stored by `SecretStore`, never logged. */
typealias SyncRecord = JsonObject

/** Reply of `installAddon`. */
@Serializable
data class InstalledAddon(
    val id: String,
    val name: String,
)

/** Reply of `approvePairing`: show [code] big, the new device must show the same; store [record]. */
@Serializable
data class ApprovedPairing(
    val code: String,
    val record: SyncRecord,
) {
    override fun toString() = "ApprovedPairing(code=$code)"
}

/** Reply of `createRecoveryKit`: show [kit] once and keep no copy; store [record]. [id] is the kit's `syncDevices` entry. */
@Serializable
data class RecoveryKitResult(
    val kit: String,
    val id: String,
    val record: SyncRecord,
) {
    override fun toString() = "RecoveryKitResult(id=$id, record)"
}
