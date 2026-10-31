/**
 * Every code an {@link FstError} can carry. Code may test `error.code`
 * against these; they do not change between releases.
 *
 * From the wrapper:
 * - `not-initialised`: a call came before `init()` had resolved.
 * - `init-failed`: `init()` could not load, compile or start the module.
 * - `poisoned`: the module trapped, for instance on a panic. Its memory cannot
 *   be trusted after that, so every later call throws this without entering it.
 * - `freed`: a call on an object after its `free()`.
 *
 * From the wrapper or the library, which `crates/divvun-fst-wasm/src/error.rs`
 * maps here:
 * - `invalid-argument`: an argument the call cannot use, such as a word that
 *   is not a string, a config field divvun-fst does not have or a value of the
 *   wrong type, or a box member that is not there.
 * - `unsupported-format`: bytes in a format the package does not read, such
 *   as an archive that is neither BHFST nor ZHFST. The message says what the
 *   bytes start with.
 * - `invalid-format`: bytes in a format the package reads that are not well
 *   formed in it: truncated tables, a missing member, broken metadata.
 * - `internal`: a failure inside the library that no input should cause.
 * - `failed`: any other failure the library reports; the message says what.
 */
export const FST_ERROR_CODES = [
  "not-initialised",
  "init-failed",
  "poisoned",
  "freed",
  "invalid-argument",
  "unsupported-format",
  "invalid-format",
  "internal",
  "failed",
] as const;

export type FstErrorCode = (typeof FST_ERROR_CODES)[number];

/** An error from `@divvun/fst`. `code` says what went wrong, in a form code can test. */
export class FstError extends Error {
  readonly code: FstErrorCode;

  constructor(code: FstErrorCode, message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "FstError";
    this.code = code;
  }
}

function isFstErrorCode(code: string): code is FstErrorCode {
  return (FST_ERROR_CODES as readonly string[]).includes(code);
}

/**
 * The error for an object a glue call threw, or null when it is not one. The
 * glue throws plain objects with `code` and `message`.
 */
export function fromGlue(thrown: unknown): FstError | null {
  if (typeof thrown !== "object" || thrown === null || thrown instanceof Error) return null;
  const code: unknown = Reflect.get(thrown, "code");
  const message: unknown = Reflect.get(thrown, "message");
  if (typeof code !== "string" || typeof message !== "string") return null;
  if (isFstErrorCode(code)) return new FstError(code, message);
  return new FstError("internal", `${message} (the module sent the unknown code ${code})`);
}

/** The message of anything a caller might throw. */
export function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
