/** Client for the core bridge protocol (see crates/mq-bridge). The same
 * messages travel over a WebSocket in the browser and over the desktop
 * shells' IPC channel, so only `Transport` differs between them. */

export interface Transport {
  send(text: string): void;
  onMessage(handler: (text: string) => void): void;
  onClose(handler: () => void): void;
}

/** `code` names an error the app acts on (sync, keystore); `kind` is what to
 * show (apps/shared/error-words.json, `said` for the core's own sentence);
 * `error` is the raw text, for the log and Details only. */
type Reply =
  | { id: number; ok: true; result: unknown }
  | { id: number; ok: false; error: string; kind?: string; code?: string; retryAfterMs?: number; views?: string[] };
type Push = { sub: number; updates: unknown[] };

export class BridgeError extends Error {
  constructor(
    message: string,
    /** The reply's structured `code`, unchanged, when it had one. */
    readonly code?: string,
    /** With `locked`: how long until the next PIN try is allowed. */
    readonly retryAfterMs?: number,
    /** With `viewsOpen`: the views still open. */
    readonly views?: string[],
    /** The reply's `kind`: what the app shows for it (`failureWords`). */
    readonly kind?: string,
  ) {
    super(message);
  }
}

interface Waiter {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
  /** Runs inside `receive`, before any later message of the same batch. */
  onResult?: (result: unknown) => void;
  /** A `subscribe`: its view may push before this reply arrives. */
  opens: boolean;
}

/** Most batches kept for one subscription whose reply has not come yet. */
const EARLY_LIMIT = 256;

export class Bridge {
  private nextId = 1;
  private pending = new Map<number, Waiter>();
  private listeners = new Map<number, (updates: unknown[]) => void>();
  // The core may push for a new view before its subscribe reply arrives, and a
  // transport may hand over the reply and pushes in one synchronous batch.
  // While a subscribe waits for its reply, a push with no listener may be its
  // view's, so it waits here by subscription id. Replies from the core and
  // from a desktop shell's host (pairing) keep no common order and their ids
  // share no range, so only the opens in flight decide: with none, a push
  // with no listener belongs to a closed view and is dropped.
  private early = new Map<number, unknown[][]>();
  private opening = 0;
  /** Who hears `{"profile": "gone", "id"}`: a profile removed here or by sync, whose keystore
   * entries the app drops. */
  private gone = new Set<(profile: string) => void>();
  /** The transport closed for good: every later request fails at once instead of waiting forever. */
  private closed = false;

  constructor(private transport: Transport) {
    transport.onMessage((text) => this.receive(text));
    transport.onClose(() => {
      this.closed = true;
      for (const { reject } of this.pending.values()) reject(new BridgeError("connection closed"));
      this.pending.clear();
      this.early.clear();
      this.opening = 0;
    });
  }

  private receive(text: string) {
    let message: Reply | Push;
    try {
      message = JSON.parse(text);
    } catch {
      return;
    }
    if ((message as { profile?: unknown }).profile === "gone") {
      const { id } = message as unknown as { id: string };
      for (const fn of this.gone) fn(id);
      return;
    }
    if ("sub" in message && "updates" in message) {
      const listener = this.listeners.get(message.sub);
      if (listener) listener(message.updates);
      else if (this.opening > 0) {
        const queue = this.early.get(message.sub) ?? [];
        if (queue.length < EARLY_LIMIT) queue.push(message.updates);
        this.early.set(message.sub, queue);
      }
      return;
    }
    const waiter = this.pending.get(message.id);
    if (!waiter) return;
    this.pending.delete(message.id);
    if (message.ok) {
      waiter.onResult?.(message.result);
      waiter.resolve(message.result);
    } else waiter.reject(new BridgeError(message.error, message.code, message.retryAfterMs, message.views, message.kind));
    // The last open settled: what still waits has no view to go to.
    if (waiter.opens && --this.opening === 0) this.early.clear();
  }

  /** Runs `fn` with each profile the core says is gone. Returns the way to stop listening. */
  onProfileGone(fn: (profile: string) => void): () => void {
    this.gone.add(fn);
    return () => this.gone.delete(fn);
  }

  /** Pushes on a subscription the host owns rather than the core (a desktop
   * shell sends player events on `0`). Returns the way to stop listening. */
  listen(sub: number, fn: (updates: unknown[]) => void): () => void {
    this.listeners.set(sub, fn);
    return () => this.listeners.delete(sub);
  }

  request<T>(op: string, fields: Record<string, unknown> = {}, onResult?: (result: T) => void): Promise<T> {
    if (this.closed) return Promise.reject(new BridgeError("connection closed"));
    const id = this.nextId++;
    const opens = op === "subscribe";
    if (opens) this.opening++;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject, onResult: onResult as Waiter["onResult"], opens });
      this.transport.send(JSON.stringify({ id, op, ...fields }));
    });
  }

  /** Opens a view. `snapshot` runs once with the opening state, then `updates`
   * with every batch in the order the core sent them, until `close`. Both run
   * while the reply is handled, so nothing pushed with or before it is lost. */
  async subscribe<S, U>(
    view: string,
    params: Record<string, unknown>,
    on: { snapshot: (snapshot: S) => void; updates: (updates: U[]) => void },
  ): Promise<{ sub: number; close: () => Promise<void>; command: (c: object) => Promise<void> }> {
    const { sub } = await this.request<{ sub: number; snapshot: S }>("subscribe", { view, params }, (r) => {
      on.snapshot(r.snapshot);
      const forward = (updates: unknown[]) => on.updates(updates as U[]);
      this.listeners.set(r.sub, forward);
      for (const updates of this.early.get(r.sub) ?? []) forward(updates);
      this.early.delete(r.sub);
    });
    return {
      sub,
      close: async () => {
        this.listeners.delete(sub);
        await this.request("unsubscribe", { sub }).catch(() => undefined);
      },
      command: async (command) => {
        await this.request("command", { sub, command });
      },
    };
  }
}

/** What a desktop shell injects at document start, top frame only. It holds
 * no secret: the document's token arrives with `ready`. */
export interface ShellInfo {
  platform: "linux" | "windows";
}

/** The shell's answer to `hello`, called in the main frame only. */
export interface ShellReady {
  /** The nonce this document's `hello` carried: a `ready` that lands in a
   * later document after a navigation carries the old one and is refused. */
  nonce: string;
  generation: number;
  token: string;
  platform: string;
  /** DeviceCaps for the shell's player (mpv), in the core's names. */
  caps?: unknown;
}

/** Where the shell reaches the page. `ready` returns whether this document
 * took it; the shell binds every later batch to the nonce of the `ready` that
 * returned true, and `recv` drops a batch bound to any other. */
export interface ShellHost {
  __mq?: { recv(nonce: string, batch: string[]): void; ready(r: ShellReady): boolean };
}

declare global {
  interface Window extends ShellHost {
    __MQ_SHELL__?: ShellInfo;
    webkit?: { messageHandlers?: { mq?: { postMessage(text: string): void } } };
    ipc?: { postMessage(text: string): void };
  }
}

/** Requests that may wait for `ready`; more means the shell is not answering. */
const PRE_READY_LIMIT = 256;

/** Desktop shells (plans/m3-desktop.md sections 2.2 and 3): requests go out
 * through the shell's message handler carrying the document's token; the
 * shell calls `__mq.recv` with batches bound to this document's nonce. Until
 * `ready`, only `hello` goes out and requests wait in order. */
export function ipcTransport(post: (text: string) => void, onReady?: (r: ShellReady) => void, host: ShellHost = window): Transport {
  // Made by this document, so only an answer to this document's hello counts.
  const nonce = crypto.randomUUID();
  let token: string | null = null;
  let closed = false;
  const waiting: string[] = [];
  let receive: (text: string) => void = () => {};
  const closers: (() => void)[] = [];
  const close = () => {
    if (closed) return;
    closed = true;
    waiting.length = 0;
    for (const c of closers) c();
  };
  // Every request is a JSON object: the token goes in as its first field.
  const withToken = (text: string) => `{"token":${JSON.stringify(token)}${text.length > 2 ? `,${text.slice(1)}` : "}"}`;
  host.__mq = {
    recv: (n, batch) => {
      // A batch the shell made for the previous document can run in this one
      // after a navigation, and request ids restart at 1 in every document:
      // it would answer this document's requests. Nothing of it is read.
      if (n !== nonce) return;
      for (const text of batch) receive(text);
    },
    ready: (r) => {
      // Only the answer to this document's hello, and only once: a repeat
      // (after a repeated hello) changes nothing.
      if (r.nonce !== nonce || token !== null || closed) return false;
      token = r.token;
      onReady?.(r);
      for (const text of waiting.splice(0)) post(withToken(text));
      return true;
    },
  };
  post(JSON.stringify({ op: "hello", nonce }));
  return {
    send(text) {
      if (closed) return;
      if (token !== null) return post(withToken(text));
      if (waiting.length >= PRE_READY_LIMIT) return close();
      waiting.push(text);
    },
    onMessage(handler) {
      receive = handler;
    },
    onClose(handler) {
      closers.push(handler);
    },
  };
}

/** The shell's message handler on this platform, or null in a browser. */
export function shellPost(host: Window = window): ((text: string) => void) | null {
  const linux = host.webkit?.messageHandlers?.mq;
  if (linux) return (text) => linux.postMessage(text);
  const win = host.ipc;
  if (win) return (text) => win.postMessage(text);
  return null;
}

/** WebSocket transport that queues sends until the socket opens. */
export function webSocketTransport(url: string): Transport {
  const socket = new WebSocket(url);
  const queue: string[] = [];
  socket.addEventListener("open", () => {
    for (const text of queue.splice(0)) socket.send(text);
  });
  return {
    send(text) {
      if (socket.readyState === WebSocket.OPEN) socket.send(text);
      else queue.push(text);
    },
    onMessage(handler) {
      socket.addEventListener("message", (event) => handler(String(event.data)));
    },
    onClose(handler) {
      socket.addEventListener("close", handler);
    },
  };
}
