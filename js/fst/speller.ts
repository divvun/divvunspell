import { configArg, type PartialSpellerConfig, parseConfig, type SpellerConfig } from "./config.ts";
import type { Core } from "./core.ts";
import { FstError } from "./errors.ts";
import type { SpellerGlue } from "./glue.ts";
import { Handle } from "./handle.ts";

/**
 * A suggestion, or an analysis, as divvunspell's `Suggestion` carries it.
 * Weights are the f32 values divvunspell computed, each as the number that
 * holds it exactly.
 */
export interface Suggestion {
  /** The suggested word form; for an analysis, the form with its tags. */
  value: string;
  /** Its total weight: lower is better. */
  weight: number;
  /** Whether it is a whole word rather than a part, where the speller says. */
  completed?: boolean;
  /** How the weight is made up, present when the config sets `verbose`. */
  weightDetails?: WeightDetails;
}

/** How a suggestion's weight is made up. */
export interface WeightDetails {
  /** What the lexicon charged for the word form. */
  lexiconWeight: number;
  /** What the error model charged for reaching it. */
  mutatorWeight: number;
  /** The reweighting penalty for edits at the start of the word. */
  reweightStart: number;
  /** The reweighting penalty for edits in the middle, or -1 when the word has no middle. */
  reweightMid: number;
  /** The reweighting penalty for edits at the end of the word. */
  reweightEnd: number;
}

/** A title in some language; `lang` is null when the archive names none. */
export interface SpellerTitle {
  lang: string | null;
  value: string;
}

/** What an archive says about its speller. */
export interface SpellerInfo {
  /** The language the speller is for, as a BCP 47 tag such as `se`. */
  locale: string;
  title: SpellerTitle[];
  description: string;
  producer: string;
}

/** What an archive says about its lexicon. */
export interface AcceptorMetadata {
  type: string;
  id: string;
  title: SpellerTitle[];
  description: string;
  /** The marker for incomplete strings, or null. */
  continuation: string | null;
}

/** What an archive says about its error model. */
export interface ErrorModelMetadata {
  id: string;
  title: SpellerTitle[];
  description: string;
}

/** An archive's metadata: `index.xml` in a ZHFST, `meta.json` in a BHFST. */
export interface SpellerMetadata {
  info: SpellerInfo;
  acceptor: AcceptorMetadata;
  errorModel: ErrorModelMetadata;
}

function deepFreeze<T>(value: T): T {
  if (typeof value === "object" && value !== null) {
    for (const inner of Object.values(value)) deepFreeze(inner);
    Object.freeze(value);
  }
  return value;
}

function wordArg(name: string, value: unknown): string {
  if (typeof value !== "string") {
    throw new FstError("invalid-argument", `${name} takes the word as a string, not ${typeof value}`);
  }
  return value;
}

let wrap: (core: Core, glue: SpellerGlue) => Speller;

/**
 * A speller loaded from a BHFST or ZHFST archive. Its metadata and bundled
 * config are read once, when it loads; reading them after `free()` throws
 * `freed`, as any other use does. It owns module memory: call `free()` when
 * done with it, or declare it with `using`.
 *
 * Every method takes an optional config. Left out, the speller runs with the
 * config the archive bundles, or else divvunspell's built-in defaults. Given,
 * it is the whole config: any field it leaves out takes its built-in default.
 */
export class Speller extends Handle<SpellerGlue> {
  readonly #metadata: Readonly<SpellerMetadata> | null;
  readonly #bundledConfig: Readonly<SpellerConfig> | null;

  private constructor(core: Core, glue: SpellerGlue) {
    super(core, glue);
    const metadata = this.use((g) => g.metadata());
    this.#metadata = metadata === undefined ? null : deepFreeze<SpellerMetadata>(JSON.parse(metadata));
    const bundled = this.use((g) => g.bundledConfig());
    this.#bundledConfig = bundled === undefined ? null : parseConfig(bundled);
  }

  static {
    wrap = (core, glue) => new Speller(core, glue);
  }

  /** The archive's metadata, or null when it carries none, as a BHFST may not. */
  get metadata(): Readonly<SpellerMetadata> | null {
    return this.use(() => this.#metadata);
  }

  /** The language the speller is for, from its metadata, or null when it has none. */
  get locale(): string | null {
    return this.use(() => this.#metadata?.info.locale ?? null);
  }

  /**
   * The config the archive bundles, which a call that passes none runs with,
   * or null when it bundles none and such a call runs with
   * {@link defaultSpellerConfig}.
   */
  get bundledConfig(): Readonly<SpellerConfig> | null {
    return this.use(() => this.#bundledConfig);
  }

  /**
   * Whether the lexicon accepts `word`, as typed or, with `recase`, in the
   * lower-case and other case variants divvunspell tries. A word with no
   * letters, the empty string among them, is correct.
   */
  isCorrect(word: string, config?: PartialSpellerConfig): boolean {
    const text = wordArg("isCorrect", word);
    const json = configArg(config);
    return this.use((g) => g.isCorrect(text, json));
  }

  /** Corrections for `word`, best first, as divvunspell's suggestion search finds them. */
  suggest(word: string, config?: PartialSpellerConfig): Suggestion[] {
    const text = wordArg("suggest", word);
    const json = configArg(config);
    return JSON.parse(this.use((g) => g.suggest(text, json)));
  }

  /**
   * The lexicon's analyses of `word` as typed, with the lexicon's tags: no
   * error model, no corrections. A speller whose lexicon is a plain word list
   * has no tags to give.
   */
  analyzeInput(word: string, config?: PartialSpellerConfig): Suggestion[] {
    const text = wordArg("analyzeInput", word);
    const json = configArg(config);
    return JSON.parse(this.use((g) => g.analyzeInput(text, json)));
  }

  /** The suggestions for `word`, each with the lexicon's tags left in, best first. */
  analyzeOutput(word: string, config?: PartialSpellerConfig): Suggestion[] {
    const text = wordArg("analyzeOutput", word);
    const json = configArg(config);
    return JSON.parse(this.use((g) => g.analyzeOutput(text, json)));
  }
}

export interface SpellerReader {
  /**
   * The speller in a speller archive's bytes: a BHFST of either box version,
   * its error model and lexicon THFST or DHFST, or a ZHFST, its members stored
   * or deflated. The format is told by the first bytes. The bytes are copied
   * into the module once and read there in place. Bytes that are no speller
   * archive throw `unsupported-format`; a broken archive throws
   * `invalid-format` with the reason.
   */
  readSpeller(bytes: Uint8Array): Speller;
}

/** The {@link SpellerReader} that loads into the module instance `core`. */
export function spellerReader(core: Core): SpellerReader {
  return {
    readSpeller: (bytes) => {
      if (!(bytes instanceof Uint8Array)) {
        throw new FstError("invalid-argument", "a speller loads from its archive's bytes, as a Uint8Array");
      }
      return core.call((glue) => wrap(core, glue.readSpeller(bytes)));
    },
  };
}
