/**
 * `@divvun/fst`: divvunspell's spelling and lookup for JavaScript, as
 * WebAssembly for browsers, Web Workers, Deno, Node and Bun. It loads BHFST
 * and ZHFST speller archives and checks and corrects words with them, and
 * looks words up in optimized-lookup, THFST and DHFST transducers, with the
 * results native divvun-fst gives.
 *
 * @module
 */
import { core } from "./binding.ts";
import type { InitSource } from "./core.ts";
import type { Logger, LogLevel } from "./logging.ts";
import { spellerReader } from "./speller.ts";
import { transducerReader } from "./transducer.ts";

// This module declares the whole public API. Results are plain values, and
// js/build.mjs fails a build whose declarations use any.
/**
 * Loads and starts the package's WebAssembly module. Call it once and wait
 * for it before anything else. With no argument it loads the `.wasm` file
 * that ships beside the package. Calling it again returns the same promise.
 */
export function init(source?: InitSource): Promise<void> {
  return core.init(source);
}

/** The divvun-fst version the package was built from. */
export function version(): string {
  return core.call((glue) => glue.version());
}

/**
 * Sends the library's log events to `logger`. `null` puts back the default,
 * which writes each event to the console method for its level.
 */
export function setLogger(logger: Logger | null): void {
  core.setLogger(logger);
}

/**
 * Logs events at `level` and the more severe levels; `off` logs nothing.
 * The default is `warn`. The module never sends the levels below it.
 */
export function setLogLevel(level: LogLevel): void {
  core.setLogLevel(level);
}

export const { readSpeller } = spellerReader(core);
export const { readTransducer } = transducerReader(core);
export { Speller } from "./speller.ts";
export type {
  AcceptorMetadata,
  ErrorModelMetadata,
  SpellerInfo,
  SpellerMetadata,
  SpellerTitle,
  Suggestion,
  WeightDetails,
} from "./speller.ts";
export { Transducer } from "./transducer.ts";
export type { LookupOptions, LookupResult, ReadTransducerOptions, TransducerFormat } from "./transducer.ts";
export { defaultSpellerConfig } from "./config.ts";
export type { PartialSpellerConfig, ReweightingConfig, SpellerConfig } from "./config.ts";
export { FstError } from "./errors.ts";
export type { FstErrorCode } from "./errors.ts";
export type { InitSource } from "./core.ts";
export type { LogEvent, Logger, LogLevel } from "./logging.ts";
