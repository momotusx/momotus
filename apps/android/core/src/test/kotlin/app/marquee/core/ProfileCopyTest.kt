package app.marquee.core

import app.marquee.core.model.ProfileInfo
import app.marquee.core.model.ProfilesSnapshot
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ProfileCopyTest {
    @Test
    fun `a stored avatar parses, and anything unknown falls back`() {
        assertEquals(ProfileCopy.Avatar("popcorn", "amber"), ProfileCopy.parseAvatar("popcorn:amber"))
        assertEquals(ProfileCopy.Avatar("letter", "amber"), ProfileCopy.parseAvatar("hologram:amber"))
        assertEquals(ProfileCopy.Avatar("popcorn", null), ProfileCopy.parseAvatar("popcorn:ultraviolet"))
        assertEquals(ProfileCopy.Avatar("letter", "ember"), ProfileCopy.parseAvatar(""))
        assertEquals(ProfileCopy.Avatar("letter", null), ProfileCopy.parseAvatar(":"))
    }

    @Test
    fun `the letter is the first grapheme, and a new profile takes an unused color`() {
        assertEquals("É", ProfileCopy.initial("  élise"))
        assertEquals("?", ProfileCopy.initial(""))
        assertEquals("letter:ember", ProfileCopy.nextAvatar(emptyList()))
        assertEquals("letter:moss", ProfileCopy.nextAvatar(listOf("cat:ember", "letter:amber")))
        assertEquals("letter:amber", ProfileCopy.nextAvatar(listOf("")))
        assertEquals("letter:ember", ProfileCopy.nextAvatar(ProfileCopy.COLORS.map { "letter:$it" }))
    }

    private fun snapshot(
        locked: Boolean,
        count: Int,
    ) = ProfilesSnapshot(
        profiles = (1..count).map { ProfileInfo("p$it", "P$it", "", it, false, it == 1, false) },
        active = "p1",
        locked = locked,
        max = 6,
    )

    @Test
    fun `the picker opens a cold start only when locked or with more than one profile`() {
        assertFalse(ProfileCopy.startsOnPicker(snapshot(locked = false, count = 1)))
        assertTrue(ProfileCopy.startsOnPicker(snapshot(locked = true, count = 1)))
        assertTrue(ProfileCopy.startsOnPicker(snapshot(locked = false, count = 2)))
    }

    @Test
    fun `the wait reads as minutes and seconds and never shows zero while waiting`() {
        assertEquals("4:59", ProfileCopy.waitText(299_000))
        assertEquals("1:00", ProfileCopy.waitText(60_000))
        assertEquals("0:01", ProfileCopy.waitText(1))
        assertEquals("0:00", ProfileCopy.waitText(0))
    }
}
