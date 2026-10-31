import { Core } from "./core.ts";
import * as glue from "./pkg/divvun_fst_wasm.js";

/** The package's module instance. Making it loads nothing. */
export const core: Core = new Core(glue, new URL("./pkg/divvun_fst_wasm_bg.wasm", import.meta.url));
