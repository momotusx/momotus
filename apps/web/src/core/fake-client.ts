/** A stand-in for ./client in unit tests. Bun keeps the first `mock.module`
 * of a path for the whole run, so every test file mocks ./client with this
 * one and sets what `op` and `subscribe` do for its own tests. */

type Handlers = { snapshot: (s: unknown) => void; updates: (u: unknown[]) => void };
type Sub = { sub: number; command: (c: object) => Promise<void>; close: () => Promise<void> };

export const fake = {
  op: async (_name: string, _fields: Record<string, unknown>): Promise<unknown> => ({}),
  subscribe: async (_view: string, _params: unknown, _handlers: Handlers): Promise<Sub> => ({ sub: 0, command: async () => undefined, close: async () => undefined }),
};

export const clientMock = () => ({
  privateLink: true,
  op: (name: string, fields: Record<string, unknown> = {}) => fake.op(name, fields),
  bridge: { subscribe: (view: string, params: unknown, handlers: Handlers) => fake.subscribe(view, params, handlers) },
});
