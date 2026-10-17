package app.marquee.core.client

data class Percentiles(
    val count: Int,
    val p50Us: Long,
    val p90Us: Long,
    val p99Us: Long,
    val maxUs: Long,
) {
    fun toJson(): String = """{"count":$count,"p50_us":$p50Us,"p90_us":$p90Us,"p99_us":$p99Us,"max_us":$maxUs}"""
}

/**
 * Timings for the bridge budget (architecture section 12: p99 emit-to-applied at most 2 ms).
 * `emitToApplied` runs from the moment a message reaches the Kotlin callback until its batch is
 * applied to view state; `batch` is the parse and apply time of one batch. Recording is cheap
 * (two array writes under a lock) and can stay on in release builds.
 */
class BridgeMetrics(
    private val capacity: Int = 8192,
) {
    private val emitToApplied = LongArray(capacity)
    private val batch = LongArray(capacity)
    private var messages = 0L
    private var batches = 0L
    private val lock = Any()

    fun recordMessage(nanos: Long) =
        synchronized(lock) {
            emitToApplied[(messages % capacity).toInt()] = nanos
            messages += 1
        }

    fun recordBatch(nanos: Long) =
        synchronized(lock) {
            batch[(batches % capacity).toInt()] = nanos
            batches += 1
        }

    fun emitToApplied(): Percentiles = synchronized(lock) { summarize(emitToApplied, messages) }

    fun batches(): Percentiles = synchronized(lock) { summarize(batch, batches) }

    fun reset() =
        synchronized(lock) {
            messages = 0
            batches = 0
        }

    private fun summarize(
        values: LongArray,
        total: Long,
    ): Percentiles {
        val n = minOf(total, capacity.toLong()).toInt()
        if (n == 0) return Percentiles(0, 0, 0, 0, 0)
        val sorted = values.copyOf(n).also { it.sort() }

        fun at(q: Double) = sorted[((n - 1) * q).toInt()] / 1000
        return Percentiles(n, at(0.5), at(0.9), at(0.99), sorted[n - 1] / 1000)
    }
}
