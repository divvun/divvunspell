/**
 * Deno FFI bindings for divvun-fst spell checking library.
 *
 * @module
 */

const LIBRARY_PATH = (() => {
  const baseDir = new URL("../..", import.meta.url).pathname;

  let libName: string;
  if (Deno.build.os === "darwin") {
    libName = "libdivvun_fst.dylib";
  } else if (Deno.build.os === "linux") {
    libName = "libdivvun_fst.so";
  } else if (Deno.build.os === "windows") {
    libName = "divvun_fst.dll";
  } else {
    throw new Error(`Unsupported platform: ${Deno.build.os}`);
  }

  const debugPath = `${baseDir}/target/debug/${libName}`;
  const releasePath = `${baseDir}/target/release/${libName}`;

  try {
    Deno.statSync(debugPath);
    return debugPath;
  } catch {
    try {
      Deno.statSync(releasePath);
      return releasePath;
    } catch {
      throw new Error(
        `Could not find ${libName} in target/debug or target/release. ` +
          "Build the FFI library first with: cd ffi && cargo build",
      );
    }
  }
})();

interface RustSlice {
  data: Deno.PointerValue;
  len: number;
}

const symbols = {
  DFST_SpellerArchive_open: {
    parameters: [{ struct: ["pointer", "usize"] }, "function"],
    result: { struct: ["pointer", "pointer"] },
  },
  DFST_SpellerArchive_speller: {
    parameters: [{ struct: ["pointer", "pointer"] }, "function"],
    result: { struct: ["pointer", "pointer"] },
  },
  DFST_SpellerArchive_locale: {
    parameters: [{ struct: ["pointer", "pointer"] }, "function"],
    result: { struct: ["pointer", "usize"] },
  },
  DFST_Speller_isCorrect: {
    parameters: [{ struct: ["pointer", "pointer"] }, {
      struct: ["pointer", "usize"],
    }, "function"],
    result: "u8",
  },
  DFST_Speller_suggest: {
    parameters: [{ struct: ["pointer", "pointer"] }, {
      struct: ["pointer", "usize"],
    }, "function"],
    result: { struct: ["pointer", "usize"] },
  },
  DFST_VecSuggestion_len: {
    parameters: [{ struct: ["pointer", "usize"] }, "function"],
    result: "usize",
  },
  DFST_VecSuggestion_getValue: {
    parameters: [{ struct: ["pointer", "usize"] }, "usize", "function"],
    result: { struct: ["pointer", "usize"] },
  },
  DFST_cstr_free: {
    parameters: ["pointer"],
    result: "void",
  },
  DFST_WordIndices_new: {
    parameters: ["buffer"],
    result: "pointer",
  },
  DFST_WordIndices_next: {
    parameters: ["pointer", "buffer", "buffer"],
    result: "u8",
  },
  DFST_WordIndices_free: {
    parameters: ["pointer"],
    result: "void",
  },
} as const;

const lib = Deno.dlopen(LIBRARY_PATH, symbols);

let lastError: string | null = null;

const errorCallback = new Deno.UnsafeCallback(
  {
    parameters: ["pointer", "usize"],
    result: "void",
  },
  (msgPtr: Deno.PointerValue, msgLen: number) => {
    const view = new Deno.UnsafePointerView(msgPtr);
    const errorBytes = new Uint8Array(msgLen);
    for (let i = 0; i < msgLen; i++) {
      errorBytes[i] = view.getUint8(i);
    }
    lastError = new TextDecoder().decode(errorBytes);
  },
);

function checkError(): void {
  if (lastError !== null) {
    const error = lastError;
    lastError = null;
    throw new Error(error);
  }
}

function stringToRustSlice(str: string): Uint8Array {
  const bytes = new TextEncoder().encode(str);
  const buffer = new ArrayBuffer(16); // pointer (8 bytes) + usize (8 bytes)
  const view = new DataView(buffer);

  const ptr = Deno.UnsafePointer.of(bytes);
  if (ptr) {
    view.setBigUint64(0, BigInt(Deno.UnsafePointer.value(ptr)), true); // little-endian
  }
  view.setBigUint64(8, BigInt(bytes.length), true);

  return new Uint8Array(buffer);
}

function rustSliceToString(slice: RustSlice): string {
  if (!slice.data || slice.len === 0) {
    return "";
  }
  // Convert BigInt to pointer
  const ptr = typeof slice.data === "bigint"
    ? Deno.UnsafePointer.create(slice.data)
    : slice.data;

  if (!ptr) {
    return "";
  }

  const view = new Deno.UnsafePointerView(ptr);
  const bytes = new Uint8Array(slice.len);
  for (let i = 0; i < slice.len; i++) {
    bytes[i] = view.getUint8(i);
  }
  return new TextDecoder().decode(bytes);
}

function isTraitObjectNull(obj: Uint8Array): boolean {
  const view = new DataView(obj.buffer, obj.byteOffset, obj.byteLength);
  const data = view.getBigUint64(0, true);
  return data === 0n;
}

const BRAND = Symbol();

/**
 * Spell checker interface.
 */
export class Speller {
  #handle: Uint8Array;

  constructor(handle: Uint8Array, brand: symbol) {
    if (brand !== BRAND) {
      throw new TypeError(
        "Speller constructor is private, use SpellerArchive.speller()",
      );
    }
    this.#handle = handle;
  }

  /**
   * Check if a word is spelled correctly.
   */
  isCorrect(word: string): boolean {
    const wordSlice = stringToRustSlice(word);
    const result = lib.symbols.DFST_Speller_isCorrect(
      this.#handle,
      wordSlice,
      errorCallback.pointer,
    ) as number;
    checkError();
    return result !== 0;
  }

  /**
   * Get spelling suggestions for a word.
   */
  suggest(word: string): string[] {
    const wordSlice = stringToRustSlice(word);
    const suggestionsSlice = lib.symbols.DFST_Speller_suggest(
      this.#handle,
      wordSlice,
      errorCallback.pointer,
    ) as Uint8Array;
    checkError();

    const sugView = new DataView(
      suggestionsSlice.buffer,
      suggestionsSlice.byteOffset,
      suggestionsSlice.byteLength,
    );
    const sugData = sugView.getBigUint64(0, true);
    if (sugData === 0n) {
      return [];
    }

    const length = lib.symbols.DFST_VecSuggestion_len(
      suggestionsSlice,
      errorCallback.pointer,
    ) as number;
    checkError();

    const results: string[] = [];
    for (let i = 0; i < length; i++) {
      const suggestionSlice = lib.symbols.DFST_VecSuggestion_getValue(
        suggestionsSlice,
        i,
        errorCallback.pointer,
      ) as Uint8Array;
      checkError();

      const view = new DataView(
        suggestionSlice.buffer,
        suggestionSlice.byteOffset,
        suggestionSlice.byteLength,
      );
      const data = view.getBigUint64(0, true);
      const len = Number(view.getBigUint64(8, true));

      results.push(rustSliceToString({ data, len }));

      if (data !== 0n) {
        lib.symbols.DFST_cstr_free(Deno.UnsafePointer.create(data));
      }
    }

    return results;
  }
}

/**
 * Spell checker archive (.bhfst file).
 */
export class SpellerArchive {
  #handle: Uint8Array;

  constructor(handle: Uint8Array, brand: symbol) {
    if (brand !== BRAND) {
      throw new TypeError(
        "SpellerArchive constructor is private, use SpellerArchive.open()",
      );
    }
    this.#handle = handle;
  }

  /**
   * Open a speller archive from a file path.
   */
  static open(path: string): SpellerArchive {
    const pathSlice = stringToRustSlice(path);
    const handle = lib.symbols.DFST_SpellerArchive_open(
      pathSlice,
      errorCallback.pointer,
    ) as Uint8Array;
    checkError();

    if (isTraitObjectNull(handle)) {
      throw new Error(`Failed to open speller archive: ${path}`);
    }

    return new SpellerArchive(handle, BRAND);
  }

  /**
   * Get the speller from this archive.
   */
  speller(): Speller {
    const spellerHandle = lib.symbols.DFST_SpellerArchive_speller(
      this.#handle,
      errorCallback.pointer,
    ) as Uint8Array;
    checkError();

    if (isTraitObjectNull(spellerHandle)) {
      throw new Error("Failed to get speller from archive");
    }

    return new Speller(spellerHandle, BRAND);
  }

  /**
   * Get the locale of this speller archive.
   */
  locale(): string {
    const localeSlice = lib.symbols.DFST_SpellerArchive_locale(
      this.#handle,
      errorCallback.pointer,
    ) as Uint8Array;
    checkError();

    const view = new DataView(
      localeSlice.buffer,
      localeSlice.byteOffset,
      localeSlice.byteLength,
    );
    const data = view.getBigUint64(0, true);
    const len = Number(view.getBigUint64(8, true));

    const locale = rustSliceToString({ data, len });
    if (data !== 0n) {
      lib.symbols.DFST_cstr_free(Deno.UnsafePointer.create(data));
    }
    return locale;
  }
}

/**
 * Iterator over word boundaries in a string.
 */
export class WordIndices implements IterableIterator<[number, string]> {
  #handle: Deno.PointerValue;
  #textBytes: Uint8Array;

  constructor(text: string) {
    this.#textBytes = new TextEncoder().encode(text + "\0");
    this.#handle = lib.symbols.DFST_WordIndices_new(
      this.#textBytes,
    ) as Deno.PointerValue;
    if (!this.#handle) {
      throw new Error("Failed to create word indices iterator");
    }
  }

  [Symbol.iterator](): IterableIterator<[number, string]> {
    return this;
  }

  next(): IteratorResult<[number, string]> {
    const indexBuf = new BigUint64Array(1);
    const wordPtrBuf = new BigUint64Array(1);

    const result = lib.symbols.DFST_WordIndices_next(
      this.#handle,
      indexBuf,
      wordPtrBuf,
    ) as number;

    if (result === 0) {
      return { done: true, value: undefined };
    }

    const index = Number(indexBuf[0]);
    const wordPtrValue = wordPtrBuf[0];
    const wordPtr = Deno.UnsafePointer.create(wordPtrValue);

    if (!wordPtr) {
      return { done: true, value: undefined };
    }

    const view = new Deno.UnsafePointerView(wordPtr);

    let wordLen = 0;
    while (view.getUint8(wordLen) !== 0) {
      wordLen++;
    }

    const wordBytes = new Uint8Array(wordLen);
    for (let i = 0; i < wordLen; i++) {
      wordBytes[i] = view.getUint8(i);
    }
    const word = new TextDecoder().decode(wordBytes);

    lib.symbols.DFST_cstr_free(wordPtr);

    return { done: false, value: [index, word] };
  }

  [Symbol.dispose](): void {
    if (this.#handle) {
      lib.symbols.DFST_WordIndices_free(this.#handle);
    }
  }
}

/**
 * Tokenize text into words with their byte indices.
 */
export function tokenize(text: string): Array<[number, string]> {
  const result: Array<[number, string]> = [];
  using iterator = new WordIndices(text);
  for (const item of iterator) {
    result.push(item);
  }
  return result;
}
