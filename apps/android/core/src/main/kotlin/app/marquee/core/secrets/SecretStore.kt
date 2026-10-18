package app.marquee.core.secrets

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.client.CoreLog
import app.marquee.core.model.DebridRecord
import app.marquee.core.model.DebridTombstone
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerializationException
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import java.io.File
import java.io.FileOutputStream
import java.security.GeneralSecurityException
import java.security.InvalidKeyException
import java.security.KeyStore
import java.security.ProviderException
import java.security.UnrecoverableKeyException
import javax.crypto.BadPaddingException
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Where the AES key comes from: the Android Keystore on a device, a plain key in tests. */
interface KeySource {
    fun key(): SecretKey

    /** Forget the key (it could not decrypt, or the Keystore misbehaved); the next [key] makes a new one. */
    fun reset()
}

/**
 * AES-256-GCM key `marquee.secrets.v1` in the AndroidKeystore. It never leaves the secure
 * hardware, needs no user authentication (TVs have no lock screen) and is not backed up, so a
 * restored copy of the encrypted file cannot be read on another device. StrongBox is not asked
 * for: it is slow and most apps do not need it (Android's own guidance).
 */
class KeystoreKeySource(
    private val alias: String = "marquee.secrets.v1",
) : KeySource {
    private fun store() = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }

    override fun key(): SecretKey {
        (store().getKey(alias, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec
                .Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setRandomizedEncryptionRequired(true)
                .build(),
        )
        return generator.generateKey()
    }

    override fun reset() {
        runCatching { store().deleteEntry(alias) }
    }
}

/**
 * Debrid keys and the sync record, sealed as `version(1) || iv(12) || ciphertext` with AAD
 * `marquee/secrets/v1` and written atomically (temp file, fsync, rename). The core keeps none of
 * these; the app hands them over per call. The sync record is opaque JSON handed back verbatim.
 *
 * Failure never crashes and never writes plaintext:
 * - A blob that cannot be opened for good (bad tag, a key from another device, an invalidated
 *   key) is deleted: the user signs in again.
 * - Anything that may pass (an I/O error, a Keystore provider that is not ready at boot, a newer
 *   format after a downgrade) leaves the file alone, runs from memory, and does not overwrite
 *   the file this session, so one flaky boot cannot cost the account.
 * - A Keystore that cannot make a key keeps secrets in memory for this session.
 * A write that fails leaves the previous blob on disk, so a removed key can come back after a
 * restart; that is the price of never losing the whole file.
 */
class SecretStore(
    private val file: File,
    private val keys: KeySource,
    private val json: Json = Json { ignoreUnknownKeys = true },
) {
    enum class Health { Ok, Reset, MemoryOnly }

    data class Secrets(
        /** Legacy `(service, key)` pairs from before debrid records; moved to records once (C10 3a). */
        val debridKeys: List<Pair<String, String>> = emptyList(),
        val syncRecord: JsonObject? = null,
        /** One record per connected debrid service (plans/c10-handoff.md A1). */
        val debridRecords: List<DebridRecord> = emptyList(),
        /** Written before a disconnect is sent: a record with this generation is never restored. */
        val debridTombstones: List<DebridTombstone> = emptyList(),
    ) {
        // The default would print every key and token.
        override fun toString() =
            "Secrets(debridKeys=${debridKeys.size}, syncRecord=${syncRecord != null}, " +
                "debridRecords=${debridRecords.size}, debridTombstones=${debridTombstones.size})"
    }

    @Serializable
    private data class Blob(
        val debridKeys: List<List<String>> = emptyList(),
        val syncRecord: JsonObject? = null,
        val debridRecords: List<DebridRecord> = emptyList(),
        val debridTombstones: List<DebridTombstone> = emptyList(),
    )

    /** Observable, so a screen that says "kept for this session" updates when a write fails. */
    var health: Health by mutableStateOf(Health.Ok)
        private set

    private var current: Secrets? = null
    private var protectExistingFile = false

    /**
     * The file on disk was sealed with the key there now (it was read or written with it). From then
     * on a failed write never resets the key: that would leave the file's accounts unreadable.
     */
    private var keyHoldsFile = false

    @Synchronized
    fun load(): Secrets {
        current?.let { return it }
        val loaded = readFromDisk()
        current = loaded
        return loaded
    }

    @Synchronized
    fun update(change: (Secrets) -> Secrets): Secrets {
        val next = change(load())
        current = next
        if (protectExistingFile) {
            health = Health.MemoryOnly
        } else {
            writeToDisk(next, retry = true)
        }
        return next
    }

    /**
     * A change that counts only once it is on disk: when the file cannot be written, memory stays as
     * it was too, so a later write of something else never stores it by accident (a tombstone whose
     * disconnect was refused, say). Returns whether it was stored.
     */
    @Synchronized
    fun updateDurably(change: (Secrets) -> Secrets): Boolean {
        if (protectExistingFile) return false
        val next = change(load())
        writeToDisk(next, retry = true)
        if (health == Health.MemoryOnly) return false
        current = next
        return true
    }

    private fun readFromDisk(): Secrets {
        if (!file.exists()) return Secrets()
        return try {
            val sealed = file.readBytes()
            check(sealed.size > 1 + IV_BYTES && sealed[0] == VERSION) { "unsupported secrets format" }
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, keys.key(), GCMParameterSpec(TAG_BITS, sealed, 1, IV_BYTES))
            cipher.updateAAD(AAD)
            val plain = cipher.doFinal(sealed, 1 + IV_BYTES, sealed.size - 1 - IV_BYTES)
            val blob = json.decodeFromString(Blob.serializer(), plain.decodeToString())
            keyHoldsFile = true
            Secrets(
                blob.debridKeys.mapNotNull { if (it.size == 2) it[0] to it[1] else null },
                blob.syncRecord,
                blob.debridRecords,
                blob.debridTombstones,
            )
        } catch (e: Exception) {
            if (e.cannotOpenForGood()) {
                CoreLog.w("the stored secrets cannot be opened, clearing them: ${CoreLog.describe(e)}")
                file.delete()
                keys.reset()
                health = Health.Reset
            } else {
                CoreLog.w("the stored secrets could not be read this time, keeping the file: ${CoreLog.describe(e)}")
                health = Health.MemoryOnly
                protectExistingFile = true
            }
            Secrets()
        }
    }

    private fun Exception.cannotOpenForGood() =
        this is BadPaddingException || // includes AEADBadTagException: tampered or from another device
            this is UnrecoverableKeyException ||
            this is InvalidKeyException || // includes KeyPermanentlyInvalidatedException
            this is SerializationException

    private fun writeToDisk(
        secrets: Secrets,
        retry: Boolean,
    ) {
        try {
            val blob =
                Blob(
                    secrets.debridKeys.map { listOf(it.first, it.second) },
                    secrets.syncRecord,
                    secrets.debridRecords,
                    secrets.debridTombstones,
                )
            val plain = json.encodeToString(Blob.serializer(), blob).encodeToByteArray()
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, keys.key())
            cipher.updateAAD(AAD)
            val body = cipher.doFinal(plain)
            val sealed = byteArrayOf(VERSION) + cipher.iv + body
            file.parentFile?.mkdirs()
            val temp = File(file.parentFile, file.name + ".tmp")
            FileOutputStream(temp).use {
                it.write(sealed)
                it.fd.sync()
            }
            check(temp.renameTo(file)) { "rename of the secrets file failed" }
            keyHoldsFile = true
            if (health == Health.MemoryOnly) health = Health.Ok
        } catch (e: Exception) {
            // A Keystore entry that is present but broken keeps every write failing: make a new key
            // once. Never when the file on disk was sealed with it: a busy Keystore would then cost
            // every account stored there, while this write fails into memory only.
            if (retry && !keyHoldsFile && (e is GeneralSecurityException || e is ProviderException)) {
                keys.reset()
                writeToDisk(secrets, retry = false)
                return
            }
            CoreLog.e("secrets cannot be stored; keeping them in memory for this session: ${CoreLog.describe(e)}")
            health = Health.MemoryOnly
        }
    }

    private companion object {
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_BYTES = 12
        const val TAG_BITS = 128
        const val VERSION: Byte = 1
        val AAD = "marquee/secrets/v1".encodeToByteArray()
    }
}
