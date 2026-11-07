# Comparison sources

Every cell of the README's comparison table, with the primary source it was checked against on 2026-10-27. Competitor code links are pinned to the commit read that day.

## Stremio

| Row | Cell | Source |
|---|---|---|
| Stremio addons | Yes | https://www.stremio.com/ (FAQ, "How does it work?": install addons from the addon catalog) |
| Installable plugins | No (addons run no local code) | https://www.stremio.com/ (FAQ, "Is Stremio safe?": "The addons do not run any code locally") |
| Android phone, Android TV | Yes | https://www.stremio.com/downloads, https://www.stremio.com/ ("dedicated apps for Android Mobile and Android TV") |
| iPhone and iPad | App Store (Lite), sideload | https://blog.stremio.com/stremio-fully-featured-sideloadable-ipa-release-for-ios-ipados-apple-tv-tvos (2026-02-02: full IPA for iOS, iPadOS, tvOS; Stremio Lite waiting on App Store review); https://www.stremio.com/downloads ("Stremio for iOS (limited version)", AltStore PAL in the EU, Brazil and Japan) |
| Apple TV | App Store (Lite), sideload | https://blog.stremio.com/stremio-lite-for-apple-tv-tvos-is-now-in-app-store (2025-10-29); the IPA post above |
| Mac | Yes | https://www.stremio.com/downloads (Stremio 5 ARM and Intel) |
| Web browser | Yes | https://www.stremio.com/downloads ("Web version", "Stremio Web - Source Code"), https://web.strem.io |
| End-to-end encrypted sync | No | https://github.com/Stremio/stremio-core/blob/029301839c86/src/types/api/request.rs#L301-L344 (the library syncs as `datastorePut` with plain `LibraryItem` values to Stremio's API); https://www.stremio.com/ (FAQ, privacy: data "to create and sync your account") |
| Torrent engine built in | Yes | https://www.stremio.com/ (FAQ: "Can Stremio play magnet links? Yes, and it can also play normal HTTP links and torrent files") |
| Offline downloads | No (planned) | https://www.stremio.com/ (FAQ: "Available offline is an upcoming feature", with a cache setting meanwhile) |
| Dual subtitles | Not found | https://github.com/Stremio/stremio-core/blob/029301839c86/src/types/profile/settings.rs#L26 (`secondary_subtitles_language` is a second preferred language, not a second track on screen); nothing else found in the Stremio organization's code |
| TV sign-in by QR code | Yes | https://github.com/Stremio/stremio-bugs/issues/1924 (Android TV shows a QR code; a Stremio maintainer: "you scan the code on android tv from your phone and log in"); https://github.com/Stremio/stremio-bugs/issues/720 (Samsung TV QR login) |
| Account needed | No (guest mode) | https://www.stremio.com/ (FAQ, privacy: "There is also a Guest mode at signup ... no calls are made to our backend", without library sync) |
| Licence | Web app GPL-2.0, core MIT | https://github.com/Stremio/stremio-web (GPL-2.0), https://github.com/Stremio/stremio-core (MIT); the GitHub organization lists no Android, Android TV or iOS app repository |

## Nuvio

| Row | Cell | Source |
|---|---|---|
| Stremio addons | Yes | https://github.com/NuvioMedia/NuvioMobile/tree/61658f366f98/composeApp/src/commonMain/kotlin/com/nuvio/app/features/addons (manifest parser, addon HTTP client); the repository keeps Stremio's addon protocol docs under `Docs/Stremio addons refer` |
| Installable plugins | Yes, sideloaded builds | https://github.com/NuvioMedia/NuvioMobile/tree/61658f366f98/composeApp/src/fullCommonMain/kotlin/com/nuvio/app/features/plugins (the full flavour only); store builds leave plugins out (https://nuvio.wiki/integrations/plugins, unofficial, agrees) |
| Android phone | Yes | https://github.com/NuvioMedia/NuvioMobile (README: Google Play, APK) |
| Android TV and Fire TV | Yes | https://github.com/NuvioMedia/NuvioTV (README: Google Play, APK) |
| iPhone and iPad | Sideload | https://github.com/NuvioMedia/NuvioMobile (README: "iOS via AltStore or SideStore"); https://nuvio.tv/ ("TestFlight unavailable right now") |
| Apple TV | No | https://nuvio.tv/ (the platform list: Android, iOS, Android TV, Windows, macOS, Linux, Samsung Tizen, LG webOS); no tvOS target in NuvioMobile |
| Mac | Alpha | https://github.com/NuvioMedia/NuvioDesktop (README: "Alpha Software, Slow Development, Testers Only", macOS DMG) |
| Web browser | No | https://nuvio.tv/ (platform list above; the web dashboard manages the account, it does not play) |
| End-to-end encrypted sync | No | https://github.com/NuvioMedia/self-host/blob/30f156807ae7/docs/architecture.md ("PostgreSQL stores accounts, profiles, add-ons, plugins, library items, watch progress ..."; access rules separate accounts) |
| Torrent engine built in | Yes, off by default | https://github.com/NuvioMedia/NuvioMobile/blob/61658f366f98/composeApp/src/commonMain/kotlin/com/nuvio/app/features/p2p/P2pStreaming.kt (`p2pEnabled: Boolean = false`); https://github.com/NuvioMedia/NuvioTV/tree/252e9eb0ceb3/app/src/main/java/com/nuvio/tv/core/torrent |
| Offline downloads | Yes, on mobile | https://github.com/NuvioMedia/NuvioMobile/tree/61658f366f98/composeApp/src/commonMain/kotlin/com/nuvio/app/features/downloads |
| Dual subtitles | Not found | NuvioMobile's `SubtitleSelectionModel.kt` takes a `secondaryPreferredLanguage` (a fallback language, not a second track); a code search of the NuvioMedia organization for dual subtitles found none |
| TV sign-in by QR code | Yes | https://github.com/NuvioMedia/NuvioTV/blob/252e9eb0ceb3/app/src/main/java/com/nuvio/tv/ui/screens/account/AuthQrSignInScreen.kt |
| Account needed | No (anonymous sign-in) | https://github.com/NuvioMedia/NuvioMobile/blob/61658f366f98/composeApp/src/commonMain/kotlin/com/nuvio/app/features/auth/AuthScreen.kt#L307-L310 ("Continue without account" calls `AuthRepository.signInAnonymously()`) |
| Licence | GPL-3.0 | https://github.com/NuvioMedia/NuvioMobile, https://github.com/NuvioMedia/NuvioTV, https://nuvio.tv/ ("Free and open source under GNU GPLv3") |

## Momotus (the trunk, feat/protocol-wire, 2026-10-27)

| Row | Cell | Source |
|---|---|---|
| Stremio addons | Yes | `crates/mq-addons` |
| Android phone, Android TV and Fire TV | Yes | the release assets; `apps/android/app/src/main/AndroidManifest.xml` (LEANBACK_LAUNCHER) |
| iPhone and iPad, Apple TV | From source | `apps/apple/project.yml` (iOS 18, tvOS 18 targets); no released build |
| Mac | Yes | the release asset `Momotus.dmg`; `apps/apple/project.yml` (macOS 15 target) |
| Web browser | From source | `apps/web` with `crates/mq-bridge` (a local core over a loopback socket) |
| End-to-end encrypted sync | Yes | `crates/mq-sync/src/lib.rs` ("End-to-end encrypted sync of library registers through a blind relay"), `crates/mq-sync/src/seal.rs` |
| Installable plugins | In progress | plans only, nothing on the trunk |
| Torrent engine built in | Yes, not yet in armv7 builds | `crates/mq-engine`; own builds carry it (`apps/apple/scripts/build-core.sh`; `apps/android/scripts/build-core.sh` keeps armeabi-v7a without it until its walk) |
| Offline downloads | In progress | not on the trunk |
| Dual subtitles | In progress | not on the trunk |
| TV sign-in by QR code | Yes | a new device shows a pairing code that a phone scans (`apps/android/app/src/main/kotlin/app/marquee/android/SyncDevice.kt`) |
| Account needed | No | first run offers Start fresh and Browse first (bench/m6/journey-android.jsonl FTV3); sync is optional |
| Licence | AGPL-3.0-or-later | `LICENSE`, `Cargo.toml` (`license = "AGPL-3.0-or-later"`) |

## Rows left out

| Row | Momotus | Stremio | Nuvio | Why left out |
|---|---|---|---|---|
| Addon catalog in the app | Yes (Find addons: Official and Community) | Yes (https://www.stremio.com/ FAQ) | not verified (NuvioTV has an essential-addon setup screen; no catalog found in NuvioMobile) | Nuvio cell not verified |
| Kodi | | | | it does not run Stremio addons, so most rows do not apply |
