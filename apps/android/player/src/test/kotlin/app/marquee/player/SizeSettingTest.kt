package app.marquee.player

import app.marquee.core.model.SubtitleSize
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlin.test.Test
import kotlin.test.assertEquals

/** The picker's Size against the profile's setting (review P3-11): no flicker on fast presses, a failed write undone. */
class SizeSettingTest {
    private val shown = mutableListOf<SubtitleSize>()
    private val answers = ArrayDeque<CompletableDeferred<SubtitleSize>>()
    private val setting =
        SizeSetting(
            CoroutineScope(Dispatchers.Unconfined),
            write = { CompletableDeferred<SubtitleSize>().also(answers::addLast).await() },
            show = { shown += it },
        )

    @Test
    fun `two fast presses show at once, the profile's answers in between move nothing`() {
        setting.fromProfile(SubtitleSize.Medium)
        setting.pick(SubtitleSize.Large)
        setting.pick(SubtitleSize.ExtraLarge)
        // The profile's view says Large while the second write is still out.
        setting.fromProfile(SubtitleSize.Large)
        answers.removeFirst().complete(SubtitleSize.Large)
        answers.removeFirst().complete(SubtitleSize.ExtraLarge)
        assertEquals(
            listOf(SubtitleSize.Medium, SubtitleSize.Large, SubtitleSize.ExtraLarge, SubtitleSize.ExtraLarge),
            shown,
        )
    }

    @Test
    fun `a write that fails puts the profile's size back`() {
        setting.fromProfile(SubtitleSize.Medium)
        setting.pick(SubtitleSize.Small)
        answers.removeFirst().completeExceptionally(IllegalStateException("the core refused"))
        assertEquals(listOf(SubtitleSize.Medium, SubtitleSize.Small, SubtitleSize.Medium), shown)
    }
}
