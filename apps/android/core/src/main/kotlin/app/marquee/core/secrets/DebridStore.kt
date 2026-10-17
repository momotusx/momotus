package app.marquee.core.secrets

import app.marquee.core.model.DebridRecord
import app.marquee.core.model.DebridTombstone
import app.marquee.core.model.RestoreAnswer
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Job
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.launch

/** The platform keystore as the debrid helper sees it. */
interface DebridKeystore {
    suspend fun records(): List<DebridRecord>

    suspend fun tombstones(): List<DebridTombstone>

    /** One record per service: replaces whatever the service had. */
    suspend fun writeRecord(record: DebridRecord)

    /** Removes the service's record only when it has this generation. */
    suspend fun deleteRecord(
        service: String,
        generation: String,
    )

    suspend fun writeTombstone(tombstone: DebridTombstone)

    suspend fun deleteTombstone(tombstone: DebridTombstone)
}

/** The core calls the helper makes: `restoreDebrid`, `ackDebrid`, `disconnectDebrid`. */
interface DebridCore {
    suspend fun restore(
        records: List<DebridRecord>,
        tombstones: List<DebridTombstone>,
    ): RestoreAnswer

    suspend fun ack(
        service: String,
        generation: String,
        revision: Long,
    )

    suspend fun disconnect(
        service: String,
        generation: String,
    )
}

/**
 * Keeps the keystore in step with the core's debrid accounts (plans/core-protocol.md, Debrid).
 * Every keystore change for a service runs through one serial queue, in the order the core
 * produced the records and the user acted. The shared cases are
 * apps/shared/debrid-storage-vectors.json; the web and Apple helpers replay the same file.
 */
class DebridStore(
    private val keystore: DebridKeystore,
    private val core: DebridCore,
    private val scope: CoroutineScope,
) {
    private class Task(
        val generation: String,
        val write: Boolean,
        val run: suspend () -> Unit,
    ) {
        @Volatile var dropped = false
        val done = CompletableDeferred<Unit>()
    }

    private val lock = Any()
    private val queues = HashMap<String, ArrayDeque<Task>>()
    private val pumps = HashMap<String, Job>()

    /** Generations with a tombstone written or queued: never written again. */
    private val gone = HashSet<String>()

    /** Tombstones whose record delete failed; retried with the next push. */
    private val unfinished = LinkedHashMap<String, DebridTombstone>()
    private val disconnecting = HashMap<String, Deferred<Unit>>()

    /** At start, before any view: finish interrupted disconnects, then restore. */
    suspend fun start() {
        for (tombstone in keystore.tombstones()) {
            synchronized(lock) { gone += tombstone.generation }
            try {
                finish(tombstone)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                synchronized(lock) { unfinished[tombstone.generation] = tombstone }
            }
        }
        // Records with a tombstone go too: the core refuses them, and a tombstone whose record
        // is still stored is never handed back as deletable.
        val answer = core.restore(keystore.records(), keystore.tombstones())
        for (tombstone in answer.tombstones) runCatching { keystore.deleteTombstone(tombstone) }
    }

    /** Every `debrid` view push: store each pending record, then acknowledge it. */
    fun pending(records: List<DebridRecord>) {
        val retry =
            synchronized(lock) {
                unfinished.values.toList().also { unfinished.clear() }
            }
        for (tombstone in retry) {
            val done = enqueue(tombstone.service, tombstone.generation, write = false) { finish(tombstone) }
            done.invokeOnCompletion { failure ->
                if (failure != null) synchronized(lock) { unfinished[tombstone.generation] = tombstone }
            }
        }
        // A failed write is not acknowledged; the next push carries it again.
        for (record in records) enqueue(record.service, record.generation, write = true) { save(record) }
    }

    /**
     * Disconnects one generation: tombstone, core, record, then the tombstone. Fails when the
     * tombstone cannot be written: nothing changed, the account stays connected. A second call
     * for the same generation shares the first.
     */
    fun disconnect(
        service: String,
        generation: String,
    ): Deferred<Unit> {
        val tombstone = DebridTombstone(service, generation)
        // One step under the lock (it is reentrant): a concurrent second call shares this one.
        synchronized(lock) {
            disconnecting[generation]?.let { return it }
            gone += generation
            queues[service]?.forEach { if (it.write && it.generation == generation) it.dropped = true }
            val done =
                enqueue(service, generation, write = false) {
                    try {
                        keystore.writeTombstone(tombstone)
                    } catch (e: Exception) {
                        synchronized(lock) { gone -= generation }
                        throw e
                    }
                    core.disconnect(service, generation)
                    try {
                        finish(tombstone)
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        synchronized(lock) { unfinished[generation] = tombstone }
                    }
                }
            disconnecting[generation] = done
            done.invokeOnCompletion { synchronized(lock) { disconnecting.remove(generation) } }
            return done
        }
    }

    /** Returns once every queue is idle (tests, shutdown). */
    suspend fun idle() {
        while (true) {
            val running = synchronized(lock) { pumps.values.toList() }
            if (running.isEmpty()) return
            running.joinAll()
        }
    }

    private suspend fun save(record: DebridRecord) {
        val stored = keystore.records().find { it.service == record.service }
        val newer = stored?.generation == record.generation && stored.revision > record.revision
        if (!newer) keystore.writeRecord(record)
        core.ack(record.service, record.generation, record.revision)
    }

    /** Deletes the record, reads back that it is gone, then the tombstone. */
    private suspend fun finish(tombstone: DebridTombstone) {
        keystore.deleteRecord(tombstone.service, tombstone.generation)
        val left =
            keystore.records().any {
                it.service == tombstone.service && it.generation == tombstone.generation
            }
        check(!left) { "record still stored" }
        keystore.deleteTombstone(tombstone)
    }

    private fun enqueue(
        service: String,
        generation: String,
        write: Boolean,
        run: suspend () -> Unit,
    ): CompletableDeferred<Unit> {
        val task = Task(generation, write, run)
        val pump =
            synchronized(lock) {
                // Checked with the insert, under one lock: a write of a generation
                // that a disconnect marked gone is never queued behind its cleanup.
                if (write && generation in gone) {
                    task.done.complete(Unit)
                    return task.done
                }
                queues.getOrPut(service) { ArrayDeque() }.addLast(task)
                if (service in pumps) {
                    null
                } else {
                    // Registered before it runs, so its own exit can remove it.
                    scope.launch(start = CoroutineStart.LAZY) { pump(service) }.also { pumps[service] = it }
                }
            }
        pump?.start()
        return task.done
    }

    private suspend fun pump(service: String) {
        while (true) {
            // In the same step as the empty check: a task queued later starts a new pump.
            val task =
                synchronized(lock) {
                    queues[service]?.removeFirstOrNull().also { if (it == null) pumps.remove(service) }
                } ?: return
            if (task.dropped) {
                task.done.complete(Unit)
                continue
            }
            try {
                task.run()
                task.done.complete(Unit)
            } catch (e: CancellationException) {
                // The scope is going: settle everything queued and unregister, so
                // `idle` returns and a later task starts a new pump.
                task.done.completeExceptionally(e)
                val left =
                    synchronized(lock) {
                        pumps.remove(service)
                        queues.remove(service)?.toList().orEmpty()
                    }
                left.forEach { it.done.completeExceptionally(e) }
                throw e
            } catch (e: Exception) {
                task.done.completeExceptionally(e)
            }
        }
    }
}
