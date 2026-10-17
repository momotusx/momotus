package app.marquee.core.session

import app.marquee.core.client.CoreClient
import app.marquee.core.model.InstalledAddon
import app.marquee.core.model.Joined
import app.marquee.core.model.Prefs
import app.marquee.core.model.SyncAccount
import app.marquee.core.model.SyncReport
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * The core's one-shot ops (plans/core-protocol.md "Ops"). Home, Discover and Search follow addon
 * changes on their own, so nothing here reopens a view. Secrets arrive as arguments; the core
 * keeps none of them.
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

    suspend fun createSyncAccount(
        server: String,
        invite: String? = null,
    ): SyncAccount =
        call(
            "createSyncAccount",
            buildMap {
                put("server", text(server))
                invite?.let { put("invite", text(it)) }
            },
            SyncAccount.serializer(),
        )

    suspend fun syncNow(account: Joined): SyncReport = call("syncNow", account.fields(), SyncReport.serializer())

    /** Hands the account to the device showing `qr`; returns the code both screens must show. */
    suspend fun approvePairing(
        account: Joined,
        qr: String,
    ): String =
        client
            .request("approvePairing", account.fields() + ("qr" to text(qr)))
            .jsonObject
            .getValue("code")
            .jsonPrimitive.content

    /** The kit is shown once and never stored. */
    suspend fun createRecoveryKit(account: Joined): String =
        client
            .request("createRecoveryKit", account.fields())
            .jsonObject
            .getValue("kit")
            .jsonPrimitive.content

    suspend fun recover(
        server: String,
        kit: String,
    ): Joined = call("recover", mapOf("server" to text(server), "kit" to text(kit)), Joined.serializer())

    private fun Joined.fields(): Map<String, JsonElement> =
        mapOf("server" to text(server), "deviceToken" to text(deviceToken), "keys" to keys)
}
