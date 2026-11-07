/** What a webOS TV's player decodes (plans/webos.md section 4, L6), in the core's names. The page cannot probe it
 * faithfully: the panel's size comes from the system (the graphics plane is 1080 on a 4K panel) and the engine's
 * `canPlayType` over-promises. */

import { chromeMajor, isWebOS, luna } from "../lib/webos";
import type { DeviceCaps } from "./client";

const SYSTEM_INFO = "luna://com.webos.service.tv.systemproperty/getSystemInfo";
// webOS 22 is Chrome 87: below it (webOS 6, Chrome 79) LG lists only H.264 and HEVC at 2160p.
const FOUR_K_VP9_AV1 = 87;

/** The panel is 4K: false until the system says so (or for good when it never answers). */
let uhd = false;

/** Asks the system once whether the panel is 4K; `uhd` is read by `webosCaps` from then on. */
export function probeSystem(bridgeClass?: Parameters<typeof luna>[3]) {
  // The system answers `"true"` as text; no answer leaves the panel at 1080p.
  luna(SYSTEM_INFO, { keys: ["UHD", "sdkVersion"] }, (reply) => (uhd = String(reply?.UHD) === "true"), bridgeClass);
}

/** Caps for a TV with this panel and Chrome major. Order matters where the core reads it: HEVC before VP9 and AV1. */
export function webosCaps(panelIsUhd: boolean, chrome: number, language: string): DeviceCaps {
  const decoders = ["h264", "h265", "vp9", "av1"];
  return {
    displayHeight: panelIsUhd ? 2160 : 1080,
    // HDR10 on a 4K panel; Dolby Vision stays off until a real TV has played one.
    hdr: panelIsUhd ? ["hdr10"] : [],
    decoders: panelIsUhd && chrome < FOUR_K_VP9_AV1 ? decoders.filter((c) => c !== "vp9" && c !== "av1") : decoders,
    tenBitHevc: true,
    // DTS, DTS-HD and TrueHD are off; these three are also the only audio at 2160p.
    audioDecoders: ["aac", "ac3", "eac3"],
    containers: ["mkv", "mp4", "ts", "m3u8"],
    passthrough: [],
    hasFallbackEngine: false,
    language,
  };
}

/** The TV's caps, or null off webOS. */
export function webosDeviceCaps(language: string): DeviceCaps | null {
  return isWebOS() ? webosCaps(uhd, chromeMajor(navigator.userAgent), language) : null;
}

if (isWebOS()) probeSystem();
