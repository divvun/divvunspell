// What deno check sees in place of pkg/divvun_fst_wasm.js, which only exists
// after a build. js/deno.json maps that path here. tsc compiles against the
// real one.
import type { Glue } from "./glue.ts";

declare const init: Glue["default"];
export default init;
export declare const install: Glue["install"];
export declare const setLogLevel: Glue["setLogLevel"];
export declare const version: Glue["version"];
export declare const readSpeller: Glue["readSpeller"];
export declare const readTransducer: Glue["readTransducer"];
