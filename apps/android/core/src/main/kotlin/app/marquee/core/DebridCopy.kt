package app.marquee.core

/** The debrid services Settings offers, equal to Apple's `DebridKeys`, and the words around them. */
object DebridCopy {
    data class Service(
        val id: String,
        val name: String,
        val keyPage: String,
    )

    val services =
        listOf(
            Service("realdebrid", "Real-Debrid", "https://real-debrid.com/apitoken"),
            Service("torbox", "TorBox", "https://torbox.app/settings"),
            Service("alldebrid", "AllDebrid", "https://alldebrid.com/apikeys"),
            Service("premiumize", "Premiumize", "https://www.premiumize.me/account"),
        )

    /**
     * The accounts after [service] gets [key] (blank removes it), in the services' order, since the
     * first is the default. A service this list does not know keeps its key, last.
     */
    fun change(
        keys: List<Pair<String, String>>,
        service: String,
        key: String?,
    ): List<Pair<String, String>> {
        val next = LinkedHashMap(keys.toMap())
        next.remove(service)
        key?.trim()?.takeIf { it.isNotEmpty() }?.let { next[service] = it }
        val known = services.mapNotNull { s -> next.remove(s.id)?.let { s.id to it } }
        return known + next.toList()
    }

    /** What the core answered about the saved keys. */
    fun inUse(count: Int) =
        when (count) {
            0 -> "No debrid service in use."
            1 -> "1 debrid service in use."
            else -> "$count debrid services in use."
        }

    /**
     * How a change ended, storing and handing over said apart: [inUse] is the core's count, or null
     * when the core refused with [refused]; [stored] is false when the keystore kept it in memory only.
     */
    fun outcome(
        stored: Boolean,
        inUse: Int?,
        refused: String? = null,
    ): String =
        listOfNotNull(
            inUse?.let(::inUse) ?: "The core did not take it: $refused.",
            if (stored) null else "Kept for this session only: this device could not store it.",
        ).joinToString(" ")

    /** "real-debrid.com/apitoken": where to get a key, to type on another device. */
    fun page(service: Service) = service.keyPage.removePrefix("https://").removePrefix("www.")
}
