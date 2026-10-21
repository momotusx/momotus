package app.marquee.core

import app.marquee.core.model.ForkPreview
import app.marquee.core.model.LostPreview
import app.marquee.core.model.MemberInfo
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.model.SyncRecord
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.longOrNull
import java.net.URI

/** Sync rules the screens share, as the web's `lib/sync.ts` and Apple's `SyncRules` have them. */
object SyncRules {
    /** Delete account shows on the owner profile only: the account holds every profile (D15). */
    fun mayDeleteAccount(snapshot: ProfilesSnapshot?) =
        snapshot?.profiles?.firstOrNull { it.id == snapshot.active }?.isOwner == true

    /** Make a recovery kit shows on the owner profile only: a kit is a copy of the whole account key (D17). */
    fun mayMakeKit(snapshot: ProfilesSnapshot?) = mayDeleteAccount(snapshot)

    /** A pairing code as the new device shows it; any other QR in view is ignored. */
    fun isPairingCode(text: String) = text.trim().startsWith("marquee:pair?")

    /**
     * The sync server a pairing code joins through, for the step before approving it; null when the
     * code names none. Only the host is ever shown.
     */
    fun pairingHost(code: String): String? {
        val query = code.trim().substringAfter('?', "")
        val server =
            query
                .split('&')
                .firstOrNull { it.startsWith("s=") }
                ?.removePrefix("s=")
                // A bad escape (%zz) throws: such a code is not one, and must not end the app.
                ?.let { runCatching { java.net.URLDecoder.decode(it, Charsets.UTF_8.name()) }.getOrNull() }
                ?: return null
        return runCatching { URI(server).host }.getOrNull()?.takeIf { it.isNotEmpty() }
    }

    /** "VW4BV9DJD5EZ" to "VW4B V9DJ D5EZ", for reading aloud. */
    fun fingerprintGroups(fingerprint: String) = fingerprint.chunked(4).joinToString(" ")

    /** A kit on two lines of four groups, so no group breaks across lines. The copy keeps the kit as it is. */
    fun kitLines(kit: String) = kit.split('-').chunked(4).joinToString("\n") { it.joinToString("-") }

    /** This device first, then the other devices and then the kits, oldest first. */
    fun sortMembers(members: List<MemberInfo>) =
        members.sortedWith(compareBy<MemberInfo>({ !it.thisDevice }, { it.kit }, { it.addedMs }))

    fun memberName(member: MemberInfo) =
        if (member.kit) {
            SyncCopy.KIT_NAME
        } else {
            member.name?.ifEmpty { null }
                ?: SyncCopy.UNNAMED
        }

    /** The name of the device that added [member], or null for the first device. */
    fun sponsorName(
        member: MemberInfo,
        members: List<MemberInfo>,
    ): String? {
        val sponsor = member.sponsor ?: return null
        // A device that joined with a kit names the kit, which the join used up.
        return members.firstOrNull { it.id == sponsor }?.let(::memberName) ?: "a device or kit since removed"
    }

    /** What a lost report drops: the lost device and what it added that the user did not untick. */
    fun lostDrop(
        lost: String,
        added: List<String>,
        keep: Set<String>,
    ) = listOf(lost) + added.filter { it !in keep }

    /**
     * The kits that go with a lost report, as the core works them out: every kit of the preview whose
     * maker does not stay (a device the user unticked keeps its kits).
     */
    fun kitsGoing(
        previewKits: List<String>,
        members: List<MemberInfo>,
        drop: List<String>,
    ): List<String> {
        val ids = members.map { it.id }.toSet()
        return previewKits.filter { kit ->
            val maker = members.firstOrNull { it.id == kit }?.sponsor ?: return@filter true
            maker !in ids || maker in drop
        }
    }

    /** One device a lost report would drop, as the page lists it. */
    data class LostRow(
        val id: String,
        val name: String,
        val detail: String,
    )

    /**
     * The preview's devices with their names from a fresh device list, oldest first; an id the list
     * does not have yet is still listed, as an unknown device, so nothing goes unseen.
     */
    fun lostRows(
        preview: LostPreview,
        members: List<MemberInfo>,
        zone: java.util.TimeZone = java.util.TimeZone.getDefault(),
    ): List<LostRow> {
        val known =
            sortMembers(members.filter { it.id in preview.devices }).map {
                LostRow(it.id, memberName(it), SyncCopy.addedLine(false, it.addedMs, sponsorName(it, members), zone))
            }
        val unknown =
            preview.devices
                .filter { id -> members.none { it.id == id } }
                .map { LostRow(it, SyncCopy.UNKNOWN, SyncCopy.UNKNOWN_DETAIL) }
        return known + unknown
    }

    /** The two sides of a different device list, by id: on both, only on the server's, only on this device's. */
    data class ForkSides(
        val both: List<MemberInfo>,
        val serverOnly: List<MemberInfo>,
        val mineOnly: List<MemberInfo>,
    )

    fun forkSides(
        mine: List<MemberInfo>,
        server: List<MemberInfo>,
    ): ForkSides {
        val here = mine.map { it.id }.toSet()
        val there = server.map { it.id }.toSet()
        return ForkSides(
            both = server.filter { it.id in here },
            serverOnly = server.filter { it.id !in here },
            mineOnly = mine.filter { it.id !in there },
        )
    }

    /**
     * What continuing with the server's list removes on the user's word: the ticked members of that list,
     * never this device and never one that goes anyway ([drops]).
     */
    fun forkDrop(
        server: List<MemberInfo>,
        drops: List<String>,
        ticked: Set<String>,
    ) = server.filter { !it.thisDevice && it.id !in drops && it.id in ticked }.map { it.id }

    /** The kits [device] made that would go with it: none of those that go anyway. */
    fun kitsOf(
        preview: ForkPreview,
        device: String,
    ) = preview.kits
        .firstOrNull { it.device == device }
        ?.kits
        .orEmpty()
        .filter { it !in preview.drops }

    /** The kits that go with the devices the user ticked ([drop]), besides what goes anyway. */
    fun forkKits(
        preview: ForkPreview,
        drop: List<String>,
    ) = drop.flatMap { kitsOf(preview, it) }

    /** The record's revision (`SyncConfig.revision`); 0 when it has none. */
    fun revision(record: SyncRecord?): Long = (record?.get("revision") as? JsonPrimitive)?.longOrNull ?: 0

    /**
     * Records carry a revision and the app keeps the highest: an answer that arrives after a newer
     * record was stored never rolls it back.
     */
    fun newer(
        held: SyncRecord?,
        next: SyncRecord,
    ): SyncRecord = if (held != null && revision(held) > revision(next)) held else next

    /** An address with a host and no user name or password in it: anything else is never kept or sent. */
    fun plainAddress(url: String) =
        runCatching { URI(url.trim()).let { it.host != null && it.rawUserInfo == null } }.getOrDefault(false)

    /** The server's host name only: never a path, a query or a user name. */
    fun hostOf(url: String) = runCatching { URI(url).host }.getOrNull() ?: "the sync server"
}
