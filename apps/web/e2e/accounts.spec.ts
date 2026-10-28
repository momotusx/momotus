/** Accounts on the web for real (plans/accounts.md phase 2): a local relay with sign-in, the mock
 * OpenID provider (crates/mq-sync-server/examples/mock-oidc.rs, also behind the relay's Apple
 * entry), and a core per device, each page reaching its own through a routed WebSocket. Every
 * host is on this machine, so the spec runs with the network off.
 *
 * Desk signs in with the mock (a cancel, a window closed without a return, then alice): `new`,
 * Create, the recovery kit step, the doors. Its Account lists the sign-in, links Apple, unlinks it
 * after Apple's revoke, and once more without one ("Unlink anyway"). Phone signs in as bob and
 * creates an account without a kit: Home nudges for a second way in; it signs out and removes its
 * library (D13's three steps), then signs in as alice: `found`, the pairing code, Desk approves.
 * The TV's first run is the code for a phone; Desk approves it; its Home shows the Phone's join
 * until OK. A deletion asked from the link host's flow shows on Home with Cancel, and with Cancel
 * and turn off; Desk deletes the account after Apple's revoke, and the TV removes its library from
 * the account-deleted card. No ticket or receipt reaches storage, history or the URL.
 *
 * Email (plans/accounts-email.md): the relay mails through the mock's SES, and the spec reads each
 * code from the mock. Mail signs in by email (both stages, Back, a wrong code, `new`, Create);
 * Desk links an address from Account; a second deletion is asked on the link host's real page by
 * email; Late's Send a new code turns on after its minute; Mail signs out and finds its account by
 * email. The relay runs with test-only limits (`--clients-per-address`, `--mail-gap-ms`): every core here
 * reaches it from 127.0.0.1, and the real minute between two codes to one mailbox would make the
 * spec wait; Late's 60 s resend timer runs on Playwright's clock. No address reaches storage,
 * history or the URL. */

import { type Browser, expect, type Page, test } from "@playwright/test";
import type { ChildProcess } from "node:child_process";
import { createHash, generateKeyPairSync, randomBytes } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { ACCOUNTS_PORTS, Client, PORTS, SHOTS, startBridge, startMockProvider, startRelay } from "./core";

const P = ACCOUNTS_PORTS;
const SERVER = `http://127.0.0.1:${P.relay}`;
const LINK_HOST = "link.example";
const procs: { proc: ChildProcess; data?: string }[] = [];
let scratch = "";
let desk: Page;
let phone: Page;
let tv: Page;
/** Email sign-in's own devices: Mail signs in by email; Late waits out Send a new code's minute. */
let mail: Page;
let late: Page;
let mockUrl = "";
const SAM = "sam@example.org";
const LEE = "lee@example.org";
const PAT = "pat@example.org";
const ANN = "ann@example.org";
/** The relay's gap between two codes to one mailbox in this spec (60 s in production). */
const MAIL_GAP_MS = 5000;
const LINKHOST = fileURLToPath(new URL("../../linkhost/", import.meta.url));
const errors: string[] = [];
/** The last pairing code each core's page showed, by the core's port. */
const codes = new Map<number, string>();
/** Every request a page sent its core: op and which fields it carried. */
const asked: { port: number; op?: string; view?: string; fields: string[]; form?: string }[] = [];
/** Tickets and receipts that crossed a socket. */
const secrets = new Set<string>();

function collect(port: number, text: string) {
  const walk = (v: unknown) => {
    if (!v || typeof v !== "object") return;
    const o = v as Record<string, unknown>;
    if (o.phase === "waiting" && typeof o.qr === "string") codes.set(port, o.qr);
    for (const k of ["ticket", "receipt"]) if (typeof o[k] === "string") secrets.add(o[k] as string);
    for (const x of Object.values(o)) walk(x);
  };
  walk(JSON.parse(text));
}

/** Answers kept from a page, by `<port>:<op>`: the core's answer to the next such request waits
 * for `release`, or a failure with no code (the sync service did not answer) stands in for it. */
const holds = new Map<string, { gate?: Promise<void>; fail?: boolean }>();
/** The held answers' gates, by `<port>:<request id>`. */
const gates = new Map<string, Promise<void>>();
/** Holds the core's answer to `port`'s next `op` until the returned release is called. */
function hold(port: number, op: string): () => void {
  let release = () => undefined as void;
  const gate = new Promise<void>((ok) => (release = ok));
  holds.set(`${port}:${op}`, { gate });
  return release;
}
/** Answers `port`'s next `op` with no code, as when the sync service does not answer; the core never sees it. */
const failNext = (port: number, op: string) => holds.set(`${port}:${op}`, { fail: true });

const FORMS = {
  desktop: { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
  phone: { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true },
  tv: { viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 },
};

/** A browser context whose page talks to the core on `port`, with this relay as the one it knows. */
async function device(browser: Browser, port: number, form: keyof typeof FORMS, approver = false, clock = false): Promise<Page> {
  const context = await browser.newContext({ ...FORMS[form], colorScheme: "dark", baseURL: `http://127.0.0.1:${PORTS.vite}` });
  // A clock the test can move; it runs at real speed until then.
  if (clock) await context.clock.install();
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await context.addInitScript((server) => localStorage.getItem("mq.syncServer") || localStorage.setItem("mq.syncServer", server), SERVER);
  if (approver) await context.addInitScript(() => localStorage.setItem("mq.devDeviceForm", "phone"));
  await context.routeWebSocket(/\/ws$/, (ws) => {
    const core = new WebSocket(`ws://127.0.0.1:${port}/ws`);
    const early: string[] = [];
    core.addEventListener("open", () => {
      for (const m of early.splice(0)) core.send(m);
    });
    core.addEventListener("message", (e) => {
      const text = String(e.data);
      collect(port, text);
      const id = (JSON.parse(text) as { id?: number }).id;
      const gate = gates.get(`${port}:${id}`);
      gates.delete(`${port}:${id}`);
      if (gate) void gate.then(() => ws.send(text));
      else ws.send(text);
    });
    core.addEventListener("close", () => ws.close());
    ws.onMessage((m) => {
      collect(port, String(m));
      const request = JSON.parse(String(m)) as Record<string, unknown>;
      const params = request.params as Record<string, unknown> | undefined;
      asked.push({ port, op: request.op as string | undefined, view: request.view as string | undefined, fields: Object.keys(request), form: (request.form ?? params?.form) as string | undefined });
      const kept = holds.get(`${port}:${request.op}`);
      holds.delete(`${port}:${request.op}`);
      if (kept?.fail) return ws.send(JSON.stringify({ id: request.id, ok: false, error: "the sync service did not answer" }));
      if (kept?.gate) gates.set(`${port}:${request.id}`, kept.gate);
      if (core.readyState === WebSocket.OPEN) core.send(String(m));
      else early.push(String(m));
    });
    ws.onClose(() => core.close());
  });
  const page = await context.newPage();
  page.on("pageerror", (e) => errors.push(`${form}: ${e.message}`));
  await page.goto(`/settings?form=${form}`);
  return page;
}

/** In-app navigation: a reload would drop the tab's sync record, as it does for a user. */
const go = async (page: Page, path: string) => {
  await page.evaluate((p) => {
    history.pushState(null, "", p);
    dispatchEvent(new PopStateEvent("popstate"));
  }, path);
};
const shot = async (page: Page, name: string) => {
  await page.waitForTimeout(400);
  await page.screenshot({ path: `${SHOTS}/${name}.jpg`, type: "jpeg", quality: 80 });
};
const press = async (page: Page, key: string) => {
  await page.locator(`[data-f="${key}"]`).focus();
  await page.keyboard.press("Enter");
};

/** Presses a sign-in button and answers the provider page that opens as `who` (or Cancel); the
 * page closes itself once it handed the return over. */
async function providerPage(page: Page, button: string, who: "alice" | "bob" | "Cancel") {
  const [popup] = await Promise.all([page.waitForEvent("popup"), page.locator(`[data-f="${button}"]`).click()]);
  await popup.getByText(who === "Cancel" ? "Cancel" : `Sign in as ${who}`).click();
  await popup.waitForEvent("close");
}

/** Desk approves the code another device shows (the development builds' paste page, the e2e
 * peer's path; the web itself approves nothing), as a scan behind its passkey: every core here is on
 * loopback, which is never near (plans/tv-qr-login.md Q2). */
async function approve(code: string) {
  await go(desk, "/sync/add");
  await desk.locator('[data-f="sync-add:code"]').fill(code);
  await desk.locator('[data-f="sync-add:approve"]').click();
  await expect(desk.locator(".confirm .step")).toHaveText("Signing in");
}

/** Presses Sync now on Account, so the next round's report (announcements, deletion) is in. */
async function syncNow(page: Page) {
  await go(page, "/sync");
  const card = page.locator(".sync-card");
  await expect(card).toBeVisible();
  const before = Number(await card.getAttribute("data-round"));
  await page.locator('[data-f="sync:now"]').click();
  await expect.poll(async () => Number((await card.getAttribute("data-round")) ?? "Infinity"), { timeout: 30_000 }).toBeGreaterThan(before);
}

/** The link host's deletion page, as its script runs it: sign in as `who` with the brokered flow
 * returning to the page, take the ticket with the verifier, ask for the deletion. */
async function requestDeletion(provider: string, who: string): Promise<number> {
  const post = async (path: string, body: unknown) => {
    const r = await fetch(`${SERVER}${path}`, { method: "POST", headers: { "content-type": "application/json", origin: `https://${LINK_HOST}` }, body: JSON.stringify(body) });
    expect(r.headers.get("access-control-allow-origin")).toBe(`https://${LINK_HOST}`);
    expect(r.ok, `${path}: ${r.status}`).toBe(true);
    return (await r.json()) as Record<string, unknown>;
  };
  const verifier = randomBytes(32).toString("base64url");
  const started = await post(`/v1/auth/${provider}/start`, {
    verifierHash: createHash("sha256").update(verifier).digest("hex"),
    purpose: "signin",
    return: `https://${LINK_HOST}/delete`,
  });
  const picked = await fetch(`${started.url}&choose=${who}`, { redirect: "manual" });
  const back = await fetch(picked.headers.get("location") ?? "", { redirect: "manual" });
  const fragment = new URLSearchParams(new URL(back.headers.get("location") ?? "").hash.slice(1));
  expect(fragment.get("relay")).toBe("127.0.0.1");
  const taken = await post("/v1/auth/take", { flow: fragment.get("flow"), verifier, code: fragment.get("code") });
  return (await post("/v1/account/delete-request", { ticket: taken.ticket })).dueMs as number;
}

/** Everything a page keeps where other code could read it: the URL, history.state and both web storages. */
const kept = (page: Page) =>
  page.evaluate(() => {
    const out = [location.href, JSON.stringify(history.state)];
    for (const s of [localStorage, sessionStorage]) for (let i = 0; i < s.length; i++) out.push(String(s.key(i)), String(s.getItem(String(s.key(i)))));
    return out.join("\n");
  });

/** The last mail text read per address, and when each address was last mailed. */
const texts = new Map<string, string>();
const mailed = new Map<string, number>();
/** The newest mail to `to` from the mock's SES, once it differs from the one read before. */
async function mailText(to: string): Promise<string> {
  let text = "";
  await expect
    .poll(
      async () => {
        const r = await fetch(`${mockUrl}/mail?to=${encodeURIComponent(to)}`);
        text = r.ok ? ((await r.json()) as { text: string }).text : "";
        return text !== "" && text !== texts.get(to);
      },
      { timeout: 15_000 },
    )
    .toBe(true);
  texts.set(to, text);
  return text;
}
const codeIn = (text: string) => /^\d{6}$/m.exec(text)?.[0] ?? "";
/** The relay mails an address once per gap: wait out what is left of it. */
async function mailable(to: string) {
  const at = mailed.get(to);
  if (at) await new Promise((ok) => setTimeout(ok, Math.max(0, at + MAIL_GAP_MS + 500 - Date.now())));
}
const emailStarts = (port: number) => asked.filter((a) => a.port === port && a.op === "emailSignInStart").length;
const codeTries = (port: number) => asked.filter((a) => a.port === port && a.op === "emailSignIn").length;

/** Light and dark shots of one stage at the page's width. */
async function shots(page: Page, name: string) {
  for (const scheme of ["dark", "light"] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    await shot(page, `${name}-${scheme}`);
  }
  await page.emulateMedia({ colorScheme: "dark" });
}

/** The link host's deletion page as it ships (apps/linkhost/delete, the sync service filled in),
 * served at its own origin, asked by email (E14): the code is read from the mock. */
async function deletionByEmail(browser: Browser, address: string) {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 } });
  await context.grantPermissions(["local-network-access"], { origin: `https://${LINK_HOST}` });
  await context.route(`https://${LINK_HOST}/**`, (route) => {
    const path = new URL(route.request().url()).pathname;
    const file = path.replace(/^\/delete\/?$/, "delete/index.html").replace(/^\//, "");
    const type = file.endsWith(".js") ? "text/javascript" : file.endsWith(".css") ? "text/css" : "text/html";
    return route.fulfill({ contentType: type, body: readFileSync(join(LINKHOST, file), "utf8").replaceAll("{{RELAY_URL}}", SERVER) });
  });
  const page = await context.newPage();
  page.on("pageerror", (e) => errors.push(`link host: ${e.message}`));
  await page.goto(`https://${LINK_HOST}/delete`);
  await page.locator('[data-provider="email"]').click();
  await expect(page.locator("#email")).toBeFocused();
  await page.locator("#email").fill(address);
  await mailable(address);
  await page.locator("#send").click();
  await expect(page.locator("#sent")).toHaveText(`We sent a 6-digit code to ${address}. It works for 10 minutes.`);
  mailed.set(address, Date.now());
  await expect(page.locator("#code")).toBeFocused();
  // A wrong code keeps the flow; the right one asks before the deletion.
  const code = codeIn(await mailText(address));
  await page.locator("#code").fill(code.replace(/^./, (d) => String((Number(d) + 1) % 10)));
  await page.locator("#take").click();
  await expect(page.locator("#said")).toHaveText("That code isn't right. Check the email and try again.");
  await page.locator("#code").fill(`${code.slice(0, 3)} ${code.slice(3)}`);
  await page.locator("#take").click();
  await expect(page.locator("#said")).toHaveText(/^Delete your Momotus account\?/);
  await page.locator("#confirm").click();
  await expect(page.locator("#said")).toHaveText(/^Your Momotus account will be deleted on /);
  // The verifier lived in the page's memory only.
  expect(await page.evaluate(() => sessionStorage.length)).toBe(0);
  await context.close();
}

test.describe.serial("accounts on the web", () => {
  test.beforeAll(async ({ browser }) => {
    scratch = mkdtempSync(join(tmpdir(), "mq-accounts-e2e-"));
    const mock = await startMockProvider(P.mock);
    procs.push(mock);
    mockUrl = mock.url;
    const apple = generateKeyPairSync("ec", { namedCurve: "P-256" }).privateKey.export({ type: "pkcs8", format: "pem" }).toString();
    const identity = join(scratch, "identity.json");
    writeFileSync(
      identity,
      JSON.stringify({
        key: randomBytes(32).toString("hex"),
        publicUrl: SERVER,
        linkHost: LINK_HOST,
        apple: { teamId: "TEAM", keyId: "KEY1", key: apple, servicesId: "apple-web", bundleIds: ["app.ios"], issuer: mock.url },
        oidc: [{ id: "mock", issuer: mock.url, clientId: "marquee", clientSecret: "ms" }],
        email: { accessKeyId: "AKIDMOCK", secretAccessKey: "mock/secret/key", region: "us-east-1", from: "noreply@example.org", endpoint: mock.url },
      }),
    );
    procs.push(await startRelay(P.relay, undefined, ["--identity", identity, "--public-host", "127.0.0.1", "--clients-per-address", "50", "--mail-gap-ms", String(MAIL_GAP_MS)]));
    for (const port of [P.a, P.b, P.c, P.d, P.e]) {
      procs.push(await startBridge(port));
      // Something to show on Home: the local sample addon (a device with none starts at the welcome steps).
      const c = new Client(`ws://127.0.0.1:${port}/ws`);
      await c.req("installAddon", { url: `http://127.0.0.1:${PORTS.source}/manifest.json` });
      c.close();
    }
    // Desk approves the others: as the e2e's peer it states a phone (only phones and tablets approve, D6).
    desk = await device(browser, P.a, "desktop", true);
    phone = await device(browser, P.b, "phone");
    tv = await device(browser, P.c, "tv");
    mail = await device(browser, P.d, "desktop");
    late = await device(browser, P.e, "phone", false, true);
  });

  test.afterAll(async () => {
    for (const p of [desk, phone, tv, mail, late]) await p?.context().close();
    for (const { proc, data } of procs) {
      proc.kill();
      if (data) rmSync(data, { recursive: true, force: true });
    }
    if (scratch) rmSync(scratch, { recursive: true, force: true });
  });

  test("email: Mail signs in by email (Back, a wrong code, new, Create); Late asks a code", async () => {
    // Its own row under the providers, never a provider button.
    await go(mail, "/welcome");
    await expect(mail.locator(".sign-in-providers .btn")).toHaveText(["Continue with Apple", "Continue with mock"]);
    const row = mail.locator('[data-f="signin:email"]');
    await expect(row).toHaveText("Continue with email");
    await expect(row.locator("svg")).toBeVisible();
    await row.click();
    await expect(mail).toHaveURL(/\/sign-in\/email\?from=welcome$/);
    await expect(mail.locator("h1")).toHaveText("Sign in with email");
    await expect(mail.locator("form.email-form")).toHaveAttribute("novalidate", "");
    const address = mail.locator('[data-f="signin:email:address"]');
    await expect(address).toHaveAttribute("type", "email");
    await expect(address).toHaveAttribute("autocomplete", "email");
    await shots(mail, "email-address-1440");

    // A malformed address: the core's line under the field, nothing mailed.
    const send = mail.locator('[data-f="signin:email:send"]');
    await address.fill("sam@");
    await send.click();
    const line = mail.locator("#signin-email-error");
    await expect(line).toHaveText("Enter an email address like name@example.com.");
    await expect(address).toHaveAttribute("aria-invalid", "true");
    await expect(address).toHaveAttribute("aria-describedby", "signin-email-error");
    await expect(address).toBeFocused();

    await address.fill(SAM);
    await send.click();
    mailed.set(SAM, Date.now());
    await expect(mail).toHaveURL(/\/sign-in\/email\?stage=code&from=welcome$/);
    await expect(mail.locator("h1")).toHaveText("Enter the code");
    await expect(mail.locator(".lead")).toHaveText(`We sent a 6-digit code to ${SAM}. It works for 10 minutes.`);
    await expect(mail.getByText("It can take a minute. Look for a mail from Momotus, and check your spam folder.")).toBeVisible();
    const field = mail.locator('[data-f="signin:email:code"]');
    await expect(field).toBeFocused();
    await expect(field).toHaveAttribute("inputmode", "numeric");
    await expect(field).toHaveAttribute("autocomplete", "one-time-code");
    await expect(field).not.toHaveAttribute("maxlength");
    const newCode = mail.locator('[data-f="signin:email:new-code"]');
    await expect(newCode).toBeDisabled();
    await expect(newCode).toHaveText(/^Send a new code in \d+ seconds$/);
    // The address shows on the page only: never in the URL, history or storage.
    expect(await kept(mail)).not.toContain(SAM);
    await shots(mail, "email-code-1440");
    const code = codeIn(await mailText(SAM));
    expect(code).toMatch(/^\d{6}$/);

    // Back: the address stage, prefilled; the same address while its flow is open goes back to
    // the code stage without a new mail.
    await mail.goBack();
    await expect(mail).toHaveURL(/\/sign-in\/email\?from=welcome$/);
    await expect(address).toHaveValue(SAM);
    const starts = emailStarts(P.d);
    await send.click();
    await expect(mail).toHaveURL(/stage=code/);
    expect(emailStarts(P.d)).toBe(starts);

    // A wrong code, typed: sent once at the sixth digit, the field read-only and busy while it is
    // with the relay; then its line, the field cleared and focused.
    const release = hold(P.d, "emailSignIn");
    await field.pressSequentially(code.replace(/^./, (d) => String((Number(d) + 1) % 10)));
    await expect(field).toHaveAttribute("readonly", "");
    await expect(field).toHaveAttribute("aria-busy", "true");
    release();
    await expect(line).toHaveText("That code isn't right. Check the email and try again.");
    await expect(field).not.toHaveAttribute("readonly");
    await expect(field).toHaveAttribute("aria-invalid", "true");
    await expect(field).toHaveValue("");
    await expect(field).toBeFocused();
    expect(codeTries(P.d)).toBe(1);
    // Pasted with a space: digits only, sent once. No answer comes: the code stays, and Try again
    // sends the same code again (the flow kept).
    failNext(P.d, "emailSignIn");
    await field.fill(`${code.slice(0, 3)} ${code.slice(3)}`);
    await expect(mail.locator(".sign-in-failure .st")).toHaveText("Momotus can't reach its sync service right now.");
    await expect(mail.locator(".sign-in-failure .btn")).toHaveText(["Try again", "Not now"]);
    await expect(field).toHaveValue(code);
    expect(codeTries(P.d)).toBe(2);
    await mail.locator('[data-f="signin:email:retry"]').click();
    await expect(mail).toHaveURL(/\/sign-in\/new\?from=welcome$/);
    expect(codeTries(P.d)).toBe(3);
    await mail.locator('[data-f="signin:create"]').click();
    await expect(mail).toHaveURL(/\/sync\/kit\?first=1&then=doors$/);
    await mail.locator('[data-f="sync-kit:cancel"]').click();
    await expect(mail).toHaveURL(/\/welcome\?step=doors$/);

    // Account reads the link as Email, and Link another offers no second one (E12).
    await go(mail, "/sync");
    const section = mail.locator('section[aria-label="Signed in with"]');
    await expect(section.locator(".sync-member h3")).toHaveText(["Email"]);
    await mail.locator('[data-f="sync:link-another"]').click();
    await expect(section.locator("[data-provider]")).toHaveText(["Continue with Apple", "Continue with mock"]);
    await expect(section.locator('[data-f="sync:link:email"]')).toHaveCount(0);

    // Late (a phone, at 375) asks a code for its Send a new code, near the end of the spec.
    await late.setViewportSize({ width: 375, height: 812 });
    await go(late, "/sign-in/email");
    await shots(late, "email-address-375");
    const lateAddress = late.locator('[data-f="signin:email:address"]');
    await lateAddress.fill(LEE);
    await late.locator('[data-f="signin:email:send"]').click();
    mailed.set(LEE, Date.now());
    await expect(late).toHaveURL(/\/sign-in\/email\?stage=code$/);
    // Use another address, then the same mailbox written another way inside the relay's gap: its
    // emailWait, with no button, and Send code still there.
    await late.locator('[data-f="signin:email:another"]').click();
    await expect(late).toHaveURL(/\/sign-in\/email$/);
    await lateAddress.fill("Lee@example.org");
    await late.locator('[data-f="signin:email:send"]').click();
    await expect(late.locator(".sign-in-failure .st")).toHaveText("Wait a minute, then send a new code.");
    await expect(late.locator(".sign-in-failure .btn")).toHaveCount(0);
    await expect(late).toHaveURL(/\/sign-in\/email$/);
    await expect(late.locator('[data-f="signin:email:send"]')).toBeEnabled();
    // The address its open flow went to: back to the code stage, no new mail.
    const lateStarts = emailStarts(P.e);
    await lateAddress.fill(LEE);
    await late.locator('[data-f="signin:email:send"]').click();
    await expect(late).toHaveURL(/\/sign-in\/email\?stage=code$/);
    expect(emailStarts(P.e)).toBe(lateStarts);
    await mailText(LEE);
    await shots(late, "email-code-375");
  });

  test("Desk signs in: a cancel says nothing, a closed window asks, then new, Create, the kit, the doors", async () => {
    await go(desk, "/welcome");
    await expect(desk.locator("h1")).toHaveText("Sign in");
    // The relay's providers, Apple first; any other by the id the relay gives it.
    await expect(desk.locator(".sign-in-providers .btn")).toHaveText(["Continue with Apple", "Continue with mock"]);
    await expect(desk.locator('[data-f="signin:other"]')).toHaveText("Other ways to sign in");
    await expect(desk.locator('[data-f="signin:not-now"]')).toHaveText("Not now");
    await shot(desk, "accounts-sign-in-desktop");

    // Cancelled on the provider page: back on the sign-in screen, nothing said.
    await providerPage(desk, "signin:mock", "Cancel");
    await expect(desk.locator('[data-f="signin:mock"]')).toBeVisible();
    await expect(desk.locator(".sign-in-failure")).toHaveCount(0);
    await expect(desk.getByText("Finish signing in in the window that opened.")).toHaveCount(0);

    // The window closed without a return, and this one is in front again: "Didn't finish signing in?"
    const [closed] = await Promise.all([desk.waitForEvent("popup"), desk.locator('[data-f="signin:mock"]').click()]);
    await expect(desk.getByText("Finish signing in in the window that opened.")).toBeVisible();
    await closed.getByText("Sign in as alice").waitFor();
    await closed.close();
    await desk.evaluate(() => dispatchEvent(new Event("focus")));
    await expect(desk.locator(".sign-in-failure .st")).toHaveText("Didn't finish signing in?");
    await expect(desk.locator(".sign-in-failure .btn")).toHaveText(["Try again", "Other ways to sign in"]);
    await shot(desk, "accounts-no-return-desktop");

    // Try again opens the provider page again, from the click.
    const [again] = await Promise.all([desk.waitForEvent("popup"), desk.locator('[data-f="signin:retry"]').click()]);
    await again.getByText("Sign in as alice").click();
    await expect(desk).toHaveURL(/\/sign-in\/new\?from=welcome$/);
    await expect(desk.locator("h1")).toHaveText("Create your Momotus account");
    await expect(desk.locator('[data-f="signin:already"]')).toHaveText("I already use Momotus on another device");
    await shot(desk, "accounts-create-desktop");
    await desk.locator('[data-f="signin:create"]').click();
    // The web has no account passkey (D11): the recovery kit step comes next, then the doors.
    await expect(desk).toHaveURL(/\/sync\/kit\?first=1&then=doors$/);
    await desk.locator('[data-f="sync-kit:make"]').click();
    await expect(desk.locator(".sync-kit-text")).toBeVisible();
    await desk.locator('[data-f="sync-kit:saved"]').click();
    await expect(desk).toHaveURL(/\/welcome\?step=doors$/);
    await expect(desk.locator('[data-f="welcome:fresh"]')).toBeVisible();
    // Signed in already: no door to another account.
    await expect(desk.locator('[data-f="welcome:pair"]')).toHaveCount(0);
    // The create carried the ticket and the web's form.
    expect(asked.find((a) => a.port === P.a && a.op === "createSyncAccount")?.fields).toEqual(expect.arrayContaining(["ticket", "form"]));
    expect(asked.find((a) => a.port === P.a && a.op === "createSyncAccount")?.form).toBe("phone");
  });

  test("Account lists the sign-in; Apple links, unlinks after its revoke, and once more without one", async () => {
    await go(desk, "/sync");
    const section = desk.locator('section[aria-label="Signed in with"]');
    const rows = section.locator(".sync-member");
    await expect(rows).toHaveCount(1);
    await expect(rows.first().locator("h3")).toHaveText("mock");
    await expect(rows.first().locator("p")).toContainText("Linked ");
    // The web signs in nothing else: it says to use the phone (4.2).
    await expect(desk.locator('[data-f="sync:add"]')).toContainText("Use your phone to sign in other devices");
    const link = async () => {
      await desk.locator('[data-f="sync:link-another"]').click();
      await expect(section.locator("[data-provider]")).toHaveText(["Continue with Apple", "Continue with mock"]);
      await providerPage(desk, "sync:link:apple", "alice");
      await expect(rows).toHaveCount(2);
      await expect(rows.nth(1).locator("h3")).toHaveText("Apple");
    };
    await link();
    await shot(desk, "accounts-signed-in-with-desktop");
    const unlinkApple = () => section.locator(".sync-member", { hasText: "Apple" }).locator('[data-f$=":unlink"]');

    // Apple's unlink: its revoke first, the receipt with the unlink.
    const [revoke] = await Promise.all([desk.waitForEvent("popup"), unlinkApple().click()]);
    await revoke.getByText("Sign in as alice").click();
    await expect(rows).toHaveCount(1);
    expect(asked.filter((a) => a.port === P.a && a.op === "unlinkIdentity").at(-1)?.fields).toContain("receipt");

    // Apple did not confirm (its window closed): the unlink goes on only on "Unlink anyway".
    await link();
    const [closed] = await Promise.all([desk.waitForEvent("popup"), unlinkApple().click()]);
    await closed.getByText("Sign in as alice").waitFor();
    await closed.close();
    await desk.evaluate(() => dispatchEvent(new Event("focus")));
    await expect(section.locator(".sign-in-failure .st")).toHaveText("Apple did not confirm, so your Apple Account may still list Momotus.");
    await expect(rows).toHaveCount(2);
    await section.locator('[data-f="sync:identity:anyway"]').click();
    await expect(rows).toHaveCount(1);
    expect(asked.filter((a) => a.port === P.a && a.op === "unlinkIdentity").at(-1)?.fields).not.toContain("receipt");
    // Apple again, for the deletion at the end.
    await link();

    // Link another by email (E12): the email row while no email link exists, both stages in place.
    // Cancel while the code is with the relay: its answer is dropped, nothing is linked.
    await desk.locator('[data-f="sync:link-another"]').click();
    await section.locator('[data-f="sync:link:email"]').click();
    await expect(section.locator("h3", { hasText: "Sign in with email" })).toBeVisible();
    await expect(section.locator('[data-f="sync:email:address"]')).toBeFocused();
    await section.locator('[data-f="sync:email:address"]').fill(ANN);
    await section.locator('[data-f="sync:email:send"]').click();
    const release = hold(P.a, "emailSignIn");
    const links = asked.filter((a) => a.port === P.a && a.op === "linkIdentity").length;
    await section.locator('[data-f="sync:email:code"]').fill(codeIn(await mailText(ANN)));
    await expect(section.locator('[data-f="sync:email:code"]')).toHaveAttribute("aria-busy", "true");
    await section.locator('[data-f="sync:email:leave"]').click();
    await expect(section.locator('[data-f="sync:email:code"]')).toHaveCount(0);
    release();
    await desk.waitForTimeout(1500);
    expect(asked.filter((a) => a.port === P.a && a.op === "linkIdentity").length).toBe(links);
    await expect(rows).toHaveCount(2);

    // The provider buttons are back, the email row with them.
    await section.locator('[data-f="sync:link:email"]').click();
    await section.locator('[data-f="sync:email:address"]').fill(PAT);
    await section.locator('[data-f="sync:email:send"]').click();
    mailed.set(PAT, Date.now());
    const field = section.locator('[data-f="sync:email:code"]');
    await expect(field).toBeFocused();
    await field.fill(codeIn(await mailText(PAT)));
    await expect(rows).toHaveCount(3);
    await expect(rows.nth(2).locator("h3")).toHaveText("Email");
    await desk.locator('[data-f="sync:link-another"]').click();
    await expect(section.locator("[data-provider]")).toHaveText(["Continue with Apple", "Continue with mock"]);
    await expect(section.locator('[data-f="sync:link:email"]')).toHaveCount(0);
  });

  test("Phone signs in as bob and skips the kit: Home nudges for a second way in", async () => {
    await go(phone, "/welcome");
    await providerPage(phone, "signin:mock", "bob");
    await expect(phone).toHaveURL(/\/sign-in\/new\?from=welcome$/);
    await phone.locator('[data-f="signin:create"]').click();
    await expect(phone).toHaveURL(/\/sync\/kit\?first=1&then=doors$/);
    await phone.locator('[data-f="sync-kit:cancel"]').click();
    // The web states its form: neither it nor a TV approves another device (D6).
    expect(asked.find((a) => a.port === P.b && a.op === "createSyncAccount")?.form).toBe("web");
    await expect(phone).toHaveURL(/\/welcome\?step=doors$/);
    await go(phone, "/");
    const card = phone.locator(".account-card");
    await expect(card).toHaveAttribute("data-card", "secondWay");
    await expect(card.locator("h2")).toHaveText("Add a second way in");
    await expect(card.locator('[data-f="home:account:kit"]')).toHaveText("Make a recovery kit");
    await shot(phone, "accounts-second-way-phone");
    // Account says the same, in place of the no-kit line.
    await go(phone, "/sync");
    await expect(phone.locator(".sync-callout")).toContainText("Add a second way in");
  });

  test("Phone signs out and removes its profiles and library: leave, then Trakt and debrid, then the wipe", async () => {
    await go(phone, "/sync");
    // D13: asked on the owner profile, both ticked by default.
    await expect(phone.locator('[data-f="sync:remove-library"]')).toHaveAttribute("aria-checked", "true");
    await expect(phone.locator('[data-f="sync:remove-debrid"]')).toHaveAttribute("aria-checked", "true");
    await phone.locator('[data-f="sync:remove-library"]').click();
    await expect(phone.locator('[data-f="sync:remove-debrid"]')).toHaveCount(0);
    await phone.locator('[data-f="sync:remove-library"]').click();
    await shot(phone, "accounts-sign-out-phone");
    const from = asked.length;
    await phone.locator('[data-f="sync:leave"]').click();
    // The tab is closed the moment the leave went out: the core recorded the removal first.
    await expect.poll(() => asked.slice(from).some((a) => a.port === P.b && a.op === "signOut" && a.fields.includes("removeAfter"))).toBe(true);
    const context = phone.context();
    await phone.close();
    // The next start finds it in the profiles view and finishes steps two and three: the
    // device is as new, at the first run's sign-in.
    phone = await context.newPage();
    phone.on("pageerror", (e) => errors.push(`phone: ${e.message}`));
    await phone.goto("/");
    await expect(phone).toHaveURL(/\/welcome$/, { timeout: 30_000 });
    await expect(phone.locator("h1")).toHaveText("Sign in");
    const ops = asked.slice(from).filter((a) => a.port === P.b).map((a) => a.op);
    // In order: the leave, every profile out of Trakt, then the record-less wipe.
    const leave = ops.indexOf("signOut");
    const trakt = ops.indexOf("traktSignOut");
    const wipe = ops.lastIndexOf("signOut");
    expect(leave).toBeGreaterThanOrEqual(0);
    expect(trakt).toBeGreaterThan(leave);
    expect(wipe).toBeGreaterThan(trakt);
    // The core holds no library: its sample addon went with it.
    const c = new Client(`ws://127.0.0.1:${P.b}/ws`);
    const addons = await c.subscribe<{ fresh: boolean; addons: unknown[] }>("addons");
    expect(addons.now().addons).toEqual([]);
    c.close();
  });

  test("the TV's first run is the code for a phone: Desk approves it, and the TV opens Home", async () => {
    // The TV has no email row, and the email route sends it to Sign in (E13).
    await expect(tv.locator("html")).toHaveAttribute("data-form", "tv");
    await go(tv, "/sign-in");
    await expect(tv.locator('[data-f="signin:mock"]')).toBeVisible();
    await expect(tv.locator('[data-f="signin:email"]')).toHaveCount(0);
    await go(tv, "/sign-in/email");
    await expect(tv).toHaveURL(/\/sign-in$/);
    await go(tv, "/welcome");
    await expect(tv.locator("h1")).toHaveText("Sign in with your phone");
    await expect(tv.getByText("Scan with your phone's camera")).toBeVisible();
    await expect(tv.locator(".qrbox svg")).toBeVisible();
    await expect(tv.locator(".qrbox svg")).toHaveAttribute("aria-label", "QR code. Scan it with your phone's camera to sign in.");
    // Nothing else to choose: Browse first is the one focus stop, and it has focus.
    await expect(tv.locator('[data-f="welcome:skip"]')).toBeFocused();
    await expect(tv.locator("main :is(button, a)")).toHaveCount(1);
    expect(asked.find((a) => a.port === P.c && a.view === "pairing")?.form).toBe("tv");
    await shot(tv, "accounts-first-run-tv");
    const code = codes.get(P.c) ?? "";
    // The relay names its link host: a phone camera opens the code as a link.
    expect(code).toMatch(new RegExp(`^https://${LINK_HOST}/pair#v=3&`));
    expect(code).toMatch(/&f=tv$/);
    await approve(code);
    await expect(tv).toHaveURL(new RegExp(`127.0.0.1:${PORTS.vite}/$`), { timeout: 30_000 });
    // Home says who let it in for 8 s, with nothing to focus (plans/tv-qr-login.md 5.5).
    const card = tv.locator(".account-card");
    await expect(card).toHaveAttribute("data-card", "signedIn");
    await expect(card).toContainText("Signed in from ");
    await expect(card.locator("button, a")).toHaveCount(0);
    await shot(tv, "accounts-tv-signed-in");
    await expect(tv.locator('.account-card[data-card="signedIn"]')).toHaveCount(0, { timeout: 12_000 });
    await desk.locator('[data-f="sync-add:done"]').click();
  });

  test("Phone signs in as alice: found, Start over asks first, then the code Desk approves", async () => {
    await go(phone, "/welcome");
    await providerPage(phone, "signin:mock", "alice");
    await expect(phone).toHaveURL(/\/sign-in\/found\?from=welcome$/);
    await expect(phone.locator(".lead")).toHaveText(
      "This sign-in already has a Momotus account. Scan this code with your phone, where Momotus is signed in, to add this browser.",
    );
    await expect(phone.locator(".qrbox svg")).toBeVisible();
    await expect(phone.locator('[data-f="signin:found-kit"]')).toHaveText("Use a recovery kit");
    await phone.locator('[data-f="signin:cant"]').click();
    await expect(phone.locator(".sync-ask p")).toHaveText("Your old library can't be opened. It is deleted in 7 days unless one of your old devices cancels.");
    await shot(phone, "accounts-found-phone");
    const given = codes.get(P.b);
    await phone.locator('[data-f="signin:start-over-cancel"]').click();
    // Cancel mounts a new session: Desk approves its code, never the one Start over gave back.
    await expect.poll(() => codes.get(P.b)).not.toBe(given);
    await approve(codes.get(P.b) ?? "");
    await expect(phone).toHaveURL(new RegExp(`127.0.0.1:${PORTS.vite}/$`), { timeout: 30_000 });
    await desk.locator('[data-f="sync-add:done"]').click();
  });

  test("the TV's Home shows the Phone's join on its owner profile until OK", async () => {
    await syncNow(tv);
    await go(tv, "/");
    const card = tv.locator(".account-card");
    await expect(card).toHaveAttribute("data-card", "announcement");
    await expect(card).toContainText("A new device joined: Momotus phone.");
    // Its place: after the first row (or under the hero when there is no row), never between the hero and
    // the row that rises into it (plans/design-v3-phase2-web.md, the account card).
    const before = await card.evaluate((el) => {
      const prev = el.previousElementSibling as HTMLElement | null;
      return { prev: prev?.classList.contains("row") ? "row" : prev?.classList.contains("hero") ? "hero" : (prev?.className ?? null), rows: document.querySelectorAll(".row").length, first: el.parentElement?.querySelector(".row") === prev };
    });
    if (before.rows > 0) expect(before).toMatchObject({ prev: "row", first: true });
    else expect(before.prev).toBe("hero");
    await shot(tv, "accounts-announcement-tv");
    await press(tv, "home:account:ok");
    await expect(card).toHaveCount(0);
    expect(asked.some((a) => a.port === P.c && a.op === "ackAnnouncements")).toBe(true);
    // Gone for good: the next round does not bring it back.
    await syncNow(tv);
    await go(tv, "/");
    await expect(tv.locator(".home .hero")).toBeAttached();
    await expect(card).toHaveCount(0);
  });

  test("a deletion asked on the link host shows on Home with Cancel, and with Cancel and turn off", async ({ browser }) => {
    const due = await requestDeletion("mock", "alice");
    expect(due).toBeGreaterThan(Date.now() + 6 * 86_400_000);
    await syncNow(tv);
    await go(tv, "/");
    const card = tv.locator(".account-card");
    await expect(card).toHaveAttribute("data-card", "deletion");
    await expect(card).toContainText("Someone signed in with your mock account and asked to delete your Momotus account on");
    await expect(card.locator(".btn")).toHaveText(["Cancel deletion", "Cancel and turn off mock sign-in"]);
    await shot(tv, "accounts-deletion-tv");
    // Any profile cancels (D16); the card goes once the round says so.
    await press(tv, "home:account:cancel");
    await expect(card).toHaveCount(0);

    // Asked again; Desk cancels and turns the mock sign-in off, so it cannot simply ask again.
    await requestDeletion("mock", "alice");
    await syncNow(desk);
    await go(desk, "/");
    const deskCard = desk.locator(".account-card");
    const cancelUnlink = desk.locator('[data-f="home:account:cancel-unlink"]');
    await expect(deskCard).toHaveAttribute("data-card", "deletion");
    await expect(cancelUnlink).toHaveText("Cancel and turn off mock sign-in");
    await cancelUnlink.click();
    await expect(deskCard).toHaveCount(0);
    await go(desk, "/sync");
    await expect(desk.locator('section[aria-label="Signed in with"] .sync-member h3')).toHaveText(["Apple", "Email"]);

    // Asked once more, on the link host's own page by email (E14): the email words, and Cancel
    // and turn off email sign-in.
    await deletionByEmail(browser, PAT);
    await syncNow(desk);
    await go(desk, "/");
    await expect(deskCard).toHaveAttribute("data-card", "deletion");
    await expect(deskCard).toContainText("Someone signed in with your email address and asked to delete your Momotus account on");
    await expect(cancelUnlink).toHaveText("Cancel and turn off email sign-in");
    await cancelUnlink.click();
    await expect(deskCard).toHaveCount(0);
    await go(desk, "/sync");
    await expect(desk.locator('section[aria-label="Signed in with"] .sync-member h3')).toHaveText(["Apple"]);
  });

  test("Desk deletes the account after Apple's revoke; the TV hears it and removes its library from Home", async () => {
    await go(desk, "/sync");
    await desk.locator('[data-f="sync:delete"]').click();
    const [revoke] = await Promise.all([desk.waitForEvent("popup"), desk.locator('[data-f="sync:delete-confirm"]').click()]);
    await revoke.getByText("Sign in as alice").click();
    await expect(desk.getByText("Your Momotus account was deleted. Your library stays on this device.")).toBeVisible();
    expect(asked.filter((a) => a.port === P.a && a.op === "deleteAccount").at(-1)?.fields).toContain("receipt");

    // The TV hears it at its next call to the sync service: Account's own reads, or Sync now.
    await go(tv, "/sync");
    await tv
      .locator('[data-f="sync:now"]')
      .click({ timeout: 3000 })
      .catch(() => undefined);
    await expect(tv.locator('[data-f="sync:gone-ok"]')).toBeVisible();
    await go(tv, "/");
    const card = tv.locator(".account-card");
    await expect(card).toHaveAttribute("data-card", "gone");
    await expect(card.locator(".btn")).toHaveText(["Sign in", "Remove from this device", "OK"]);
    await shot(tv, "accounts-gone-tv");
    await press(tv, "home:account:remove");
    await expect(card.locator('[data-f="home:account:remove-debrid"]')).toHaveAttribute("aria-checked", "true");
    await press(tv, "home:account:remove-confirm");
    // Removed: the TV starts as new, at its first run.
    await expect(tv).toHaveURL(/\/welcome$/, { timeout: 30_000 });
    await expect(tv.locator("h1")).toHaveText("Sign in with your phone");
  });

  test("Late's Send a new code turns on after its minute; Mail signs out and finds its account by email", async () => {
    // Counted from the mail's time: off until the minute is out on the page's clock, then on,
    // then off again for the next.
    await mailable(LEE);
    const newCode = late.locator('[data-f="signin:email:new-code"]');
    await expect(newCode).toBeDisabled();
    await late.clock.fastForward(61_000);
    await expect(newCode).toHaveText("Send a new code");
    await expect(newCode).toBeEnabled();
    const starts = emailStarts(P.e);
    await newCode.click();
    await expect(late.getByText("We sent a new code. Use the newest email: older codes stop working.")).toBeVisible();
    mailed.set(LEE, Date.now());
    expect(emailStarts(P.e)).toBe(starts + 1);
    await expect(newCode).toBeDisabled();
    // Back while the right code is with the relay: the address stage, and its answer is dropped
    // (no `new`, no line).
    const release = hold(P.e, "emailSignIn");
    await late.locator('[data-f="signin:email:code"]').fill(codeIn(await mailText(LEE)));
    await expect(late.locator('[data-f="signin:email:code"]')).toHaveAttribute("aria-busy", "true");
    await late.goBack();
    await expect(late).toHaveURL(/\/sign-in\/email$/);
    release();
    await late.waitForTimeout(1500);
    await expect(late).toHaveURL(/\/sign-in\/email$/);
    await expect(late.locator('[data-f="signin:email:address"]')).toHaveValue(LEE);
    await expect(late.locator("#signin-email-error")).toHaveCount(0);

    // Mail signs out and keeps its library, then signs in by email again: `found`.
    await go(mail, "/sync");
    await mail.locator('[data-f="sync:remove-library"]').click();
    await mail.locator('[data-f="sync:leave"]').click();
    const signIn = mail.locator('[data-f="sync:sign-in"]');
    await expect(signIn.or(mail.locator('[data-f="sync:leave-again"]'))).toBeVisible();
    if (await mail.locator('[data-f="sync:leave-again"]').isVisible()) await mail.locator('[data-f="sync:leave-again"]').click();
    await expect(signIn).toBeVisible();
    await go(mail, "/sign-in");
    await mail.locator('[data-f="signin:email"]').click();
    await expect(mail).toHaveURL(/\/sign-in\/email$/);
    // The address stays in memory while the tab lives (never in storage or the URL).
    await expect(mail.locator('[data-f="signin:email:address"]')).toHaveValue(SAM);
    await mailable(SAM);
    await mail.locator('[data-f="signin:email:send"]').click();
    mailed.set(SAM, Date.now());
    await expect(mail).toHaveURL(/\/sign-in\/email\?stage=code$/);
    await mail.locator('[data-f="signin:email:code"]').fill(codeIn(await mailText(SAM)));
    await expect(mail).toHaveURL(/\/sign-in\/found$/);
    await expect(mail.locator(".lead")).toHaveText(
      "This sign-in already has a Momotus account. Scan this code with your phone, where Momotus is signed in, to add this browser.",
    );
    // Back to the code stage of a code already used: no dead field, the address stage instead.
    await mail.goBack();
    await expect(mail).toHaveURL(/\/sign-in\/email$/);
    await expect(mail.locator('[data-f="signin:email:address"]')).toHaveValue(SAM);

    // A reload at the code stage has no flow: it is the address stage.
    await go(late, "/sign-in/email?stage=code");
    await late.reload();
    await expect(late).toHaveURL(/\/sign-in\/email$/);
    await expect(late.locator('[data-f="signin:email:address"]')).toHaveValue("");
  });

  test("Desk signs in as bob, whose account has no device left: Start over makes a new one", async () => {
    await go(desk, "/welcome");
    await providerPage(desk, "signin:mock", "bob");
    await expect(desk).toHaveURL(/\/sign-in\/found\?from=welcome$/);
    await expect(desk.locator(".qrbox svg")).toBeVisible();
    await desk.locator('[data-f="signin:cant"]').click();
    // The code gave the account slot back while Start over is asked.
    await expect(desk.locator(".qrbox svg")).toHaveCount(0);
    const from = asked.length;
    await desk.locator('[data-f="signin:start-over"]').click();
    await expect(desk).toHaveURL(/\/sync\/kit\?first=1&then=doors$/, { timeout: 30_000 });
    expect(asked.slice(from).find((a) => a.port === P.a && a.op === "createSyncAccount")?.fields).toContain("move");
    await expect(desk.locator(".sign-in-failure")).toHaveCount(0);
  });

  test("no ticket or receipt reaches storage, history or the URL, and no page threw", async () => {
    expect(secrets.size).toBeGreaterThan(4);
    for (const page of [desk, phone, tv, mail, late]) {
      const text = await kept(page);
      for (const secret of secrets) expect(text.includes(secret), `a ticket or receipt is kept on ${page.url()}`).toBe(false);
      for (const address of [SAM, LEE, PAT]) expect(text.includes(address), `an address is kept on ${page.url()}`).toBe(false);
    }
    expect(errors).toEqual([]);
  });
});
