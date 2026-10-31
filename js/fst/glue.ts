// The generated wasm-bindgen glue, as the wrapper uses it. tsc checks at
// build time that the generated module satisfies these types when binding.ts
// hands it to Core. Declaring them here also lets deno check the wrapper on
// a checkout that has no build output. Structured results cross as JSON text,
// in the shapes the wrapper's declarations give them.

/** What the generated init function is given. The wrapper resolves URLs itself. */
export type GlueInitInput = BufferSource | Response | WebAssembly.Module;

/**
 * What the module calls back into, from `crates/divvun-fst-wasm/src/sink.rs`.
 * Levels are numbers: 0 is off, then 1 error, 2 warn, 3 info, 4 debug, 5 trace.
 * These calls arrive while the module is running, so none of them may throw:
 * an exception would unwind through the module and leave it half run.
 */
export interface GlueSink {
  /** One tracing event, which the module has already checked against the level. */
  log(level: number, target: string, message: string): void;
  /** A panic's message. The trap that ends the call follows at once. */
  panic(message: string): void;
}

/**
 * An object the glue hands out, which owns module memory until `free()`.
 * wasm-bindgen also frees one from its own `FinalizationRegistry` when the
 * object becomes unreachable, which the wrapper must never let happen after a
 * trap; see `Core`.
 */
export interface GlueObject {
  free(): void;
}

/**
 * A speller the glue holds, from `crates/divvun-fst-wasm/src/speller.rs`.
 * `config` is a config's JSON text, or undefined for the speller's own.
 */
export interface SpellerGlue extends GlueObject {
  /** The archive's metadata as JSON text, or undefined when it has none. */
  metadata(): string | undefined;
  /** The archive's bundled config as JSON text, or undefined when it has none. */
  bundledConfig(): string | undefined;
  isCorrect(word: string, config: string | undefined): boolean;
  /** Suggestions, as the JSON text of an array. */
  suggest(word: string, config: string | undefined): string;
  analyzeInput(word: string, config: string | undefined): string;
  analyzeOutput(word: string, config: string | undefined): string;
}

/** A transducer the glue holds, from `crates/divvun-fst-wasm/src/transducer.rs`. */
export interface TransducerGlue extends GlueObject {
  /** The name of its format, one the wrapper lists as a transducer format. */
  format(): string;
  /** The results for `word`, as the JSON text of an array, at most `limit` of them. */
  lookup(word: string, limit: number | undefined): string;
}

/** The exports of `crates/divvun-fst-wasm`. */
export interface Glue {
  /** Instantiates the module. The generated glue keeps the instance it makes. */
  default(options: { module_or_path: GlueInitInput }): Promise<unknown>;
  /** Starts the instance with the wrapper's sink and a log level. Called once, by `init()`. */
  install(sink: GlueSink, level: number): void;
  /** Sends events at `level` and above from here on, and nothing below it. */
  setLogLevel(level: number): void;
  version(): string;
  /** The speller in a BHFST or ZHFST archive's bytes. */
  readSpeller(bytes: Uint8Array): SpellerGlue;
  /** The transducer in `bytes`, or the box member `member` names. */
  readTransducer(bytes: Uint8Array, member: string | undefined): TransducerGlue;
}
