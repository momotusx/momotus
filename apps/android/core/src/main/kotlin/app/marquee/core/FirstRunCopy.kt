package app.marquee.core

/**
 * First run words (plans/first-screens.md 3.1 to 3.5), word for word `apps/shared/first-run-words.json`, which the web
 * and Apple read too; a test checks every key and value. Sign in, Not now, the provider buttons and the unreachable
 * and refused lines stay [AccountCopy]'s; the stream question stays [StreamChoiceCopy]'s.
 */
object FirstRunCopy {
    const val WELCOME_TITLE = "Your films and series, on every screen."
    const val WELCOME_LEAD =
        "Bring your Stremio addons. Your library and where you left off follow you, end-to-end encrypted."
    const val GET_STARTED = "Get started"
    const val ALREADY_USE = "I already use Momotus"
    const val PAUSE_MOTION = "Pause motion"
    const val PLAY_MOTION = "Play motion"
    const val LOGIN_LINE = "Keep your library and where you left off on every screen."
    const val LOGIN_RETURNING = "Sign in the way you did on your other device, or use another device."
    const val USE_ANOTHER_DEVICE = "Use another device"
    const val TV_LOGIN_TITLE = "Sign in to Momotus"
    const val QR_TITLE = "Sign in with your phone"
    const val QR_HOW = "Point your phone's camera at the code, then sign in on the page that opens."
    const val QR_WAITING = "Waiting for your phone"
    val TV_STEPS =
        listOf(
            "Point your phone's camera at the code.",
            "Sign in on the page that opens.",
            "This TV signs in by itself.",
        )
    const val TV_NO_QR = "This TV can't show a sign-in code right now."
    const val TRY_AGAIN = "Try again"
    const val GETTING_PROFILES = "Getting your profiles"
    const val WHO_TITLE = "Who's watching?"
    const val WHO_PROVIDER = "Signed in with {provider}. Pick your profile."
    const val WHO_DEVICE = "Signed in from {device}."
    const val WHO_SIGNED_IN = "Signed in."
    const val WHO_KIDS = "Only titles for children"
    const val ADD_PROFILE = "Add profile"
    const val PROFILE_TITLE = "Make your first profile"
    const val PROFILE_LINE_ACCOUNT =
        "Your account has no profiles yet. A profile keeps its own library, subtitles and Continue Watching."
    const val PROFILE_LINE_LOCAL = "A profile keeps its own library, subtitles and Continue Watching."
    const val NAME_LABEL = "Name"
    const val NAME_PLACEHOLDER = "Main"
    const val COLOUR_LABEL = "Colour"
    const val CONTINUE = "Continue"
    const val SETUP_TITLE = "Set up {profile}"
    const val SETUP_LINE = "Picked for you. Change any of it later in Settings."
    const val SKIP = "Skip"
    const val BRING_TITLE = "Bring anything from Stremio?"
    const val BRING_LIBRARY = "Bring my Stremio library"
    const val BRING_LIBRARY_LINE = "Addons, library and Continue Watching."
    const val START_FRESH = "Start fresh"
    const val START_FRESH_LINE = "Start with the recommended sources."
    const val SUBTITLES_TITLE = "Subtitles start in"
    const val NO_SUBTITLES = "No subtitles"
    const val MORE_LANGUAGES = "More"
    const val SOURCES_ON = "Recommended sources are on."
    const val REVIEW = "Review"
    const val START_WATCHING = "Start watching"
    const val SETUP_PREFS_FAILED = "Your choices could not be saved."
    const val SETUP_SOURCES_FAILED = "The recommended sources could not be added."
    const val SKIP_IMPORT = "Skip import"

    /** Every key of the shared file, for the test. */
    val all: Map<String, Any> =
        mapOf(
            "welcomeTitle" to WELCOME_TITLE,
            "welcomeLead" to WELCOME_LEAD,
            "getStarted" to GET_STARTED,
            "alreadyUse" to ALREADY_USE,
            "pauseMotion" to PAUSE_MOTION,
            "playMotion" to PLAY_MOTION,
            "loginLine" to LOGIN_LINE,
            "loginReturning" to LOGIN_RETURNING,
            "useAnotherDevice" to USE_ANOTHER_DEVICE,
            "tvLoginTitle" to TV_LOGIN_TITLE,
            "qrTitle" to QR_TITLE,
            "qrHow" to QR_HOW,
            "qrWaiting" to QR_WAITING,
            "tvSteps" to TV_STEPS,
            "tvNoQr" to TV_NO_QR,
            "tryAgain" to TRY_AGAIN,
            "gettingProfiles" to GETTING_PROFILES,
            "whoTitle" to WHO_TITLE,
            "whoProvider" to WHO_PROVIDER,
            "whoDevice" to WHO_DEVICE,
            "whoSignedIn" to WHO_SIGNED_IN,
            "whoKids" to WHO_KIDS,
            "addProfile" to ADD_PROFILE,
            "profileTitle" to PROFILE_TITLE,
            "profileLineAccount" to PROFILE_LINE_ACCOUNT,
            "profileLineLocal" to PROFILE_LINE_LOCAL,
            "nameLabel" to NAME_LABEL,
            "namePlaceholder" to NAME_PLACEHOLDER,
            "colourLabel" to COLOUR_LABEL,
            "continue" to CONTINUE,
            "setupTitle" to SETUP_TITLE,
            "setupLine" to SETUP_LINE,
            "skip" to SKIP,
            "bringTitle" to BRING_TITLE,
            "bringLibrary" to BRING_LIBRARY,
            "bringLibraryLine" to BRING_LIBRARY_LINE,
            "startFresh" to START_FRESH,
            "startFreshLine" to START_FRESH_LINE,
            "subtitlesTitle" to SUBTITLES_TITLE,
            "noSubtitles" to NO_SUBTITLES,
            "moreLanguages" to MORE_LANGUAGES,
            "sourcesOn" to SOURCES_ON,
            "review" to REVIEW,
            "startWatching" to START_WATCHING,
            "setupPrefsFailed" to SETUP_PREFS_FAILED,
            "setupSourcesFailed" to SETUP_SOURCES_FAILED,
            "skipImport" to SKIP_IMPORT,
        )
}
