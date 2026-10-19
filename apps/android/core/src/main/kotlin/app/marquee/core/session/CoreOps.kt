package app.marquee.core.session

import app.marquee.core.client.CoreClient
import app.marquee.core.model.ApprovedPairing
import app.marquee.core.model.DebridRecord
import app.marquee.core.model.DebridTombstone
import app.marquee.core.model.Devices
import app.marquee.core.model.ForkPreview
import app.marquee.core.model.HomeCatalog
import app.marquee.core.model.InstalledAddon
import app.marquee.core.model.LostPreview
import app.marquee.core.model.Prefs
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.model.RecoveryKitResult
import app.marquee.core.model.RestoreAnswer
import app.marquee.core.model.SyncRecord
import app.marquee.core.model.SyncReport
import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
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
    private val catalogs = ListSerializer(HomeCatalog.serializer())
    private val profilesSnap = ProfilesSnapshot.serializer()

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

    /**
     * Installs an entry of an open `addonCatalog` view by its `key`. A refusal is a [CoreError] whose
     * `code` is `timeout`, `unreachable`, `needsSetup`, `mismatch`, `adult`, `notListed` or
     * `switched`, with the core's words for the card as its message.
     */
    suspend fun installListed(key: String): InstalledAddon =
        call("installListed", mapOf("key" to text(key)), InstalledAddon.serializer())

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

    // Profiles (plans/m11-profiles.md section 6). Each answers the picker's snapshot, or a CoreError
    // whose `code` names the refusal (wrongPin, locked with retryAfterMs, viewsOpen with views...).

    suspend fun createProfile(
        name: String,
        avatar: String,
    ): ProfilesSnapshot = call("createProfile", mapOf("name" to text(name), "avatar" to text(avatar)), profilesSnap)

    /** Only the fields given change: an avatar this app did not pick is never rewritten. */
    suspend fun updateProfile(
        profile: String,
        name: String? = null,
        avatar: String? = null,
    ): ProfilesSnapshot =
        call(
            "updateProfile",
            buildMap {
                put("profile", text(profile))
                name?.let { put("name", text(it)) }
                avatar?.let { put("avatar", text(it)) }
            },
            profilesSnap,
        )

    suspend fun deleteProfile(profile: String): ProfilesSnapshot =
        call("deleteProfile", mapOf("profile" to text(profile)), profilesSnap)

    /** Sets [pin], or clears it with null; [currentPin] when the profile has one. */
    suspend fun setPin(
        profile: String,
        pin: String?,
        currentPin: String? = null,
    ): ProfilesSnapshot =
        call(
            "setPin",
            mapOf(
                "profile" to text(profile),
                "pin" to (pin?.let(::text) ?: JsonNull),
                "currentPin" to (currentPin?.let(::text) ?: JsonNull),
            ),
            profilesSnap,
        )

    /** Only the picker may be open: the app closes every other view first. */
    suspend fun switchProfile(
        profile: String,
        pin: String?,
    ): ProfilesSnapshot =
        call("switchProfile", mapOf("profile" to text(profile), "pin" to (pin?.let(::text) ?: JsonNull)), profilesSnap)

    suspend fun unlockProfile(pin: String?): ProfilesSnapshot =
        call("unlockProfile", mapOf("pin" to (pin?.let(::text) ?: JsonNull)), profilesSnap)

    suspend fun lockSession(): ProfilesSnapshot = call("lockSession", emptyMap(), profilesSnap)

    /** Clears the owner's PIN without it: only after the device's own lock screen said yes. */
    suspend fun resetOwnerPin(): ProfilesSnapshot = call("resetOwnerPin", emptyMap(), profilesSnap)

    /** The catalogs Home can show after Continue Watching, in order; hidden ones sit where they would show. */
    suspend fun homeCatalogs(): List<HomeCatalog> = call("homeCatalogs", emptyMap(), catalogs)

    /** Shows or hides a Home row. Returns the new list. */
    suspend fun setHomeRow(
        rowId: String,
        hidden: Boolean,
    ): List<HomeCatalog> =
        call("setHomeRow", mapOf("rowId" to text(rowId), "hidden" to JsonPrimitive(hidden)), catalogs)

    /** Moves a Home row [by] places (negative is up). Returns the new list. */
    suspend fun moveHomeRow(
        rowId: String,
        by: Int,
    ): List<HomeCatalog> = call("moveHomeRow", mapOf("rowId" to text(rowId), "by" to JsonPrimitive(by)), catalogs)

    /** Nested objects merge, e.g. `{"streams": {"languages": ["tr"]}}`. Returns the full prefs. */
    suspend fun setPrefs(patch: JsonObject): Prefs = call("setPrefs", mapOf("patch" to patch), Prefs.serializer())

    /** At start, before any view: what the keystore holds. The answer names tombstones the app may drop. */
    suspend fun restoreDebrid(
        records: List<DebridRecord>,
        tombstones: List<DebridTombstone>,
    ): RestoreAnswer =
        call(
            "restoreDebrid",
            mapOf(
                "records" to client.json.encodeToJsonElement(ListSerializer(DebridRecord.serializer()), records),
                "tombstones" to
                    client.json.encodeToJsonElement(ListSerializer(DebridTombstone.serializer()), tombstones),
            ),
            RestoreAnswer.serializer(),
        )

    /** The app stored this pending record. */
    suspend fun ackDebrid(
        service: String,
        generation: String,
        revision: Long,
    ) {
        client.request(
            "ackDebrid",
            mapOf("service" to text(service), "generation" to text(generation), "revision" to JsonPrimitive(revision)),
        )
    }

    /** A typed key: a new connection; its record arrives as pending in the `debrid` view. Returns its generation. */
    suspend fun setDebridKey(
        service: String,
        key: String,
    ): String =
        client
            .request("setDebridKey", mapOf("service" to text(service), "key" to text(key)))
            .jsonObject
            .getValue("generation")
            .jsonPrimitive.content

    /** Sent after the app wrote the tombstone for this generation. */
    suspend fun disconnectDebrid(
        service: String,
        generation: String,
    ) {
        client.request("disconnectDebrid", mapOf("service" to text(service), "generation" to text(generation)))
    }

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

    /** The account's members, fingerprint and epoch. A changed `record` in the reply must be stored. */
    suspend fun syncDevices(record: SyncRecord): Devices =
        call("syncDevices", mapOf("record" to record), Devices.serializer())

    /** What reporting [lost] would drop by default: the devices it vouched for, and the kits. */
    suspend fun lostPreview(
        record: SyncRecord,
        lost: String,
    ): LostPreview = call("lostPreview", mapOf("record" to record, "lost" to text(lost)), LostPreview.serializer())

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

    /**
     * After a roster conflict: both sides, read only. Fails with `http status 401` when the sync
     * server no longer knows this device's token (a restore without it).
     */
    suspend fun forkPreview(record: SyncRecord): ForkPreview =
        call("forkPreview", mapOf("record" to record), ForkPreview.serializer())

    /**
     * After a roster conflict: takes the server's roster as [forkPreview] showed it ([head]), dropping
     * what the user ticked. Refused with `rosterChanged` when it moved since. Returns the record to
     * store. Put behind the OS re-authentication prompt.
     */
    suspend fun useServerRoster(
        record: SyncRecord,
        drop: List<String>,
        head: String,
    ): SyncRecord =
        client
            .request(
                "useServerRoster",
                mapOf("record" to record, "drop" to JsonArray(drop.map(::text)), "head" to text(head)),
            ).jsonObject

    /**
     * The digits on the new device differ: ends the approval of [qr] now, with a key rotation that drops
     * the device if it joined meanwhile. Returns the record to store. Put behind the OS re-authentication
     * prompt.
     */
    suspend fun cancelPairing(
        record: SyncRecord,
        qr: String,
    ): SyncRecord = client.request("cancelPairing", mapOf("record" to record, "qr" to text(qr))).jsonObject
}
