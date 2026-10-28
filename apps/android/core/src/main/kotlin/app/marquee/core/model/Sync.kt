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

/** Reply of `relayInfo`: the sign-in providers a relay offers (none: only the pairing and kit doors). */
@Serializable
data class RelayInfo(
    val providers: List<String> = emptyList(),
)

/**
 * Reply of `nativeSignInStart`: [nonce] goes to the native sheet as it is (the relay's nonce bound to its
 * host); the relay takes [flow] until [expiresMs], its time.
 */
@Serializable
data class NativeStart(
    val flow: String,
    val nonce: String,
    val expiresMs: Long,
) {
    override fun toString() = "NativeStart(flow=hidden, expiresMs=$expiresMs)"
}

/** Reply of `emailSignInStart`: the relay handed a code to its mail service; it takes [flow] until [expiresMs]. */
@Serializable
data class EmailStart(
    val flow: String,
    val expiresMs: Long,
) {
    override fun toString() = "EmailStart(flow=hidden, expiresMs=$expiresMs)"
}

/** Reply of `recover`: store [record]; [via] is `kit` or `passkey` (only `kit` asks for a new kit). */
@Serializable
data class Recovered(
    val record: SyncRecord,
    val via: String,
) {
    override fun toString() = "Recovered(via=$via)"
}

/** Reply of `savePasskey`: store [record]; [id] is the kit's `syncDevices` entry. */
@Serializable
data class SavedPasskey(
    val id: String,
    val record: SyncRecord,
) {
    override fun toString() = "SavedPasskey(id=$id)"
}

/** Reply of `identities`: the account's sign-in links; store [record] when set. */
@Serializable
data class Identities(
    val links: List<IdentityLink> = emptyList(),
    val record: SyncRecord? = null,
) {
    override fun toString() = "Identities(links=$links)"
}

/** A reply that is only the record to store when it changed (`linkIdentity`, `unlinkIdentity`, `cancelDeletion`). */
@Serializable
data class RecordReply(
    val record: SyncRecord? = null,
) {
    override fun toString() = "RecordReply(record)"
}
