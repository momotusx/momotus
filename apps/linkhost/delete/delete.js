// The deletion page (plans/accounts.md 2.3, 5.6, D14): sign in with the provider through the
// sync service's brokered flow and ask for the account's deletion, which runs in 7 days unless
// one of its devices cancels. Apple's flow both signs in and revokes Sign in with Apple. Only a
// flow whose verifier sits in this page's own sessionStorage completes; the code in the return
// is used once and leaves the address bar at once. Continue with email (plans/accounts-email.md
// E14) mails a code and takes it on this page, its verifier in memory only. Nothing here logs.

/** The owner's sync service, for example https://sync.example.org (no slash at the end). */
export const RELAY = "{{RELAY_URL}}";

const KEY = "mq.delete.";

const b64url = (bytes) => btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");

/** A fresh verifier: 32 random bytes, base64url. */
export const newVerifier = () => b64url(crypto.getRandomValues(new Uint8Array(32)));

/** What the sync service keeps of it: lowercase hex SHA-256 of its text. */
export async function verifierHash(verifier) {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier)));
  return [...digest].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** A return's fragment, only from this page's sync service: `{flow, code}` or `{flow, error}`; else null. */
export function readReturn(fragment, relay = RELAY) {
  const fields = new URLSearchParams(fragment.replace(/^#/, ""));
  const flow = fields.get("flow");
  if (!flow || fields.get("relay") !== new URL(relay).hostname) return null;
  const error = fields.get("error");
  const code = fields.get("code");
  return error ? { flow, error } : code ? { flow, code } : null;
}

/** Apple's flow revokes too (its receipt goes with the request); the others sign in. */
export const purposeOf = (provider) => (provider === "apple" ? "revoke" : "signin");

async function post(path, body) {
  const answer = await fetch(`${RELAY}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    credentials: "omit",
    referrerPolicy: "no-referrer",
  });
  const json = await answer.json().catch(() => ({}));
  if (!answer.ok) throw Object.assign(new Error(json.error ?? `status ${answer.status}`), { code: json.code ?? (answer.status === 429 ? "busy" : "") });
  return json;
}

export const WORDS = {
  cancelled: "Sign-in was cancelled. Choose a sign-in to try again.",
  expired: "That sign-in took too long. Choose a sign-in to start again.",
  stranger: "This sign-in was not started on this page. Choose a sign-in to start again.",
  busy: "Too many sign-ins from this network just now. Try again in a minute.",
  failed: "The sign-in did not work. Try again in a moment.",
  noAccount: "This sign-in has no Momotus account. Nothing was deleted.",
  ask: "Delete your Momotus account? It is deleted for every device in 7 days. Until then, any of your devices can cancel it from Home.",
  asked: "Your Momotus account will be deleted on {date}. Any of your devices can cancel it from Home before then.",
  sent: "We sent a 6-digit code to {email}. It works for 10 minutes.",
  badAddress: "Enter an email address like name@example.com.",
  wrongCode: "That code isn't right. Check the email and try again.",
  codeExpired: "This code can't be used any more. Send a new code.",
  emailWait: "Wait a minute, then send a new code.",
  emailLimit: "Too many codes were asked for just now. Try again tomorrow, or use another way to sign in.",
  mailUnavailable: "The code couldn't be sent right now. Try again later, or use another way to sign in.",
};

/** What an email step's answer says (the apps' words, as plans/accounts-email.md 4 has them). */
export function mailWords(code) {
  const said = {
    badAddress: WORDS.badAddress,
    wrongCode: WORDS.wrongCode,
    badCode: WORDS.wrongCode,
    ticketExpired: WORDS.codeExpired,
    wrongFlow: WORDS.codeExpired,
    signInRefused: WORDS.codeExpired,
    emailWait: WORDS.emailWait,
    emailLimit: WORDS.emailLimit,
    mailUnavailable: WORDS.mailUnavailable,
    busy: WORDS.busy,
  };
  return said[code] ?? WORDS.failed;
}

/** The digits of a typed or pasted code (`123 456`, `123-456`). */
export const codeDigits = (typed) => typed.replace(/\D/g, "").slice(0, 6);

function say(text, tone = "") {
  const line = document.getElementById("said");
  line.textContent = text;
  line.className = tone;
}

async function start(provider) {
  say("");
  try {
    const verifier = newVerifier();
    const { flow, url } = await post(`/v1/auth/${provider}/start`, {
      verifierHash: await verifierHash(verifier),
      purpose: purposeOf(provider),
      return: `${location.origin}/delete`,
    });
    sessionStorage.setItem(KEY + flow, verifier);
    location.assign(url);
  } catch (e) {
    say(e.code === "busy" ? WORDS.busy : WORDS.failed, "bad");
  }
}

async function finish(back) {
  const verifier = sessionStorage.getItem(KEY + back.flow);
  sessionStorage.removeItem(KEY + back.flow);
  if (back.error) return say(back.error === "expired" ? WORDS.expired : WORDS.cancelled, "bad");
  if (!verifier) return say(WORDS.stranger, "bad");
  let taken;
  try {
    taken = await post("/v1/auth/take", { flow: back.flow, verifier, code: back.code });
  } catch (e) {
    return say(e.code === "ticketExpired" ? WORDS.expired : e.code === "busy" ? WORDS.busy : WORDS.failed, "bad");
  }
  ask(taken);
}

/** A sign-in's ticket: ask before the deletion, or say there is no account. */
function ask(taken) {
  const confirm = document.getElementById("confirm");
  confirm.hidden = true;
  if (!taken.account) return say(WORDS.noAccount);
  say(WORDS.ask);
  confirm.hidden = false;
  // One handler, for this ticket only: a later sign-in on the page replaces it.
  confirm.onclick = async () => {
    confirm.disabled = true;
    try {
      const { dueMs } = await post("/v1/account/delete-request", { ticket: taken.ticket });
      confirm.hidden = true;
      say(WORDS.asked.replace("{date}", new Date(dueMs).toLocaleDateString(undefined, { dateStyle: "long" })), "ok");
    } catch (e) {
      confirm.disabled = false;
      say(e.code === "ticketExpired" ? WORDS.expired : WORDS.failed, "bad");
    }
  };
}

/** Continue with email's flow: in this page's memory only, never in sessionStorage (E14). */
let mail = null;

const $ = (id) => document.getElementById(id);

/** One call at a time: its button off while it runs. */
async function busy(button, work) {
  if (button.disabled) return;
  button.disabled = true;
  try {
    await work();
  } finally {
    button.disabled = false;
  }
}

function showEmail() {
  say("");
  $("confirm").hidden = true;
  $("email-form").hidden = false;
  $("email").focus();
}

async function sendCode() {
  say("");
  const email = $("email").value.trim();
  try {
    const verifier = newVerifier();
    const { flow } = await post("/v1/auth/email/start", { verifierHash: await verifierHash(verifier), purpose: "signin", email });
    mail = { flow, verifier };
    $("sent").textContent = WORDS.sent.replace("{email}", () => email);
    $("code").value = "";
    $("code-form").hidden = false;
    $("code").focus();
  } catch (e) {
    say(mailWords(e.code), "bad");
    if (e.code === "badAddress") $("email").focus();
  }
}

async function takeCode() {
  const mine = mail;
  if (!mine) return say(WORDS.codeExpired, "bad");
  say("");
  let taken;
  try {
    taken = await post("/v1/auth/take", { flow: mine.flow, verifier: mine.verifier, code: codeDigits($("code").value) });
  } catch (e) {
    const code = e.code ?? "";
    // No answer (a network error): the code stays for Continue again. A coded answer clears it;
    // a wrong code or a busy relay keep the flow, anything else ended it (emailLimit included).
    if (code) {
      $("code").value = "";
      if (!["wrongCode", "badCode", "busy"].includes(code) && mail === mine) mail = null;
    }
    $("code").focus();
    return say(mailWords(code), "bad");
  }
  if (mail === mine) mail = null;
  $("email-form").hidden = true;
  $("code-form").hidden = true;
  ask(taken);
}

function main() {
  for (const button of document.querySelectorAll("[data-provider]")) {
    button.addEventListener("click", () => (button.dataset.provider === "email" ? showEmail() : void start(button.dataset.provider)));
  }
  $("email-form").addEventListener("submit", (e) => {
    e.preventDefault();
    void busy($("send"), sendCode);
  });
  $("code-form").addEventListener("submit", (e) => {
    e.preventDefault();
    void busy($("take"), takeCode);
  });
  const fragment = location.hash;
  if (!fragment) return;
  // The code leaves the address bar and the history at once.
  history.replaceState(null, "", location.pathname);
  const back = readReturn(fragment);
  if (back) void finish(back);
  else say(WORDS.stranger, "bad");
}

if (typeof document !== "undefined") main();
