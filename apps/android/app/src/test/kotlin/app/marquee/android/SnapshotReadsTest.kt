package app.marquee.android

import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals

/**
 * A screen reads a view with `view.current()`, never `view.snapshot` in composition: a snapshot that came while the
 * window drew for the first time reached no recomposer, and the page stayed empty until the next change. The reads
 * of `snapshot` left are outside composition: a `snapshotFlow`, and the ViewModel and callback reads named here.
 * The profile gate's own `snapshot` is not a view's and is left out.
 */
class SnapshotReadsTest {
    private val outside =
        setOf(
            "val autoplayNext: Boolean get() = prefs.snapshot?.autoplayNext == true",
            "prefs.snapshot?.subtitleSize",
            "val snapshot = versions.view(key)?.snapshot",
            "for (a in app.session.debrid.snapshot",
            "val ready = view.snapshot?.state is DiscoverState.Ready",
            "val snapshot: ProfilesSnapshot? get() = view?.snapshot?.takeIf { current }",
            "val first = v.snapshot ?: return@launch",
            "val joined = current.snapshot as? PairingSnapshot.Joined ?: return",
        )

    @Test
    fun `screens read a view's snapshot with current()`() {
        val read = Regex("""(?<!gate|profiles)\.snapshot\b""")
        val found =
            listOf("src/main/kotlin", "../ui-phone/src/main/kotlin", "../ui-tv/src/main/kotlin")
                .flatMap { File(it).walk().filter { f -> f.extension == "kt" } }
                .flatMap { file -> file.readLines().map { it.trim() } }
                .filter { read.containsMatchIn(it) && "snapshotFlow {" !in it }
        assertEquals(emptyList(), found.filter { it !in outside }, "read these with current() in composition")
    }
}
