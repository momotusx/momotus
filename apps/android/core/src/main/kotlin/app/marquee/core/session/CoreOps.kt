package app.marquee.core.session

import app.marquee.core.client.CoreClient
import app.marquee.core.model.ApprovedPairing
import app.marquee.core.model.InstalledAddon
import app.marquee.core.model.Prefs
import app.marquee.core.model.RecoveryKitResult
import app.marquee.core.model.SyncRecord
import app.marquee.core.model.SyncReport
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

enum class LifecycleEvent(
    val wire: String,
) {
    Foreground("foreground"),
    Background("background"),
    MemoryPressure("memoryPressure"),
    NetworkChanged("networkChanged"),
}

/**
 * The core's one-shot ops (plans/core-protocol.md "Ops"). Home, Discover and Search follow addon
 * changes on their own, so nothing here reopens a view. Secrets (debrid keys, the sync record)
 * arrive as arguments; the core keeps none of them. Replies that carry a changed `record` must be
 * stored by the caller before the next call.
 */
class CoreOps(
    private val client: CoreClient,
) {
    private suspend fun <T> call(
        op: String,
        fields: Map<String, JsonElement>,
        serializer: KSerializer<T>,
    ): T = client.json.decodeFromJsonElement(serializer, client.request(op, fields))

    private fun text(value: String) = JsonPrimitive(value)

    /** Answers at once from the core's request loop: use it for a liveness check, not a network op. */
    suspend fun ping() {
        client.request("ping")
    }

    /**
     * The app's life as the OS reports it, sent without waiting for the answer. `memoryPressure`
     * frees the core's in-memory caches; the others are accepted today and need no action.
     */
    fun lifecycle(event: LifecycleEvent) = client.post("lifecycle", mapOf("event" to text(event.wire)))

    suspend fun installAddon(url: String): InstalledAddon =
        call("installAddon", mapOf("url" to text(url)), InstalledAddon.serializer())

    suspend fun removeAddon(url: String) {
        client.request("removeAddon", mapOf("url" to text(url)))
    }

    suspend fun moveAddon(
        url: String,
        to: Int,
    ) {
        client.request("moveAddon", mapOf("url" to text(url), "to" to JsonPrimitive(to)))
    }

    suspend fun setAddonEnabled(
        url: String,
        enabled: Boolean,
    ) {
        client.request("setAddonEnabled", mapOf("url" to text(url), "enabled" to JsonPrimitive(enabled)))
    }

    /** Nested objects merge, e.g. `{"streams": {"languages": ["tr"]}}`. Returns the full prefs. */
    suspend fun setPrefs(patch: JsonObject): Prefs = call("setPrefs", mapOf("patch" to patch), Prefs.serializer())

    /** Replaces the debrid accounts; the first key is the default. Returns how many stores were set. */
    suspend fun setDebridKeys(keys: List<Pair<String, String>>): Int {
        val pairs = JsonArray(keys.map { (service, key) -> JsonArray(listOf(text(service), text(key))) })
        return client
            .request("setDebridKeys", mapOf("keys" to pairs))
            .jsonObject
            .getValue("stores")
            .jsonPrimitive.int
    }

    /** Starts an account with this device as its first member (`name` is how other devices show it). Returns the record to store. */
    suspend fun createSyncAccount(
        server: String,
        name: String,
        invite: String? = null,
    ): SyncRecord =
        client
            .request(
                "createSyncAccount",
                buildMap {
                    put("server", text(server))
                    put("name", text(name))
                    invite?.let { put("invite", text(it)) }
                },
            ).jsonObject

    /** One sync round. When the report carries a `record`, store it and call again at once (`again`). */
    suspend fun syncNow(record: SyncRecord): SyncReport =
        call("syncNow", mapOf("record" to record), SyncReport.serializer())

    /** Vouches for the device showing `qr`; show the code big, the new device must show the same. Store the returned record. */
    suspend fun approvePairing(
        record: SyncRecord,
        qr: String,
    ): ApprovedPairing =
        call("approvePairing", mapOf("record" to record, "qr" to text(qr)), ApprovedPairing.serializer())

    /** The kit is shown once and never stored. Store the returned record. */
    suspend fun createRecoveryKit(record: SyncRecord): RecoveryKitResult =
        call("createRecoveryKit", mapOf("record" to record), RecoveryKitResult.serializer())

    /** Joins the account a recovery kit belongs to; returns the record to store (then make a new kit). */
    suspend fun recover(
        server: String,
        kit: String,
        name: String,
    ): SyncRecord =
        client.request("recover", mapOf("server" to text(server), "kit" to text(kit), "name" to text(name))).jsonObject

    /** The account's members, fingerprint and epoch (see plans/core-protocol.md for the shape). */
    suspend fun syncDevices(record: SyncRecord): JsonObject =
        client.request("syncDevices", mapOf("record" to record)).jsonObject

    /** What reporting [lost] would drop: `{devices, kits}`. */
    suspend fun lostPreview(
        record: SyncRecord,
        lost: String,
    ): JsonObject = client.request("lostPreview", mapOf("record" to record, "lost" to text(lost))).jsonObject

    /** Returns the record to store. Put behind the OS re-authentication prompt. */
    suspend fun removeDevice(
        record: SyncRecord,
        device: String,
    ): SyncRecord = client.request("removeDevice", mapOf("record" to record, "device" to text(device))).jsonObject

    /** Returns the record to store. Put behind the OS re-authentication prompt. */
    suspend fun reportLost(
        record: SyncRecord,
        lost: String,
        keep: List<String> = emptyList(),
    ): SyncRecord =
        client
            .request(
                "reportLost",
                mapOf("record" to record, "lost" to text(lost), "keep" to JsonArray(keep.map(::text))),
            ).jsonObject

    /** Sends what is queued, then leaves the account; the caller deletes its stored record. Without [force] it fails while edits are unsent. */
    suspend fun signOut(
        record: SyncRecord,
        force: Boolean = false,
    ) {
        client.request("signOut", mapOf("record" to record, "force" to JsonPrimitive(force)))
    }

    /** After a roster conflict: takes the server's roster, dropping what the user ticked. Returns the record to store. */
    suspend fun useServerRoster(
        record: SyncRecord,
        drop: List<String> = emptyList(),
    ): SyncRecord =
        client
            .request("useServerRoster", mapOf("record" to record, "drop" to JsonArray(drop.map(::text))))
            .jsonObject
}
