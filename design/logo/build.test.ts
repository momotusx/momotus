// The generated brand assets: every file each platform names exists at its size, the kept parts of the bird
// stay inside each mask's safe circle, and the Apple asset catalog compiles clean for iOS, macOS and tvOS.
// Run with `bun test design/logo/build.test.ts` (after `bun design/logo/build.ts`).
import { expect, test } from "bun:test";
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ANDROID_K, ART, KEEP, MASKABLE_K } from "./build";

const root = join(import.meta.dir, "../..");
const at = (...p: string[]) => join(root, ...p);
const json = (...p: string[]) => JSON.parse(readFileSync(at(...p), "utf8"));

/** A PNG's width and height, from its header. */
function size(file: string): [number, number] {
  const b = readFileSync(file);
  expect(b.subarray(1, 4).toString()).toBe("PNG");
  return [b.readUInt32BE(16), b.readUInt32BE(20)];
}

/** The kept points in tile units, the art shrunk by k about the middle. */
function kept(k: number): [number, number][] {
  const [vx, vy, vw] = readFileSync(at("design/logo/motmot.svg"), "utf8").match(/viewBox="([^"]+)"/)![1].split(/\s+/).map(Number);
  return KEEP.map(([x, y]) => [
    0.5 + (ART.x - 0.5 + ((x - vx) / vw) * ART.scale) * k,
    0.5 + (ART.y - 0.5 + ((y - vy) / vw) * ART.scale) * k,
  ]);
}
const farthest = (k: number) => Math.max(...kept(k).map(([x, y]) => Math.hypot(x - 0.5, y - 0.5)));

test("the head, eye, beak and racket stay whole in the tile, Android's 66 dp circle and the maskable circle", () => {
  for (const [x, y] of kept(1)) {
    expect(x).toBeGreaterThan(0.03);
    expect(x).toBeLessThan(0.97);
    expect(y).toBeGreaterThan(0.03);
    expect(y).toBeLessThan(0.97);
  }
  expect(farthest(ANDROID_K)).toBeLessThanOrEqual(33 / 72);
  expect(farthest(MASKABLE_K)).toBeLessThanOrEqual(0.4);
});

test("Apple: every image the catalog names exists at its size", () => {
  const xc = "apps/apple/Marquee/Resources/Brand.xcassets";
  const icon = json(xc, "AppIcon.appiconset/Contents.json");
  for (const i of icon.images) {
    const px = Number(i.size.split("x")[0]) * (i.scale ? Number(i.scale[0]) : 1);
    expect(size(at(xc, "AppIcon.appiconset", i.filename))).toEqual([px, px]);
  }
  const brand = `${xc}/App Icon & Top Shelf Image.brandassets`;
  for (const a of json(brand, "Contents.json").assets) {
    const [w, h] = a.size.split("x").map(Number);
    const sets = a.filename.endsWith(".imagestack")
      ? json(brand, a.filename, "Contents.json").layers.map((l: { filename: string }) => join(a.filename, l.filename, "Content.imageset"))
      : [a.filename];
    for (const set of sets) {
      for (const i of json(brand, set, "Contents.json").images) {
        const s = Number(i.scale[0]);
        const file = at(brand, set, i.filename);
        if (file.endsWith(".png")) expect(size(file)).toEqual([w * s, h * s]);
        else expect(existsSync(file)).toBe(true);
      }
    }
  }
  for (const set of ["Mark", "LaunchMark", "Wordmark"]) {
    for (const i of json(xc, `${set}.imageset/Contents.json`).images) expect(existsSync(at(xc, `${set}.imageset`, i.filename))).toBe(true);
  }
});

test("Apple: the catalog compiles with no warning for iOS, macOS and tvOS", { timeout: 120_000 }, () => {
  if (!Bun.which("xcrun")) return;
  const out = mkdtempSync(join(tmpdir(), "mq-brand-"));
  try {
    for (const [platform, device, icon, min] of [
      ["iphoneos", "iphone", "AppIcon", "18.0"],
      ["macosx", "mac", "AppIcon", "15.0"],
      ["appletvos", "tv", "App Icon & Top Shelf Image", "18.0"],
    ]) {
      const r = Bun.spawnSync(["xcrun", "actool", at("apps/apple/Marquee/Resources/Brand.xcassets"), "--compile", out,
        "--platform", platform, "--target-device", device, "--app-icon", icon, "--minimum-deployment-target", min,
        "--output-partial-info-plist", join(out, `${platform}.plist`), "--output-format", "human-readable-text",
        "--warnings", "--errors"]);
      const text = r.stdout.toString() + r.stderr.toString();
      expect(r.exitCode).toBe(0);
      expect(text).not.toMatch(/warning|error/i);
    }
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
});

test("Android: the adaptive icon, its legacy icons and the TV banner at every density", () => {
  const res = "apps/android/app/src/main/res";
  const manifest = readFileSync(at("apps/android/app/src/main/AndroidManifest.xml"), "utf8");
  expect(manifest).toContain('android:icon="@mipmap/ic_launcher"');
  expect(manifest).toContain('android:banner="@drawable/banner"');
  const adaptive = readFileSync(at(res, "mipmap-anydpi-v26/ic_launcher.xml"), "utf8");
  for (const ref of ["@color/ic_launcher_background", "@mipmap/ic_launcher_foreground", "@drawable/ic_launcher_monochrome"]) {
    expect(adaptive).toContain(ref);
  }
  for (const [d, m] of Object.entries({ mdpi: 1, hdpi: 1.5, xhdpi: 2, xxhdpi: 3, xxxhdpi: 4 })) {
    expect(size(at(res, `mipmap-${d}/ic_launcher_foreground.png`))).toEqual([108 * m, 108 * m]);
    expect(size(at(res, `mipmap-${d}/ic_launcher.png`))).toEqual([48 * m, 48 * m]);
    expect(size(at(res, `drawable-${d}/banner.png`))).toEqual([160 * m, 90 * m]);
  }
  // No placeholder left beside the generated images.
  expect(readdirSync(at(res, "drawable"))).not.toContain("banner.xml");
  expect(readdirSync(at(res, "drawable"))).not.toContain("ic_launcher.xml");
});

test("web: the manifest's icons and the page's links exist at their sizes", () => {
  const pub = "apps/web/public";
  const manifest = json(pub, "manifest.webmanifest");
  expect(manifest.icons.filter((i: { purpose: string }) => i.purpose === "maskable")).toHaveLength(1);
  for (const i of manifest.icons) {
    const [w, h] = i.sizes.split("x").map(Number);
    expect(size(at(pub, i.src))).toEqual([w, h]);
  }
  const page = readFileSync(at("apps/web/index.html"), "utf8");
  for (const href of [...page.matchAll(/(?:href|src)="(\/[^"]+)"/g)].map((m) => m[1]).filter((h) => !h.startsWith("/src/"))) {
    expect(existsSync(at(pub, href))).toBe(true);
  }
  expect(size(at(pub, "apple-touch-icon.png"))).toEqual([180, 180]);
});
