# Changelog

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
