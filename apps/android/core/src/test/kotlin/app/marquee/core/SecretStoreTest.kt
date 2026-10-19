package app.marquee.core

import app.marquee.core.secrets.DeviceId
import app.marquee.core.secrets.KeySource
import app.marquee.core.secrets.SecretStore
import app.marquee.core.trakt.SecretTraktRecords
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.io.File
import java.nio.file.Files
import java.security.ProviderException
import java.security.SecureRandom
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotEquals
import kotlin.test.assertTrue

class SecretStoreTest {
    private class SoftwareKeys : KeySource {
        private var key: SecretKey = make()

        private fun make() = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()

        override fun key() = key

        override fun reset() {
            key = make()
        }
    }

    private val dir: File = Files.createTempDirectory("secrets").toFile()
    private val file = File(dir, "secrets.bin")
    private val record =
        JsonObject(
            mapOf(
                "server" to JsonPrimitive("https://sync.example"),
                "token" to JsonPrimitive("token-abc"),
            ),
        )

    @Test
    fun `secrets round trip and never sit on disk in plaintext`() {
        val keys = SoftwareKeys()
        SecretStore(
            file,
            keys,
        ).update { it.copy(debridKeys = listOf("realdebrid" to "RD-SECRET-KEY"), syncRecord = record) }

        val text = String(file.readBytes(), Charsets.ISO_8859_1)
        assertFalse(text.contains("RD-SECRET-KEY"))
        assertFalse(text.contains("token-abc"))

        val loaded = SecretStore(file, keys).load()
        assertEquals(listOf("realdebrid" to "RD-SECRET-KEY"), loaded.debridKeys)
        assertEquals(record, loaded.syncRecord)
    }

    @Test
    fun `a blob that was tampered with is cleared, not fatal`() {
        val keys = SoftwareKeys()
        SecretStore(file, keys).update { it.copy(debridKeys = listOf("torbox" to "TB")) }
        file.writeBytes(file.readBytes().also { it[it.size - 1] = (it[it.size - 1].toInt() xor 1).toByte() })

        val store = SecretStore(file, keys)
        assertTrue(store.load().debridKeys.isEmpty())
        assertEquals(SecretStore.Health.Reset, store.health)
        assertFalse(file.exists())
    }

    @Test
    fun `the header is bound to the blob`() {
        val keys = SoftwareKeys()
        SecretStore(file, keys).update { it.copy(debridKeys = listOf("torbox" to "TB")) }
        // Flipping a bit of the IV makes the tag fail: the IV is not free to change.
        file.writeBytes(file.readBytes().also { it[3] = (it[3].toInt() xor 1).toByte() })
        assertEquals(SecretStore.Health.Reset, SecretStore(file, keys).also { it.load() }.health)
    }

    @Test
    fun `a key from another device cannot open a restored blob`() {
        SecretStore(file, SoftwareKeys()).update { it.copy(debridKeys = listOf("torbox" to "TB")) }
        val otherDevice = SecretStore(file, SoftwareKeys())
        assertTrue(otherDevice.load().debridKeys.isEmpty())
        assertEquals(SecretStore.Health.Reset, otherDevice.health)
    }

    @Test
    fun `a Keystore that is not ready keeps the file and never overwrites it`() {
        val good = SoftwareKeys()
        SecretStore(file, good).update { it.copy(debridKeys = listOf("torbox" to "TB")) }
        val before = file.readBytes()

        val flaky =
            object : KeySource {
                override fun key(): SecretKey = throw ProviderException("keystore is not ready yet")

                override fun reset() = Unit
            }
        val store = SecretStore(file, flaky)
        assertTrue(store.load().debridKeys.isEmpty())
        assertEquals(SecretStore.Health.MemoryOnly, store.health)
        store.update { it.copy(debridKeys = listOf("alldebrid" to "AD")) }

        assertTrue(file.readBytes().contentEquals(before)) // one flaky boot does not cost the account
        assertEquals(listOf("torbox" to "TB"), SecretStore(file, good).load().debridKeys)
    }

    @Test
    fun `a Keystore that cannot make a key keeps secrets in memory only`() {
        val broken =
            object : KeySource {
                override fun key(): SecretKey = throw ProviderException("keystore is broken")

                override fun reset() = Unit
            }
        val store = SecretStore(file, broken)
        store.update { it.copy(debridKeys = listOf("alldebrid" to "AD")) }
        assertEquals(SecretStore.Health.MemoryOnly, store.health)
        assertFalse(file.exists())
        assertEquals(listOf("alldebrid" to "AD"), store.load().debridKeys)
    }

    @Test
    fun `a durable change that cannot be stored leaves memory as it was`() {
        var failing = false
        val keys = SoftwareKeys()
        val switchable =
            object : KeySource {
                override fun key(): SecretKey = if (failing) throw ProviderException("keystore is busy") else keys.key()

                override fun reset() = Unit
            }
        val store = SecretStore(file, switchable)
        store.update { it.copy(syncRecord = record) }
        failing = true
        assertFalse(store.updateDurably { it.copy(debridKeys = listOf("torbox" to "TB")) })
        assertTrue(store.load().debridKeys.isEmpty())
        // A later write of something else never stores the refused change by accident.
        failing = false
        assertTrue(store.updateDurably { it.copy(syncRecord = null) })
        assertTrue(SecretStore(file, switchable).load().debridKeys.isEmpty())
    }

    @Test
    fun `a Trakt record's delete counts only once it is on disk`() {
        var failing = false
        val keys = SoftwareKeys()
        val switchable =
            object : KeySource {
                override fun key(): SecretKey = if (failing) throw ProviderException("keystore is busy") else keys.key()

                override fun reset() = Unit
            }
        val records = SecretTraktRecords(SecretStore(file, switchable))
        assertTrue(records.save("p1", record))
        failing = true
        // The file cannot be written: the delete says so, and the record is still there, also after a restart.
        assertFalse(records.delete("p1"))
        assertEquals(record, records.load("p1"))
        failing = false
        assertEquals(record, SecretTraktRecords(SecretStore(file, switchable)).load("p1"))
        assertTrue(records.delete("p1"))
        assertEquals(null, SecretTraktRecords(SecretStore(file, switchable)).load("p1"))
    }

    @Test
    fun `a busy Keystore during a write never costs the stored accounts`() {
        var busy = false
        val keys = SoftwareKeys()
        val source =
            object : KeySource {
                override fun key(): SecretKey = if (busy) throw ProviderException("keystore is busy") else keys.key()

                override fun reset() = keys.reset()
            }
        SecretStore(file, source).update { it.copy(debridKeys = listOf("torbox" to "TB")) }
        val store = SecretStore(file, source)
        assertEquals(listOf("torbox" to "TB"), store.load().debridKeys)
        busy = true
        assertFalse(store.updateDurably { it.copy(syncRecord = record) })
        busy = false
        // The key that sealed the file is still the one there: its accounts open after a restart.
        assertEquals(listOf("torbox" to "TB"), SecretStore(file, source).load().debridKeys)
    }

    @Test
    fun `printing the secrets never shows them`() {
        val text = SecretStore.Secrets(listOf("realdebrid" to "RD-SECRET-KEY"), record).toString()
        assertFalse(text.contains("RD-SECRET-KEY"))
        assertFalse(text.contains("token-abc"))
    }

    @Test
    fun `the device id is stable and random`() {
        val a = DeviceId.load(dir)
        assertNotEquals(0UL, a)
        assertEquals(a, DeviceId.load(dir))
        assertNotEquals(a, DeviceId.load(Files.createTempDirectory("other").toFile()))
    }

    @Test
    fun `the device id is never zero even if the random source says so`() {
        val zeroFirst =
            object : SecureRandom() {
                private var calls = 0

                override fun nextLong(): Long = if (calls++ == 0) 0L else 42L
            }
        assertEquals(42UL, DeviceId.load(Files.createTempDirectory("zero").toFile(), zeroFirst))
    }
}
