/** The addon install sheet (plans/addon-install-sheet.md): nothing installs from a list or a pasted
 * link until the person has seen what the addon is, where it comes from and what it adds, and
 * pressed Install here. A page sheet on the phone, a centered card on desktop and TV; on TV Cancel
 * comes first and takes focus (DECISION "Confirmation"). Back, Escape and the scrim cancel. */

import { createSignal, createUniqueId, onCleanup, onMount, Show } from "solid-js";
import { Portal } from "solid-js/web";
import { pushBack } from "../focus/back";
import { focusEl } from "../focus/dom";
import { holdModal } from "../focus/modal";
import { form } from "../form";
import { addsLine, sheetFrom, sheetTitle, sheetWords } from "../lib/install-sheet";
import "./install-sheet.css";

export type SheetAddon = { name: string; host: string; provides: string[]; types: string[] };

const ARM_MS = 400;

/** `words` replaces the addon words for another kind of install (a plugin repository). */
export type SheetWords = { title: string; from: string; adds: string; note: string; install: string };

export function InstallSheet(props: { addon: SheetAddon; onInstall: () => void; onCancel: () => void; words?: SheetWords }) {
  let root!: HTMLDivElement;
  const id = createUniqueId();
  const tv = () => form() === "tv";
  const adds = () => props.words?.adds ?? addsLine(props.addon.provides, props.addon.types);
  // One answer per sheet: a double click installs once. Install is armed a moment after the
  // sheet opens, so the press that opened it (a double tap, a held Enter) never answers it.
  let answered = false;
  const [armed, setArmed] = createSignal(false);
  const arming = setTimeout(() => setArmed(true), ARM_MS);
  onCleanup(() => clearTimeout(arming));
  // Escape and Back cancel at once; a press (Install, Cancel, the scrim) counts once armed, so the
  // second click of a double click that opened the sheet does not close it again.
  const answer = (install: boolean) => {
    if (answered) return;
    answered = true;
    if (install) props.onInstall();
    else props.onCancel();
  };
  const press = (install: boolean) => armed() && answer(install);
  holdModal(() => root);
  onMount(() => {
    // Its own history entry: the browser's Back cancels too, and a button's answer drops the entry.
    onCleanup(pushBack(() => answer(false), { entry: true }));
    queueMicrotask(() => {
      const first = root.querySelector<HTMLElement>(`[data-f="sheet:${tv() ? "cancel" : "install"}"]`);
      if (first) focusEl(first);
    });
  });
  const cancel = (
    <button type="button" class="btn" classList={{ primary: tv() }} data-f="sheet:cancel" onClick={() => press(false)}>
      {sheetWords.cancel}
    </button>
  );
  const install = (
    <button type="button" class="btn" classList={{ keyfill: !tv() }} data-f="sheet:install" onClick={() => press(true)}>
      {props.words?.install ?? sheetWords.install}
    </button>
  );
  return (
    <Portal mount={document.querySelector(".app") ?? document.body}>
      <div
        class="is-layer"
        ref={root}
        role="dialog"
        aria-modal="true"
        data-armed={armed() ? "" : undefined}
        aria-labelledby={`${id}-title`}
        aria-describedby={[adds() ? `${id}-adds` : "", `${id}-note`].filter(Boolean).join(" ")}
        // A held key repeats: only its first press counts, so a held Enter or OK never installs.
        onKeyDown={(e) => {
          if (e.repeat && (e.key === "Enter" || e.key === " ")) {
            e.preventDefault();
            e.stopPropagation();
          }
        }}
      >
        <div class="is-scrim" aria-hidden="true" onClick={() => press(false)} />
        <section class="is-card" data-fgroup="sheet">
          <Show when={form() === "phone"}>
            <i class="is-grabber" aria-hidden="true" />
          </Show>
          <h2 id={`${id}-title`} dir="auto">
            {props.words?.title ?? sheetTitle(props.addon.name)}
          </h2>
          <Show when={adds()}>
            <p class="is-adds" id={`${id}-adds`}>{adds()}</p>
          </Show>
          <p class="is-note" id={`${id}-note`}>
            {props.words?.note ?? sheetWords.note}
          </p>
          {/* Where it comes from is a raw host: only behind Details, never on a TV. */}
          <Show when={!tv()}>
            <details class="why-raw is-details">
              <summary data-f="sheet:details">Details</summary>
              <p class="is-from">{props.words?.from ?? sheetFrom(props.addon.host)}</p>
            </details>
          </Show>
          <div class="is-acts">{tv() ? [cancel, install] : form() === "phone" ? [install, cancel] : [cancel, install]}</div>
        </section>
      </div>
    </Portal>
  );
}
