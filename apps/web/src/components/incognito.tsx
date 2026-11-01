/** Incognito's own screens and notices (plans/batch-10-incognito.md 5 and 6): its mark, the intro on every entry, the
 * desktop's one-minute warning and the ended notice over Home. */

import HatGlasses from "lucide-solid/icons/hat-glasses";
import { createSignal, For, onCleanup, onMount, Show } from "solid-js";
import { debridAccounts } from "../core/debrid";
import { reasonOf } from "../core/client";
import { ackIncognitoEnded, closeIntro, endsSoon, leaveIncognito, profiles } from "../core/profiles";
import { pushBack } from "../focus/back";
import { autoFocusSoon } from "../focus/dom";
import { form } from "../form";
import { stayIncognito } from "../incognito";
import "./incognito.css";

/** The words of Incognito (section 6), one place for every screen. */
export const incognitoWords = {
  name: "Incognito",
  opening: "Opening Incognito",
  leave: "Leave Incognito",
  kept: "Nothing you watch here is kept.",
  ended: "Incognito ended. Its history and addons were cleared.",
  addonsNote: "Changes here last until you leave Incognito.",
  downloadNote: "This download stays on this device. Every profile can see it in Downloads.",
  libraryEmpty: "Nothing here yet. Titles you add in Incognito are gone when you leave.",
  trailersOff: "Off in Incognito.",
  debrid: "Uses this device's debrid account",
  pinTitle: (name: string) => `Enter the PIN for ${name} to open Incognito`,
  kidsReason: "Kids profiles use this device, so Incognito needs this PIN.",
  endsSoon: "Incognito ends in 1 minute.",
  stay: "Stay in Incognito",
};

/** The intro's lines for this form; `debrid` adds the debrid account's line. */
export function introLines(f: ReturnType<typeof form>, debrid: boolean) {
  return {
    notKept: ["What you watch and search.", "Addons and settings you change."],
    seen: [
      "Downloads stay on this device, and every profile can see them.",
      ...(debrid ? ["Your debrid account keeps its own history."] : []),
      "Addons, your internet provider and torrent peers can still see what you play.",
      "Your browser may keep images it showed.",
    ],
    ends:
      f === "tv"
        ? "It ends when you leave, or after 30 minutes away."
        : f === "desktop"
          ? "It ends when you leave, close this tab, or after 30 minutes without use."
          : "It ends when you leave, close the app, or after 30 minutes away.",
  };
}

/** The in-session mark: a sea-blue circle holding the white hat-glasses glyph, in the avatar's place and size. */
export function IncognitoMark(props: { size?: "s" | "m" | "l" | "intro" }) {
  return (
    <span class={`avatar avatar-${props.size ?? "m"} incog-mark`} aria-hidden="true">
      <HatGlasses aria-hidden />
    </span>
  );
}

/** Leave Incognito, wherever it is offered: back to where it was entered, without a PIN. A failure says so in place. */
export function useLeave() {
  const [problem, setProblem] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  const leave = () => {
    if (busy()) return;
    setBusy(true);
    setProblem(null);
    leaveIncognito()
      .catch((e: unknown) => setProblem(reasonOf(e)))
      .finally(() => setBusy(false));
  };
  return { leave, problem, busy };
}

/** The intro, on every entry (5.1): what is not kept, what is still kept or seen, and how it ends. */
export function IncognitoIntro() {
  const { leave, problem, busy } = useLeave();
  const lines = () => introLines(form(), (debridAccounts()?.accounts.length ?? 0) > 0);
  let cont!: HTMLButtonElement;
  let heading!: HTMLHeadingElement;
  // TV starts on Continue. Elsewhere the heading takes the focus at once, whatever the last input was, so a screen
  // reader reads where it is before any button (a heading shows no ring).
  onMount(() => (form() === "tv" ? autoFocusSoon(() => cont) : heading.focus({ preventScroll: true })));
  // Back on TV leaves.
  if (form() === "tv") onCleanup(pushBack(leave));
  const group = (title: string, items: string[]) => (
    <section class="incog-group" aria-label={title}>
      <h2>{title}</h2>
      <ul>
        <For each={items}>{(line) => <li>{line}</li>}</For>
      </ul>
    </section>
  );
  return (
    <main class="incog-intro page" data-vscroll>
      <IncognitoMark size="intro" />
      <h1 tabindex="-1" ref={heading}>
        You're Incognito
      </h1>
      <div class="incog-groups">
        {group("Not kept", lines().notKept)}
        {group("Still kept or seen", lines().seen)}
      </div>
      <p class="incog-ends">{lines().ends}</p>
      <div class="acts" data-fgroup="incognito-intro">
        <button class="btn primary" ref={cont} data-f="incognito:continue" data-autofocus={form() === "tv" ? "" : undefined} onClick={closeIntro}>
          Continue
        </button>
        <button class="btn" data-f="incognito:leave" aria-disabled={busy()} onClick={leave}>
          {incognitoWords.leave}
        </button>
      </div>
      <p class="st bad" role="status">
        {problem()}
      </p>
    </main>
  );
}

/** The desktop's warning a minute before Incognito ends (2.7): announced once, its button reached by Tab and never
 * taking the focus by itself. */
export function IncognitoWarning() {
  return (
    <Show when={endsSoon()}>
      <div class="incog-notice" role="alert">
        <p>{incognitoWords.endsSoon}</p>
        {/* No focus key (data-f): the focus system would hand a lost focus to a later warning's button, and this button
            never takes the focus by itself. The warning is the desktop's, where Tab reaches it. */}
        <button class="btn incog-stay" onClick={stayIncognito}>
          {incognitoWords.stay}
        </button>
      </div>
    </Show>
  );
}

/** Incognito ended while this profile shows no chooser (a single profile's Home, 5.4): the line stays, across reloads
 * too, until OK says it was read. */
export function EndedNotice() {
  const [problem, setProblem] = createSignal<string | null>(null);
  return (
    <Show when={profiles()?.incognitoEnded}>
      <div class="incog-notice" role="status">
        <p>{problem() ?? incognitoWords.ended}</p>
        <button class="btn" data-f="incognito:ended-ok" onClick={() => ackIncognitoEnded().catch((e: unknown) => setProblem(reasonOf(e)))}>
          OK
        </button>
      </div>
    </Show>
  );
}
