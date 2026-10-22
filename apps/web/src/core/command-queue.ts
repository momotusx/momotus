/** Commands for a view that may still be opening. Until it opens they are
 * held in order; when it opens they go first, in that order, before any
 * later command; after that a command goes at once (synchronously), so a
 * last report sent as a view closes still leaves before the close. Once the
 * view closes, a command is a no-op: its subscription is gone. */
export function commandQueue<C>() {
  let target: ((c: C) => Promise<void>) | null = null;
  let dropped = false;
  const held: { c: C; done: (sent: Promise<void>) => void }[] = [];
  return {
    send(c: C): Promise<void> {
      if (dropped) return Promise.resolve();
      if (target) return target(c);
      return new Promise<void>((done) => held.push({ c, done }));
    },
    /** The view opened: held commands go first, in order (one held while
     * these go out lands after them), then every command goes at once. */
    open(send: (c: C) => Promise<void>) {
      for (let h = held.shift(); h; h = held.shift()) h.done(send(h.c));
      target = send;
    },
    /** The view failed or closed: held and later commands are dropped. */
    drop() {
      dropped = true;
      for (let h = held.shift(); h; h = held.shift()) h.done(Promise.resolve());
    },
  };
}
