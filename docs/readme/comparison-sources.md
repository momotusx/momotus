# Comparison sources

Every cell of the README's comparison table, with the primary source it was checked against on 2026-10-27, and again on 2026-11-08. Competitor code links are pinned to the commit read that day.

## Stremio

| Row | Cell | Source |
|---|---|---|
| Price | Free, with a paid Supporter plan | https://www.stremio.com/plans (Basic free forever; Supporter yearly $39.99 or lifetime $199.99), checked 2026-11-14 |
| Stremio addons | Yes | https://www.stremio.com/ (FAQ, "How does it work?": install addons from the addon catalog) |
| Installable plugins | No (addons run no local code) | https://www.stremio.com/ (FAQ, "Is Stremio safe?": "The addons do not run any code locally") |
| Android phone, Android TV | Yes | https://www.stremio.com/downloads, https://www.stremio.com/ ("dedicated apps for Android Mobile and Android TV") |
| iPhone and iPad | Sideload, AltStore PAL in the EU, Brazil and Japan | https://blog.stremio.com/stremio-fully-featured-sideloadable-ipa-release-for-ios-ipados-apple-tv-tvos (2026-02-02: full IPA for iOS, iPadOS, tvOS; still waiting for Apple on Stremio Lite); https://blog.stremio.com/stremio-released-on-apple-altstore-for-europe-and-japan (2026-06-09); https://unarr.app/guides/stremio-on-iphone (2026-09-14: Stremio Lite was removed and the downloads page has no App Store link) |
| Apple TV | Sideload | the IPA post above; https://blog.stremio.com/stremio-tech-update-86-stremio-update-for-macos-and-ios (2026-10-22: Stremio 2.1 for tvOS); https://docs.elfhosted.com/guides/media/stremio-on-apple-tv (Stremio Lite reached the tvOS App Store in October 2025 and was pulled in January 2026) |
| LG TV | Yes, 2020 and later models | https://blog.stremio.com/stremio-for-lg-tv-is-back-2020-models-including-2026-models (2026-08-25: relisted in the LG TV store for 2020 and later models, 2026 models included) |
| Mac | Yes | https://www.stremio.com/downloads (Stremio 5 ARM and Intel) |
| Web browser | Yes | https://www.stremio.com/downloads ("Web version", "Stremio Web - Source Code"), https://web.strem.io |
| End-to-end encrypted sync | No | https://github.com/Stremio/stremio-core/blob/029301839c86/src/types/api/request.rs#L301-L344 (the library syncs as `datastorePut` with plain `LibraryItem` values to Stremio's API); https://www.stremio.com/ (FAQ, privacy: data "to create and sync your account") |
| Torrent engine built in | Yes | https://www.stremio.com/ (FAQ: "Can Stremio play magnet links? Yes, and it can also play normal HTTP links and torrent files") |
| Profiles | Yes, with the paid Supporter plan | https://www.stremio.com/plans ("Up to 6 user profiles", "Child-safe profiles & Catalogs" among the Supporter features), checked 2026-11-14 |
| Offline downloads | Yes, with the paid Supporter plan | https://www.stremio.com/plans (Download Manager, a Supporter feature: $39.99, or $199.99 lifetime); the FAQ on https://www.stremio.com/ still calls offline "upcoming" |
| Dual subtitles | Not found | https://github.com/Stremio/stremio-core/blob/029301839c86/src/types/profile/settings.rs#L26 (`secondary_subtitles_language` is a second preferred language, not a second track on screen); nothing else found in the Stremio organization's code |
| TV sign-in by QR code | Yes | https://github.com/Stremio/stremio-bugs/issues/1924 (Android TV shows a QR code; a Stremio maintainer: "you scan the code on android tv from your phone and log in"); https://github.com/Stremio/stremio-bugs/issues/720 (Samsung TV QR login) |
| Account needed | No (guest mode) | https://www.stremio.com/ (FAQ, privacy: "There is also a Guest mode at signup ... no calls are made to our backend", without library sync) |
| Licence | Web app GPL-2.0, core MIT | https://github.com/Stremio/stremio-web (GPL-2.0), https://github.com/Stremio/stremio-core (MIT); the GitHub organization lists no Android, Android TV or iOS app repository |

## Nuvio

| Row | Cell | Source |
|---|---|---|
| Price | Free, with a paid Supporter plan (cosmetic extras) | https://www.nuvio.tv/support ("The core app stays free"; Supporter $1.50 a month: themes, backgrounds, avatars, a badge), checked 2026-11-14 |
| Stremio addons | Yes | https://github.com/NuvioMedia/NuvioMobile/tree/61658f366f98/composeApp/src/commonMain/kotlin/com/nuvio/app/features/addons (manifest parser, addon HTTP client); the repository keeps Stremio's addon protocol docs under `Docs/Stremio addons refer` |
| Installable plugins | Yes, sideloaded builds | https://github.com/NuvioMedia/NuvioMobile/tree/61658f366f98/composeApp/src/fullCommonMain/kotlin/com/nuvio/app/features/plugins (the full flavour only); store builds leave plugins out (https://nuvio.wiki/integrations/plugins, unofficial, agrees) |
| Android phone | Yes | https://github.com/NuvioMedia/NuvioMobile (README: Google Play, APK) |
| Android TV and Fire TV | Yes | https://github.com/NuvioMedia/NuvioTV (README: Google Play, APK) |
| iPhone and iPad | Sideload | https://github.com/NuvioMedia/NuvioMobile (README: "iOS via AltStore or SideStore"); https://nuvio.tv/ ("TestFlight unavailable right now") |
| Apple TV | No | https://nuvio.tv/ (the platform list: Android, iOS, Android TV, Windows, macOS, Linux, Samsung Tizen, LG webOS); no tvOS target in NuvioMobile |
| LG TV | Sideload, with its own installer | https://github.com/NuvioMedia/NuvioTVWebOS (README, read 2026-11-08: the Nuvio WebTV Installer for Windows, macOS and Linux installs the `.ipk` on an LG webOS TV); https://nuvio.tv/ (the platform list names LG webOS) |
| Mac | Alpha | https://github.com/NuvioMedia/NuvioDesktop (README: "Alpha Software, Slow Development, Testers Only", macOS DMG) |
| Web browser | No | https://nuvio.tv/ (platform list above; the web dashboard manages the account, it does not play) |
| End-to-end encrypted sync | No | https://github.com/NuvioMedia/self-host/blob/30f156807ae7/docs/architecture.md ("PostgreSQL stores accounts, profiles, add-ons, plugins, library items, watch progress ..."; access rules separate accounts) |
| Torrent engine built in | Yes, off by default | https://github.com/NuvioMedia/NuvioMobile/blob/61658f366f98/composeApp/src/commonMain/kotlin/com/nuvio/app/features/p2p/P2pStreaming.kt (`p2pEnabled: Boolean = false`); https://github.com/NuvioMedia/NuvioTV/tree/252e9eb0ceb3/app/src/main/java/com/nuvio/tv/core/torrent |
| Profiles | Yes | https://github.com/NuvioMedia/NuvioMobile/tree/46febb568348/composeApp/src/androidMain/kotlin/com/nuvio/app/features/profiles (profiles with a PIN) |
| Offline downloads | Yes, on mobile | https://github.com/NuvioMedia/NuvioMobile/tree/61658f366f98/composeApp/src/commonMain/kotlin/com/nuvio/app/features/downloads |
| Dual subtitles | Not found | NuvioMobile's `SubtitleSelectionModel.kt` takes a `secondaryPreferredLanguage` (a fallback language, not a second track); a code search of the NuvioMedia organization for dual subtitles found none |
| TV sign-in by QR code | Yes | https://github.com/NuvioMedia/NuvioTV/blob/252e9eb0ceb3/app/src/main/java/com/nuvio/tv/ui/screens/account/AuthQrSignInScreen.kt |
| Account needed | No (anonymous sign-in) | https://github.com/NuvioMedia/NuvioMobile/blob/61658f366f98/composeApp/src/commonMain/kotlin/com/nuvio/app/features/auth/AuthScreen.kt#L307-L310 ("Continue without account" calls `AuthRepository.signInAnonymously()`) |
| Licence | GPL-3.0 | https://github.com/NuvioMedia/NuvioMobile, https://github.com/NuvioMedia/NuvioTV, https://nuvio.tv/ ("Free and open source under GNU GPLv3") |

## Momotus (the trunk, feat/protocol-wire, 2026-11-08)

| Row | Cell | Source |
|---|---|---|
| Price | Free, nothing paid | no purchase, subscription, ad or donation code in `apps/` or `crates/` |
| Stremio addons | Yes | `crates/mq-addons` |
| Android phone, Android TV and Fire TV | Yes | the release assets; `apps/android/app/src/main/AndroidManifest.xml` (LEANBACK_LAUNCHER) |
| iPhone and iPad, Apple TV | From source | `apps/apple/project.yml` (iOS 18, tvOS 18 targets); no released build |
| LG TV | In progress | the TV layout of `apps/web` on webOS (`apps/web/src/lib/webos.ts`) and the TV app with its local service in `apps/webos`; no released build |
| Mac | From source | `apps/apple/project.yml` (macOS 15 target); the releases carry Android APKs only |
| Web browser | From source | `apps/web` with `crates/mq-bridge` (a local core over a loopback socket) |
| End-to-end encrypted sync | Yes | `crates/mq-sync/src/lib.rs` ("End-to-end encrypted sync of library registers through a blind relay"), `crates/mq-sync/src/seal.rs` |
| Installable plugins | Yes, Nuvio plugin repositories | `crates/mq-plugins`; plugin repositories in Settings and "From plugins" in Versions on every app (`apps/web/src/screens/settings.tsx`, `apps/android/app/src/main/kotlin/app/marquee/android/PluginsScreen.kt`, `apps/apple/Marquee/Settings/PluginsScreen.swift`) |
| Torrent engine built in | Yes | `crates/mq-engine`; own builds carry it on every slice (`apps/apple/scripts/build-core.sh`, `apps/android/scripts/build-core.sh` for arm64-v8a and armeabi-v7a) |
| Profiles | Yes | `crates/mq-runtime/src/profiles.rs` |
| Offline downloads | Yes, on phones and computers | `apps/web/src/screens/downloads.tsx`, `apps/android/app/src/main/kotlin/app/marquee/android/DownloadsScreen.kt`, `apps/apple/Marquee/Downloads/DownloadsScreen.swift`; TVs show no downloads (`AppRoot.kt`, `onDownloads = if (tv) null`) |
| Dual subtitles | Yes | a second subtitle line in another language, a profile setting with three layouts (`secondSubtitle` in `crates/mq-runtime`, the player and Settings on every app); plans/dual-subtitles.md |
| TV sign-in by QR code | Yes | a new device shows a pairing code that a phone scans (`apps/android/app/src/main/kotlin/app/marquee/android/SyncDevice.kt`) |
| Account needed | No | first run offers Start fresh and Browse first (bench/m6/journey-android.jsonl FTV3); sync is optional |
| Licence | AGPL-3.0-or-later | `LICENSE`, `Cargo.toml` (`license = "AGPL-3.0-or-later"`) |

## Rows left out

| Row | Momotus | Stremio | Nuvio | Why left out |
|---|---|---|---|---|
| Addon catalog in the app | Yes (Find addons: Official and Community) | Yes (https://www.stremio.com/ FAQ) | not verified (NuvioTV has an essential-addon setup screen; no catalog found in NuvioMobile) | Nuvio cell not verified |
| Kodi | | | | it does not run Stremio addons, so most rows do not apply |
