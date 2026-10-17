import { A } from "@solidjs/router";
import House from "lucide-solid/icons/house";
import Wrench from "lucide-solid/icons/wrench";
import type { JSX } from "solid-js";

/** A destination that is not built yet says so and offers a way on. */
export function Stub(props: { title: string; text: string; children?: JSX.Element }) {
  return (
    <main class="stub" data-vscroll>
      <h1>{props.title}</h1>
      <p>{props.text}</p>
      {props.children ?? (
        <div class="acts">
          <A class="btn primary" href="/" data-f="stub:home" data-autofocus>
            <House aria-hidden />
            Back to Home
          </A>
          <A class="btn" href="/welcome" data-f="stub:setup">
            <Wrench aria-hidden />
            Setup check
          </A>
        </div>
      )}
    </main>
  );
}
