# Accuracy viewer

A web viewer for `divvunspell` accuracy reports, written in Rust with
[Dioxus](https://dioxuslabs.com/) and built to WebAssembly with
[Trunk](https://trunkrs.dev/). It is a static site — no Node toolchain — that
fetches a `report.json` served alongside it and renders the speller
configuration, performance/classification/suggestion statistics, and a sortable,
colour-coded results table.

## Prerequisites

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk          # or: brew install trunk
```

This crate is intentionally **outside** the main `divvunspell` Cargo workspace
(it has its own `[workspace]` table), so building it never interferes with the
native library/CLI build.

## Generate a report

```bash
# from the divvunspell repo root
cargo run -p divvunspell-cli --features accuracy -- \
    accuracy -o report.json typos.tsv path/to/language.bhfst
```

`typos.tsv` is a tab-separated `input<TAB>expected` list; rows with an empty
`expected` column are treated as correct words (to measure false positives).
Add `-v` to include the per-suggestion weight breakdown (lexicon / mutator /
reweight) in the report.

## Develop

```bash
trunk serve --open
```

Place the `report.json` to view in `dist/` (Trunk serves that directory), or copy
it there after `trunk build`. The app fetches `report.json` relative to the page.

## Build for deployment

```bash
trunk build --release
```

The static site is emitted to `dist/`. Copy your `report.json` into `dist/` and
serve/publish the directory.

### GitHub Pages

When serving from a project subpath (e.g. `https://<org>.github.io/<repo>/`),
build with a matching public URL so asset links resolve:

```bash
trunk build --release --public-url /<repo>/
```

Then publish `dist/` (with `report.json` inside it) to the Pages branch/directory.
