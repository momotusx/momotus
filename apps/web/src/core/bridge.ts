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

export class Bridge {
  private nextId = 1;
  private pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();
  private listeners = new Map<number, (updates: unknown[]) => void>();

  constructor(private transport: Transport) {
    transport.onMessage((text) => this.receive(text));
    transport.onClose(() => {
      for (const { reject } of this.pending.values()) reject(new BridgeError("connection closed"));
      this.pending.clear();
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
      this.listeners.get(message.sub)?.(message.updates);
      return;
    }
    const waiter = this.pending.get(message.id);
    if (!waiter) return;
    this.pending.delete(message.id);
    if (message.ok) waiter.resolve(message.result);
    else waiter.reject(new BridgeError(message.error));
  }

  request<T>(op: string, fields: Record<string, unknown> = {}): Promise<T> {
    const id = this.nextId++;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.transport.send(JSON.stringify({ id, op, ...fields }));
    });
  }

  /** Opens a view; `onUpdates` receives coalesced batches until `close`. */
  async subscribe<S, U>(
    view: string,
    params: Record<string, unknown>,
    onUpdates: (updates: U[]) => void,
  ): Promise<{ sub: number; snapshot: S; close: () => Promise<void>; command: (c: object) => Promise<void> }> {
    const { sub, snapshot } = await this.request<{ sub: number; snapshot: S }>("subscribe", { view, params });
    this.listeners.set(sub, (updates) => onUpdates(updates as U[]));
    return {
      sub,
      snapshot,
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
