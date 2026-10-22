/** The subtitle Timing row, as Stremio's stepper (lib/delay.ts): minus, the
 * value, plus, and a Reset by the label while it is not 0; a tap on the value
 * resets too. A press is 0.1 s; held, it repeats and speeds up. On TV the
 * value takes the focus and the remote's held Left and Right move it by the
 * same rules. */

import Minus from "lucide-solid/icons/minus";
import Plus from "lucide-solid/icons/plus";
import { onCleanup, Show } from "solid-js";
import { form } from "../form";
import { DELAY_STEP, delayText, delayWords, FIRST_REPEAT_MS, holdStep, nudge, REPEAT_MS } from "../lib/delay";

export function DelayStepper(props: { value: number; set: (sec: number) => void }) {
  const tv = () => form() === "tv";
  const step = (by: number) => props.set(nudge(props.value, by));
  let val!: HTMLButtonElement;
  // Reset leaves with the 0 it made: the focus it held moves to the value.
  const reset = (e: MouseEvent) => {
    const held = e.currentTarget === document.activeElement;
    props.set(0);
    if (held) val.focus();
  };

  // One hold at a time, on the button it began on (its blur ends it, not the
  // blur a press on another button causes): a release before the first repeat is one step.
  let timer: ReturnType<typeof setTimeout> | undefined;
  let holder: EventTarget | null = null;
  let dir = 0;
  let since = 0;
  let repeated = false;
  const stop = (single: boolean) => {
    clearTimeout(timer);
    if (dir && single && !repeated) step(dir * DELAY_STEP);
    dir = 0;
  };
  const start = (d: number, on: EventTarget | null) => {
    stop(false);
    holder = on;
    dir = d;
    since = performance.now();
    repeated = false;
    const tick = () => {
      repeated = true;
      step(d * holdStep(performance.now() - since));
      timer = setTimeout(tick, REPEAT_MS);
    };
    timer = setTimeout(tick, FIRST_REPEAT_MS);
  };
  onCleanup(() => clearTimeout(timer));

  /** Minus or plus: the pointer and Enter or Space hold by the stepper's rules (not the
   * system's key repeat); a click with no press behind it (a screen reader's) is one step. */
  const blur = (e: FocusEvent) => e.currentTarget === holder && stop(false);
  const holds = (d: number) => ({
    onPointerDown: (e: PointerEvent & { currentTarget: HTMLElement }) => {
      if (e.button !== 0) return;
      e.currentTarget.setPointerCapture(e.pointerId);
      start(d, e.currentTarget);
    },
    onPointerUp: () => stop(true),
    onPointerCancel: () => stop(false),
    onContextMenu: (e: MouseEvent) => e.preventDefault(),
    onKeyDown: (e: KeyboardEvent) => {
      if (e.key !== "Enter" && e.key !== " ") return;
      e.preventDefault();
      if (!e.repeat) start(d, e.currentTarget);
    },
    onKeyUp: (e: KeyboardEvent) => {
      if (e.key !== "Enter" && e.key !== " ") return;
      e.preventDefault();
      stop(true);
    },
    onBlur: blur,
    onClick: (e: MouseEvent) => e.detail === 0 && step(d * DELAY_STEP),
  });

  return (
    <div class="sub-delay" role="group" aria-label="Subtitle timing">
      <span>Timing</span>
      {/* By the label, so minus and plus stay put while it comes and goes. */}
      <Show when={props.value !== 0}>
        <button class="sub-reset" data-f="sub:reset" onClick={reset}>
          Reset
        </button>
      </Show>
      {/* On TV the value holds the focus; these stay for a pointer remote. */}
      <button class="sub-step" data-f={tv() ? undefined : "sub:earlier"} tabIndex={tv() ? -1 : undefined} aria-label="Show subtitles 0.1 seconds earlier" {...holds(-1)}>
        <Minus aria-hidden />
      </button>
      <button
        ref={val}
        class="sub-val num"
        data-f="sub:delay"
        aria-label={props.value === 0 ? delayWords(0) : `${delayWords(props.value)}, press to reset`}
        onClick={() => props.set(0)}
        onKeyDown={(e) => {
          if (!tv() || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
          e.preventDefault();
          if (!e.repeat) start(e.key === "ArrowLeft" ? -1 : 1, e.currentTarget);
        }}
        onKeyUp={(e) => {
          if (!tv() || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
          e.preventDefault();
          stop(true);
        }}
        onBlur={blur}
      >
        {delayText(props.value)}
      </button>
      <button class="sub-step" data-f={tv() ? undefined : "sub:later"} tabIndex={tv() ? -1 : undefined} aria-label="Show subtitles 0.1 seconds later" {...holds(1)}>
        <Plus aria-hidden />
      </button>
      <span class="sr" aria-live="polite">
        {delayWords(props.value)}
      </span>
    </div>
  );
}
