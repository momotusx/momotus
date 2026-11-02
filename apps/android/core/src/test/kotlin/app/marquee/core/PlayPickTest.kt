package app.marquee.core

import app.marquee.core.model.EmptyAction
import app.marquee.core.model.EmptyKind
import app.marquee.core.model.EmptyVersions
import app.marquee.core.model.Group
import app.marquee.core.model.HiddenSummary
import app.marquee.core.model.Reason
import app.marquee.core.model.SourceRows
import app.marquee.core.model.VersionRow
import app.marquee.core.model.VersionStatus
import app.marquee.core.model.VersionsSnapshot
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs

class PlayPickTest {
    private fun row(key: String) =
        VersionRow(
            key = key,
            quality = "1080p",
            features = emptyList(),
            sizeBytes = null,
            status = VersionStatus.Direct,
            via = "Local",
            releaseName = "$key.mkv",
            languages = emptyList(),
            reasons = listOf(Reason.PlaysDirectly),
            group = Group.Best,
            failoverCount = 0,
            line = VersionsFixtures.line,
            sourceClass = VersionsFixtures.sourceClass,
            kind = VersionsFixtures.kind,
            score = VersionsFixtures.score,
        )

    private fun snapshot(
        best: VersionRow? = null,
        rows: List<VersionRow> = emptyList(),
        done: Boolean = false,
        frozen: Boolean = false,
        pending: List<String> = emptyList(),
    ) = VersionsSnapshot(
        conditions = VersionsFixtures.conditions,
        recommended = best,
        rows = rows,
        hidden = HiddenSummary(0, emptyList()),
        pendingAddons = pending,
        slowAddons = emptyList(),
        failedAddons = emptyList(),
        done = done,
        frozen = frozen,
        nobodySharing = false,
    )

    @Test
    fun `the version chosen on Versions plays at once, even while addons still answer`() {
        val pick =
            pickVersion(snapshot(best = row("a"), rows = listOf(row("b")), pending = listOf("X")), "b", settled = false)
        assertEquals(PlayPick.Use(row("b")), pick)
    }

    @Test
    fun `a plugin row chosen on Versions plays at once, while the plugins' pass keeps the view from done`() {
        val plugins = snapshot().copy(plugins = SourceRows(rows = listOf(row("p")), pending = true))
        assertEquals(PlayPick.Use(row("p")), pickVersion(plugins, "p", settled = false))
    }

    @Test
    fun `a recommendation plays once every addon answered, it was pinned, or it stood still`() {
        val best = row("a")
        assertIs<PlayPick.Wait>(pickVersion(snapshot(best = best, pending = listOf("X")), null, settled = false))
        assertEquals(PlayPick.Use(best), pickVersion(snapshot(best = best, done = true), null, settled = false))
        assertEquals(PlayPick.Use(best), pickVersion(snapshot(best = best, frozen = true), null, settled = false))
        assertEquals(
            PlayPick.Use(best),
            pickVersion(snapshot(best = best, pending = listOf("X")), null, settled = true),
        )
    }

    @Test
    fun `a chosen version missing from a new list waits for every addon, then opens by its key, never the best`() {
        val best = row("a")
        assertIs<PlayPick.Wait>(pickVersion(snapshot(best = best, pending = listOf("X")), "gone", settled = false))
        // Three quiet seconds are no reason to play another version than the one picked.
        assertEquals(
            PlayPick.Wait("Finding this version"),
            pickVersion(snapshot(best = best, pending = listOf("X")), "gone", settled = true),
        )
        assertEquals(PlayPick.Wait("Finding this version"), pickVersion(snapshot(best = best), "gone", settled = true))
        // Every addon answered without it: the core is asked by the key and says whether it is gone.
        assertEquals(PlayPick.Open("gone"), pickVersion(snapshot(best = best, done = true), "gone", settled = false))
    }

    @Test
    fun `sources with no recommendation do not leave the player waiting for ever`() {
        assertEquals(
            PlayPick.None("No version is ready to play yet"),
            pickVersion(snapshot(rows = listOf(row("a")), done = true), null, settled = true),
        )
    }

    @Test
    fun `nothing that plays says the core's empty line instead of failing over nothing`() {
        val empty = { kind: EmptyKind, key: String? ->
            snapshot(done = true).copy(empty = EmptyVersions(kind, EmptyAction.None, 42, key = key))
        }
        // The core's line, its kind for the card's actions, and the version this device keeps.
        assertEquals(
            PlayPick.None("None of these plays on this device", EmptyKind.NothingPlaysHere),
            pickVersion(empty(EmptyKind.NothingPlaysHere, null), null, settled = true),
        )
        assertEquals(
            PlayPick.None("Can't reach any source", EmptyKind.Unreachable, "kept"),
            pickVersion(empty(EmptyKind.Unreachable, "kept"), null, false),
        )
    }

    @Test
    fun `the recommended row is found by its key too`() {
        assertEquals(PlayPick.Use(row("a")), pickVersion(snapshot(best = row("a")), "a", settled = false))
    }

    @Test
    fun `no version at all waits for the core's state, and waiting never names an addon`() {
        // Every addon answered with nothing while the core still checks what this device keeps: it says nothing yet.
        assertEquals(
            PlayPick.Wait("Finding the best version"),
            pickVersion(snapshot(done = true), null, settled = true),
        )
        assertEquals(
            PlayPick.Wait("Finding the best version"),
            pickVersion(snapshot(pending = listOf("Cinemeta", "Local")), null, settled = false),
        )
        assertEquals(PlayPick.Wait("Finding the best version"), pickVersion(snapshot(), null, settled = false))
        assertEquals(PlayPick.Wait("Opening"), pickVersion(null, null, settled = false))
    }
}
