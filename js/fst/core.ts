import { fromGlue, FstError, messageOf } from "./errors.ts";
import type { Glue, GlueInitInput, GlueObject, GlueSink } from "./glue.ts";
import {
  consoleLogger,
  eventLevel,
  isLogLevel,
  levelNumber,
  type Logger,
  type LogLevel,
} from "./logging.ts";

/**
 * Where `init()` loads the module from: a URL (a string is read as one), a
 * `Response`, the module's bytes, or a compiled `WebAssembly.Module`.
 */
export type InitSource = string | URL | Response | BufferSource | WebAssembly.Module;

/**
 * A WebAssembly module instance and the state the wrapper keeps about it.
 * The package makes exactly one, in binding.ts, and every call into the
 * module goes through it.
 */
export class Core {
  readonly #glue: Glue;
  readonly #defaultSource: URL;
  #starting: Promise<void> | null = null;
  #ready = false;
  /** Set by the first trap, and never cleared. */
  #poison: { message: string; trap: unknown } | null = null;
  /** The message of a panic whose trap has not reached the wrapper yet. */
  #panic: string | null = null;
  #logger: Logger = consoleLogger;
  /**
   * Glue objects held for the life of the page once the instance is poisoned.
   * Unreachable, wasm-bindgen's own registry would free them by entering the
   * module, which a poisoned instance must never do.
   */
  readonly #kept: GlueObject[] = [];
  /**
   * Frees the glue object of a wrapper the caller dropped. Holding the glue
   * object as the held value keeps it reachable, so wasm-bindgen's registry
   * never frees it first.
   */
  readonly #registry = new FinalizationRegistry<GlueObject>((glue) => this.#free(glue, false));
  readonly #sink: GlueSink = {
    log: (level, target, message) => this.#deliver(level, target, message),
    panic: (message) => {
      this.#panic = message;
    },
  };

  /** `defaultSource` is the package's own `.wasm` file. Nothing is loaded yet. */
  constructor(glue: Glue, defaultSource: URL) {
    this.#glue = glue;
    this.#defaultSource = defaultSource;
  }

  /**
   * Loads and starts the module. Every call after the first returns the first
   * call's promise, whatever its argument, so there is only ever one instance.
   * A start that fails leaves no instance, and the next call tries again.
   */
  init(source?: InitSource): Promise<void> {
    if (this.#starting === null) {
      const starting = this.#start(source ?? this.#defaultSource);
      this.#starting = starting;
      starting.catch(() => {
        if (this.#starting === starting) this.#starting = null;
      });
    }
    return this.#starting;
  }

  async #start(source: InitSource): Promise<void> {
    try {
      await this.#glue.default({ module_or_path: await load(source) });
    } catch (error) {
      throw new FstError("init-failed", `cannot start @divvun/fst: ${messageOf(error)}`, {
        cause: error,
      });
    }
    this.#enter(() => this.#glue.install(this.#sink, levelNumber("warn")));
    this.#ready = true;
  }

  /**
   * Runs `f` against the glue once `init()` has resolved, and turns whatever
   * it throws into an `FstError`. After a trap it throws `poisoned` without
   * running `f`.
   */
  call<T>(f: (glue: Glue) => T): T {
    this.#check();
    return this.#enter(() => f(this.#glue));
  }

  #check(): void {
    if (this.#poison !== null) throw this.#poisoned();
    if (!this.#ready) {
      throw new FstError(
        "not-initialised",
        "@divvun/fst is not initialised: call init() and wait for it to resolve first",
      );
    }
  }

  #enter<T>(f: () => T): T {
    let result: T;
    try {
      result = f();
    } catch (error) {
      throw this.#translate(error);
    }
    // A logger that called back in can trap the module in a nested call. That
    // poisons the instance without unwinding this call, whose result is then
    // as untrustworthy as the memory it came from.
    if (this.#poison !== null) throw this.#poisoned();
    return result;
  }

  #translate(error: unknown): unknown {
    if (error instanceof FstError) return error;
    if (error instanceof WebAssembly.RuntimeError) {
      this.#poisonWith(error);
      return this.#poisoned();
    }
    return fromGlue(error) ?? error;
  }

  #poisonWith(trap: WebAssembly.RuntimeError): void {
    if (this.#poison !== null) return;
    const panic = this.#panic;
    this.#panic = null;
    const message =
      panic === null ? `the module trapped: ${trap.message}` : `the module panicked: ${panic}`;
    this.#poison = { message, trap };
  }

  #poisoned(): FstError {
    const message = this.#poison?.message ?? "the module trapped";
    return new FstError("poisoned", `${message}. Load the page or process again to recover.`, {
      cause: this.#poison?.trap,
    });
  }

  /** Sends the library's log events to `logger`, or to the console for null. */
  setLogger(logger: Logger | null): void {
    this.#check();
    if (logger !== null && typeof logger !== "function") {
      throw new FstError("invalid-argument", "a logger is a function, or null for the console");
    }
    this.#logger = logger ?? consoleLogger;
  }

  /** Logs events at `level` and the levels before it. The module skips the rest. */
  setLogLevel(level: LogLevel): void {
    this.#check();
    if (!isLogLevel(level)) {
      throw new FstError("invalid-argument", `there is no log level called ${String(level)}`);
    }
    this.#enter(() => this.#glue.setLogLevel(levelNumber(level)));
  }

  // A logger that throws must not throw into the module, which is mid-call.
  #deliver(level: number, target: string, message: string): void {
    try {
      this.#logger({ level: eventLevel(level), target, message });
    } catch (error) {
      console.error("@divvun/fst: the logger threw:", error);
    }
  }

  /** Frees `glue` when `owner` becomes unreachable, unless `release` frees it first. */
  adopt(owner: object, glue: GlueObject): void {
    this.#registry.register(owner, glue, owner);
  }

  /** Frees `glue` at once, for `owner.free()`. Throws `poisoned` only if freeing traps. */
  release(owner: object, glue: GlueObject): void {
    this.#registry.unregister(owner);
    this.#free(glue, true);
  }

  #free(glue: GlueObject, explicit: boolean): void {
    if (this.#poison !== null) {
      this.#kept.push(glue);
      return;
    }
    try {
      this.#enter(() => glue.free());
    } catch (error) {
      // A finalizer has no caller to throw to. The next call reports the poison.
      if (explicit) throw error;
    }
  }
}

async function load(source: InitSource): Promise<GlueInitInput> {
  if (typeof source === "string" || source instanceof URL) {
    const url = typeof source === "string" && URL.canParse(source) ? new URL(source) : source;
    if (url instanceof URL && url.protocol === "file:") return await readFile(url);
    return checked(await fetch(url));
  }
  if (source instanceof Response) return checked(source);
  return source;
}

function checked(response: Response): Response {
  if (!response.ok) {
    throw new Error(`${response.url || "the response"} answered HTTP ${response.status}`);
  }
  return response;
}

/** The part of `node:fs/promises` that {@link readFile} uses. */
interface NodeFs {
  readFile(path: URL): Promise<Uint8Array<ArrayBuffer>>;
}

// Node's fetch cannot read file: URLs, so they are read with node:fs. The
// specifier is a variable so that bundlers and type checkers do not follow it:
// code that never passes a file URL never asks for node:fs.
async function readFile(url: URL): Promise<Uint8Array<ArrayBuffer>> {
  const specifier = "node:fs/promises";
  const fs: NodeFs = await import(/* @vite-ignore */ /* webpackIgnore: true */ specifier);
  return await fs.readFile(url);
}
