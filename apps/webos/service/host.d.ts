// The contract between the webOS service and the core (plans/webos.md 1, 3.1
// to 3.5). L1 to L5 code against this file. The core child installs `mqHost`
// on `globalThis` before it requires the wasm glue; the glue's imports call it
// synchronously. No i64 crosses the boundary: offsets, sizes and times are
// numbers (f64), ids are u32.

/** Host imports the wasm calls (`js_namespace = mqHost`). */
export interface MqHost {
  // Files, synchronous, for the SQLite VFS only (3.3). Paths are absolute
  // under the data dir. Failures return -1 or false, never throw.
  fsOpen(path: string, create: boolean): number;
  fsExists(path: string): boolean;
  /** Reads into `buf` at `offset`; returns the bytes read (short at EOF). */
  fsRead(fd: number, buf: Uint8Array, offset: number): number;
  fsWrite(fd: number, buf: Uint8Array, offset: number): boolean;
  fsSync(fd: number): boolean;
  fsTruncate(fd: number, size: number): boolean;
  /** File size in bytes, or -1. */
  fsSize(fd: number): number;
  fsClose(fd: number): void;
  fsUnlink(path: string): boolean;

  // Timers (3.1): the host calls the export `fire(id)` once after `ms`.
  setTimer(ms: number, id: number): void;
  clearTimer(id: number): void;

  // Network (3.2): one hop, no redirect following; the host answers through
  // the export `fetched(id, response)` exactly once, or never after `abort`.
  resolve(id: number, host: string): void;
  interfaces(): string;
  fetch(id: number, request: string): void;
  abort(id: number): void;

  // Logs and death (3.1). `died` closes every descriptor `fsOpen` handed out,
  // sends `{ died }` to the parent and exits the child.
  log(level: number, line: string): void;
  died(message: string): void;
}

/** `fetch`'s request, JSON in the `request` string. */
export interface HostRequest {
  method: string;
  url: string;
  headers: [string, string][];
  /** base64, absent for no body. */
  body?: string;
  /** Approved addresses in order, IPv4 first; the host connects to these. */
  addresses: string[];
  timeoutMs: number;
  /** Cap on the decoded body. */
  maxBytes: number;
  /** For `get_prefix` (3.4): destroy the response after this many bytes. */
  prefixBytes?: number;
}

/** What `fetched` gets, JSON. `error` is set instead of `status` on failure. */
export interface HostResponse {
  status?: number;
  headers?: [string, string][];
  /** base64 of the decoded body (or prefix). */
  body?: string;
  truncated?: boolean;
  error?: 'timeout' | 'refused' | 'dns' | 'tls' | 'reset' | 'cap' | 'aborted';
}

/** Exports of the core module (`mq_wasm`, 3.5). */
export interface CoreModule {
  open(configJson: string): string;
  send(json: string): void;
  onMessage(cb: (json: string) => void): void;
  serve(path: string): Promise<ServeAnswer | null>;
  fire(id: number): void;
  fetched(id: number, responseJson: string): void;
  resolved(id: number, addressesJson: string): void;
  ping(): string;
}

export interface ServeAnswer {
  status: number;
  headers: [string, string][];
  body: Uint8Array;
}

/** `open`'s config (3.5). */
export interface CoreConfig {
  dataDir: string;
  deviceSeed: string;
  /** `process.version`, which maps one to one to the webOS generation. */
  node: string;
  /** Base for lease, subtitle and logo URLs: `http://127.0.0.1:47801`. */
  origin: string;
  /** Only with the `dev-hooks` feature. */
  dev?: Record<string, string>;
}

// Restart rules (section 1, core r4 and r5). The parent forks the child with
// `execArgv: []` and attaches handlers per child, so a late event from an old
// child never touches the new one. Socket text is queued until the new child
// sends `ready`. On `died`, `error` or an unexpected `exit` the parent sends
// SIGTERM, then SIGKILL after 2 s, waits for `exit`, and only then forks
// again (at most 3 times in 60 s). Before the first fork it clears a stale
// `core.pid` (only a process whose command line holds `core-worker`, 5 s at
// most). The child exits on `disconnect`, treats `unhandledRejection` and
// `uncaughtException` as `died`, and after `died` fails every file call.
// The host copies any wasm memory view it keeps past the call that gave it.
// The host never calls back into the core from inside the import call that
// started the work: `fetched`, `resolved` and `fire` always run from a later
// turn of the event loop (a bad request, no addresses and a synchronous
// failure are answered through `setImmediate` too), so the core is never
// re-entered while it still holds its own state.

/** Parent to child over the fork IPC channel (section 1). */
export type ToChild =
  | { kind: 'open'; config: CoreConfig }
  | { kind: 'socket'; text: string }
  | { kind: 'serve'; id: number; path: string };

/** Child to parent. */
export type ToParent =
  | { kind: 'ready'; open: string }
  | { kind: 'socket'; text: string }
  | { kind: 'served'; id: number; answer: { status: number; headers: [string, string][]; body: string } | null }
  | { kind: 'died'; message: string };

// Page and parent over the websocket (plans/webos.md 1 and 4). The first
// message on a socket is the token as plain text; a bad token closes the
// socket with `CLOSE_BAD_TOKEN` and the page calls `relaunch()`. Text that
// starts with `{"host":"keystore"` is the keystore protocol: only the parent
// reads it, only on an authenticated socket, and the core never sees it.
// Writes run one at a time in arrival order and are answered only after the
// new `secrets.json` is written to a temp file, fsynced, renamed and the
// directory fsynced.
export declare const CLOSE_BAD_TOKEN: 4401;

/** Keys: `debrid`, `sync`, `trakt:<profile id>`. */
export type KeystoreRequest =
  | { host: 'keystore'; id: number; op: 'read' }
  /** `value: null` deletes the key. */
  | { host: 'keystore'; id: number; op: 'write'; key: string; value: unknown };

export type KeystoreReply =
  /** `value` is the whole store, one object. */
  | { host: 'keystore'; id: number; ok: true; value?: Record<string, unknown> }
  | { host: 'keystore'; id: number | null; ok: false; error: string };
