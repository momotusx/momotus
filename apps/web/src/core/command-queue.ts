/** Commands for a view that may still be opening. Until it opens they are
 * held in order; when it opens they go first, in that order, before any
 * later command; after that a command goes at once (synchronously), so a
 * last report sent as a view closes still leaves before the close. */
export function commandQueue<C>() {
  let target: ((c: C) => Promise<void>) | null = null;
  let dropped = false;
  const held: { c: C; done: (sent: Promise<void>) => void }[] = [];
  return {
    send(c: C): Promise<void> {
      if (target) return target(c);
      if (dropped) return Promise.resolve();
      return new Promise<void>((done) => held.push({ c, done }));
    },
    /** The view opened: held commands go first, in order (one held while
     * these go out lands after them), then every command goes at once. */
    open(send: (c: C) => Promise<void>) {
      for (let h = held.shift(); h; h = held.shift()) h.done(send(h.c));
      target = send;
    },
    /** The view will not open (it failed, or closed first): held and later commands are dropped. */
    drop() {
      dropped = true;
      for (let h = held.shift(); h; h = held.shift()) h.done(Promise.resolve());
    },
  };
}
