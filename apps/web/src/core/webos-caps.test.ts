import { expect, test } from "bun:test";
import { chromeMajor, isWebOSAgent, mediaKey } from "../lib/webos";
import { probeSystem, webosCaps, webosDeviceCaps } from "./webos-caps";

const UA6 = "Mozilla/5.0 (Web0S; Linux/SmartTV) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/79.0.3945.79 Safari/537.36 WebAppManager";
const UA22 = "Mozilla/5.0 (Web0S; Linux/SmartTV) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/87.0.4280.88 Safari/537.36 WebAppManager";

test("webOS is the Web0S agent plus a Chrome major", () => {
  expect(isWebOSAgent(UA6)).toBe(true);
  expect(isWebOSAgent("Mozilla/5.0 (Macintosh) Chrome/120.0.0.0 Safari/537.36")).toBe(false);
  expect(chromeMajor(UA6)).toBe(79);
  expect(chromeMajor(UA22)).toBe(87);
  expect(chromeMajor("Web0S without a browser name")).toBe(0);
});

test("a 1080p panel: no HDR, every video codec", () => {
  const caps = webosCaps(false, 79, "en");
  expect(caps.displayHeight).toBe(1080);
  expect(caps.hdr).toEqual([]);
  expect(caps.decoders).toEqual(["h264", "h265", "vp9", "av1"]);
});

test("a 4K panel is HDR10 and, on Chrome 79, loses VP9 and AV1", () => {
  const old = webosCaps(true, 79, "en");
  expect(old.displayHeight).toBe(2160);
  expect(old.hdr).toEqual(["hdr10"]);
  expect(old.decoders).toEqual(["h264", "h265"]);
  expect(webosCaps(true, 87, "en").decoders).toEqual(["h264", "h265", "vp9", "av1"]);
});

test("containers and audio are the TV's, with DTS and TrueHD off", () => {
  const caps = webosCaps(true, 87, "tr");
  expect(caps.containers).toEqual(["mkv", "mp4", "ts", "m3u8"]);
  expect(caps.audioDecoders).toEqual(["aac", "ac3", "eac3"]);
  expect(caps.passthrough).toEqual([]);
  expect(caps.language).toBe("tr");
});

test("the system's answer sets the panel, asked for UHD and sdkVersion", () => {
  const nav = Object.getOwnPropertyDescriptor(globalThis, "navigator");
  Object.defineProperty(globalThis, "navigator", { value: { userAgent: UA6 }, configurable: true });
  const calls: string[][] = [];
  class Bridge {
    onservicecallback: ((m: string) => void) | null = null;
    call(uri: string, payload: string) {
      expect(uri).toBe("luna://com.webos.service.tv.systemproperty/getSystemInfo");
      calls.push((JSON.parse(payload) as { keys: string[] }).keys);
      this.onservicecallback?.(JSON.stringify({ returnValue: true, UHD: "true", sdkVersion: "6.0.0" }));
    }
  }
  expect(webosDeviceCaps("en")?.displayHeight).toBe(1080);
  probeSystem(Bridge);
  expect(calls).toEqual([["UHD", "sdkVersion"]]);
  expect(webosDeviceCaps("en")?.displayHeight).toBe(2160);
  expect(webosDeviceCaps("en")?.decoders).toEqual(["h264", "h265"]);
  Object.defineProperty(globalThis, "navigator", { value: { userAgent: "Mozilla/5.0 Chrome/120" }, configurable: true });
  expect(webosDeviceCaps("en")).toBeNull();
  if (nav) Object.defineProperty(globalThis, "navigator", nav);
});

test("the LG remote's media keys are named on webOS only", () => {
  const key = (keyCode: number, key = "Unidentified") => ({ keyCode, key }) as KeyboardEvent;
  expect(mediaKey(key(415), true)).toBe("MediaPlay");
  expect(mediaKey(key(19), true)).toBe("MediaPause");
  expect(mediaKey(key(413), true)).toBe("MediaStop");
  expect(mediaKey(key(417), true)).toBe("MediaFastForward");
  expect(mediaKey(key(412), true)).toBe("MediaRewind");
  expect(mediaKey(key(19, "Pause"), false)).toBe("Pause");
  expect(mediaKey(key(13, "Enter"), true)).toBe("Enter");
});
