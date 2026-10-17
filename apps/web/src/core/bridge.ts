/** Client for the core bridge protocol (see crates/mq-bridge). The same
 * messages travel over a WebSocket in the browser and over the desktop
 * shells' IPC channel, so only `Transport` differs between them. */

export interface Transport {
  send(text: string): void;
  onMessage(handler: (text: string) => void): void;
  onClose(handler: () => void): void;
}

type Reply = { id: number; ok: true; result: unknown } | { id: number; ok: false; error: string };
type Push = { sub: number; updates: unknown[] };

export class BridgeError extends Error {}

interface Waiter {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
  /** Runs inside `receive`, before any later message of the same batch. */
  onResult?: (result: unknown) => void;
}

/** Most batches kept for one subscription whose reply has not come yet. */
const EARLY_LIMIT = 256;

export class Bridge {
  private nextId = 1;
  private pending = new Map<number, Waiter>();
  private listeners = new Map<number, (updates: unknown[]) => void>();
  // The core may push for a new view before its subscribe reply arrives, and a
  // transport may hand over the reply and pushes in one synchronous batch.
  // Pushes for an id above every registered one wait here for their reply;
  // ids only grow, so a push at or below that mark with no listener belongs
  // to a closed view and is dropped.
  private early = new Map<number, unknown[][]>();
  private highest = 0;

  constructor(private transport: Transport) {
    transport.onMessage((text) => this.receive(text));
    transport.onClose(() => {
      for (const { reject } of this.pending.values()) reject(new BridgeError("connection closed"));
      this.pending.clear();
      this.early.clear();
    });
  }

  private receive(text: string) {
    let message: Reply | Push;
    try {
      message = JSON.parse(text);
    } catch {
      return;
    }
    if ("sub" in message && "updates" in message) {
      const listener = this.listeners.get(message.sub);
      if (listener) listener(message.updates);
      else if (message.sub > this.highest) {
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
    } else waiter.reject(new BridgeError(message.error));
  }

  request<T>(op: string, fields: Record<string, unknown> = {}, onResult?: (result: T) => void): Promise<T> {
    const id = this.nextId++;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject, onResult: onResult as Waiter["onResult"] });
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
      this.highest = Math.max(this.highest, r.sub);
      for (const updates of this.early.get(r.sub) ?? []) forward(updates);
      // Replies come in request order: anything older than this id is stale.
      for (const id of this.early.keys()) if (id <= r.sub) this.early.delete(id);
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
