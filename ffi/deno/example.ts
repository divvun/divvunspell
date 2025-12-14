#!/usr/bin/env -S deno run --allow-ffi --allow-read
/**
 * Example usage of divvun-fst Deno bindings.
 */

import { SpellerArchive, tokenize } from "./mod.ts";

function main() {
  const archivePath = Deno.args[0] ?? "../../se.bhfst";

  try {
    Deno.statSync(archivePath);
  } catch {
    console.error(`Error: Archive not found at ${archivePath}`);
    console.error(
      "Usage: deno run --allow-ffi --allow-read example.ts [path/to/archive.bhfst]",
    );
    Deno.exit(1);
  }

  console.log(`Opening speller archive: ${archivePath}`);
  const archive = SpellerArchive.open(archivePath);

  try {
    const locale = archive.locale();
    console.log(`Archive locale: ${locale}`);
  } catch (e) {
    console.error(`Could not get locale: ${e}`);
  }

  const speller = archive.speller();
  console.log("Speller loaded successfully\n");

  const testWords = [
    "sámegiella", // Correct Northern Sami word
    "samegiel", // Misspelled
    "boahtin", // Correct
    "boatin", // Misspelled
  ];

  for (const word of testWords) {
    const isCorrect = speller.isCorrect(word);
    const status = isCorrect ? "CORRECT" : "INCORRECT";
    console.log(`Word: '${word}' - ${status}`);

    if (!isCorrect) {
      const suggestions = speller.suggest(word);
      console.log(`  Found ${suggestions.length} suggestions:`);
      for (let i = 0; i < Math.min(suggestions.length, 5); i++) {
        const sug = suggestions[i];
        const completedStr = sug.completed === null ? "unknown" : (sug.completed ? "completed" : "not completed");
        console.log(`    ${i + 1}. ${sug.value} (weight: ${sug.weight.toFixed(4)}, ${completedStr})`);
      }
    }
    console.log();
  }

  console.log("\nTokenization example:");
  const text = "This is a test of the word tokenizer.";
  console.log(`Text: "${text}"`);
  console.log("Words found:");
  const words = tokenize(text);
  for (let i = 0; i < words.length; i++) {
    const [index, word] = words[i];
    console.log(`  ${i + 1}. [${index}] ${word}`);
  }
}

if (import.meta.main) {
  main();
}
