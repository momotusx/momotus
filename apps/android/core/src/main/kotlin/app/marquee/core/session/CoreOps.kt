package app.marquee.core.session

import app.marquee.core.client.CoreClient
import app.marquee.core.model.AddonInfo
import app.marquee.core.model.ApprovedPairing
import app.marquee.core.model.DebridRecord
import app.marquee.core.model.DebridTombstone
import app.marquee.core.model.DefaultAddons
import app.marquee.core.model.DefaultChoice
import app.marquee.core.model.Devices
import app.marquee.core.model.EmailStart
import app.marquee.core.model.FirstRunStill
import app.marquee.core.model.ForkPreview
import app.marquee.core.model.HomeCatalog
import app.marquee.core.model.Identities
import app.marquee.core.model.InstalledAddon
import app.marquee.core.model.KeystoreRecord
import app.marquee.core.model.LanguageChoice
import app.marquee.core.model.LearnedChoice
import app.marquee.core.model.LostPreview
import app.marquee.core.model.NativeStart
import app.marquee.core.model.Network
import app.marquee.core.model.Prefs
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.model.RecordReply
import app.marquee.core.model.Recovered
import app.marquee.core.model.RecoveryKitResult
import app.marquee.core.model.RelayInfo
import app.marquee.core.model.RestoreAnswer
import app.marquee.core.model.SavedPasskey
import app.marquee.core.model.SignInSnapshot
import app.marquee.core.model.StreamConditions
import app.marquee.core.model.SyncRecord
import app.marquee.core.model.SyncReport
import app.marquee.core.model.TraktReport
import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.nullable
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
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
    private val languageList = ListSerializer(LanguageChoice.serializer())

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
     * frees the core's in-memory caches; `networkChanged` drops pooled connections and, with [network],
     * says which speed estimate applies now.
     */
    fun lifecycle(
        event: LifecycleEvent,
        network: Network? = null,
    ) = client.post(
        "lifecycle",
        buildMap {
            put("event", text(event.wire))
            network?.let { put("network", text(it.wire)) }
        },
    )

    /**
     * The app came to the front: as [lifecycle] with `foreground`, waiting for the answer. True when the
     * core's stream server had to take another port while the app was away (the system took its
     * listener): a loaded item must then be opened again. The core also locks a profile with a PIN that
     * was away more than 30 minutes; the profiles view says so.
     */
    suspend fun foreground(): Boolean =
        client
            .request("lifecycle", mapOf("event" to text(LifecycleEvent.Foreground.wire)))
            .let { it as? JsonObject }
            ?.get("serverMoved")
            ?.jsonPrimitive
            ?.booleanOrNull == true

    /** "Remove from Continue Watching": the title leaves it and Next Up on every synced device until it is played again. */
    suspend fun removeFromContinue(itemId: String) {
        client.request("removeFromContinue", mapOf("itemId" to text(itemId)))
    }

    /**
     * Every language the core knows, by English name, for the subtitle and audio pickers; with the [device]'s
     * code the one it reads as is marked (`device`: "nb" marks Norwegian, "no").
     */
    suspend fun languages(device: String? = null): List<LanguageChoice> =
        call("languages", device?.let { mapOf("device" to text(it)) } ?: emptyMap(), languageList)

    /**
     * What a pasted link would install, for the install sheet: its manifest read, nothing written. The
     * same sign-in rule as [installAddon] comes first.
     */
    suspend fun previewAddon(
        url: String,
        acknowledgeToken: Boolean = false,
    ): AddonInfo =
        call(
            "previewAddon",
            buildMap {
                put("url", text(url))
                if (acknowledgeToken) put("acknowledgeToken", JsonPrimitive(true))
            },
            AddonInfo.serializer(),
        )

    /**
     * A link whose settings carry a sign-in is refused with code `tokenInLink` before anything is
     * fetched; sent again with [acknowledgeToken] once the user said it is theirs.
     */
    suspend fun installAddon(
        url: String,
        acknowledgeToken: Boolean = false,
    ): InstalledAddon =
        call(
            "installAddon",
            buildMap {
                put("url", text(url))
                if (acknowledgeToken) put("acknowledgeToken", JsonPrimitive(true))
            },
            InstalledAddon.serializer(),
        )

    /** The default addons in order, each with whether the profile has it (plans/m13-default-addons.md). */
    suspend fun defaultAddons(): DefaultAddons = call("defaultAddons", emptyMap(), DefaultAddons.serializer())

    /**
     * The still first run frames its questions with: a title of the default catalog's first catalog, asked
     * without installing it. Null when none came within 1.5 s: the frame then stays black.
     */
    suspend fun firstRunStill(): FirstRunStill? = call("firstRunStill", emptyMap(), FirstRunStill.serializer().nullable)

    /**
     * Installs the defaults in one write, no network: a named absent one goes in on or off as chosen,
     * a named present one takes its switch. A key left out is never added or switched, and an empty
     * list writes nothing. Refused for a kids profile.
     */
    suspend fun installDefaults(choices: List<DefaultChoice>) {
        client.request(
            "installDefaults",
            mapOf("choices" to client.json.encodeToJsonElement(ListSerializer(DefaultChoice.serializer()), choices)),
        )
    }

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

    /** This device's network, measured speeds and free space, for Settings' "This device". */
    suspend fun streamConditions(): StreamConditions =
        call("streamConditions", emptyMap(), StreamConditions.serializer())

    /** "Measure again": the current network's speeds go back to their estimates. */
    suspend fun resetSpeed(): StreamConditions = call("resetSpeed", emptyMap(), StreamConditions.serializer())

    /** Undo of a learned note: that choice on that screen class goes back to zero and no longer learns. */
    suspend fun undoLearned(
        screenClass: Int,
        choice: LearnedChoice,
    ): Prefs =
        call(
            "undoLearned",
            mapOf("screenClass" to JsonPrimitive(screenClass), "choice" to text(choice.wire)),
            Prefs.serializer(),
        )

    /** "Forget what was learned": every counter of the profile reads as zero from now on. */
    suspend fun forgetLearned(): Prefs = call("forgetLearned", emptyMap(), Prefs.serializer())

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

    /**
     * Starts an account with this device as its first member (`name` is how other devices show it). On a relay
     * with sign-in it takes a sign-in's [ticket], whose identity is linked; [move] takes an identity linked
     * elsewhere over (Start over). [form] is sealed in the roster. Returns the record to store.
     */
    suspend fun createSyncAccount(
        server: String,
        name: String,
        invite: String? = null,
        ticket: String? = null,
        move: Boolean = false,
        form: String? = null,
    ): SyncRecord =
        client
            .request(
                "createSyncAccount",
                buildMap {
                    put("server", text(server))
                    put("name", text(name))
                    invite?.let { put("invite", text(it)) }
                    ticket?.let { put("ticket", text(it)) }
                    if (move) put("move", JsonPrimitive(true))
                    form?.let { put("form", text(it)) }
                },
            ).jsonObject

    /** The sign-in providers [server] offers, read before any account exists. */
    suspend fun relayInfo(server: String): RelayInfo =
        call("relayInfo", mapOf("server" to text(server)), RelayInfo.serializer())

    /** A native sheet's flow (Google through Credential Manager): give the answer's nonce to the sheet as it is. */
    suspend fun nativeSignInStart(
        server: String,
        provider: String,
    ): NativeStart =
        call(
            "nativeSignInStart",
            mapOf("server" to text(server), "provider" to text(provider)),
            NativeStart.serializer(),
        )

    /** The sheet's ID token for [flow]: `found` or `new` (a [SignInSnapshot]). */
    suspend fun nativeSignIn(
        flow: String,
        idToken: String,
    ): SignInSnapshot =
        call("nativeSignIn", mapOf("flow" to text(flow), "idToken" to text(idToken)), SignInSnapshot.serializer())

    /** Mails a 6-digit sign-in code to [email] (plans/accounts-email.md); the flow's verifier stays in the core. */
    suspend fun emailSignInStart(
        server: String,
        email: String,
    ): EmailStart =
        call("emailSignInStart", mapOf("server" to text(server), "email" to text(email)), EmailStart.serializer())

    /** The mailed [code] for [flow]: `found` or `new` (a [SignInSnapshot]). */
    suspend fun emailSignIn(
        flow: String,
        code: String,
    ): SignInSnapshot =
        call("emailSignIn", mapOf("flow" to text(flow), "code" to text(code)), SignInSnapshot.serializer())

    /** The account's sign-in links; a returned record must be stored. Any profile. */
    suspend fun identities(record: SyncRecord): Identities =
        call("identities", mapOf("record" to record), Identities.serializer())

    /** Links a sign-in's identity to this account; [move] takes one linked elsewhere. The owner profile only. */
    suspend fun linkIdentity(
        record: SyncRecord,
        ticket: String,
        move: Boolean = false,
    ): RecordReply =
        call(
            "linkIdentity",
            buildMap {
                put("record", record)
                put("ticket", text(ticket))
                if (move) put("move", JsonPrimitive(true))
            },
            RecordReply.serializer(),
        )

    /** Unlinks by link id; an Apple link sends Apple's revoke [receipt] when there is one. The owner only. */
    suspend fun unlinkIdentity(
        record: SyncRecord,
        link: Long,
        receipt: String? = null,
    ): RecordReply =
        call(
            "unlinkIdentity",
            buildMap {
                put("record", record)
                put("link", JsonPrimitive(link))
                receipt?.let { put("receipt", text(it)) }
            },
            RecordReply.serializer(),
        )

    /** Cancels a scheduled deletion; [unlink] also removes the identity that asked for it. Any profile. */
    suspend fun cancelDeletion(
        record: SyncRecord,
        unlink: Boolean = false,
    ): RecordReply =
        call(
            "cancelDeletion",
            buildMap {
                put("record", record)
                if (unlink) put("unlink", JsonPrimitive(true))
            },
            RecordReply.serializer(),
        )

    /** The user saw these announcements; security notices are the owner's (`notOwner` otherwise). */
    suspend fun ackAnnouncements(ids: List<String>) {
        client.request("ackAnnouncements", mapOf("ids" to JsonArray(ids.map(::text))))
    }

    /** Appends a passkey kit from the passkey's PRF output (base64url); the owner profile only. */
    suspend fun savePasskey(
        record: SyncRecord,
        prf: String,
        label: String,
    ): SavedPasskey =
        call(
            "savePasskey",
            mapOf("record" to record, "prf" to text(prf), "label" to text(label)),
            SavedPasskey.serializer(),
        )

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

    /**
     * Joins the account a recovery kit belongs to; returns the record to store (then make a new kit). The
     * answer is `{record, via}`: `via` says the way in, which with a kit is always `kit`.
     */
    suspend fun recover(
        server: String,
        kit: String?,
        name: String,
        prf: String? = null,
        form: String? = null,
    ): Recovered =
        call(
            "recover",
            buildMap {
                put("server", text(server))
                kit?.let { put("kit", text(it)) }
                prf?.let { put("prf", text(it)) }
                put("name", text(name))
                form?.let { put("form", text(it)) }
            },
            Recovered.serializer(),
        )

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

    /**
     * Sends what is queued, then leaves the account; the caller deletes its stored record. Without [force] it
     * fails while edits are unsent. With [local]: no round and nothing sent, only this device forgets the
     * account (the way out for a device the sync server will not let leave).
     */
    suspend fun signOut(
        record: SyncRecord?,
        force: Boolean = false,
        local: Boolean = false,
        removeAfter: Boolean? = null,
        wipe: Boolean = false,
        disconnectDebrid: Boolean = false,
    ) {
        client.request(
            "signOut",
            buildMap {
                record?.let { put("record", it) }
                put("force", JsonPrimitive(force))
                put("local", JsonPrimitive(local))
                // D13's removal: recorded before the leave goes out; `debrid` says whether the debrid accounts go too.
                removeAfter?.let { put("removeAfter", JsonObject(mapOf("debrid" to JsonPrimitive(it)))) }
                if (wipe) put("wipe", JsonPrimitive(true))
                if (disconnectDebrid) put("disconnectDebrid", JsonPrimitive(true))
            },
        )
    }

    /**
     * Deletes the account on the sync server for every device, once the server confirmed; this device then
     * forgets it as after [signOut] (the library stays) and the caller deletes its stored record. Put behind
     * the OS re-authentication prompt.
     */
    suspend fun deleteAccount(
        record: SyncRecord,
        receipt: String? = null,
    ) {
        client.request(
            "deleteAccount",
            buildMap {
                put("record", record)
                receipt?.let { put("receipt", text(it)) }
            },
        )
    }

    // Trakt (plans/m7-trakt.md section 3): the record is the profile's, from the keystore; the core keeps it in
    // memory only. Refused with `locked` while the session is locked, and `notConfigured` in a build without Trakt.

    /** The profile's stored record (null: none here), after `open`, `unlockProfile` and `switchProfile`, and as the ack. */
    suspend fun setTrakt(record: KeystoreRecord?) {
        client.request("setTrakt", mapOf("record" to (record ?: JsonNull)))
    }

    /** One round now, the "Sync now" button; the core schedules the others. */
    suspend fun traktSync(): TraktReport = call("traktSync", emptyMap(), TraktReport.serializer())

    /** Signs this device out of Trakt for the profile (revoke best effort); the app then deletes the record. */
    suspend fun traktSignOut(profile: String? = null) {
        client.request("traktSignOut", profile?.let { mapOf("profile" to text(it)) } ?: emptyMap())
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
