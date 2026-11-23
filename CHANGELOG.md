# Changelog

## [0.6.1](https://github.com/momotusx/momotus/compare/v0.6.0...v0.6.1) (2026-11-23)


### Bug Fixes

* **web:** a resting pointer's repeated place keeps the Incognito warning ([7bab550](https://github.com/momotusx/momotus/commit/7bab550e6392a148038d79d272544a45e90b3d34))
* **web:** the app's own focus moves keep the Incognito warning ([1099b17](https://github.com/momotusx/momotus/commit/1099b17b731d3e6fb3552e3e1af7aa8aad1eaac5))


### Performance Improvements

* **plugins:** a provider out of CPU twice stops, and passes yield to the app ([999b428](https://github.com/momotusx/momotus/commit/999b428f696d8a75db28bca06f0d83cf323df7a7))
* **plugins:** a provider's runs out of CPU are kept with its script ([b4a729b](https://github.com/momotusx/momotus/commit/b4a729bae5dfe1560d457b2c67031cb0b9e828bb))
* **plugins:** each provider script is parsed once, kept as bytecode ([bc48798](https://github.com/momotusx/momotus/commit/bc48798cfc2897193e335e06ed12e0b0fe512e62))
* **plugins:** kept bytecode carries the source, one run per call ([d33b0f2](https://github.com/momotusx/momotus/commit/d33b0f2f64e27f33d23dffe3715b63741271a2df))

## [0.6.0](https://github.com/momotusx/momotus/compare/v0.5.0...v0.6.0) (2026-11-23)


### Features

* **android:** the app updates itself from GitHub Releases ([b3a16a4](https://github.com/momotusx/momotus/commit/b3a16a409c22032a2fe326ce35d1075bcb41c88d))
* **android:** verify the update feed and pick this device's APK ([7e2656b](https://github.com/momotusx/momotus/commit/7e2656b38b61b57b9bfff029670fdf4ec9bed0b6))
* **webos:** About shows the app's update and Install, Home a quiet line ([79f8878](https://github.com/momotusx/momotus/commit/79f887848fbaf4a1050073a5cfcd26863347d4eb))
* **webos:** the service checks for updates and installs the new IPK ([62f2503](https://github.com/momotusx/momotus/commit/62f2503924acf2ec54313deb609f2c101f631d2c))


### Bug Fixes

* **android:** a benchmark build is always silent ([c66d033](https://github.com/momotusx/momotus/commit/c66d03379fa4cee8fd9d30400d26efe5fa30cfcd))
* **android:** a benchmark build plays silent when a walk asks ([46f65eb](https://github.com/momotusx/momotus/commit/46f65eb7ad35838a3e6b39bf1baeb28c51a89b8b))
* **android:** a fire-and-forget core request cut short by shutdown is not a failure ([e282a8d](https://github.com/momotusx/momotus/commit/e282a8dbad6ff62c4d56411bc8479c2e8fe47762))
* **android:** state flows never miss a change made while they first read ([2b7a6c3](https://github.com/momotusx/momotus/commit/2b7a6c3eb708c3e6edcf343f79d1dbe35769234a))
* **apple:** a card a scroll brought under the pointer lights on a move ([e3d8209](https://github.com/momotusx/momotus/commit/e3d8209fcaaeed630173a9db3a3df1f9f5a32a88))
* **apple:** a shelf is asked its height at its width alone ([91dbba5](https://github.com/momotusx/momotus/commit/91dbba5fad5f7e69e6527599d453b1eceb8f2baf))
* **apple:** art draws the current url's picture after a change ([a5678b8](https://github.com/momotusx/momotus/commit/a5678b8390c2d0d1fd0eaa08de6f53f2e4128c6f))
* **apple:** the spare trailer web view goes with playback and low memory ([1ff4e96](https://github.com/momotusx/momotus/commit/1ff4e9679cf5e8bd5b53fe25e07ce78ec4208e4c))
* **design:** the brand mark is decoded once per process ([f3adb9d](https://github.com/momotusx/momotus/commit/f3adb9d0d82b3006566c81bad57e9313cfda3276))
* **engine:** a window that holds its budget is never stopped as growing ([b43cf12](https://github.com/momotusx/momotus/commit/b43cf120e558f793574eeed3bbd85b2e60f8c6d9))
* **turkcealtyazi:** a request cut short spaces the next from its end ([ce451fd](https://github.com/momotusx/momotus/commit/ce451fde265f6b6d34f6302e272d7369b566aa0c))
* **web:** About and error reports follow the release version ([30242a3](https://github.com/momotusx/momotus/commit/30242a3bca5abfc1fbafb6c2fd5d7d10f7309489))


### Performance Improvements

* **android:** a drag or fling keeps a preview waiting until it ends ([04bdfed](https://github.com/momotusx/momotus/commit/04bdfed31c493b8fbaa798c97faf99d9a32a7cd0))
* **android:** previews start only once input has rested ([2dd1e67](https://github.com/momotusx/momotus/commit/2dd1e67ac008a88ff832b4fb79ba1ec347538111))
* **apple:** a card a scroll carries under a still pointer stays unlit ([9e839e3](https://github.com/momotusx/momotus/commit/9e839e3db807271b3ff7075ee599b86aaefdf02c))
* **apple:** a let-go trailer web view is the next player's spare ([fb6a5cb](https://github.com/momotusx/momotus/commit/fb6a5cb4e945dee4c4c9348771a04f14a6b0f1d3))
* **apple:** art already in memory draws in its first frame ([24d6029](https://github.com/momotusx/momotus/commit/24d60294f5c00080cd9a17978664ab815b63abd6))
* **apple:** places fill their frame without measuring every page ([b1aa9b3](https://github.com/momotusx/momotus/commit/b1aa9b3a96f6fbcfba40c2cb03da19d39715ad14))
* **apple:** shelves measure their cards once per width ([25eea3b](https://github.com/momotusx/momotus/commit/25eea3bddb7cb3e500bb39c786381a8862704b89))

## [0.5.0](https://github.com/momotusx/momotus/compare/v0.4.0...v0.5.0) (2026-11-20)


### Features

* **deploy:** deploy.sh ships a relay binary built elsewhere ([e6e33cd](https://github.com/momotusx/momotus/commit/e6e33cd8674ba22314ab545fdf4d8d8d5ed708aa))
* **deploy:** relay units, deploy and backups take an instance file ([fc703ff](https://github.com/momotusx/momotus/commit/fc703ffe54da97a74f722aa4198a16e07b581995))
* **relay:** an identity file may list its own brokered sign-in returns ([b56e637](https://github.com/momotusx/momotus/commit/b56e6379c248f303bc9c0e37ded7755522ed082c))
* **sync:** app registers behind traits and an escrow mode for the relay ([c52a4ef](https://github.com/momotusx/momotus/commit/c52a4ef3d444174372e1b84e5d191daf43889dfb))
* **web:** webOS title pages start no trailer by themselves ([06c18d2](https://github.com/momotusx/momotus/commit/06c18d259b3299510b317b93b428f94a89d27516))


### Bug Fixes

* **deploy:** the relay deploy checks the published main, not the old trunk ([e96b2e6](https://github.com/momotusx/momotus/commit/e96b2e69dee71cba7f77579d2021cfc00ba647d2))
* **engine:** a message is answered after the events queued before it ([6091d6a](https://github.com/momotusx/momotus/commit/6091d6ada75b0882016e26cd03ce47588158817b))
* **engine:** the trim keeps what a play resumed there fetches at once ([0f1c2ce](https://github.com/momotusx/momotus/commit/0f1c2ce32286b18794ea48602d151c526496c5a3))
* **runtime:** Home's decode limit comes from mq_rt::parallelism ([15a99b6](https://github.com/momotusx/momotus/commit/15a99b66cca4832a0253dee7fe06d3f7166ebc1b))
* **web:** a kept Home gives up the focus when a title opens ([aec5634](https://github.com/momotusx/momotus/commit/aec563414502f9f0c4ae5ca6ad3313a159b66601))
* **web:** a kept Home shows no Undo of its own under a title ([d469fa6](https://github.com/momotusx/momotus/commit/d469fa64686838e3179e4c28a582c38383941392))
* **web:** a new search query renders with its first screenful only ([cb284e8](https://github.com/momotusx/momotus/commit/cb284e8dc6825ef1f84937a76876659b31988be4))


### Performance Improvements

* **addons:** cache lookups run off the core's workers ([37a6801](https://github.com/momotusx/momotus/commit/37a6801a1178fc66b10c5ea2cc804df338836f55))
* **runtime:** a snapshot equal to the last push is not pushed again ([1774c39](https://github.com/momotusx/momotus/commit/1774c396d681d9a50a1e3fa7a7bdd3cf9e2fc29b))
* **runtime:** Home reads its catalogs off the core's workers ([3099c27](https://github.com/momotusx/momotus/commit/3099c278dac88c615f1b648ad15b903cfbf9bc90))
* **runtime:** Home reads only the titles a row shows, on every worker ([498efea](https://github.com/momotusx/momotus/commit/498efeab28eacd14af67e51493280851f2e6a05d))
* **runtime:** Home's row answers never come back as a reset ([89e30c0](https://github.com/momotusx/momotus/commit/89e30c089ce9d4b9c0a98e94e9bc998348d853cd))
* **runtime:** tile images at the size they are drawn, not the largest ([e49657f](https://github.com/momotusx/momotus/commit/e49657f6cab0cbf4027607f700a218ebd10b3968))
* **web:** Home waits under a title instead of being rebuilt on Back ([16b69e4](https://github.com/momotusx/momotus/commit/16b69e4e8013c512a724b595c975a294559f1722))
* **web:** row scrollers stack their own tiles ([3c6c722](https://github.com/momotusx/momotus/commit/3c6c7220ca6067afab9b87af155f03df428acb63))
* **web:** search builds its hits as the list nears them ([2a21651](https://github.com/momotusx/momotus/commit/2a21651fbe2e76df419fe087d605f6bfc8e523e6))
* **web:** title links render a plain anchor ([345034d](https://github.com/momotusx/momotus/commit/345034d405779e8f7a264cf13dddcf9dac661257))

## [0.4.0](https://github.com/momotusx/momotus/compare/v0.3.0...v0.4.0) (2026-11-18)


### Features

* **bridge:** a free port, a start heartbeat and a set device id ([88eef50](https://github.com/momotusx/momotus/commit/88eef503bc175a4e03ff12f45a12dbc962f7014e))
* **bridge:** a token, parent exit and an armv7 build for the TV ([338521d](https://github.com/momotusx/momotus/commit/338521deae3b4e65c63d17499d7a99464d5c0637))
* **engine:** bound peer dials in flight and per minute ([0368a62](https://github.com/momotusx/momotus/commit/0368a62d610a001e850f285a1610b9e5e54d306e))
* **web:** downloads on webOS with the native core ([fcb9dc9](https://github.com/momotusx/momotus/commit/fcb9dc9ac80f6b760f5560ec38298464613c6c51))
* **web:** error reports on webOS with the native core ([bd52287](https://github.com/momotusx/momotus/commit/bd522878011ee604592f14005d7137b9ca8b91dc))
* **webos:** keep the wasm library on update, start on a free port ([af3357b](https://github.com/momotusx/momotus/commit/af3357b8e8024cb1f44b3673011cce2eac16bf1a))
* **webos:** the service runs the native core, the wasm one as fallback ([b7731af](https://github.com/momotusx/momotus/commit/b7731af7d70291b2e7be6599964c852789d24b08))
* **web:** webOS lists torrents and header streams with the native core ([9267db7](https://github.com/momotusx/momotus/commit/9267db760139cd28270b37e65363592aef602f84))


### Bug Fixes

* **engine:** 32-bit Linux writes and punches past 2 GiB ([4055884](https://github.com/momotusx/momotus/commit/40558842354740ee935c3ebb2bc32452ef1a2a99))
* **webos:** bindgen gets zig's C headers as absolute paths ([53f8be9](https://github.com/momotusx/momotus/commit/53f8be942335b36d9d7ced42e0bd4b4d32a881e2))

## [0.3.0](https://github.com/momotusx/momotus/compare/v0.2.0...v0.3.0) (2026-11-18)


### Features

* **streams:** a direct-only player hides streams that need headers ([3c2ae2e](https://github.com/momotusx/momotus/commit/3c2ae2e04e0370111648d90da0b2c55f9acb2bff))
* **webos:** mq-rt runtime shim for the wasm core ([07a5fd7](https://github.com/momotusx/momotus/commit/07a5fd7f9a981cbe8d8ba371a9eea187a7013569))
* **webos:** run the core's HTTP through the webOS host ([ac4cd42](https://github.com/momotusx/momotus/commit/ac4cd424bf7630d190722437f376b47486734db5))
* **webos:** splash, icons, Try again on the stub, package.sh end to end ([924eea2](https://github.com/momotusx/momotus/commit/924eea2d31b0fed869a9a582cdcc795948c77a5a))
* **webos:** SQLite over the webOS host's files ([96597a0](https://github.com/momotusx/momotus/commit/96597a02b0b2b578db8879ec770f1b72bc732a5e))
* **webos:** the wasm core, its binding and its build ([2b61ce5](https://github.com/momotusx/momotus/commit/2b61ce5be62f9f672456d2945447cf418053120b))
* **web:** webOS asks the core for direct-only streams and its probe ([6fb05a6](https://github.com/momotusx/momotus/commit/6fb05a6c79310044c026fd82e7ef733548c61851))
* **web:** webOS player, Settings and modal fallbacks for Chromium 79 ([4a53bf4](https://github.com/momotusx/momotus/commit/4a53bf4e47a38fc01402f69cfd3d6a6d0710cbff))


### Bug Fixes

* **core:** Versions says why nothing plays without waiting on the Archive ([04a3ecb](https://github.com/momotusx/momotus/commit/04a3ecb398c127073124f3a0c410e990a635657b))
* **streams:** a direct-only player hides a stream whose URL carries a login ([13eceaa](https://github.com/momotusx/momotus/commit/13eceaa9fbc520dd5c6e2ad596a99ef1c2199001))
* **web:** a hand-picked version waits for the Archive and plugin passes ([c189a9c](https://github.com/momotusx/momotus/commit/c189a9c5d2db9445ea1621feee9d8905542d97ae))
* **web:** inert fallback never leaves focus inside an inert region ([4c7b0be](https://github.com/momotusx/momotus/commit/4c7b0be760a560b60b84acc95f7ccef2d499522f))
* **web:** inert works below Chrome 102 for every caller ([64f5791](https://github.com/momotusx/momotus/commit/64f579139f7364ce11110a3f2d166d4b36293e05))
* **webos:** a capped response no longer resets the next request ([8695342](https://github.com/momotusx/momotus/commit/8695342f03df941f2c23c09c7e60283c5978c9d5))
* **webos:** a request waiting for its host's slot ends at its deadline ([ae1ce2a](https://github.com/momotusx/momotus/commit/ae1ce2a505490e7a670e861d02f6aab421d5b54f))
* **webos:** each page socket gets its own Session in the core ([50dea55](https://github.com/momotusx/momotus/commit/50dea558ce5980f4f3600a61d628d80795a91e5b))
* **webos:** no new core starts beside a stale one that is still alive ([2f96889](https://github.com/momotusx/momotus/commit/2f9688927843b3a4bba24429464bbde592dbaa97))
* **webos:** package.sh reads the archive listing before checking it ([5327d25](https://github.com/momotusx/momotus/commit/5327d25530b513dad554d7ac10da9ded2a42360e))
* **web:** script selectors the TV can parse, checked from the AST ([01cb28c](https://github.com/momotusx/momotus/commit/01cb28c3b4abecb92b288f31c5b15bc7dbcd80b0))
* **web:** the Settings list keeps its icon gap on Chromium 79 ([c4e2318](https://github.com/momotusx/momotus/commit/c4e23189967730284a932d1b4d28a846449e6812))
* **web:** the sign-in autofocus selector runs on Chromium 79 ([6bb819d](https://github.com/momotusx/momotus/commit/6bb819dd921c15fa158c73d394eaf708c35ff2e0))
* **web:** the TV's poster wall and trailer frame keep their shape ([078551b](https://github.com/momotusx/momotus/commit/078551b77c821bbf0250d82382c3744dfacc1bec))
* **web:** Up and Down leave a text field on webOS ([2e6acd7](https://github.com/momotusx/momotus/commit/2e6acd79ce43601fdc24df7832874029f351b9a0))
* **web:** webOS build fails on an :is() it cannot pad to its weight ([17cf712](https://github.com/momotusx/momotus/commit/17cf712ecca4fbc277ec0feef804eea333d0ef38))
* **web:** webOS player focus ring follows a pill's radius ([5c352fb](https://github.com/momotusx/momotus/commit/5c352fb4461b2101f5f7405cdce41b1e0344d502))
* **web:** webOS speed pill follows the core's word on the lease ([161d98c](https://github.com/momotusx/momotus/commit/161d98c1d4b55a60e1048b45c64c40e35f0c2f6b))

## [0.2.0](https://github.com/momotusx/momotus/compare/v0.1.0...v0.2.0) (2026-11-18)


### Features

* **android:** Zap on phone and TV with the player's zap mode ([6db340b](https://github.com/momotusx/momotus/commit/6db340b21fce4756a841f1f3e30842c5c5eb489d))
* **apple:** Zap on iPhone and Mac with the player's zap mode ([b5af86d](https://github.com/momotusx/momotus/commit/b5af86d74e5a8cd3e93a37e13cc3c67e1f087e35))
* **runtime:** zap deals a random title from its middle ([06da07b](https://github.com/momotusx/momotus/commit/06da07ba5dc758fbd894b28fa482edaa1d8ddf30))
* **web:** Zap button, zap page and the player's zap mode ([47e23b3](https://github.com/momotusx/momotus/commit/47e23b35b02233263e80b4c6301d1be71e74f2c5))


### Bug Fixes

* a zap waits for its start instead of zapping on at the 20 s offer ([4ce0441](https://github.com/momotusx/momotus/commit/4ce04418eff2709eca35550be9b295b5d6c16811))
* **apple:** What is this? panel never sits under the phone's transport ([69e8e58](https://github.com/momotusx/momotus/commit/69e8e58c8a4f16db1eafc1ea2471da56c54da321))
* **runtime:** a Resume does not keep a release that did not start here ([bac8166](https://github.com/momotusx/momotus/commit/bac81664ab82d33166bd1a1f03b3c016ce86d5a7))
* **runtime:** a swarm sending too slowly to start moves on at 30 s ([476a8fe](https://github.com/momotusx/momotus/commit/476a8fe670f55bf6468d322cb609d1b23b302e56))
* **runtime:** a zap's start holds in a file shorter than its runtime ([26f8621](https://github.com/momotusx/momotus/commit/26f8621d6b5bf749012e2b575cb820ac9a889aa7))
