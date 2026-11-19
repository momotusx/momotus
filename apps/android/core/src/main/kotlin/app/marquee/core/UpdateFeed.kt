package app.marquee.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import java.security.KeyFactory
import java.security.PublicKey
import java.security.Signature
import java.security.spec.X509EncodedKeySpec
import kotlin.io.encoding.Base64

/**
 * The update feed (plans/auto-update.md, "Discovery"): `latest.json` on the `updates` release, an envelope
 * `{"signed": "<JSON text>", "sig": "<base64 DER ECDSA P-256 SHA-256 over signed>"}` written by
 * .github/scripts/update-feed.sh. The app verifies it against [KEY], then picks the APK of its own ABI.
 */
object UpdateFeed {
    /** The public half of the release environment's UPDATE_FEED_KEY. */
    const val KEY =
        "-----BEGIN PUBLIC KEY-----\n" +
            "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEb0Roduc6c3tj7mEvcIpb+KB8qS3P\n" +
            "sr14L1bFYSTs86jyf5roDGA2LNw892HgA/0q2KcRHPVAL66kzX5x6KbYaw==\n" +
            "-----END PUBLIC KEY-----\n"

    /** The feed's own size limit: a few hundred bytes today. */
    const val MAX_FEED_BYTES = 64 * 1024

    /** An APK larger than this is refused before any download. */
    const val MAX_APK_BYTES = 200L * 1024 * 1024

    /** One APK the feed offers this device. */
    data class Offer(
        val version: String,
        val versionCode: Long,
        val url: String,
        val sha256: String,
        val size: Long,
    )

    fun publicKey(pem: String): PublicKey {
        val der =
            pem
                .lineSequence()
                .filterNot { it.startsWith("-----") }
                .joinToString("")
                .trim()
        return KeyFactory.getInstance("EC").generatePublic(X509EncodedKeySpec(Base64.decode(der)))
    }

    /** The feed's body when [envelope] is one and carries [key]'s signature over it; null otherwise. */
    fun verified(
        envelope: String,
        key: PublicKey,
    ): JsonObject? =
        runCatching {
            val outer = Json.parseToJsonElement(envelope).jsonObject
            val signed = outer.getValue("signed").jsonPrimitive.content
            val sig = Base64.decode(outer.getValue("sig").jsonPrimitive.content)
            val ok =
                Signature.getInstance("SHA256withECDSA").run {
                    initVerify(key)
                    update(signed.toByteArray(Charsets.UTF_8))
                    verify(sig)
                }
            if (ok) Json.parseToJsonElement(signed).jsonObject else null
        }.getOrNull()

    /**
     * The APK for the running build: the role of its ABI digit (the last of [installed], 2 arm64, 1 armv7), offered
     * only when its versionCode is higher than [installed]. A malformed entry offers nothing.
     */
    fun pick(
        feed: JsonObject,
        installed: Long,
    ): Offer? {
        val abi = (installed % 10).toInt()
        val role = ROLES[abi] ?: return null
        val entry = runCatching { feed[role]?.jsonObject }.getOrNull() ?: return null

        fun text(name: String) = runCatching { entry[name]?.jsonPrimitive?.contentOrNull }.getOrNull()
        val version = text("version") ?: return null
        val url = text("url")?.takeIf { it.startsWith("https://") } ?: return null
        val sha256 = text("sha256")?.takeIf { SHA256.matches(it) } ?: return null
        val size =
            runCatching { entry["size"]?.jsonPrimitive?.longOrNull }.getOrNull()?.takeIf { it in 1..MAX_APK_BYTES }
                ?: return null
        val code = versionCode(version, abi) ?: return null
        return if (code > installed) Offer(version, code, url, sha256, size) else null
    }

    /** build.gradle.kts's versionCode: (MAJOR*1_000_000 + MINOR*1_000 + PATCH)*10 + the ABI digit. */
    fun versionCode(
        version: String,
        abi: Int,
    ): Long? {
        val parts = version.split('.').map { it.toLongOrNull() ?: return null }
        if (parts.size != 3 || parts.any { it < 0 } || parts[1] > 999 || parts[2] > 999 || parts[0] >= 210) return null
        return (parts[0] * 1_000_000 + parts[1] * 1_000 + parts[2]) * 10 + abi
    }

    /**
     * A downloaded APK may be installed: our package, the offered versionCode, and the same signers as the running app
     * (each a SHA-256 of a certificate). Android refuses another signer too; this stops it before a session.
     */
    fun archiveMatches(
        offer: Offer,
        packageName: String?,
        versionCode: Long?,
        signers: Set<String>,
        ownPackage: String,
        ownSigners: Set<String>,
    ): Boolean =
        packageName == ownPackage &&
            versionCode == offer.versionCode &&
            signers.isNotEmpty() &&
            signers == ownSigners

    private val ROLES = mapOf(2 to "android-arm64", 1 to "android-armv7")
    private val SHA256 = Regex("[0-9a-f]{64}")
}

/** When the updater may act (plans/auto-update.md section 1). */
object UpdateRules {
    /**
     * The install needs no press: API 31 and later, "Install unknown apps" allowed for Momotus, and no other package
     * owns its updates. The system can still ask (its throttle, a later target floor): the session then reports that.
     */
    fun silent(
        sdk: Int,
        canRequestInstalls: Boolean,
        updateOwner: String?,
        ownPackage: String,
    ): Boolean = sdk >= 31 && canRequestInstalls && (updateOwner == null || updateOwner == ownPackage)

    /** A check or a download that nobody asked for waits while something plays, and a download waits off a metered network. */
    fun mayDownload(
        manual: Boolean,
        metered: Boolean,
        playing: Boolean,
    ): Boolean = manual || (!metered && !playing)

    /** The silent install runs when the app has gone to the background with nothing playing and no download running. */
    fun mayCommitInBackground(
        playing: Boolean,
        downloading: Boolean,
    ): Boolean = !playing && !downloading
}
