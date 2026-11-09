/**
 * Live check before plans/trakt-account.md phase 2 (review finding 2): what Trakt does when two
 * devices hold one token chain and both refresh it.
 *
 * Device A and device B share one chain, as every device of an account would through the synced
 * link. The script plays both: one device-code sign-in (the owner types the code once), then
 *   1. A refreshes (r0 -> a1, r1) and uses a1 once.
 *   2. The old access token a0 is tried right away.
 *   3. B reuses the spent r0 (the losing refresh of a race).
 *   4. The winner survives? a1 is used, r1 is refreshed (-> a2, r2), a2 is used.
 *   5. a0 and a1 are tried again at each wait (minutes after a1 was first used).
 *   6. Revoking the pre-rotation a0: does it reach the rotated chain? (r2 refresh, a2 use after it)
 *   7. Cleanup: the newest access token is revoked, then its refresh token must fail.
 *
 * Run: MARQUEE_TRAKT_CLIENT_ID=<the app's client id> MARQUEE_TRAKT_REDIRECT_URI=<its redirect uri> \
 *        bun bench/m7/trakt-race.ts
 * Optional: TRAKT_RACE_WAITS=0,15,60 (minutes), TRAKT_AUTH and TRAKT_API (hosts, for a dry run
 * against apps/android/scripts/fake-trakt.ts), TRAKT_RACE_OUT (rows file).
 *
 * Rows go to bench/m7/trakt-live.jsonl. No token, code or body is ever printed or written:
 * statuses, Trakt's `error` field and token lengths only.
 */
import { appendFileSync } from "node:fs";
import { join } from "node:path";

const clientId = process.env.MARQUEE_TRAKT_CLIENT_ID ?? "";
const redirectUri = process.env.MARQUEE_TRAKT_REDIRECT_URI ?? "";
if (!clientId || !redirectUri) {
  console.error("Set MARQUEE_TRAKT_CLIENT_ID and MARQUEE_TRAKT_REDIRECT_URI (the build's values).");
  process.exit(2);
}
const auth = process.env.TRAKT_AUTH ?? "https://auth.trakt.tv";
const api = process.env.TRAKT_API ?? "https://api.trakt.tv";
const waits = (process.env.TRAKT_RACE_WAITS ?? "0,15,60").split(",").map(Number);
const out = process.env.TRAKT_RACE_OUT ?? join(import.meta.dir, "trakt-live.jsonl");
const date = new Date().toISOString().slice(0, 10);

type Tokens = { access: string; refresh: string };
type Answer = { status: number; error?: string; body?: Record<string, unknown> };

const row = (step: string, data: Record<string, unknown>) => {
  const line = JSON.stringify({ date, check: "trakt-race", step, ...data });
  appendFileSync(out, `${line}\n`);
  console.log(line);
};

async function post(path: string, body: Record<string, unknown>): Promise<Answer> {
  const res = await fetch(`${auth}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json", "User-Agent": "Momotus-race-check/1" },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  let parsed: Record<string, unknown> | undefined;
  try {
    parsed = text ? JSON.parse(text) : undefined;
  } catch {
    parsed = undefined;
  }
  const error = typeof parsed?.error === "string" ? parsed.error : undefined;
  return { status: res.status, error, body: parsed };
}

/** One authenticated call: only its status. */
async function use(access: string): Promise<number> {
  const res = await fetch(`${api}/users/settings`, {
    headers: {
      "Content-Type": "application/json",
      "User-Agent": "Momotus-race-check/1",
      "trakt-api-version": "2",
      "trakt-api-key": clientId,
      Authorization: `Bearer ${access}`,
    },
  });
  await res.arrayBuffer();
  return res.status;
}

function tokens(a: Answer): Tokens | null {
  const access = a.body?.access_token;
  const refresh = a.body?.refresh_token;
  return typeof access === "string" && typeof refresh === "string" ? { access, refresh } : null;
}

const refresh = (token: string) =>
  post("/oauth/token", {
    refresh_token: token,
    client_id: clientId,
    redirect_uri: redirectUri,
    grant_type: "refresh_token",
  });

const revoke = (token: string) => post("/oauth/revoke", { token, client_id: clientId });

const sleepMin = (min: number) => new Promise((r) => setTimeout(r, min * 60_000));

async function signIn(): Promise<Tokens> {
  const code = await post("/oauth/device/code", { client_id: clientId });
  const body = code.body ?? {};
  if (code.status !== 200 || typeof body.device_code !== "string") {
    row("sign-in", { status: code.status, error: code.error });
    throw new Error(`the device code was refused (status ${code.status})`);
  }
  console.log(`\nOpen ${body.verification_url} and enter ${body.user_code}\n`);
  const interval = Number(body.interval ?? 5) * 1000;
  const deadline = Date.now() + Number(body.expires_in ?? 600) * 1000;
  while (Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, interval));
    const poll = await post("/oauth/device/token", { code: body.device_code, client_id: clientId });
    const got = poll.status === 200 ? tokens(poll) : null;
    if (got) return got;
    if (![400, 429].includes(poll.status)) throw new Error(`the sign-in ended with status ${poll.status}`);
  }
  throw new Error("the code ran out");
}

const t0 = await signIn();
row("0 signed in", { accessLen: t0.access.length, use: await use(t0.access) });

const r1 = await refresh(t0.refresh);
const t1 = tokens(r1);
if (!t1) {
  row("1 A refreshes", { status: r1.status, error: r1.error });
  process.exit(1);
}
const firstUse = await use(t1.access);
const usedAt = Date.now();
row("1 A refreshes and uses a1", { status: r1.status, accessLen: t1.access.length, use: firstUse });
row("2 old a0 at once", { use: await use(t0.access) });

const stale = await refresh(t0.refresh);
row("3 B reuses spent r0", { status: stale.status, error: stale.error, gotTokens: tokens(stale) !== null });

const a1After = await use(t1.access);
const r2 = await refresh(t1.refresh);
const t2 = tokens(r2);
const a2Use = t2 ? await use(t2.access) : null;
row("4 winner after the stale refresh", {
  a1Use: a1After,
  r1Refresh: r2.status,
  r1Error: r2.error,
  a2Use,
  verdict: a1After === 200 && t2 && a2Use === 200 ? "winner survives" : "chain revoked by the stale refresh",
});

for (const min of waits) {
  const left = usedAt + min * 60_000 - Date.now();
  if (left > 0) await sleepMin(left / 60_000);
  row(`5 old tokens ${min} min after a1's first use`, { a0Use: await use(t0.access), a1Use: await use(t1.access) });
}

let newest = t2 ?? t1;
if (t2) {
  const rv = await revoke(t0.access);
  const r3 = await refresh(t2.refresh);
  const t3 = tokens(r3);
  row("6 revoke pre-rotation a0, then the rotated chain", {
    revoke: rv.status,
    r2Refresh: r3.status,
    r2Error: r3.error,
    a3Use: t3 ? await use(t3.access) : null,
  });
  if (t3) newest = t3;
}

const end = await revoke(newest.access);
const after = await refresh(newest.refresh);
row("7 cleanup", { revoke: end.status, refreshAfterRevoke: after.status, error: after.error });
