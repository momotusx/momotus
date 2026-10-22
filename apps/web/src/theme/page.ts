/** Each form's page color (design/tokens.json, D10): what a backdrop's ramps fade to and the
 * browser bar takes. The CSS reads the same keys from tokens.css under `[data-form]`. */

import tokens from "../../../../design/tokens.json";
import type { Form } from "../form";

const { dark } = tokens.color;

export const pageColor = (form: Form) => (form === "phone" ? dark.bgPhone : form === "desktop" ? dark.bgDesktop : dark.bg);
