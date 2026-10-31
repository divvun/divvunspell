import { FstError } from "./errors.ts";

/**
 * Extra penalties for an edit by where in the word it falls, divvunspell's
 * `ReweightingConfig`.
 */
export interface ReweightingConfig {
  /** What an edit at the first character costs. Default 10. */
  startPenalty: number;
  /** What an edit at the last character costs. Default 10. */
  endPenalty: number;
  /** What an edit between them costs. Default 5. */
  midPenalty: number;
  /**
   * How sharply the start and end penalties fall away into the word. With a
   * number `k` the three penalties are anchors on the curve
   * `mid + (start - mid) * (1 - r)^k + (end - mid) * r^k`, `r` being the
   * edit's position relative to the word's length; about 5 fits measured
   * misspellings. `null`, the default, keeps three flat zones.
   */
  curve: number | null;
}

/**
 * divvunspell's `SpellerConfig`, field for field, under camelCase names: what
 * tunes the suggestion search. {@link defaultSpellerConfig} holds the
 * built-in defaults. A config file a speller archive bundles is the same
 * object, written in kebab-case (`n-best` for `nBest`).
 */
export interface SpellerConfig {
  /** The most suggestions to return. Default 10; `null` returns every one found. */
  nBest: number | null;
  /** The greatest weight a suggestion may have. Default 10000; `null` for no limit. */
  maxWeight: number | null;
  /** How much heavier than the best suggestion the worst may be. Default `null`, no limit. */
  beam: number | null;
  /** Penalties for edits by where they fall. Default the {@link ReweightingConfig} defaults; `null` adds none. */
  reweight: ReweightingConfig | null;
  /**
   * Whether to keep the input's capitals on lower-case lexicon forms, and to
   * offer the lexicon's own capitals for a lower-case misspelling. Default
   * `true`. It changes suggestions only: a lower-cased proper noun is still
   * misspelt.
   */
  recase: boolean;
  /** The marker for suggesting an incomplete part of a word. Default `null`, none. */
  completionMarker: string | null;
  /**
   * Whether the search orders itself by the A* heuristic, which needs a
   * precomputation per transducer. The heuristic is admissible, so this
   * changes how long the search runs, not what it finds. Default `false`.
   */
  astarLookahead: boolean;
  /**
   * Whether the search walks states rather than paths. It changes what the
   * search costs, not what it finds; leave it on. Default `true`.
   */
  searchDedup: boolean;
  /**
   * Whether the search determinises the error model as it goes. It changes
   * what the search costs, not what it finds; leave it on. Default `true`.
   */
  mutatorSubsets: boolean;
  /**
   * How many nodes the search may examine for a word before it returns what
   * it has found, the cheapest first. Default `null`: no limit, which is
   * exact but can take seconds for a word with no correction near it.
   */
  searchBudget: number | null;
  /**
   * What putting a space back into a run-together word costs, on top of the
   * halves' lexicon weights. Default `null`, never.
   */
  wordSplitWeight: number | null;
  /**
   * What a lexicon form one inserted, deleted or replaced separator away
   * costs, such as `DNA-prosessajda` for `dnaprosessajda`. Default `null`,
   * never.
   */
  boundaryEditWeight: number | null;
  /**
   * What starting a capitalised lexicon form from a lower-case letter costs,
   * on top of the edits, as in `Skottlánda` for `skotlánda`. Default `null`,
   * never.
   */
  initialCaseWeight: number | null;
  /** Whether each suggestion carries its `weightDetails`. Default `false`. */
  verbose: boolean;
}

/**
 * A config as a call takes it: any field left out takes its built-in
 * default, as in a config file, and so does any penalty left out of
 * `reweight`. Fields left out never come from the archive's bundled config;
 * spread {@link Speller.bundledConfig} into the object to start from that.
 */
export type PartialSpellerConfig = Partial<Omit<SpellerConfig, "reweight">> & {
  reweight?: Partial<ReweightingConfig> | null;
};

/**
 * divvunspell's built-in defaults, which a call that passes no config runs
 * with when the archive bundles none.
 */
export const defaultSpellerConfig: Readonly<SpellerConfig> = Object.freeze({
  nBest: 10,
  maxWeight: 10000,
  beam: null,
  reweight: Object.freeze({
    startPenalty: 10,
    endPenalty: 10,
    midPenalty: 5,
    curve: null,
  }),
  recase: true,
  completionMarker: null,
  astarLookahead: false,
  searchDedup: true,
  mutatorSubsets: true,
  searchBudget: null,
  wordSplitWeight: null,
  boundaryEditWeight: null,
  initialCaseWeight: null,
  verbose: false,
} satisfies SpellerConfig);

/**
 * The JSON text the glue takes for `config`, or undefined for none. A field
 * set to undefined is left out. JSON has no infinities and no NaN, and would
 * write them as null, which means none, so they are refused here; the module
 * refuses fields divvun-fst does not have and values of the wrong type.
 */
export function configArg(config: unknown): string | undefined {
  if (config === undefined) return undefined;
  if (typeof config !== "object" || config === null || Array.isArray(config)) {
    throw new FstError("invalid-argument", "a speller config is an object, or left out for the speller's own");
  }
  try {
    return JSON.stringify(config, (key, value: unknown) => {
      if (typeof value === "number" && !Number.isFinite(value)) {
        throw new FstError(
          "invalid-argument",
          `a speller config: ${key} is ${value}, and numbers in a config are finite; null means none`,
        );
      }
      return value;
    });
  } catch (error) {
    if (error instanceof FstError) throw error;
    throw new FstError("invalid-argument", `a speller config: ${error instanceof Error ? error.message : String(error)}`);
  }
}

/** A config the module wrote, frozen, its `reweight` too. */
export function parseConfig(json: string): Readonly<SpellerConfig> {
  const config: SpellerConfig = JSON.parse(json);
  if (config.reweight !== null) Object.freeze(config.reweight);
  return Object.freeze(config);
}
