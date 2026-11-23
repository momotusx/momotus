package app.marquee.core.client

/**
 * The JVM's threads as a wait that ran out sees them: how many of each kind, and the top frames of every one that
 * is running or blocked on a lock. A test that waits on work done by another thread says with it whether that thread was busy, blocked
 * or gone.
 */
fun threadReport(frames: Int = 4): String {
    val all = Thread.getAllStackTraces()
    val kinds =
        all.keys
            .groupingBy { it.name.replace(Regex("[-#_ ]?\\d+$"), "") }
            .eachCount()
            .entries
            .sortedByDescending { it.value }
            .joinToString(", ") { "${it.key} x${it.value}" }
    val running =
        all
            .filter { (t, stack) ->
                t != Thread.currentThread() &&
                    (t.state == Thread.State.RUNNABLE || t.state == Thread.State.BLOCKED) &&
                    stack.isNotEmpty()
            }.entries
            .joinToString("; ") { (t, stack) -> "${t.name} ${t.state}: ${stack.take(frames).joinToString(" < ")}" }
    return "threads ${all.size} ($kinds); running: $running"
}
