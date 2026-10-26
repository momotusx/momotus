import { Show } from "solid-js";

/**
 * The app's mark (design/logo, plans/owner-picks.md), sized by the font size it sits in. B: the owner's motmot,
 * then the name (Unbounded SemiBold, outlined, in mint), where the name helps; `alone` is C, the bird by itself,
 * where room is tight. `fill`: while the app starts, a faint copy with the solid one revealed from the left,
 * over and over (design/DECISION.md, Loading); solid under reduced motion.
 */
export default function Brand(props: { alone?: boolean; fill?: boolean }) {
  const art = () => (
    <>
      <img class="mk" src="/brand/mark.webp" alt="" />
      <Show when={!props.alone}>
        <img class="wm" src="/brand/wordmark.svg" alt="" />
      </Show>
    </>
  );
  return (
    <>
      <Show when={props.fill} fallback={art()}>
        <span class="brand-fill" aria-hidden="true">
          <span class="faint">{art()}</span>
          <span class="solid">{art()}</span>
        </span>
      </Show>
      {/* The boot's status already names the app. */}
      <Show when={!props.alone && !props.fill}>
        <span class="sr">Momotus</span>
      </Show>
    </>
  );
}
