import { expect, test } from "bun:test";
import { hlsLease } from "./hls";

const at = (phase: "resolving" | "ready" | "failed", manifest: boolean, container: string | null) => ({ phase, manifest, container });

test("hlsLease: the core's word on the ready source, kept until the next one is ready", () => {
  // Before the first source: not known.
  expect(hlsLease(null)).toBe(undefined);
  expect(hlsLease(at("resolving", false, null))).toBe(undefined);
  // An HLS lease (a DASH one the core serves as HLS too): m3u8.
  expect(hlsLease(at("ready", true, "m3u8"))).toBe(true);
  // A DASH manifest served as itself, and a file, are not.
  expect(hlsLease(at("ready", true, "mpd"))).toBe(false);
  expect(hlsLease(at("ready", false, "mkv"))).toBe(false);
  expect(hlsLease(at("ready", false, null))).toBe(false);
  // A failover or Try again releases the lease (manifest false, container null): the last word stands.
  expect(hlsLease(at("resolving", false, null), true)).toBe(true);
  expect(hlsLease(at("failed", false, null), false)).toBe(false);
  // The next ready source decides again.
  expect(hlsLease(at("ready", false, "mp4"), true)).toBe(false);
});
