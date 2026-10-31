import type { Core } from "./core.ts";
import { FstError } from "./errors.ts";
import type { TransducerGlue } from "./glue.ts";
import { Handle } from "./handle.ts";

/**
 * The formats a transducer loads from. They do not change between releases.
 *
 * - `optimized-lookup`: HFST optimized lookup with its HFST3 header, as
 *   analysers, generators and ZHFST members are written.
 * - `thfst`: divvunspell's THFST tables, a directory in a box archive.
 * - `dhfst-acceptor`: a DHFST acceptor (type 2), a speller's lexicon.
 * - `dhfst-error-model`: a DHFST error model (type 1).
 */
const TRANSDUCER_FORMATS = ["optimized-lookup", "thfst", "dhfst-acceptor", "dhfst-error-model"] as const;

export type TransducerFormat = (typeof TRANSDUCER_FORMATS)[number];

function isTransducerFormat(format: string): format is TransducerFormat {
  return (TRANSDUCER_FORMATS as readonly string[]).includes(format);
}

/** One output a transducer gives a word. */
export interface LookupResult {
  /** The output: the text of `symbols`, in order. */
  output: string;
  /**
   * The output symbols, by their text. Epsilons and flag diacritics are left
   * out; an identity symbol writes the symbol it read.
   */
  symbols: string[];
  /** The path's weight, arcs and final weight, as the number that holds its f32 exactly. */
  weight: number;
}

/** Limits on one lookup. */
export interface LookupOptions {
  /** The most results to return, an integer 0 or more: the cheapest ones. */
  limit?: number;
}

/** Which transducer to read from a box archive. */
export interface ReadTransducerOptions {
  /**
   * The member to read: a THFST directory, such as a BHFST's
   * `acceptor.default.thfst`, `.` for one whose files are at the root, or a
   * single HFST or DHFST file, such as `acceptor.default.dhfst`. Needed only
   * when the box holds more than one transducer.
   */
  member?: string;
}

/** The largest limit the glue takes; more than any lookup can find. */
const MAX_LIMIT = 0xffffffff;

function lookupLimit(options: unknown): number | undefined {
  if (options === undefined) return undefined;
  if (typeof options !== "object" || options === null) {
    throw new FstError("invalid-argument", "lookup takes its options as an object");
  }
  const limit: unknown = Reflect.get(options, "limit");
  if (limit === undefined) return undefined;
  if (!(typeof limit === "number" && Number.isInteger(limit) && limit >= 0)) {
    throw new FstError("invalid-argument", `a lookup limit is an integer 0 or more, not ${String(limit)}`);
  }
  return Math.min(limit, MAX_LIMIT);
}

let wrap: (core: Core, glue: TransducerGlue) => Transducer;

/**
 * A transducer loaded from bytes, for lookup: an analyser, a generator, or a
 * speller's lexicon or error model. Its format is read once, when it loads;
 * reading it after `free()` throws `freed`, as any other use does. It owns
 * module memory: call `free()` when done with it, or declare it with `using`.
 */
export class Transducer extends Handle<TransducerGlue> {
  readonly #format: TransducerFormat;

  private constructor(core: Core, glue: TransducerGlue) {
    super(core, glue);
    const format = this.use((g) => g.format());
    if (!isTransducerFormat(format)) {
      throw new FstError("internal", `the module sent a transducer of the unknown format ${format}`);
    }
    this.#format = format;
  }

  static {
    wrap = (core, glue) => new Transducer(core, glue);
  }

  /** The format the transducer was read in. */
  get format(): TransducerFormat {
    return this.use(() => this.#format);
  }

  /**
   * Every output the transducer gives `word`, cheapest first, then by output,
   * with no two the same in output and weight: divvun-fst's
   * `transducer::lookup`. The word is read as `hfst-optimized-lookup` reads
   * it, by longest match over the input symbols, so a generator's tags such
   * as `+N` are read whole; text no symbol matches is read a grapheme cluster
   * at a time as the unknown symbol, where the transducer has one, and has no
   * results where it has none. Flag diacritics are obeyed.
   */
  lookup(word: string, options?: LookupOptions): LookupResult[] {
    if (typeof word !== "string") {
      throw new FstError("invalid-argument", `lookup takes the word as a string, not ${typeof word}`);
    }
    const limit = lookupLimit(options);
    return JSON.parse(this.use((g) => g.lookup(word, limit)));
  }
}

export interface TransducerReader {
  /**
   * The transducer in `bytes`, told by its first bytes: HFST optimized
   * lookup, a DHFST acceptor or error model, or a box archive holding one, a
   * THFST directory or a single file. A box holding several, as a BHFST
   * does, needs `options.member` to say which. The bytes are copied into the
   * module once and read there in place. Bytes in no format the package
   * reads throw `unsupported-format`; broken ones throw `invalid-format`
   * with the reason; a member that is not there, or a box holding several
   * transducers and no member named, throws `invalid-argument`.
   */
  readTransducer(bytes: Uint8Array, options?: ReadTransducerOptions): Transducer;
}

/** The {@link TransducerReader} that loads into the module instance `core`. */
export function transducerReader(core: Core): TransducerReader {
  return {
    readTransducer: (bytes, options) => {
      if (!(bytes instanceof Uint8Array)) {
        throw new FstError("invalid-argument", "a transducer loads from its bytes, as a Uint8Array");
      }
      if (options !== undefined && (typeof options !== "object" || options === null)) {
        throw new FstError("invalid-argument", "readTransducer takes its options as an object");
      }
      const member: unknown = options === undefined ? undefined : Reflect.get(options, "member");
      if (member !== undefined && typeof member !== "string") {
        throw new FstError("invalid-argument", `a member is named by a string, not ${typeof member}`);
      }
      return core.call((glue) => wrap(core, glue.readTransducer(bytes, member)));
    },
  };
}
