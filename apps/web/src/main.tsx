import "@fontsource-variable/inter/index.css";
import "@fontsource/ibm-plex-mono/latin-400.css";
import "./theme/tokens.css";
import "./theme/app.css";
import { render } from "solid-js/web";
import { App } from "./app";
import { markBack } from "./history";
import { startFitting } from "./form";

// Registered before the router's own listener so a browser Back is known
// before the new screen asks whether to restore focus.
window.addEventListener("popstate", markBack);
startFitting();
const root = document.getElementById("root") as HTMLElement;
// The page's own wordmark (index.html) gives way to the app's, which stays
// until the core has said who is watching.
root.replaceChildren();
render(() => <App />, root);
