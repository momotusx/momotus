/** Customize Home: which addon catalogs Home shows, and in what order. The
 * list comes from `homeCatalogs` when the screen opens and from the reply to
 * every edit; Home itself follows the change live. On TV a row is picked up
 * with select, moved with up and down, and dropped with select or Back. */

import { A } from "@solidjs/router";
import ArrowDown from "lucide-solid/icons/arrow-down";
import ArrowUp from "lucide-solid/icons/arrow-up";
import Eye from "lucide-solid/icons/eye";
import EyeOff from "lucide-solid/icons/eye-off";
import { createSignal, For, onMount, Show } from "solid-js";
import { createStore, reconcile } from "solid-js/store";
import { op, reasonOf } from "../core/client";
import type { HomeCatalog } from "../core/generated/HomeCatalog";
import { focusEl } from "../focus/dom";
import { form } from "../form";
import "./browse.css";
import "./settings.css";

const BACK_CODES = new Set([10009, 461]);
const isBack = (e: KeyboardEvent) => e.key === "Escape" || e.key === "GoBack" || e.key === "Backspace" || BACK_CODES.has(e.keyCode);
const key = (rowId: string) => `hrow:${rowId}`;
const rowEl = (rowId: string) => [...document.querySelectorAll<HTMLElement>("[data-f]")].find((el) => el.dataset.f === key(rowId)) ?? null;

export default function HomeRows() {
  const [rows, setRows] = createStore<{ list: HomeCatalog[]; loaded: boolean }>({ list: [], loaded: false });
  const [error, setError] = createSignal<string | null>(null);
  const [moving, setMoving] = createSignal<string | null>(null);
  const apply = (list: HomeCatalog[]) => {
    setRows("list", reconcile(list, { key: "rowId" }));
    setRows("loaded", true);
  };
  // Edits run one at a time, in the order pressed, each applying its reply.
  let queue: Promise<unknown> = Promise.resolve();
  const edit = (name: string, fields: Record<string, unknown>, after?: () => void) => {
    queue = queue
      .then(() => op<HomeCatalog[]>(name, fields))
      .then((list) => {
        setError(null);
        apply(list);
        after?.();
      })
      .catch((e) => setError(reasonOf(e)));
  };
  onMount(() => {
    op<HomeCatalog[]>("homeCatalogs")
      .then(apply)
      .catch((e) => {
        setError(reasonOf(e));
        setRows("loaded", true);
      });
  });

  // Reordering moves the row's element, which loses focus: put it back.
  const refocus = (rowId: string) =>
    queueMicrotask(() => {
      const el = rowEl(rowId);
      if (el && document.activeElement !== el) focusEl(el);
    });
  const move = (rowId: string, by: number) => edit("moveHomeRow", { rowId, by }, () => refocus(rowId));
  const toggle = (row: HomeCatalog) => edit("setHomeRow", { rowId: row.rowId, hidden: !row.hidden });

  const onKey = (e: KeyboardEvent, rowId: string) => {
    if (moving() !== rowId) return;
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      e.stopPropagation();
      move(rowId, e.key === "ArrowUp" ? -1 : 1);
    } else if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      // A picked-up row stays put until it is dropped.
      e.preventDefault();
      e.stopPropagation();
    } else if (isBack(e)) {
      // Back drops the row; it does not leave the screen.
      e.preventDefault();
      e.stopPropagation();
      setMoving(null);
    }
  };

  const phone = () => form() === "phone";
  return (
    <main class="browse page settings-page column home-rows" data-vscroll>
      <h1>Home rows</h1>
      <p class="lede">
        Continue Watching always comes first. Choose which catalogs follow it, and their order.
        <Show when={!phone()}> Select a row to move it with up and down, then select again to drop it.</Show>
      </p>
      <Show when={error()}>
        <p class="st bad" role="status">
          <i class="dot" />
          {error()}
        </p>
      </Show>
      <Show when={rows.loaded && rows.list.length === 0 && !error()}>
        <p class="note">
          No catalogs yet. Addons with catalogs add rows to Home: <A href="/addons">install one</A>.
        </p>
      </Show>
      <ol class="hrows">
        <For each={rows.list}>
          {(row, i) => (
            <li class="hrow" classList={{ off: row.hidden, moving: moving() === row.rowId }}>
              <Show
                when={!phone()}
                fallback={
                  <div class="hrow-main">
                    <RowText row={row} n={i() + 1} />
                  </div>
                }
              >
                <button
                  class="hrow-main"
                  data-f={key(row.rowId)}
                  data-autofocus={i() === 0 ? "" : undefined}
                  aria-pressed={moving() === row.rowId}
                  aria-label={`${row.title}, ${row.source}, ${row.hidden ? "hidden" : "shown"}, row ${i() + 1} of ${rows.list.length}`}
                  onClick={() => setMoving(moving() === row.rowId ? null : row.rowId)}
                  onKeyDown={(e) => onKey(e, row.rowId)}
                  onBlur={(e) => {
                    // Focus moved elsewhere by the user (not by a reorder): drop.
                    if (moving() === row.rowId && e.relatedTarget) setMoving(null);
                  }}
                >
                  <RowText row={row} n={i() + 1} />
                  <Show when={moving() === row.rowId}>
                    <span class="hrow-hint">Up or down to move, select to drop</span>
                  </Show>
                </button>
              </Show>
              <div class="hrow-acts">
                <button class="btn" data-f={`${key(row.rowId)}:shown`} aria-pressed={!row.hidden} onClick={() => toggle(row)}>
                  <Show when={row.hidden} fallback={<Eye aria-hidden />}>
                    <EyeOff aria-hidden />
                  </Show>
                  {row.hidden ? "Hidden" : "Shown"}
                </button>
                <button class="btn hrow-move" data-f={`${key(row.rowId)}:up`} aria-disabled={i() === 0} onClick={() => i() > 0 && move(row.rowId, -1)}>
                  <ArrowUp aria-hidden />
                  Up
                </button>
                <button
                  class="btn hrow-move"
                  data-f={`${key(row.rowId)}:down`}
                  aria-disabled={i() === rows.list.length - 1}
                  onClick={() => i() < rows.list.length - 1 && move(row.rowId, 1)}
                >
                  <ArrowDown aria-hidden />
                  Down
                </button>
              </div>
            </li>
          )}
        </For>
      </ol>
    </main>
  );
}

/** The row's title, then its addon as a quiet second line: the one place outside the Addons
 * pages that names an addon, so two rows of one title can be told apart (design/DECISION.md). */
function RowText(props: { row: HomeCatalog; n: number }) {
  return (
    <>
      <span class="hrow-n num">{props.n}</span>
      <span class="hrow-text">
        <b>{props.row.title}</b>
        <small>
          {props.row.source}
          <Show when={props.row.hidden}> · Hidden from Home</Show>
        </small>
      </span>
    </>
  );
}
