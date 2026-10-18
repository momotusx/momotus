package app.marquee.core

import app.marquee.core.model.ProfilesSnapshot
import java.text.BreakIterator
import java.util.Locale

/**
 * Profile rules the screens share (plans/m11-profiles.md, P1 apps A, D and E), equal to the web's
 * `lib/profiles.ts` and `lib/avatar.ts`.
 */
object ProfileCopy {
    /** Avatars: the register stores `glyph:color`, both ids from these fixed lists. */
    val GLYPHS =
        listOf(
            "letter",
            "popcorn",
            "clapperboard",
            "tv",
            "rocket",
            "ghost",
            "cat",
            "dog",
            "leaf",
            "moon",
            "flame",
            "headphones",
            "gamepad",
        )
    val COLORS = listOf("ember", "amber", "moss", "teal", "sky", "plum", "rose", "graphite")

    /** A stored avatar; an unknown or empty glyph is the letter, an unknown color is null (drawn on `raised`). */
    data class Avatar(
        val glyph: String,
        val color: String?,
    )

    fun parseAvatar(stored: String): Avatar {
        val parts = stored.split(":")
        val glyph = parts.getOrNull(0)?.takeIf { it in GLYPHS } ?: "letter"
        val color = parts.getOrNull(1)?.takeIf { it in COLORS }
        return Avatar(glyph, color)
    }

    fun formatAvatar(
        glyph: String,
        color: String,
    ) = "$glyph:$color"

    /** The name's first grapheme, upper case, as the letter avatar draws it. */
    fun initial(name: String): String {
        val text = name.trim()
        if (text.isEmpty()) return "?"
        val it = BreakIterator.getCharacterInstance()
        it.setText(text)
        return text.substring(0, it.next()).uppercase(Locale.getDefault())
    }

    /** A new profile's avatar: the letter, on the first color no profile uses yet (else the next in turn). */
    fun nextAvatar(existing: List<String>): String {
        val used = existing.map { parseAvatar(it).color }.toSet()
        val color = COLORS.firstOrNull { it !in used } ?: COLORS[existing.size % COLORS.size]
        return formatAvatar("letter", color)
    }

    /** At a cold start: the picker when the session is locked or there is more than one profile, else Home. */
    fun startsOnPicker(s: ProfilesSnapshot) = s.locked || s.profiles.size > 1

    /** "4:59" for the wait after too many tries, rounded up so it never shows 0:00 while still waiting. */
    fun waitText(ms: Long): String {
        val total = maxOf(0L, (ms + 999) / 1000)
        return "${total / 60}:${(total % 60).toString().padStart(2, '0')}"
    }

    /** The profile codes the core sends, in words. */
    fun problem(
        code: String?,
        fallback: String,
    ): String =
        when (code) {
            "wrongPin" -> "Wrong PIN"
            "weakPin" -> "Pick a less obvious PIN: no repeats, runs or 1234."
            "badName" -> "Names are 1 to 30 characters."
            "tooMany" -> "This account has as many profiles as it can hold."
            "notOwner" -> "Only the main profile can do that."
            "viewsOpen" -> "Something is still open. Try again."
            "unknownProfile" -> "That profile was removed on another device."
            "pinUnreadable" -> "This PIN was set by a newer version. Remove it, then set a new one."
            else -> fallback
        }

    /** The spoken name of a glyph choice. */
    fun glyphName(glyph: String) =
        when (glyph) {
            "letter" -> "First letter"
            "gamepad" -> "Game controller"
            else -> glyph.replaceFirstChar { it.uppercase() }
        }
}
