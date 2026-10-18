package app.marquee.core

import app.marquee.core.model.DebridCredential
import app.marquee.core.model.DebridRecord
import app.marquee.core.model.DebridTombstone
import app.marquee.core.model.RestoreAnswer
import app.marquee.core.secrets.DebridCore
import app.marquee.core.secrets.KeySource
import app.marquee.core.secrets.LegacyDebridKeystore
import app.marquee.core.secrets.SecretDebridKeystore
import app.marquee.core.secrets.SecretStore
import app.marquee.core.session.DebridAccounts
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import java.io.File
import java.nio.file.Files
import javax.crypto.KeyGenerator
import kotlin.test.Test
import kotlin.test.assertEquals

/** The accounts controller over the real sealed keystore (plans/c10-handoff.md, phase 3a, revised). */
class DebridAccountsTest {
    private class SoftwareKeys : KeySource {
        private val key = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()

        override fun key() = key

        override fun reset() = Unit
    }

    /** Hands out generations for keys and remembers what it was asked, the way the core answers. */
    private class Core : DebridCore {
        val restores = mutableListOf<List<String>>()
        val restoredGenerations = mutableListOf<List<String>>()
        val acks = mutableListOf<String>()
        val connects = mutableListOf<String>()
        var failConnectAfter = Int.MAX_VALUE
        private var next = 0

        fun connect(
            service: String,
            key: String,
        ): DebridRecord {
            if (connects.size >= failConnectAfter) error("core gone")
            connects += service
            next++
            return DebridRecord(service, "g$next", 1, DebridCredential.Key(key))
        }

        override suspend fun restore(
            records: List<DebridRecord>,
            tombstones: List<DebridTombstone>,
        ): RestoreAnswer {
            restores += records.map { it.service }
            restoredGenerations += records.map { it.generation }
            return RestoreAnswer(refused = emptyList(), tombstones = emptyList())
        }

        override suspend fun ack(
            service: String,
            generation: String,
            revision: Long,
        ) {
            acks += "$service $generation"
        }

        override suspend fun disconnect(
            service: String,
            generation: String,
        ) = Unit
    }

    private val file = File(Files.createTempDirectory("debrid").toFile(), "secrets.bin")
    private val secrets = SecretStore(file, SoftwareKeys())
    private val keystore = SecretDebridKeystore(secrets)
    private val core = Core()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    /** A controller whose connections come back as a push, as the `debrid` view brings them. */
    private fun accounts(): DebridAccounts {
        lateinit var accounts: DebridAccounts
        accounts =
            DebridAccounts(keystore, core, connect = { s, k -> accounts.pushed(listOf(core.connect(s, k))) }, scope)
        return accounts
    }

    private fun legacy(vararg pairs: Pair<String, String>) = secrets.update { it.copy(debridKeys = pairs.toList()) }

    @Test
    fun `legacy keys move one by one, and a half-finished move goes on at the next start`() =
        runBlocking {
            legacy("premiumize" to "p", "realdebrid" to "r", "torbox" to "t")
            core.failConnectAfter = 1
            val first = accounts()
            first.opened { null }
            first.store.idle()
            // One moved: its record is stored and its legacy key is gone; the rest wait.
            assertEquals(listOf("premiumize"), secrets.load().debridRecords.map { it.service })
            assertEquals(listOf("realdebrid", "torbox"), secrets.load().debridKeys.map { it.first })

            core.failConnectAfter = Int.MAX_VALUE
            val second = accounts()
            second.opened { null }
            second.store.idle()
            assertEquals(emptyList(), secrets.load().debridKeys)
            assertEquals(listOf("premiumize", "realdebrid", "torbox"), core.connects)
            // The first record was not replaced by a second connection.
            assertEquals(
                "g1",
                secrets
                    .load()
                    .debridRecords
                    .first { it.service == "premiumize" }
                    .generation,
            )
        }

    @Test
    fun `a sign-in or a disconnect between two starts is never undone by the legacy key`() =
        runBlocking {
            legacy("realdebrid" to "old", "torbox" to "t")
            // Real-Debrid signed in by code meanwhile; TorBox was disconnected.
            keystore.writeRecord(DebridRecord("realdebrid", "code", 1, DebridCredential.Key("new")))
            keystore.writeTombstone(DebridTombstone("torbox", "gone"))
            assertEquals(emptyList(), secrets.load().debridKeys)
            val accounts = accounts()
            accounts.opened { null }
            accounts.store.idle()
            assertEquals(emptyList(), core.connects)
            assertEquals(
                "code",
                secrets
                    .load()
                    .debridRecords
                    .single()
                    .generation,
            )
        }

    @Test
    fun `records keep the services' order, so the default never moves`() =
        runBlocking {
            for (service in listOf("premiumize", "torbox", "realdebrid")) {
                keystore.writeRecord(DebridRecord(service, service, 1, DebridCredential.Key("k")))
            }
            // A token refresh rewrites Real-Debrid last.
            keystore.writeRecord(DebridRecord("realdebrid", "realdebrid", 2, DebridCredential.Key("k")))
            assertEquals(listOf("realdebrid", "torbox", "premiumize"), keystore.records().map { it.service })
            val accounts = accounts()
            accounts.opened { null }
            assertEquals(listOf("realdebrid", "torbox", "premiumize"), core.restores.single())
        }

    @Test
    fun `pushes before this core's restore are held, then the current pending goes once`() =
        runBlocking {
            val accounts = accounts()
            val waiting = DebridRecord("alldebrid", "a1", 1, DebridCredential.Key("a"))
            accounts.pause()
            accounts.pushed(listOf(waiting))
            accounts.store.idle()
            assertEquals(emptyList(), core.acks)
            accounts.opened { listOf(waiting) }
            accounts.store.idle()
            assertEquals(listOf("alldebrid a1"), core.acks)
            assertEquals(listOf("alldebrid"), secrets.load().debridRecords.map { it.service })
        }

    @Test
    fun `a restart waits for the write the last core left queued`() =
        runBlocking {
            val gate = CompletableDeferred<Unit>()
            val gated =
                object : LegacyDebridKeystore by keystore {
                    override suspend fun writeRecord(record: DebridRecord) {
                        if (record.generation == "g2") gate.await()
                        keystore.writeRecord(record)
                    }
                }
            val accounts = DebridAccounts(gated, core, connect = { _, _ -> }, scope)
            keystore.writeRecord(DebridRecord("realdebrid", "g1", 1, DebridCredential.Key("old")))
            accounts.opened { null }
            // The core replaced the connection; its record is queued, and the core dies before it is stored.
            accounts.pushed(listOf(DebridRecord("realdebrid", "g2", 1, DebridCredential.Key("new"))))
            accounts.pause()
            val restart = scope.launch { accounts.opened { null } }
            delay(300)
            assertEquals(1, core.restores.size, "the restore waits for the queued write")
            gate.complete(Unit)
            restart.join()
            assertEquals(listOf("g2"), core.restoredGenerations.last())
        }
}
