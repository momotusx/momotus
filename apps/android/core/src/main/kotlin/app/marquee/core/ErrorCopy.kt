package app.marquee.core

import app.marquee.core.client.CoreError
import app.marquee.core.client.CoreLog

/**
 * Failure words, word for word `apps/shared/error-words.json`, which the web and Apple read too; a test
 * checks every key. A screen shows [words] for a failure, never its message: the core's raw text is for
 * the log only (the owner's rule: no status code, raw error or addon name on screen).
 */
object ErrorCopy {
    const val FAILED = "Momotus could not finish this. Try again in a moment."
    const val NETWORK = "It could not be reached. Check the connection, then try again."
    const val TIMEOUT = "It did not answer in time. Check the connection, then try again."
    const val NOT_FOUND = "It has nothing at this address."
    const val UNAUTHORIZED = "It refused access."
    const val FORBIDDEN = "It refused access."
    const val RATE_LIMITED = "It is busy; try again soon."
    const val SERVER_ERROR = "It had an error on its side."
    const val HTTP_ERROR = "It answered with an error."
    const val UNREADABLE = "It sent something Momotus cannot read."
    const val BLOCKED_ADDRESS = "It points to a private network address, which Momotus does not open."
    const val INVALID_URL = "That is not a web address."
    const val CORE_STOPPED = "Momotus stopped unexpectedly. Open it again to carry on."
    const val CORE_NOT_STARTED = "Momotus could not start. Open it again to try once more."

    /** The words by the core's `kind`, as the shared file keys them. */
    val byKind: Map<String, String> =
        mapOf(
            "failed" to FAILED,
            "network" to NETWORK,
            "timeout" to TIMEOUT,
            "notFound" to NOT_FOUND,
            "unauthorized" to UNAUTHORIZED,
            "forbidden" to FORBIDDEN,
            "rateLimited" to RATE_LIMITED,
            "serverError" to SERVER_ERROR,
            "httpError" to HTTP_ERROR,
            "unreadable" to UNREADABLE,
            "blockedAddress" to BLOCKED_ADDRESS,
            "invalidUrl" to INVALID_URL,
            "coreStopped" to CORE_STOPPED,
            "coreNotStarted" to CORE_NOT_STARTED,
        )

    /**
     * A failure in words: the core's own sentence when its kind is `said`, else the words for its kind.
     * Anything else (no kind, a core that did not answer, an app exception) is [FAILED]. The raw text goes
     * to the log.
     */
    fun words(e: Throwable?): String {
        if (e != null) CoreLog.d("failure shown in words: ${CoreLog.describe(e)}")
        val core = e as? CoreError
        if (core?.kind == "said") return core.message.orEmpty()
        return byKind[core?.kind] ?: FAILED
    }
}
