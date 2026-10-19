/** "Remember this browser": the sync record sealed with AES-GCM under a key
 * derived (HKDF) from a passkey's PRF output. IndexedDB holds only the
 * ciphertext, the credential id and the PRF salt; the key exists in this
 * page's memory, as a non-extractable CryptoKey, only after the passkey
 * answered with user verification. Each page load asks for it again, and so
 * do removing a device, reporting one lost and making a kit. Without PRF
 * (or on an address a passkey cannot belong to, such as a bare IP) nothing
 * is stored and the record stays in the tab's memory. */

import type { SyncRecord } from "../lib/sync";

const DB = "marquee-vault";
const STORE = "sealed";
const ENTRY = "sync";
const INFO = new TextEncoder().encode("marquee/web-record/v1");

interface Sealed {
  credential: ArrayBuffer;
  salt: Uint8Array<ArrayBuffer>;
  iv: Uint8Array<ArrayBuffer>;
  data: ArrayBuffer;
}

export class VaultError extends Error {
  constructor(readonly code: "noPrf" | "passkey" | "forget") {
    super(code === "noPrf" ? "this passkey has no PRF" : code === "forget" ? "the sealed copy could not be deleted" : "the passkey did not confirm");
  }
}

let key: CryptoKey | null = null;
let held: Sealed | null = null;

function db(): Promise<IDBDatabase> {
  return new Promise((ok, fail) => {
    const open = indexedDB.open(DB, 1);
    open.onupgradeneeded = () => open.result.createObjectStore(STORE);
    open.onsuccess = () => ok(open.result);
    open.onerror = () => fail(open.error);
  });
}

/** One request in its own transaction. It settles when the transaction
 * commits, not when the request succeeds: a write the browser then aborts
 * (quota, eviction, a closing tab) is a failed write. */
async function tx<T>(mode: IDBTransactionMode, work: (s: IDBObjectStore) => IDBRequest): Promise<T> {
  const d = await db();
  try {
    return await new Promise<T>((ok, fail) => {
      const t = d.transaction(STORE, mode);
      const r = work(t.objectStore(STORE));
      t.oncomplete = () => ok(r.result as T);
      t.onabort = () => fail(t.error ?? new DOMException("the write did not commit", "AbortError"));
      t.onerror = () => fail(t.error);
    });
  } finally {
    d.close();
  }
}

const read = () => tx<Sealed | undefined>("readonly", (s) => s.get(ENTRY));
const write = (v: Sealed) => tx("readwrite", (s) => s.put(v, ENTRY));

type Prf = { prf?: { enabled?: boolean; results?: { first?: ArrayBuffer } } };
const random = (n: number) => crypto.getRandomValues(new Uint8Array(n));

/** A passkey can belong here: WebAuthn exists, the page is secure, and the
 * host is a name (a passkey cannot belong to a bare IP address). PRF itself
 * is known only once a passkey answers, unless the browser says so first. */
export async function available(): Promise<boolean> {
  if (!window.PublicKeyCredential || !isSecureContext) return false;
  if (/^[\d.]+$|:/.test(location.hostname)) return false;
  const caps = (PublicKeyCredential as unknown as { getClientCapabilities?: () => Promise<Record<string, boolean>> }).getClientCapabilities;
  if (!caps) return true;
  try {
    return (await caps())["extension:prf"] !== false;
  } catch {
    return true;
  }
}

/** A sealed record waits in this browser (it needs the passkey to open).
 * A storage that cannot be read rejects: it may hold one, so callers never
 * read the failure as "none". */
export async function stored(): Promise<boolean> {
  return (await read()) !== undefined;
}

/** Remembering is on for this page: the key is in memory. */
export const active = () => key !== null;

async function derive(first: ArrayBuffer): Promise<CryptoKey> {
  const base = await crypto.subtle.importKey("raw", first, "HKDF", false, ["deriveKey"]);
  return crypto.subtle.deriveKey({ name: "HKDF", hash: "SHA-256", salt: new Uint8Array(0), info: INFO }, base, { name: "AES-GCM", length: 256 }, false, [
    "encrypt",
    "decrypt",
  ]);
}

/** Asks the passkey, with user verification, for its PRF output on `salt`. */
async function evaluate(credential: ArrayBuffer, salt: Uint8Array<ArrayBuffer>): Promise<ArrayBuffer> {
  let answer: PublicKeyCredential;
  try {
    answer = (await navigator.credentials.get({
      publicKey: {
        challenge: random(32),
        allowCredentials: [{ type: "public-key", id: credential }],
        userVerification: "required",
        extensions: { prf: { eval: { first: salt } } } as AuthenticationExtensionsClientInputs,
      },
    })) as PublicKeyCredential;
  } catch {
    throw new VaultError("passkey");
  }
  // The authenticator's own word that it verified the user (flag UV, bit 2).
  const flags = new Uint8Array((answer.response as AuthenticatorAssertionResponse).authenticatorData)[32] ?? 0;
  if (!(flags & 0x04)) throw new VaultError("passkey");
  const first = (answer.getClientExtensionResults() as Prf).prf?.results?.first;
  if (!first) throw new VaultError("noPrf");
  return first;
}

async function seal(record: SyncRecord, with_: CryptoKey, credential: ArrayBuffer, salt: Uint8Array<ArrayBuffer>): Promise<Sealed> {
  const iv = random(12);
  const data = await crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData: INFO }, with_, new TextEncoder().encode(JSON.stringify(record)));
  return { credential, salt, iv, data };
}

async function open(sealed: Sealed, with_: CryptoKey): Promise<SyncRecord> {
  try {
    const plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: sealed.iv, additionalData: INFO }, with_, sealed.data);
    return JSON.parse(new TextDecoder().decode(plain)) as SyncRecord;
  } catch {
    throw new VaultError("passkey");
  }
}

/** Makes a passkey with PRF for this browser and seals `record` under it. */
export async function enable(record: SyncRecord): Promise<void> {
  const salt = random(32);
  let made: PublicKeyCredential;
  try {
    made = (await navigator.credentials.create({
      publicKey: {
        rp: { name: "Marquee" },
        user: { id: random(16), name: "Marquee sync", displayName: "Marquee sync" },
        challenge: random(32),
        pubKeyCredParams: [
          { type: "public-key", alg: -7 },
          { type: "public-key", alg: -257 },
        ],
        authenticatorSelection: { userVerification: "required", residentKey: "preferred" },
        extensions: { prf: { eval: { first: salt } } } as AuthenticationExtensionsClientInputs,
      },
    })) as PublicKeyCredential;
  } catch {
    throw new VaultError("passkey");
  }
  try {
    const prf = (made.getClientExtensionResults() as Prf).prf;
    if (!prf?.enabled) throw new VaultError("noPrf");
    // Some authenticators answer PRF only when asked to sign in.
    const first = prf.results?.first ?? (await evaluate(made.rawId, salt));
    const k = await derive(first);
    const sealed = await seal(record, k, made.rawId, salt);
    await write(sealed);
    // Ask the browser not to evict this storage when it runs low or sits idle.
    void navigator.storage?.persist?.().catch(() => false);
    key = k;
    held = sealed;
  } catch (e) {
    // From here on any failure (no PRF, a cancelled sign-in, a seal or write that failed) leaves
    // a passkey that guards nothing: ask its manager to drop it, where the browser can.
    void forgetPasskey(made.rawId);
    throw e;
  }
}

/** Tells the passkey manager a credential guards nothing (WebAuthn Signal API; best effort). */
function forgetPasskey(id: ArrayBuffer) {
  const signal = (PublicKeyCredential as unknown as { signalUnknownCredential?: (o: { rpId: string; credentialId: string }) => Promise<void> }).signalUnknownCredential;
  const credentialId = btoa(String.fromCharCode(...new Uint8Array(id))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  return signal?.call(PublicKeyCredential, { rpId: location.hostname, credentialId }).catch(() => undefined);
}

/** Opens the record this browser remembers, after the passkey verified the user. */
export async function unlock(): Promise<SyncRecord> {
  const sealed = await read();
  if (!sealed) throw new VaultError("passkey");
  const k = await derive(await evaluate(sealed.credential, sealed.salt));
  const record = await open(sealed, k);
  key = k;
  held = sealed;
  return record;
}

/** The passkey again, before a change that cannot be undone: it must verify
 * the user and still open what this browser holds. */
export async function verify(): Promise<void> {
  if (!held) throw new VaultError("passkey");
  await open(held, await derive(await evaluate(held.credential, held.salt)));
}

let saving: Promise<unknown> = Promise.resolve();
/** Seals a newer record in place, in the order records arrive. */
export function save(record: SyncRecord): Promise<void> {
  const k = key;
  const h = held;
  if (!k || !h) return Promise.resolve();
  const run = saving.then(async () => {
    const sealed = await seal(record, k, h.credential, h.salt);
    if (key !== k) return;
    await write(sealed);
    held = sealed;
  });
  saving = run.catch(() => undefined);
  return run;
}

/** Forgets the sealed record here.
 * A delete that does not commit rejects (`forget`) and keeps what this page held. Once deleted, the
 * passkey manager is told the passkey guards nothing (where the browser can). */
export async function clear(): Promise<void> {
  const [k, h] = [key, held];
  key = null;
  held = null;
  await saving;
  // Which passkey guarded it: held when unlocked, else read (a locked page never opened it).
  const entry = h ?? (await read().catch(() => undefined));
  try {
    await tx("readwrite", (s) => s.delete(ENTRY));
  } catch {
    if (key === null) [key, held] = [k, h];
    throw new VaultError("forget");
  }
  // Nothing sealed here any more: that passkey guards nothing.
  if (entry) void forgetPasskey(entry.credential);
}
