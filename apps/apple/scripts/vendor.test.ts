// The vendored AetherEngine's patches hold, and no input open goes unlisted (plans/apple-second-engine.md
// 2.6, 2.7; apps/apple/Vendor/AetherEngine/VENDOR.md). Run: bun test apps/apple/scripts/vendor.test.ts
import { expect, test } from "bun:test";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const vendor = join(import.meta.dir, "..", "Vendor", "AetherEngine");
const sources = join(vendor, "Sources", "AetherEngine");

function swiftFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return swiftFiles(path);
    return name.endsWith(".swift") ? [path] : [];
  });
}

/** Each `avformat_open_input(` outside comments, as "File.swift: the trimmed line". */
export function inputOpens(root: string): string[] {
  return swiftFiles(root).flatMap((path) =>
    readFileSync(path, "utf8")
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.includes("avformat_open_input(") && !line.startsWith("//"))
      .map((line) => `${relative(root, path)}: ${line}`),
  );
}

const vendorMd = () => readFileSync(join(vendor, "VENDOR.md"), "utf8");
const source = (path: string) => readFileSync(join(sources, path), "utf8");

test("every input open is listed in VENDOR.md with its whitelist", () => {
  const opens = inputOpens(sources);
  expect(opens.length).toBe(4);
  for (const open of opens) {
    const call = open.slice(open.indexOf("avformat_open_input("));
    // Each listed call names its options dictionary, so none opens without the whitelist.
    expect(vendorMd()).toContain(call.replace(/^let ret = /, "").replace(/;$/, ""));
    expect(call).toMatch(/&(opts|noProtocols)\)/);
  }
});

test("the options every Demuxer open takes carry the empty whitelist", () => {
  const demuxer = source("Demuxer/Demuxer.swift");
  const options = demuxer.slice(demuxer.indexOf("private static func applyDemuxerOptions("));
  expect(options.slice(0, options.indexOf("\n    }\n"))).toContain('av_dict_set(&opts, "protocol_whitelist", "", 0)');
  // Both Demuxer opens build their options there.
  expect(demuxer.split("Self.applyDemuxerOptions(&opts").length - 1).toBe(2);
  expect(source("Decoder/SubtitleDecoder.swift").split('av_dict_set(&noProtocols, "protocol_whitelist", "", 0)').length - 1).toBe(2);
});

test("both players refuse external playback, and the server listens on loopback", () => {
  expect(source("Native/NativeAVPlayerHost.swift")).toMatch(/let player = AVPlayer\(\)\n(.*\n){0,3}\s*player\.allowsExternalPlayback = false/);
  expect(source("Audio/AudioAVPlayerHost.swift")).toMatch(/let player = AVPlayer\(\)\n\s*player\.allowsExternalPlayback = false/);
  const players = swiftFiles(sources).flatMap((path) => readFileSync(path, "utf8").match(/= AVPlayer\(\)/g) ?? []);
  expect(players.length).toBe(2);
  const server = source("Network/HLSLocalServer.swift");
  expect(server).toContain('addr.sin_addr.s_addr = inet_addr("127.0.0.1")');
  expect(server).not.toContain('inet_addr("0.0.0.0")');
});

test("the app never hands the engine a subtitle URL to open", () => {
  // The sidecar decoder opens a URL with FFmpeg (plans/apple-second-engine.md 2.6): Marquee draws addon
  // subtitles itself and never calls it.
  const app = join(import.meta.dir, "..", "Marquee");
  const calls = swiftFiles(app).flatMap((path) =>
    (readFileSync(path, "utf8").match(/\b(addExternalSubtitleTrack|selectSidecarSubtitle|selectSecondarySidecarSubtitle)\b/g) ?? []).map(
      (call) => `${relative(app, path)}: ${call}`,
    ),
  );
  expect(calls).toEqual([]);
  // AetherSession is the one place that drives the engine's playback.
  const loads = swiftFiles(app).filter((path) => /\.load\((url|source):/.test(readFileSync(path, "utf8")));
  expect(loads.map((path) => relative(app, path))).toEqual(["Player/AetherSession.swift"]);
});
