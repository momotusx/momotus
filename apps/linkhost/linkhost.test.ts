// `bun test apps/linkhost`: the deletion page's own steps, and the strict CSP its pages keep.
import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { newVerifier, purposeOf, readReturn, verifierHash } from "./delete/delete.js";

const here = import.meta.dir;
const read = (path: string) => readFileSync(join(here, path), "utf8");

test("the verifier is 32 random bytes, and the sync service gets its hex SHA-256 only", async () => {
  const v = newVerifier();
  expect(v).toMatch(/^[A-Za-z0-9_-]{43}$/);
  expect(newVerifier()).not.toBe(v);
  expect(await verifierHash("abc")).toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
});

test("a return counts only from this page's sync service, and names its flow", () => {
  const relay = "https://sync.example.org";
  expect(readReturn("#flow=f1&code=c1&relay=sync.example.org", relay)).toEqual({ flow: "f1", code: "c1" });
  expect(readReturn("#flow=f1&error=cancelled&relay=sync.example.org", relay)).toEqual({ flow: "f1", error: "cancelled" });
  expect(readReturn("#flow=f1&code=c1&relay=evil.example", relay)).toBeNull();
  expect(readReturn("#code=c1&relay=sync.example.org", relay)).toBeNull();
  expect(readReturn("#flow=f1&relay=sync.example.org", relay)).toBeNull();
});

test("Apple's flow revokes as it signs in; the others sign in", () => {
  expect(purposeOf("apple")).toBe("revoke");
  expect(purposeOf("google")).toBe("signin");
  expect(purposeOf("facebook")).toBe("signin");
});

test("the pages run no inline script or style, and each policy names only what it needs", () => {
  for (const page of ["pair/index.html", "delete/index.html"]) {
    const html = read(page);
    expect(html).not.toMatch(/<script(?![^>]*\ssrc=)[^>]*>/);
    expect(html).not.toMatch(/\sstyle=|<style/);
    expect(html).toMatch(/http-equiv="Content-Security-Policy"/);
    expect(html).toMatch(/default-src 'none'/);
  }
  // The pairing page has no script at all: the code after # is never read there.
  expect(read("pair/index.html")).not.toMatch(/<script/);
  expect(read("pair/index.html")).not.toMatch(/script-src/);
  // The deletion page calls the sync service and nothing else.
  expect(read("delete/index.html")).toMatch(/connect-src \{\{RELAY_URL\}\};/);
  expect(read("_headers")).toMatch(/\/delete\*\n {2}Content-Security-Policy: [^\n]*connect-src \{\{RELAY_URL\}\};[^\n]*frame-ancestors 'none'/);
});

test("the app association files are valid JSON with the owner's values left to fill", () => {
  const apple = JSON.parse(read(".well-known/apple-app-site-association"));
  expect(apple.applinks.details[0].components[0]["/"]).toBe("/pair");
  expect(apple.webcredentials.apps[0]).toBe("{{APPLE_TEAM_ID}}.{{APPLE_BUNDLE_ID}}");
  const android = JSON.parse(read(".well-known/assetlinks.json"));
  expect(android[0].target.package_name).toBe("app.marquee.android");
  expect(android[0].target.sha256_cert_fingerprints).toEqual(["{{ANDROID_SIGNING_SHA256}}"]);
});
