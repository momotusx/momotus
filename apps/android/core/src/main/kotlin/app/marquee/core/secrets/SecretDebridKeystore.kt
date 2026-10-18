package app.marquee.core.secrets

import app.marquee.core.DebridCopy
import app.marquee.core.model.DebridRecord
import app.marquee.core.model.DebridTombstone
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.IOException

/** The keystore as the accounts controller sees it: the helper's, plus the keys from before records. */
interface LegacyDebridKeystore : DebridKeystore {
    /** `(service, key)` pairs from before debrid records, still to move. */
    suspend fun legacy(): List<Pair<String, String>>
}

/**
 * The debrid keystore over the encrypted [SecretStore] (plans/c10-handoff.md, phase 3a): records and
 * tombstones live in the same sealed file as the sync record. A write the store could keep in memory
 * only fails, so the helper never acknowledges a record that would be gone after a restart.
 *
 * A record or a tombstone for a service drops that service's legacy key in the same file write, so a
 * half-finished move from legacy keys never overwrites a later sign-in or brings back a disconnect.
 */
class SecretDebridKeystore(
    private val secrets: SecretStore,
) : LegacyDebridKeystore {
    // In the services' order, the order the legacy list kept: `restoreDebrid` makes the first the
    // default, so a token refresh never changes which store that is.
    override suspend fun records(): List<DebridRecord> =
        withContext(Dispatchers.IO) { inServiceOrder(secrets.load().debridRecords) }

    override suspend fun tombstones(): List<DebridTombstone> =
        withContext(Dispatchers.IO) {
            secrets.load().debridTombstones
        }

    override suspend fun legacy(): List<Pair<String, String>> =
        withContext(Dispatchers.IO) { secrets.load().debridKeys }

    override suspend fun writeRecord(record: DebridRecord) =
        change { s ->
            s.copy(
                debridRecords = s.debridRecords.filter { it.service != record.service } + record,
                debridKeys = s.debridKeys.filter { it.first != record.service },
            )
        }

    override suspend fun deleteRecord(
        service: String,
        generation: String,
    ) = change { s ->
        s.copy(debridRecords = s.debridRecords.filterNot { it.service == service && it.generation == generation })
    }

    override suspend fun writeTombstone(tombstone: DebridTombstone) =
        change { s ->
            s.copy(
                debridTombstones =
                    if (tombstone in
                        s.debridTombstones
                    ) {
                        s.debridTombstones
                    } else {
                        s.debridTombstones + tombstone
                    },
                debridKeys = s.debridKeys.filter { it.first != tombstone.service },
            )
        }

    override suspend fun deleteTombstone(tombstone: DebridTombstone) =
        change { s -> s.copy(debridTombstones = s.debridTombstones - tombstone) }

    // On disk or not at all: a write that fails leaves memory as it was, so nothing the helper was
    // told failed can reach the file later with another write.
    private suspend fun change(edit: (SecretStore.Secrets) -> SecretStore.Secrets) =
        withContext(Dispatchers.IO) {
            if (!secrets.updateDurably(edit)) throw IOException("the keystore could not store it")
        }

    companion object {
        /** Known services first, in [DebridCopy.services] order; any other after them, as stored. */
        fun inServiceOrder(records: List<DebridRecord>): List<DebridRecord> {
            val rank = DebridCopy.services.withIndex().associate { (i, s) -> s.id to i }
            return records.sortedBy { rank[it.service] ?: Int.MAX_VALUE }
        }
    }
}
