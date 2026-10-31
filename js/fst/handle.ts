import type { Core } from "./core.ts";
import { FstError } from "./errors.ts";
import type { GlueObject } from "./glue.ts";

/**
 * The base of every object that owns WebAssembly memory. `free()` and
 * `[Symbol.dispose]()` release the memory, so a `using` declaration frees the
 * object at the end of its block. Freeing twice does nothing, and any other
 * call on a freed object throws `FstError` with code `freed`. An object
 * dropped without `free()` is released once it has been garbage collected.
 */
export abstract class Handle<G extends GlueObject> implements Disposable {
  readonly #core: Core;
  #glue: G | null;

  protected constructor(core: Core, glue: G) {
    this.#core = core;
    this.#glue = glue;
    core.adopt(this, glue);
  }

  /** Runs `f` on the glue object, with the module instance's call checks. */
  protected use<T>(f: (glue: G) => T): T {
    return this.#core.call(() => {
      const glue = this.#glue;
      if (glue === null) throw new FstError("freed", `this ${this.constructor.name} was freed`);
      return f(glue);
    });
  }

  /** Releases the object's WebAssembly memory. Freeing twice does nothing. */
  free(): void {
    const glue = this.#glue;
    if (glue === null) return;
    this.#glue = null;
    this.#core.release(this, glue);
  }

  [Symbol.dispose](): void {
    this.free();
  }
}
