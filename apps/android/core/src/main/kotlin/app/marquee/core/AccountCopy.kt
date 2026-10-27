package app.marquee.core

/**
 * Account words (plans/accounts.md 3.2, 4.1 to 4.3, 5, D13), word for word `apps/shared/account-words.json`,
 * which the web and Apple read too; a test checks every key and value. Its `about` gives the rules
 * [AccountRules] follows: which key each announcement takes, `{device}`, `{provider}`, `{providerAccount}`
 * and the owner rule.
 */
object AccountCopy {
    const val ANNOUNCE_DEVICE = "A new device joined: {name}."
    const val ANNOUNCE_DEVICE_UNNAMED = "A new device joined."
    const val ANNOUNCE_PASSKEY = "A passkey was added on {device}."
    const val ANNOUNCE_KIT = "A recovery kit was made on {device}."
    const val ANNOUNCE_REMOVED = "{name} was removed from your account on {device}."
    const val ANNOUNCE_REMOVED_UNNAMED = "A device was removed from your account on {device}."
    const val ANNOUNCE_REMOVED_PASSKEY = "A passkey was removed from your account on {device}."
    const val ANNOUNCE_REMOVED_KIT = "A recovery kit was removed from your account on {device}."
    const val ANNOUNCE_LEFT = "{name} signed out of your account."
    const val ANNOUNCE_LEFT_UNNAMED = "A device signed out of your account."
    const val ANNOUNCE_DROPPED = "Older changes were not kept. Check the Account page."
    const val ANNOUNCE_UNNAMED_DEVICE = "a device with no name"
    const val ANNOUNCE_GONE_DEVICE = "a device no longer in your account"
    const val ANNOUNCE_NOT_YOU = "Not you? Remove this passkey"
    const val REMOVE_PASSKEY_ASK = "Remove this passkey? It stops opening your account, on every device that has it."
    const val ANNOUNCE_OK = "OK"
    val PROVIDER_ACCOUNT =
        mapOf(
            "apple" to "Apple Account",
            "google" to "Google account",
            "facebook" to "Facebook account",
            "email" to "email address",
        )
    const val PROVIDER_ACCOUNT_OTHER = "{provider} sign-in"
    val PROVIDER_NAME = mapOf("apple" to "Apple", "google" to "Google", "facebook" to "Facebook", "email" to "Email")
    const val SIGN_IN_TITLE = "Sign in"
    const val CONTINUE_WITH = "Continue with {provider}"
    const val CONTINUE = "Continue"
    const val OTHER_WAYS = "Other ways to sign in"
    const val USE_PASSKEY = "Use a passkey"
    const val SCAN_FROM_DEVICE = "Scan from a signed-in device"
    const val USE_RECOVERY_KIT = "Use a recovery kit"
    const val USE_OWN_SERVER = "Use my own sync server"
    const val NOT_NOW = "Not now"
    const val PASSKEY_HERE = "You have a Momotus passkey on this device"
    const val CREATE_ACCOUNT = "Create your Momotus account"
    const val ALREADY_USE = "I already use Momotus on another device"
    const val LIBRARY_ADDED = "This device's library will be added to your account."
    const val CANT_USE_ANY = "I can't use any of these"
    const val TV_SIGN_IN = "Sign in with your phone"
    const val TV_SIGN_IN_HOW = "On your phone, open Momotus and tap Sign in another device"
    const val TV_WITHOUT_ACCOUNT = "Use without an account"
    const val SWITCH_PROFILE = "Switch profile"
    const val SIGN_IN_ANOTHER = "Sign in another device"
    const val APPROVE_WARNING =
        "This screen will see your whole library and every profile. Only approve a screen you can see " +
            "right now."
    const val SCAN_AGAIN_HERE = "To sign in, scan the code on the screen in front of you once more here."
    const val CODE_FOUND = "Code found"
    const val USE_PHONE_TO_SIGN_IN = "Use your phone to sign in other devices"
    const val TV_SCAN_HOW = "Scan with your phone's camera"
    const val TV_QR_LABEL = "QR code. Scan it with your phone's camera to sign in."
    const val BROWSE_FIRST = "Browse first"
    const val TV_SIGNED_IN_FROM = "Signed in from {device}"
    const val APPROVE_ASK = "Sign in \"{device}\"?"
    const val APPROVE_ASK_TV = "Sign in this TV?"
    const val APPROVE_ASK_OTHER = "Sign in this device?"
    const val APPROVE_UNNAMED = "The other device"
    const val APPROVE_BUTTON = "Sign in"
    const val APPROVE_NEAR = "Only sign in a screen you can see."
    const val APPROVE_SCAN_HERE =
        "Your phone isn't on the same network as that screen. Turn off mobile data or VPN, or scan the code here."
    const val APPROVE_SIGNING_IN = "Signing in"
    const val APPROVE_DONE = "{device} is signed in"
    const val APPROVE_NOT_YOU = "Not you? Remove it"
    const val APPROVE_SLOW = "{device} hasn't finished signing in. If it shows a new code, scan that one."
    const val APPROVE_ANSWERED = "Another phone already signed in this screen. If that wasn't you, sign out on it."
    const val APPROVE_EXPIRED = "This code expired. Scan the new one on the screen."
    const val APPROVE_KIDS = "Ask a grown-up to sign in this screen."
    const val PHONE_SCAN_HOW = "Scan with your other phone's camera"
    const val PASSKEY_LABEL = "Passkey, saved on {device}"
    const val RECOVERY_KIT_NAME = "Recovery kit"
    const val JOINED_WITH_PASSKEY = "Joined with a passkey on {date}"
    const val LAST_USED = "Last used: {device}, {date}"
    const val GIVING_AWAY = "Giving this phone away? Sign out on it first to keep your passkey."
    const val KIT_HERE = "Your passkey already works on this device."
    const val KIT_DROPPED = "This passkey was removed from your account. Save a new one."
    const val TOO_MANY_KITS = "Your account has 8 passkeys and recovery kits. Remove one first."
    const val KIT_TAKEN = "This passkey belongs to another Momotus account. Save a new passkey."
    const val PLAIN_LINK =
        "Passkeys don't work on this page: its link to Momotus is not encrypted. Use the app on this " +
            "device."
    const val DEBRID_CONNECTED =
        "Your streaming service accounts are still connected here. Disconnect them, then try again."
    const val TRAKT_CONNECTED = "A profile is still signed in to Trakt here. Sign it out, then try again."
    const val UNLINK_ANYWAY = "Unlink anyway"
    const val DELETE_ANYWAY = "Delete anyway"
    const val ACCOUNT_GONE = "Your Momotus account was deleted. Your library stays on this device."
    val ACCOUNT_GONE_ACTIONS = listOf("Sign in", "Remove from this device", "OK")
    const val DELETION_SCHEDULED = "Your account will be deleted on {date}."
    const val DELETION_SCHEDULED_BY =
        "Someone signed in with your {provider} account and asked to delete your Momotus account on " +
            "{date}. Not you? Cancel, and turn off {provider} sign-in."
    const val DELETION_CANCEL = "Cancel deletion"
    const val DELETION_CANCEL_UNLINK = "Cancel and turn off {provider} sign-in"
    const val SECOND_WAY_IN = "Add a second way in"
    const val NO_SECOND_WAY_IN = "Without a passkey or recovery kit, losing this phone means losing what it syncs."
    val NO_SECOND_WAY_IN_ACTIONS = listOf("Try again", "Use a recovery kit instead", "Later")
    const val REMOVE_LIBRARY_ASK = "Remove your profiles and library from this device?"
    const val REMOVE_LIBRARY_DEBRID = "Also disconnect your streaming service accounts"
    val REPORT_LOST_REMOVE = listOf("Remove {n} device", "Remove {n} devices")
    const val UNREACHABLE = "Momotus can't reach its sync service right now."
    val UNREACHABLE_ACTIONS = listOf("Try again", "Not now")
    const val NO_REDIRECT = "Didn't finish signing in?"
    val NO_REDIRECT_ACTIONS = listOf("Try again", "Other ways to sign in")
    const val PASSKEY_CANCELLED = "Your passkey wasn't used."
    val PASSKEY_CANCELLED_ACTIONS = listOf("Try again", "Scan from a signed-in device")
    const val KIT_UNKNOWN_PASSKEY =
        "Your passkey didn't open your account on this device. Scan this code with a device that's " +
            "signed in."
    val KIT_UNKNOWN_PASSKEY_ACTIONS = listOf("Use a recovery kit", "I can't use any of these")
    const val START_OVER_WARNING =
        "Your old library can't be opened. It is deleted in 7 days unless one of your old devices " +
            "cancels."
    const val TICKET_EXPIRED = "Sign in again to finish."
    val TICKET_EXPIRED_ACTIONS = listOf("Continue")
    const val SIGNUP_LIMIT =
        "This {providerAccount} has made several Momotus accounts lately. Try again later or use another " +
            "way."
    val SIGNUP_LIMIT_ACTIONS = listOf("Other ways to sign in")
    const val SIGNUP_LIMIT_NETWORK =
        "Too many new accounts from this network today. Connect to Wi-Fi and try again, or sign in on a device you " +
            "already use."
    const val LINKED_ELSEWHERE = "This {providerAccount} already signs in to another Momotus account."
    val LINKED_ELSEWHERE_ACTIONS = listOf("Use it for this account instead", "Cancel")
    const val BUSY = "Too many sign-ins from this network just now. Try again in a minute."
    const val OTHER_RELAY = "This sign-in finished for another sync service. Start again here."
    const val OTHER_RELAY_CODE = "This code is from another sync service."
    val OTHER_RELAY_CODE_ACTIONS = listOf("Scan again")
    const val SIGN_IN_REFUSED = "The sign-in did not work. Try again or use another way."
    const val SIGN_IN_WINDOW = "Finish signing in in the window that opened."
    const val POPUP_BLOCKED = "Your browser blocked the sign-in window. Allow pop-ups for Momotus, then try again."
    const val RETURN_CLOSE = "You can close this window and go back to Momotus."
    const val OWN_SERVER_LABEL = "Your sync server's address"
    const val OWN_SERVER_USE = "Use this sync server"
    const val FOUND_WEB =
        "This sign-in already has a Momotus account. Scan this code with your phone, where Momotus is " +
            "signed in, to add this browser."
    const val START_OVER = "Start over"
    const val SIGNED_IN_WITH = "Signed in with"
    const val LINK_ANOTHER = "Link another"
    const val LINKED_ON = "Linked on {date}"
    const val UNLINK = "Unlink"
    const val SAVE_PASSKEY_HERE = "Save a passkey here"
    const val UNLINK_LAST = "Without a linked sign-in, you sign in with a passkey or another device."
    const val APPLE_REVOKE_FAILED = "Apple did not confirm, so your Apple Account may still list Momotus."
    const val WIPING = "Removing your profiles and library"
    const val WIPE_FAILED = "Signed out, but your profiles and library are still on this device. Try again."
    const val SECOND_WAY_IN_TEXT =
        "Make a recovery kit or sign in on a second device, so losing this one never locks you out."
    const val NOT_OWNER = "Only the main profile can do that."
    const val CONTINUE_WITH_EMAIL = "Continue with email"
    const val EMAIL_TITLE = "Sign in with email"
    const val EMAIL_LABEL = "Email address"
    const val EMAIL_SEND = "Send code"
    const val BAD_ADDRESS = "Enter an email address like name@example.com."
    const val CODE_TITLE = "Enter the code"
    const val CODE_SENT_TO = "We sent a 6-digit code to {email}. It works for 10 minutes."
    const val CODE_LABEL = "6-digit code"
    const val CODE_RESENT = "We sent a new code. Use the newest email: older codes stop working."
    const val CODE_MISSING = "It can take a minute. Look for a mail from Momotus, and check your spam folder."
    const val CODE_RESEND = "Send a new code"
    const val CODE_RESEND_IN = "Send a new code in {seconds} seconds"
    const val CODE_OTHER_ADDRESS = "Use another address"
    const val WRONG_CODE = "That code isn't right. Check the email and try again."
    const val CODE_EXPIRED = "This code can't be used any more. Send a new code."
    val CODE_EXPIRED_ACTIONS = listOf("Send a new code", "Use another address")
    const val EMAIL_WAIT = "Wait a minute, then send a new code."
    const val EMAIL_LIMIT =
        "Too many codes were asked for just now. Try again tomorrow, or use another way to sign in."
    val EMAIL_LIMIT_ACTIONS = listOf("Other ways to sign in")
    const val MAIL_UNAVAILABLE =
        "The code couldn't be sent right now. Try again later, or use another way to sign in."
    val MAIL_UNAVAILABLE_ACTIONS = listOf("Try again", "Other ways to sign in")
    const val DELETION_SCHEDULED_BY_EMAIL =
        "Someone signed in with your email address and asked to delete your Momotus account on {date}. " +
            "Not you? Cancel, and turn off email sign-in."
    const val DELETION_CANCEL_UNLINK_EMAIL = "Cancel and turn off email sign-in"

    /** Every key of the shared file with its words, for the reader test. */
    val all: Map<String, Any> =
        mapOf(
            "announceDevice" to ANNOUNCE_DEVICE,
            "announceDeviceUnnamed" to ANNOUNCE_DEVICE_UNNAMED,
            "announcePasskey" to ANNOUNCE_PASSKEY,
            "announceKit" to ANNOUNCE_KIT,
            "announceRemoved" to ANNOUNCE_REMOVED,
            "announceRemovedUnnamed" to ANNOUNCE_REMOVED_UNNAMED,
            "announceRemovedPasskey" to ANNOUNCE_REMOVED_PASSKEY,
            "announceRemovedKit" to ANNOUNCE_REMOVED_KIT,
            "announceLeft" to ANNOUNCE_LEFT,
            "announceLeftUnnamed" to ANNOUNCE_LEFT_UNNAMED,
            "announceDropped" to ANNOUNCE_DROPPED,
            "announceUnnamedDevice" to ANNOUNCE_UNNAMED_DEVICE,
            "announceGoneDevice" to ANNOUNCE_GONE_DEVICE,
            "announceNotYou" to ANNOUNCE_NOT_YOU,
            "removePasskeyAsk" to REMOVE_PASSKEY_ASK,
            "announceOk" to ANNOUNCE_OK,
            "providerAccount" to PROVIDER_ACCOUNT,
            "providerAccountOther" to PROVIDER_ACCOUNT_OTHER,
            "providerName" to PROVIDER_NAME,
            "signInTitle" to SIGN_IN_TITLE,
            "continueWith" to CONTINUE_WITH,
            "continue" to CONTINUE,
            "otherWays" to OTHER_WAYS,
            "usePasskey" to USE_PASSKEY,
            "scanFromDevice" to SCAN_FROM_DEVICE,
            "useRecoveryKit" to USE_RECOVERY_KIT,
            "useOwnServer" to USE_OWN_SERVER,
            "notNow" to NOT_NOW,
            "passkeyHere" to PASSKEY_HERE,
            "createAccount" to CREATE_ACCOUNT,
            "alreadyUse" to ALREADY_USE,
            "libraryAdded" to LIBRARY_ADDED,
            "cantUseAny" to CANT_USE_ANY,
            "tvSignIn" to TV_SIGN_IN,
            "tvSignInHow" to TV_SIGN_IN_HOW,
            "tvWithoutAccount" to TV_WITHOUT_ACCOUNT,
            "switchProfile" to SWITCH_PROFILE,
            "signInAnother" to SIGN_IN_ANOTHER,
            "approveWarning" to APPROVE_WARNING,
            "scanAgainHere" to SCAN_AGAIN_HERE,
            "codeFound" to CODE_FOUND,
            "usePhoneToSignIn" to USE_PHONE_TO_SIGN_IN,
            "tvScanHow" to TV_SCAN_HOW,
            "tvQrLabel" to TV_QR_LABEL,
            "browseFirst" to BROWSE_FIRST,
            "tvSignedInFrom" to TV_SIGNED_IN_FROM,
            "approveAsk" to APPROVE_ASK,
            "approveAskTv" to APPROVE_ASK_TV,
            "approveAskOther" to APPROVE_ASK_OTHER,
            "approveUnnamed" to APPROVE_UNNAMED,
            "approveButton" to APPROVE_BUTTON,
            "approveNear" to APPROVE_NEAR,
            "approveScanHere" to APPROVE_SCAN_HERE,
            "approveSigningIn" to APPROVE_SIGNING_IN,
            "approveDone" to APPROVE_DONE,
            "approveNotYou" to APPROVE_NOT_YOU,
            "approveSlow" to APPROVE_SLOW,
            "approveAnswered" to APPROVE_ANSWERED,
            "approveExpired" to APPROVE_EXPIRED,
            "approveKids" to APPROVE_KIDS,
            "phoneScanHow" to PHONE_SCAN_HOW,
            "passkeyLabel" to PASSKEY_LABEL,
            "recoveryKitName" to RECOVERY_KIT_NAME,
            "joinedWithPasskey" to JOINED_WITH_PASSKEY,
            "lastUsed" to LAST_USED,
            "givingAway" to GIVING_AWAY,
            "kitHere" to KIT_HERE,
            "kitDropped" to KIT_DROPPED,
            "tooManyKits" to TOO_MANY_KITS,
            "kitTaken" to KIT_TAKEN,
            "plainLink" to PLAIN_LINK,
            "debridConnected" to DEBRID_CONNECTED,
            "traktConnected" to TRAKT_CONNECTED,
            "unlinkAnyway" to UNLINK_ANYWAY,
            "deleteAnyway" to DELETE_ANYWAY,
            "accountGone" to ACCOUNT_GONE,
            "accountGoneActions" to ACCOUNT_GONE_ACTIONS,
            "deletionScheduled" to DELETION_SCHEDULED,
            "deletionScheduledBy" to DELETION_SCHEDULED_BY,
            "deletionCancel" to DELETION_CANCEL,
            "deletionCancelUnlink" to DELETION_CANCEL_UNLINK,
            "secondWayIn" to SECOND_WAY_IN,
            "noSecondWayIn" to NO_SECOND_WAY_IN,
            "noSecondWayInActions" to NO_SECOND_WAY_IN_ACTIONS,
            "removeLibraryAsk" to REMOVE_LIBRARY_ASK,
            "removeLibraryDebrid" to REMOVE_LIBRARY_DEBRID,
            "reportLostRemove" to REPORT_LOST_REMOVE,
            "unreachable" to UNREACHABLE,
            "unreachableActions" to UNREACHABLE_ACTIONS,
            "noRedirect" to NO_REDIRECT,
            "noRedirectActions" to NO_REDIRECT_ACTIONS,
            "passkeyCancelled" to PASSKEY_CANCELLED,
            "passkeyCancelledActions" to PASSKEY_CANCELLED_ACTIONS,
            "kitUnknownPasskey" to KIT_UNKNOWN_PASSKEY,
            "kitUnknownPasskeyActions" to KIT_UNKNOWN_PASSKEY_ACTIONS,
            "startOverWarning" to START_OVER_WARNING,
            "ticketExpired" to TICKET_EXPIRED,
            "ticketExpiredActions" to TICKET_EXPIRED_ACTIONS,
            "signupLimit" to SIGNUP_LIMIT,
            "signupLimitActions" to SIGNUP_LIMIT_ACTIONS,
            "signupLimitNetwork" to SIGNUP_LIMIT_NETWORK,
            "linkedElsewhere" to LINKED_ELSEWHERE,
            "linkedElsewhereActions" to LINKED_ELSEWHERE_ACTIONS,
            "busy" to BUSY,
            "otherRelay" to OTHER_RELAY,
            "otherRelayCode" to OTHER_RELAY_CODE,
            "otherRelayCodeActions" to OTHER_RELAY_CODE_ACTIONS,
            "signInRefused" to SIGN_IN_REFUSED,
            "signInWindow" to SIGN_IN_WINDOW,
            "popupBlocked" to POPUP_BLOCKED,
            "returnClose" to RETURN_CLOSE,
            "ownServerLabel" to OWN_SERVER_LABEL,
            "ownServerUse" to OWN_SERVER_USE,
            "foundWeb" to FOUND_WEB,
            "startOver" to START_OVER,
            "signedInWith" to SIGNED_IN_WITH,
            "linkAnother" to LINK_ANOTHER,
            "linkedOn" to LINKED_ON,
            "unlink" to UNLINK,
            "savePasskeyHere" to SAVE_PASSKEY_HERE,
            "unlinkLast" to UNLINK_LAST,
            "appleRevokeFailed" to APPLE_REVOKE_FAILED,
            "wiping" to WIPING,
            "wipeFailed" to WIPE_FAILED,
            "secondWayInText" to SECOND_WAY_IN_TEXT,
            "notOwner" to NOT_OWNER,
            "continueWithEmail" to CONTINUE_WITH_EMAIL,
            "emailTitle" to EMAIL_TITLE,
            "emailLabel" to EMAIL_LABEL,
            "emailSend" to EMAIL_SEND,
            "badAddress" to BAD_ADDRESS,
            "codeTitle" to CODE_TITLE,
            "codeSentTo" to CODE_SENT_TO,
            "codeLabel" to CODE_LABEL,
            "codeResent" to CODE_RESENT,
            "codeMissing" to CODE_MISSING,
            "codeResend" to CODE_RESEND,
            "codeResendIn" to CODE_RESEND_IN,
            "codeOtherAddress" to CODE_OTHER_ADDRESS,
            "wrongCode" to WRONG_CODE,
            "codeExpired" to CODE_EXPIRED,
            "codeExpiredActions" to CODE_EXPIRED_ACTIONS,
            "emailWait" to EMAIL_WAIT,
            "emailLimit" to EMAIL_LIMIT,
            "emailLimitActions" to EMAIL_LIMIT_ACTIONS,
            "mailUnavailable" to MAIL_UNAVAILABLE,
            "mailUnavailableActions" to MAIL_UNAVAILABLE_ACTIONS,
            "deletionScheduledByEmail" to DELETION_SCHEDULED_BY_EMAIL,
            "deletionCancelUnlinkEmail" to DELETION_CANCEL_UNLINK_EMAIL,
        )
}
