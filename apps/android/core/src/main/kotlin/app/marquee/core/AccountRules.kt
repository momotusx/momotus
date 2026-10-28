package app.marquee.core

import app.marquee.core.model.Announced
import app.marquee.core.model.Announcement
import app.marquee.core.model.MemberInfo
import app.marquee.core.model.ProfileInfo
import app.marquee.core.model.ProfilesSnapshot
import app.marquee.core.model.ScheduledDeletion
import java.net.URI

/**
 * Accounts on Android (plans/accounts.md 4.1 to 4.4, 5, D13 to D17): which words each sign-in state and card
 * takes, and who sees which row, apart from the screens so both are tested. The web's `src/lib/account.ts`
 * follows the same `about` of `apps/shared/account-words.json`; the words are [AccountCopy]'s.
 */
object AccountRules {
    /** `{name}` fields filled; a field with no value stays as written. */
    fun fill(
        text: String,
        fields: Map<String, Any>,
    ): String = Regex("""\{(\w+)}""").replace(text) { m -> fields[m.groupValues[1]]?.toString() ?: m.value }

    /** A provider as people read it: Apple, Google, Facebook, or another OpenID provider's id as it is. */
    fun providerName(id: String): String = AccountCopy.PROVIDER_NAME[id] ?: id

    /** "Apple Account", "Google account", ..., else "<provider> sign-in". */
    fun providerAccount(id: String): String =
        AccountCopy.PROVIDER_ACCOUNT[id]
            ?: fill(AccountCopy.PROVIDER_ACCOUNT_OTHER, mapOf("provider" to providerName(id)))

    fun continueWith(id: String): String = fill(AccountCopy.CONTINUE_WITH, mapOf("provider" to providerName(id)))

    /** Android's order (4.1.1): Google first, then Apple and Facebook, then any other in the relay's order. */
    private val ORDER = listOf("google", "apple", "facebook")

    /** Email is never a provider button: it is its own "Continue with email" row (plans/accounts-email.md 4). */
    fun providerOrder(ids: List<String>): List<String> =
        ids.filter { it in ORDER }.sortedBy { ORDER.indexOf(it) } + ids.filter { it !in ORDER && it != EMAIL }

    const val EMAIL = "email"

    /** What a button under a failure does; [Resend] and [OtherAddress] are the email code stage's. */
    enum class Act { Retry, Other, NotNow, Move, Anyway, Cancel, Scan, Kit, CantUse, Resend, OtherAddress }

    data class Action(
        val label: String,
        val act: Act,
    )

    /** A failure's words and its buttons, the first the primary. */
    data class Failure(
        val text: String,
        val actions: List<Action>,
    )

    private fun acts(
        labels: List<String>,
        kinds: List<Act>,
    ) = labels.mapIndexed { i, label -> Action(label, kinds.getOrElse(i) { Act.Cancel }) }

    private fun retry() = acts(AccountCopy.NO_REDIRECT_ACTIONS.take(1), listOf(Act.Retry))

    /** A sign-in that ended in no account (section 5); null when it says nothing (a cancelled provider page). */
    fun signInFailure(
        code: String,
        provider: String = "",
    ): Failure? {
        val account = providerAccount(provider)
        return when (code) {
            "cancelled" -> {
                null
            }

            "noRedirect" -> {
                Failure(AccountCopy.NO_REDIRECT, acts(AccountCopy.NO_REDIRECT_ACTIONS, listOf(Act.Retry, Act.Other)))
            }

            "ticketExpired" -> {
                Failure(AccountCopy.TICKET_EXPIRED, acts(AccountCopy.TICKET_EXPIRED_ACTIONS, listOf(Act.Retry)))
            }

            "signupLimit" -> {
                Failure(
                    fill(AccountCopy.SIGNUP_LIMIT, mapOf("providerAccount" to account)),
                    acts(AccountCopy.SIGNUP_LIMIT_ACTIONS, listOf(Act.Other)),
                )
            }

            "linkedElsewhere" -> {
                Failure(
                    fill(AccountCopy.LINKED_ELSEWHERE, mapOf("providerAccount" to account)),
                    acts(AccountCopy.LINKED_ELSEWHERE_ACTIONS, listOf(Act.Move, Act.Cancel)),
                )
            }

            "busy" -> {
                Failure(AccountCopy.BUSY, retry())
            }

            "otherRelay" -> {
                Failure(AccountCopy.OTHER_RELAY, retry())
            }

            "notOwner" -> {
                Failure(AccountCopy.NOT_OWNER, emptyList())
            }

            "unreachable" -> {
                Failure(AccountCopy.UNREACHABLE, acts(AccountCopy.UNREACHABLE_ACTIONS, listOf(Act.Retry, Act.NotNow)))
            }

            "passkeyCancelled" -> {
                Failure(
                    AccountCopy.PASSKEY_CANCELLED,
                    acts(AccountCopy.PASSKEY_CANCELLED_ACTIONS, listOf(Act.Retry, Act.Scan)),
                )
            }

            "kitUnknown" -> {
                Failure(
                    AccountCopy.KIT_UNKNOWN_PASSKEY,
                    acts(AccountCopy.KIT_UNKNOWN_PASSKEY_ACTIONS, listOf(Act.Kit, Act.CantUse)),
                )
            }

            else -> {
                Failure(
                    AccountCopy.SIGN_IN_REFUSED,
                    acts(AccountCopy.NO_REDIRECT_ACTIONS, listOf(Act.Retry, Act.Other)),
                )
            }
        }
    }

    /**
     * An email stage's failure with its buttons (plans/accounts-email.md 4, the answers table); `badAddress`,
     * `wrongCode` and `badCode` are lines under the field instead ([emailFieldError]). Null [code]: no answer came.
     */
    fun emailFailure(code: String?): Failure {
        val expired = listOf(Act.Resend, Act.OtherAddress)
        return when (code) {
            "ticketExpired", "wrongFlow" -> {
                Failure(AccountCopy.CODE_EXPIRED, acts(AccountCopy.CODE_EXPIRED_ACTIONS, expired))
            }

            // Its own words, with the code stage's way on.
            "signInRefused" -> {
                Failure(AccountCopy.SIGN_IN_REFUSED, acts(AccountCopy.CODE_EXPIRED_ACTIONS, expired))
            }

            "emailWait" -> {
                Failure(AccountCopy.EMAIL_WAIT, emptyList())
            }

            "emailLimit" -> {
                Failure(AccountCopy.EMAIL_LIMIT, acts(AccountCopy.EMAIL_LIMIT_ACTIONS, listOf(Act.Other)))
            }

            "mailUnavailable" -> {
                Failure(
                    AccountCopy.MAIL_UNAVAILABLE,
                    acts(AccountCopy.MAIL_UNAVAILABLE_ACTIONS, listOf(Act.Retry, Act.Other)),
                )
            }

            // busy, unreachable and the rest: today's words.
            else -> {
                signInFailure(code ?: "unreachable", EMAIL) ?: Failure(AccountCopy.SIGN_IN_REFUSED, emptyList())
            }
        }
    }

    /** The line under an email stage's field, or null when the answer is a [emailFailure]. */
    fun emailFieldError(code: String?): String? =
        when (code) {
            "badAddress" -> AccountCopy.BAD_ADDRESS
            "wrongCode", "badCode" -> AccountCopy.WRONG_CODE
            else -> null
        }

    /** A code stage answer that ends its flow: the field stays, read-only, until a new code or another address. */
    fun emailEnds(code: String?): Boolean = code in setOf("ticketExpired", "wrongFlow", "signInRefused", "emailLimit")

    /** The deletion card's Cancel and turn off <provider> sign-in. */
    fun cancelUnlink(provider: String): String =
        if (provider == EMAIL) {
            AccountCopy.DELETION_CANCEL_UNLINK_EMAIL
        } else {
            fill(AccountCopy.DELETION_CANCEL_UNLINK, mapOf("provider" to providerName(provider)))
        }

    /** Apple's revoke ended in no receipt: the unlink or deletion may go on without it; a cancel says nothing. */
    fun revokeFailure(
        code: String,
        anyway: String,
        cancel: String,
    ): Failure? =
        if (code == "cancelled") {
            null
        } else {
            Failure(AccountCopy.APPLE_REVOKE_FAILED, listOf(Action(anyway, Act.Anyway), Action(cancel, Act.Cancel)))
        }

    /** Sign out on a profile and form (D13): whether it shows, whether the owner is asked, and a TV's always-remove. */
    data class LeaveChoice(
        val show: Boolean,
        val ask: Boolean,
        val always: Boolean,
    )

    fun leaveChoice(
        profile: ProfileInfo?,
        tv: Boolean,
    ): LeaveChoice {
        val owner = profile?.isOwner == true
        return if (tv) LeaveChoice(show = owner, ask = false, always = owner) else LeaveChoice(true, owner, false)
    }

    /**
     * The Account page's rows by profile and form (D15 to D17): links, deletion, kits and passkeys are the
     * owner's; approving another device is any unlocked profile's, on a phone or tablet only (D6, the core's
     * `cannotApprove`); a TV saves no passkey (4.4).
     */
    data class Rows(
        val links: Boolean,
        val delete: Boolean,
        val makeKit: Boolean,
        val savePasskey: Boolean,
        val approve: Boolean,
        val leave: LeaveChoice,
    )

    fun rows(
        profile: ProfileInfo?,
        tv: Boolean,
        locked: Boolean,
    ): Rows {
        val owner = profile?.isOwner == true && !locked
        return Rows(
            links = owner,
            delete = owner,
            makeKit = owner,
            savePasskey = owner && !tv,
            // A kids profile reaches no account page: the shortcut and the /pair link open no scanner there.
            approve = !tv && profile != null && !profile.kids && !locked,
            leave = leaveChoice(profile, tv),
        )
    }

    /**
     * A brokered return names the relay it came from (`relay=<host>`): one that is not the relay this sign-in
     * started on is another service's and never reaches the core.
     */
    fun returnRelay(fragment: String): String? =
        fragment
            .split('&')
            .map { it.split('=', limit = 2) }
            .firstOrNull { it[0] == "relay" }
            ?.getOrNull(1)
            ?.let { java.net.URLDecoder.decode(it, "UTF-8") }

    fun sameRelay(
        fragment: String,
        server: String,
    ): Boolean {
        val named = returnRelay(fragment) ?: return true
        return runCatching { URI(server).host == named }.getOrDefault(false)
    }

    fun activeProfile(s: ProfilesSnapshot?): ProfileInfo? = s?.profiles?.firstOrNull { it.id == s.active }

    /** A join, a passkey, a recovery kit, or a trim that may have hidden one: a way into the account. */
    fun securityKind(a: Announcement) = a.kind != Announced.Removed

    /** How a profile shows an announcement: not at all on a kids profile for a security notice, OK only on the owner. */
    data class Shown(
        val show: Boolean,
        val ok: Boolean,
    )

    fun shown(
        a: Announcement,
        profile: ProfileInfo?,
    ): Shown =
        when {
            profile == null -> Shown(false, false)
            !securityKind(a) -> Shown(true, true)
            profile.kids -> Shown(false, false)
            else -> Shown(true, profile.isOwner)
        }

    /** The name `{device}` stands for: the member `by`, else a member with no name, else gone. */
    fun byName(
        by: String?,
        members: List<MemberInfo>?,
    ): String {
        val m = by?.let { id -> members?.firstOrNull { it.id == id } } ?: return AccountCopy.ANNOUNCE_GONE_DEVICE
        return m.name?.takeIf { it.isNotEmpty() } ?: AccountCopy.ANNOUNCE_UNNAMED_DEVICE
    }

    /** An announcement's one line, by the `about` rules. */
    fun announcementLine(
        a: Announcement,
        members: List<MemberInfo>?,
    ): String {
        val device = byName(a.by, members)
        val name = a.name.orEmpty()
        return when (a.kind) {
            Announced.Device -> {
                if (name.isNotEmpty()) {
                    fill(
                        AccountCopy.ANNOUNCE_DEVICE,
                        mapOf("name" to name),
                    )
                } else {
                    AccountCopy.ANNOUNCE_DEVICE_UNNAMED
                }
            }

            Announced.Passkey -> {
                fill(AccountCopy.ANNOUNCE_PASSKEY, mapOf("device" to device))
            }

            Announced.Kit -> {
                fill(AccountCopy.ANNOUNCE_KIT, mapOf("device" to device))
            }

            Announced.Dropped -> {
                AccountCopy.ANNOUNCE_DROPPED
            }

            else -> {
                when {
                    a.by != null && a.by == a.member -> {
                        if (name.isNotEmpty()) {
                            fill(
                                AccountCopy.ANNOUNCE_LEFT,
                                mapOf("name" to name),
                            )
                        } else {
                            AccountCopy.ANNOUNCE_LEFT_UNNAMED
                        }
                    }

                    a.passkey -> {
                        fill(AccountCopy.ANNOUNCE_REMOVED_PASSKEY, mapOf("device" to device))
                    }

                    a.kit -> {
                        fill(AccountCopy.ANNOUNCE_REMOVED_KIT, mapOf("device" to device))
                    }

                    name.isNotEmpty() -> {
                        fill(AccountCopy.ANNOUNCE_REMOVED, mapOf("name" to name, "device" to device))
                    }

                    else -> {
                        fill(AccountCopy.ANNOUNCE_REMOVED_UNNAMED, mapOf("device" to device))
                    }
                }
            }
        }
    }

    /** Home's one account card (4.3), by priority. */
    sealed interface Card {
        data class Deletion(
            val text: String,
            val provider: String?,
        ) : Card

        data class Gone(
            val text: String,
            val remove: Boolean,
        ) : Card

        data class Notice(
            val text: String,
            val id: String,
            val ok: Boolean,
            val notYou: Boolean,
            val member: String?,
        ) : Card

        data class SecondWay(
            val text: String,
            val detail: String,
        ) : Card
    }

    data class CardState(
        val deletion: ScheduledDeletion?,
        val gone: Boolean,
        val announcements: List<Announcement>,
        val members: List<MemberInfo>?,
        val profile: ProfileInfo?,
        /** This device syncs, knows the members, and none of them is a second way in. */
        val alone: Boolean,
        val date: (Long) -> String,
    )

    fun homeCard(s: CardState): Card? {
        // A kids profile reaches no account page: it shows no account card at all.
        val profile = s.profile ?: return null
        if (profile.kids) return null
        s.deletion?.let { d ->
            val date = s.date(d.dueMs)
            val provider = d.provider
            return Card.Deletion(
                if (provider == EMAIL) {
                    fill(AccountCopy.DELETION_SCHEDULED_BY_EMAIL, mapOf("date" to date))
                } else if (provider != null) {
                    fill(AccountCopy.DELETION_SCHEDULED_BY, mapOf("provider" to providerName(provider), "date" to date))
                } else {
                    fill(AccountCopy.DELETION_SCHEDULED, mapOf("date" to date))
                },
                provider,
            )
        }
        if (s.gone) return Card.Gone(AccountCopy.ACCOUNT_GONE, remove = profile.isOwner)
        for (a in s.announcements) {
            val seen = shown(a, profile)
            if (seen.show) {
                return Card.Notice(
                    announcementLine(a, s.members),
                    a.id,
                    ok = seen.ok,
                    notYou = a.kind == Announced.Passkey && seen.ok,
                    member = a.member,
                )
            }
        }
        // Kits and passkeys are the owner's to make (D17): the nudge shows there only.
        if (s.alone && profile.isOwner) return Card.SecondWay(AccountCopy.SECOND_WAY_IN, AccountCopy.SECOND_WAY_IN_TEXT)
        return null
    }

    /**
     * No second way in yet (4.1.7): no recovery kit, no passkey that has opened the account on a device, and no
     * other phone or tablet, the only devices that sign others in.
     */
    fun needsSecondWayIn(members: List<MemberInfo>?): Boolean =
        members != null &&
            members.none { m ->
                (m.kit && !m.passkey) ||
                    (m.kit && m.passkey && m.lastUsedBy != null) ||
                    (!m.kit && !m.thisDevice && (m.form == "phone" || m.form == "tablet"))
            }
}
