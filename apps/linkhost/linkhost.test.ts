// `bun test apps/linkhost`: the deletion page's own steps, and the strict CSP its pages keep.
import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import shared from "../shared/account-words.json";
import { codeDigits, mailWords, newVerifier, purposeOf, readReturn, verifierHash, WORDS } from "./delete/delete.js";

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

test("Apple's flow revokes as it signs in, and the page says so before its button; the others sign in", () => {
  expect(purposeOf("apple")).toBe("revoke");
  expect(purposeOf("google")).toBe("signin");
  expect(purposeOf("facebook")).toBe("signin");
  const html = read("delete/index.html");
  expect(html.indexOf('id="apple-note"')).toBeGreaterThan(0);
  expect(html.indexOf('id="apple-note"')).toBeLessThan(html.indexOf('data-provider="apple"'));
});

test("the pages run no inline script or style, and each policy names only what it needs", () => {
  for (const page of ["pair/index.html", "delete/index.html"]) {
    const html = read(page);
    expect(html).not.toMatch(/<script(?![^>]*\ssrc=)[^>]*>/);
    expect(html).not.toMatch(/\sstyle=|<style/);
    expect(html).toMatch(/http-equiv="Content-Security-Policy"/);
    expect(html).toMatch(/default-src 'none'/);
  }
  // The pairing page runs one script of its own, which only hands the code to the app (plans/tv-qr-login.md 5.7).
  const pair = read("pair/index.html");
  expect(pair.match(/<script/g)).toHaveLength(1);
  expect(pair).toMatch(/<script src="\/pair\/pair.js" defer><\/script>/);
  expect(pair).toMatch(/script-src 'self';/);
  expect(read("_headers")).toMatch(/\/pair\*\n {2}Content-Security-Policy: default-src 'none'; script-src 'self';/);
  // It makes no request and writes no markup.
  expect(read("pair/pair.js")).not.toMatch(/fetch|XMLHttpRequest|sendBeacon|Image|WebSocket|innerHTML|outerHTML|insertAdjacent|document.write/);
  // The deletion page calls the sync service and nothing else.
  expect(read("delete/index.html")).toMatch(/connect-src \{\{RELAY_URL\}\};/);
  expect(read("_headers")).toMatch(/\/delete\*\n {2}Content-Security-Policy: [^\n]*connect-src \{\{RELAY_URL\}\};[^\n]*frame-ancestors 'none'/);
});

test("the app association files are valid JSON with the owner's values left to fill", () => {
  const apple = JSON.parse(read(".well-known/apple-app-site-association"));
  expect(apple.applinks.details[0].components[0]["/"]).toBe("/pair");
  expect(apple.webcredentials.apps[0]).toBe("{{APPLE_TEAM_ID}}.{{APPLE_BUNDLE_ID}}");
  const android = JSON.parse(read(".well-known/assetlinks.json"));
  expect(android[0].target.package_name).toBe("app.momotus.android");
  expect(android[0].target.sha256_cert_fingerprints).toEqual(["{{ANDROID_SIGNING_SHA256}}"]);
});

// The build (build.ts): what the Worker serves, with the owner's values in and nothing left to fill.
import { cpSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { build } from "./build";

const ENV = {
  RELAY_URL: "https://sync.example.org",
  PROVIDERS: "google,facebook",
  ANDROID_PACKAGES: "app.momotus.android,app.momotus.android.walk",
  ANDROID_SIGNING_SHA256: Array(32).fill("AB").join(":"),
};
const built = (env: Record<string, string | undefined>) => {
  const out = mkdtempSync(join(tmpdir(), "linkhost-"));
  return { files: build(env, out), read: (path: string) => readFileSync(join(out, path), "utf8") };
};

test("the build fills every placeholder and lists every file, .well-known included", () => {
  const { files, read } = built(ENV);
  expect(files).toEqual([
    ".well-known/assetlinks.json",
    "_headers",
    "delete/delete.js",
    "delete/index.html",
    "linkhost.css",
    "pair/index.html",
    "pair/pair.js",
    "privacy/index.html",
    "terms/index.html",
  ]);
  for (const file of files) expect(read(file)).not.toContain("{{");
  expect(read("delete/delete.js")).toContain('export const RELAY = "https://sync.example.org";');
  expect(read("_headers")).toContain("connect-src https://sync.example.org;");
});

test("an unset store address drops its button, and a set one fills it", () => {
  expect(built(ENV).read("pair/index.html")).not.toMatch(/App Store|Google Play/);
  const play = built({ ...ENV, PLAY_STORE_URL: "https://play.example/marquee" }).read("pair/index.html");
  expect(play).toContain('href="https://play.example/marquee"');
  expect(play).not.toContain("App Store");
});

test("the deletion page offers only the sync service's providers, and Apple's line only with Apple", () => {
  const html = built(ENV).read("delete/index.html");
  expect(html).toContain('data-provider="google"');
  expect(html).toContain('data-provider="facebook"');
  expect(html).not.toContain('data-provider="apple"');
  expect(html).not.toContain("apple-note");
  expect(built({ ...ENV, PROVIDERS: "apple,google" }).read("delete/index.html")).toContain('id="apple-note"');
  expect(() => built({ ...ENV, PROVIDERS: "google,myspace" })).toThrow("unknown providers");
});

test("assetlinks names each package with the certificate, and the Apple file comes only with Apple's values", () => {
  const { files, read } = built(ENV);
  const links = JSON.parse(read(".well-known/assetlinks.json"));
  expect(links.map((s: { target: { package_name: string } }) => s.target.package_name)).toEqual(["app.momotus.android", "app.momotus.android.walk"]);
  for (const statement of links) {
    expect(statement.relation).toContain("delegate_permission/common.get_login_creds");
    expect(statement.target.sha256_cert_fingerprints).toEqual([ENV.ANDROID_SIGNING_SHA256]);
  }
  expect(files).not.toContain(".well-known/apple-app-site-association");
  expect(read("_headers")).not.toContain("apple-app-site-association");
  const apple = built({ ...ENV, APPLE_TEAM_ID: "TEAM123456", APPLE_BUNDLE_ID: "app.momotus.dev" });
  expect(JSON.parse(apple.read(".well-known/apple-app-site-association")).webcredentials.apps).toEqual(["TEAM123456.app.momotus.dev"]);
  expect(apple.read("_headers")).toContain("/.well-known/apple-app-site-association");
  expect(() => built({ ...ENV, APPLE_TEAM_ID: "TEAM123456" })).toThrow("together");
});

test("the build refuses a missing or malformed value instead of shipping a placeholder", () => {
  for (const name of ["RELAY_URL", "PROVIDERS", "ANDROID_PACKAGES", "ANDROID_SIGNING_SHA256"]) {
    expect(() => built({ ...ENV, [name]: undefined })).toThrow(`${name} is not set`);
  }
  expect(() => built({ ...ENV, RELAY_URL: "http://sync.example.org" })).toThrow("https://host");
  expect(() => built({ ...ENV, RELAY_URL: "https://sync.example.org/" })).toThrow("https://host");
  expect(() => built({ ...ENV, ANDROID_SIGNING_SHA256: "ab:cd" })).toThrow("32 bytes");
});

test("the privacy page keeps the strict policy and says what the service keeps and for how long", () => {
  const html = read("privacy/index.html");
  expect(html).not.toMatch(/<script|\sstyle=|<style/);
  expect(read("_headers")).toMatch(/\/privacy\*\n {2}Content-Security-Policy: default-src 'none'; style-src 'self';[^\n]*frame-ancestors 'none'/);
  expect(html).toContain("cannot read it");
  expect(html).toContain("kept 30 days");
  expect(html).toContain("up to 14 days more");
  expect(html).toContain("hey@normalisdead.mom");
  expect(read("delete/index.html")).toContain("up to 14 days more");
});

test("the terms page keeps the strict policy, and the privacy and deletion pages link it", () => {
  const html = read("terms/index.html");
  expect(html).not.toMatch(/<script|\sstyle=|<style/);
  expect(html).toMatch(/default-src 'none'/);
  expect(read("_headers")).toMatch(/\/terms\*\n {2}Content-Security-Policy: default-src 'none'; style-src 'self';[^\n]*frame-ancestors 'none'/);
  expect(html).toContain("as is");
  expect(html).toContain('href="/delete"');
  expect(html).toContain("hey@normalisdead.mom");
  expect(read("privacy/index.html")).toContain('href="/terms"');
  expect(read("delete/index.html")).toContain('href="/terms"');
});

test("a placeholder no value fills stops the build", () => {
  const from = mkdtempSync(join(tmpdir(), "linkhost-src-"));
  cpSync(here, from, { recursive: true, filter: (path) => !path.includes("/dist") });
  writeFileSync(join(from, "privacy/index.html"), read("privacy/index.html").replace("</main>", "{{OWNER_NAME}}</main>"));
  expect(() => build(ENV, mkdtempSync(join(tmpdir(), "linkhost-")), from)).toThrow("placeholders left in privacy/index.html");
});

test("the deletion page names where the request lives in the app: Settings, Account, never Home", () => {
  const html = read("delete/index.html");
  expect(html).toContain("in Settings, Account, with Cancel");
  expect(html).not.toMatch(/\bHome\b/);
});

test("Continue with email comes with email in PROVIDERS, takes its code on the page, and keeps the verifier in memory", () => {
  const html = built({ ...ENV, PROVIDERS: "google,email" }).read("delete/index.html");
  expect(html).toContain('data-provider="email"');
  expect(built(ENV).read("delete/index.html")).not.toContain('data-provider="email"');
  // The address field and the code field, the forms never checked by the browser before the page's words.
  expect(html).toMatch(/<form id="email-form" novalidate hidden>/);
  expect(html).toMatch(/<input id="email"[^>]*type="email"[^>]*autocomplete="email"/);
  expect(html).toMatch(/<input id="code"[^>]*inputmode="numeric"[^>]*autocomplete="one-time-code"/);
  expect(html).not.toMatch(/<input id="code"[^>]*maxlength/);
  // E14: the email flow never reaches sessionStorage; only the brokered flow's verifier does.
  const script = read("delete/delete.js");
  expect(script.match(/sessionStorage\.setItem/g)?.length).toBe(1);
  expect(script).toContain("let mail = null;");
  // The CSP is the one every deletion page has.
  expect(html).toMatch(/connect-src https:\/\/sync\.example\.org;/);
});

test("the email step's answers read as the apps word them, and a pasted code keeps its digits", () => {
  expect(mailWords("badAddress")).toBe("Enter an email address like name@example.com.");
  expect(mailWords("wrongCode")).toBe(mailWords("badCode"));
  expect(mailWords("ticketExpired")).toBe("This code can't be used any more. Send a new code.");
  expect(mailWords("emailWait")).toBe("Wait a minute, then send a new code.");
  expect(mailWords("mailUnavailable")).toContain("couldn't be sent");
  expect(mailWords("wrongFlow")).toBe(mailWords("ticketExpired"));
  expect(mailWords("signInRefused")).toBe(mailWords("ticketExpired"));
  expect(mailWords("")).toBe("The sign-in did not work. Try again in a moment.");
  expect(codeDigits("123 456")).toBe("123456");
  expect(codeDigits("123-4567")).toBe("123456");
});

test("the privacy page says what email sign-in keeps and who carries the mail", () => {
  const html = read("privacy/index.html");
  expect(html).toContain("a keyed hash of the address, never the address itself");
  expect(html).toContain("Amazon SES (Amazon Web Services, US East)");
  expect(html).toContain("kept for 24 hours");
});

test("the deletion page's email words are the apps' own, from apps/shared/account-words.json", () => {
  for (const key of ["badAddress", "wrongCode", "codeExpired", "emailWait", "emailLimit", "mailUnavailable"] as const) expect(WORDS[key], key).toBe(shared[key]);
  expect(WORDS.sent).toBe(shared.codeSentTo);
  expect(WORDS.busy).toBe(shared.busy);
  const html = read("delete/index.html");
  for (const key of ["codeMissing", "continueWithEmail", "emailLabel", "emailSend", "codeLabel"] as const) expect(html, key).toContain(shared[key]);
});

/** pair.js run against a stub page: the app link only for a well-formed code, the fragment as an href. */
function runPair(hash: string) {
  const el = (hidden: boolean) => ({ hidden, href: "", setAttribute(_: string, v: string) { this.href = v; } });
  const page: Record<string, ReturnType<typeof el>> = { open: el(true), "open-note": el(true), "no-code": el(false), "open-app": el(false) };
  const run = new Function("location", "document", "URLSearchParams", read("pair/pair.js"));
  run({ hash }, { getElementById: (id: string) => page[id] }, URLSearchParams);
  return page;
}

test("the pairing page opens the app only for a pairing code", () => {
  const code = "#v=3&s=https%3A%2F%2Fsync.example&p=slot&k=box&g=sign&n=Den&f=tv";
  const page = runPair(code);
  expect(page["open-app"].href).toBe(`momotus://pair${code}`);
  expect([page.open.hidden, page["open-note"].hidden, page["no-code"].hidden]).toEqual([false, false, true]);
  for (const odd of ["", "#v=2&s=a&p=b&k=c&g=d", "#v=3&s=a&p=b&k=c", "#x#v=3&s=a&p=b&k=c&g=d"]) {
    const kept = runPair(odd);
    expect([kept.open.hidden, kept["open-app"].href, kept["no-code"].hidden], odd).toEqual([true, "", false]);
  }
});
