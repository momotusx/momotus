package app.marquee.core.views

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import app.marquee.core.client.CoreClient
import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog
import app.marquee.core.client.Subscription
import app.marquee.core.client.ViewBinding
import app.marquee.core.client.ViewSink
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject

sealed interface ViewStatus {
    data object Opening : ViewStatus

    data object Open : ViewStatus

    data class Failed(
        val reason: String,
    ) : ViewStatus
}

/** `{"type": name, ...}` for `command`. */
internal fun command(
    type: String,
    build: JsonObjectBuilder.() -> Unit = {},
): JsonObject =
    buildJsonObject {
        put("type", JsonPrimitive(type))
        build()
    }

/**
 * One open core view. [status] and the view's own fields are Compose state written only by the
 * client's state thread (inside one snapshot per batch) and read by the UI.
 *
 * A view that loses sync with the core (a diff that does not fit, an update that does not
 * decode) subscribes again and keeps showing what it has until the new snapshot arrives. Each
 * subscription attempt has a generation; a late message from an older attempt is ignored, so two
 * attempts never apply diffs to the same list. Commands sent before the subscription is open, or
 * while it is replaced, wait and go out in order.
 */
abstract class LiveView<S, U>(
    private val client: CoreClient,
    private val scope: CoroutineScope,
    private val view: String,
    private val params: JsonObject,
    private val snapshotSerializer: KSerializer<S>,
    private val updateSerializer: KSerializer<U>,
) : AutoCloseable {
    var status: ViewStatus by mutableStateOf(ViewStatus.Opening)
        private set

    /** Completes with the first subscription; fails if the view closes or cannot open. */
    val opened: CompletableDeferred<Subscription> = CompletableDeferred()

    private val lock = Any()
    private var generation = 0
    private var subscription: Subscription? = null
    private val queuedCommands = ArrayList<JsonObject>()
    private var closed = false
    private var job: Job? = null
    private val resubscribeTimes = ArrayDeque<Long>()

    /** The subscription now (it changes when the view subscribes again), or null before the first reply. */
    val currentSubscription: Subscription?
        get() = synchronized(lock) { subscription }

    fun open(): LiveView<S, U> {
        start()
        return this
    }

    private fun start() {
        val gen = synchronized(lock) { ++generation }
        job = scope.launch { subscribe(gen) }
    }

    private fun isCurrent(gen: Int) = synchronized(lock) { !closed && gen == generation }

    private suspend fun subscribe(gen: Int) {
        try {
            val sub = client.subscribe(view, params, ViewBinding(snapshotSerializer, updateSerializer, Sink(gen)))
            val flush = ArrayList<JsonObject>()
            val stale =
                synchronized(lock) {
                    if (closed || gen != generation) {
                        true
                    } else {
                        subscription = sub
                        flush.addAll(queuedCommands)
                        queuedCommands.clear()
                        false
                    }
                }
            if (stale) {
                sub.close()
                return
            }
            for (command in flush) sub.post(command)
            opened.complete(sub)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            client.onState { if (isCurrent(gen)) status = ViewStatus.Failed(reasonOf(e)) }
            opened.completeExceptionally(e)
        }
    }

    /** Sends a command to this view, or holds it until the subscription is open. */
    protected fun post(command: JsonObject) {
        val sub =
            synchronized(lock) {
                subscription ?: run {
                    queuedCommands.add(command)
                    null
                }
            }
        sub?.post(command)
    }

    protected abstract fun applySnapshot(snapshot: S)

    protected abstract fun applyUpdates(updates: List<U>)

    private inner class Sink(
        private val gen: Int,
    ) : ViewSink<S, U> {
        override fun onSnapshot(snapshot: S) {
            if (!isCurrent(gen)) return
            applySnapshot(snapshot)
            status = ViewStatus.Open
        }

        override fun onUpdates(updates: List<U>) {
            if (isCurrent(gen)) applyUpdates(updates)
        }

        override fun onFailure(reason: String) {
            if (!isCurrent(gen)) return
            if (reason.startsWith("desync") && mayResubscribe()) {
                CoreLog.w("$view lost sync ($reason), subscribing again")
                resubscribe()
            } else {
                status = ViewStatus.Failed(reason)
            }
        }
    }

    /** At most three fresh subscriptions in ten seconds: a core that keeps sending what we cannot read is a failure. */
    private fun mayResubscribe(): Boolean {
        val now = System.nanoTime() / 1_000_000
        synchronized(lock) {
            while (resubscribeTimes.isNotEmpty() &&
                now - resubscribeTimes.first() > 10_000
            ) {
                resubscribeTimes.removeFirst()
            }
            if (resubscribeTimes.size >= 3) return false
            resubscribeTimes.addLast(now)
            return true
        }
    }

    /** Opens the view again after it failed: the action behind a "Try again" button. */
    fun retry() {
        status = ViewStatus.Opening
        resubscribe()
    }

    private fun resubscribe() {
        val old = synchronized(lock) { subscription.also { subscription = null } }
        old?.close()
        start()
    }

    override fun close() {
        val old =
            synchronized(lock) {
                closed = true
                generation++
                queuedCommands.clear()
                subscription.also { subscription = null }
            }
        old?.close()
        job?.cancel()
        opened.cancel()
    }

    private fun reasonOf(e: Exception): String =
        when {
            e is CoreError && e.timedOut -> "The core is not responding."
            e is CoreError -> e.message ?: "The core refused the request."
            else -> e.message ?: e.javaClass.simpleName
        }
}
