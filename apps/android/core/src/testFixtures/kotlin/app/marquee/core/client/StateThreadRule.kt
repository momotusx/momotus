package app.marquee.core.client

import org.junit.rules.ExternalResource
import java.util.concurrent.CopyOnWriteArrayList

/**
 * A UI test's [StateThread], quit after the test. Nothing but the dispatcher's own handler may post to its Looper:
 * Compose sends a view's commands to the view's handler, never here, and one that ran here would touch views off
 * the main thread. After the test, every message the Looper ran was the dispatcher's, and nothing threw there.
 */
class StateThreadRule : ExternalResource() {
    private val state = StateThread("test-state")
    val dispatcher = state.dispatcher
    val looper = state.thread.looper!!
    private val foreign = CopyOnWriteArrayList<String>()
    private val failures = CopyOnWriteArrayList<Throwable>()

    init {
        val ours = ">>>>> Dispatching to ${state.handler} "
        looper.setMessageLogging { line -> if (line.startsWith(">>>>>") && !line.startsWith(ours)) foreign += line }
        state.thread.setUncaughtExceptionHandler { _, e -> failures += e }
    }

    override fun after() {
        state.thread.quitSafely()
        state.thread.join(5_000)
        failures.firstOrNull()?.let { throw AssertionError("the state thread threw", it) }
        if (foreign.isNotEmpty()) throw AssertionError("something else posted to the state thread: $foreign")
    }
}
