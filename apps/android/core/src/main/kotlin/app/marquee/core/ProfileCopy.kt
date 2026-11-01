package app.marquee.core

import app.marquee.core.model.ProfileInfo
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

    /**
     * A stored avatar. Never chosen (empty): the letter on ember. An unknown glyph is the letter, an
     * unknown color is null (drawn on `raised`).
     */
    data class Avatar(
        val glyph: String,
        val color: String?,
    )

    fun parseAvatar(stored: String): Avatar {
        if (stored.isEmpty()) return Avatar("letter", "ember")
        if (stored == INCOGNITO) return Avatar(INCOGNITO, null)
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

    /**
     * The profile codes the core sends, in words. A full disk at Incognito's entry (`noSpace`) reads as a quiet
     * failure: no space words on any screen (plans/no-space-warnings.md, the owner's rule, over section 6's line).
     */
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
            "noSpace", "incognitoFailed" -> "Couldn't open Incognito. Try again."
            "incognito" -> "Not available in Incognito."
            else -> fallback
        }

    // Incognito (plans/batch-10-incognito.md): a throwaway profile the core never lists among the profiles.

    /** The session's profile id while Incognito runs; never a real id (those are 32 hex digits). */
    const val INCOGNITO = "incognito"

    /** Incognito as a profile, for every place that shows the current one (2.8, the app face). */
    val incognito =
        ProfileInfo(
            id = INCOGNITO,
            name = "Incognito",
            avatar = INCOGNITO,
            position = 0,
            hasPin = false,
            isOwner = false,
            syncing = false,
            kids = false,
        )

    fun inIncognito(s: ProfilesSnapshot?) = s?.incognitoHost != null

    /** The keypad entering Incognito opens on, by the core's rule; null when no PIN is asked. */
    fun incognitoPinProfile(s: ProfilesSnapshot): ProfileInfo? = s.profiles.firstOrNull { it.id == s.incognitoPin }

    /** The PIN was asked by the kids rule, not because the session is locked: the keypad says why (5.1). */
    fun incognitoKidsReason(s: ProfilesSnapshot) = s.incognitoPin != null && !s.locked && s.profiles.any { it.kids }

    /** The keypad's title for entering Incognito with [name]'s PIN. */
    fun incognitoPinTitle(name: String) = "Enter the PIN for $name to open Incognito"

    const val INCOGNITO_KIDS_REASON = "Kids profiles use this device, so Incognito needs this PIN."
    const val INCOGNITO_OPENING = "Opening Incognito"
    const val INCOGNITO_LEAVE = "Leave Incognito"
    const val INCOGNITO_ENDED = "Incognito ended. Its history and addons were cleared."
    const val INCOGNITO_KEPT_NOTHING = "Nothing you watch here is kept."
    const val INCOGNITO_ADDONS_NOTE = "Changes here last until you leave Incognito."
    const val INCOGNITO_DOWNLOAD_NOTE = "This download stays on this device. Every profile can see it in Downloads."
    const val INCOGNITO_LIBRARY_EMPTY = "Nothing here yet. Titles you add in Incognito are gone when you leave."
    const val INCOGNITO_TRAILERS_OFF = "Off in Incognito."
    const val INCOGNITO_DEBRID = "Uses this device's debrid account"
    const val INCOGNITO_SOURCES = "Addons and plugins"

    /** The intro's two groups (section 6); [debrid] adds the debrid line, [tv] picks the end line. */
    data class Intro(
        val notKept: List<String>,
        val seen: List<String>,
        val ends: String,
    )

    fun intro(
        debrid: Boolean,
        tv: Boolean,
    ) = Intro(
        notKept = listOf("What you watch and search.", "Addons and settings you change."),
        seen =
            listOfNotNull(
                "Downloads stay on this device, and every profile can see them.",
                "Your debrid account keeps its own history.".takeIf { debrid },
                "Addons, your internet provider and torrent peers can still see what you play.",
            ),
        ends =
            if (tv) {
                "It ends when you leave, or after 30 minutes away."
            } else {
                "It ends when you leave, close the app, or after 30 minutes away."
            },
    )

    /** The spoken name of a glyph choice. */
    fun glyphName(glyph: String) =
        when (glyph) {
            "letter" -> "First letter"
            "gamepad" -> "Game controller"
            else -> glyph.replaceFirstChar { it.uppercase() }
        }
}
