package app.marquee.core.sync

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import java.security.SecureRandom
import kotlin.io.encoding.Base64

/** What a passkey sheet gave: a passkey used (its id, its `user.id`, and the PRF output), or why not. */
sealed interface PasskeyResult {
    data class Used(
        /** The credential id, base64url. */
        val credentialId: String,
        /** The passkey's `user.id` (the account id, base64url), when the answer carries it. */
        val userHandle: String?,
        /** The 32-byte PRF output as base64url without padding, as `savePasskey` and `recover` take it. */
        val prf: String?,
    ) : PasskeyResult {
        override fun toString() = "Used(prf=${if (prf == null) "none" else "hidden"})"
    }

    /** The user closed the sheet. */
    data object Cancelled : PasskeyResult

    /** No passkey for this relying party on the device (an immediate request), or none was made. */
    data object None : PasskeyResult

    data class Failed(
        val reason: String,
    ) : PasskeyResult
}

/**
 * The account passkey (plans/accounts.md 3.1) as Credential Manager's WebAuthn JSON: requests built here, the
 * answers read here, so the platform adapter only hands strings to the sheet. Nothing checks the assertion:
 * only the PRF output is used, and the core derives the kit from it.
 */
object Passkeys {
    /** The one PRF input every platform evaluates, `marquee/passkey/v1` as base64url (`apps/shared/passkey-vectors.json`). */
    const val PRF_INPUT = "bWFycXVlZS9wYXNza2V5L3Yx"

    private val json = Json { ignoreUnknownKeys = true }
    private val random = SecureRandom()

    // Kotlin's codec, not java.util.Base64: that one needs API 26 and the app starts at 24.
    private val base64url = Base64.UrlSafe.withPadding(Base64.PaddingOption.ABSENT_OPTIONAL)

    private fun b64(bytes: ByteArray): String = base64url.encode(bytes)

    /** 32 random bytes: the challenge, which nothing checks (3.1). */
    fun challenge(): String = b64(ByteArray(32).also(random::nextBytes))

    /** "<owner profile name> (Marquee)": the passkey's `user.name` (3.1). */
    fun userName(owner: String) = "$owner (Momotus)"

    private fun prfEval() =
        buildJsonObject {
            put(
                "prf",
                buildJsonObject {
                    put("eval", buildJsonObject { put("first", PRF_INPUT) })
                },
            )
        }

    /**
     * A new passkey for the account [userId] (base64url, `syncDevices`'s `account`) on [rpId], the link host:
     * resident, user-verified, no attestation, no `excludeCredentials` (3.1).
     */
    fun create(
        rpId: String,
        userId: String,
        owner: String,
        challenge: String = challenge(),
    ): String =
        buildJsonObject {
            put(
                "rp",
                buildJsonObject {
                    put("id", rpId)
                    put("name", "Momotus")
                },
            )
            put(
                "user",
                buildJsonObject {
                    put("id", userId)
                    put("name", userName(owner))
                    put("displayName", userName(owner))
                },
            )
            put("challenge", challenge)
            put(
                "pubKeyCredParams",
                buildJsonArray {
                    for (alg in listOf(-7, -257)) {
                        add(
                            buildJsonObject {
                                put("type", "public-key")
                                put("alg", alg)
                            },
                        )
                    }
                },
            )
            put(
                "authenticatorSelection",
                buildJsonObject {
                    put("residentKey", "required")
                    put("requireResidentKey", true)
                    put("userVerification", "required")
                },
            )
            put("attestation", "none")
            put("extensions", prfEval())
        }.toString()

    /** An assertion with the PRF input; [allow] empty asks for any of the link host's passkeys (sign-in). */
    fun get(
        rpId: String,
        allow: List<String> = emptyList(),
        challenge: String = challenge(),
    ): String =
        buildJsonObject {
            put("rpId", rpId)
            put("challenge", challenge)
            put(
                "allowCredentials",
                JsonArray(
                    allow.map { id ->
                        buildJsonObject {
                            put("type", "public-key")
                            put("id", id)
                        }
                    },
                ),
            )
            put("userVerification", "required")
            put("extensions", prfEval())
        }.toString()

    /** A create or get answer (`registrationResponseJson`, `authenticationResponseJson`) as a [PasskeyResult.Used]. */
    fun used(answer: String): PasskeyResult.Used {
        val o = json.parseToJsonElement(answer).jsonObject
        val id = (o["rawId"] ?: o["id"])?.jsonPrimitive?.content.orEmpty()
        val user =
            (o["response"] as? JsonObject)
                ?.get(
                    "userHandle",
                )?.let { (it as? JsonPrimitive)?.contentOrNullSafe() }
        val prf =
            (o["clientExtensionResults"] as? JsonObject)
                ?.get("prf")
                ?.let { it as? JsonObject }
                ?.get("results")
                ?.let { it as? JsonObject }
                ?.get("first")
                ?.let { (it as? JsonPrimitive)?.contentOrNullSafe() }
        return PasskeyResult.Used(id, user?.let(::unpadded), prf?.let(::unpadded))
    }

    /**
     * Whether a passkey's `user.id` (base64url) is the account [account]: the sign-in's `found` names it in hex,
     * `syncDevices` in base64url; the bytes are compared.
     */
    fun sameAccount(
        userHandle: String?,
        account: String,
    ): Boolean {
        val user = userHandle?.let(::fromBase64) ?: return false
        val other = (if (HEX.matches(account)) fromHex(account) else fromBase64(account)) ?: return false
        return user.isNotEmpty() && user.contentEquals(other)
    }

    private val HEX = Regex("^([0-9a-f]{2})+$")

    private fun fromHex(value: String) = value.chunked(2).map { it.toInt(16).toByte() }.toByteArray()

    private fun fromBase64(value: String): ByteArray? =
        runCatching {
            base64url.decode(unpadded(value))
        }.getOrNull()

    private fun unpadded(value: String) = value.trimEnd('=')

    private fun JsonPrimitive.contentOrNullSafe(): String? =
        if (this is kotlinx.serialization.json.JsonNull) null else content
}
