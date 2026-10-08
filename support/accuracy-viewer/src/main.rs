//! Dioxus web viewer for divvunspell accuracy reports.
//!
//! Fetches `speller-accuracy.json.gz` (served alongside the page, e.g. on GitHub
//! Pages, or from the repo's `generated/docs-data` branch) and
//! renders the speller configuration, performance/classification/suggestion
//! statistics, and a sortable, filterable, paged, colour-coded results table.
//! This is a Rust/WASM reimplementation of the former Svelte app — no Node
//! toolchain required.

use std::rc::Rc;

use dioxus::prelude::*;
use serde::Deserialize;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;

fn main() {
    console_error_panic_hook::set_once();
    dioxus::launch(App);
}

// ===========================================================================
// Data model — mirrors the JSON emitted by `divvunspell accuracy --json-output`
// (see cli/src/accuracy.rs). `Weight` values serialise transparently as f32.
// ===========================================================================

#[derive(Deserialize, Clone, PartialEq)]
struct Report {
    #[serde(default)]
    metadata: Option<serde_json::Value>,
    #[serde(default)]
    config: serde_json::Value,
    summary: Summary,
    results: Vec<AccuracyResult>,
    #[serde(default)]
    total_time: Time,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
struct Time {
    secs: u64,
    subsec_nanos: u32,
}

impl Time {
    fn to_secs_f64(self) -> f64 {
        self.secs as f64 + self.subsec_nanos as f64 / 1e9
    }
    fn to_millis_f64(self) -> f64 {
        self.secs as f64 * 1000.0 + self.subsec_nanos as f64 / 1e6
    }
    fn total_nanos(self) -> u128 {
        self.secs as u128 * 1_000_000_000 + self.subsec_nanos as u128
    }
}

#[derive(Deserialize, Clone, PartialEq)]
struct AccuracyResult {
    input: String,
    #[serde(default)]
    expected: Option<String>,
    distance: usize,
    suggestions: Vec<Suggestion>,
    #[serde(default)]
    position: Option<usize>,
    time: Time,
    false_accept: bool,
}

#[derive(Deserialize, Clone, PartialEq)]
struct Suggestion {
    value: String,
    weight: f32,
    #[serde(default)]
    weight_details: Option<WeightDetails>,
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
struct WeightDetails {
    lexicon_weight: f32,
    mutator_weight: f32,
    reweight_start: f32,
    reweight_mid: f32,
    reweight_end: f32,
}

#[derive(Deserialize, Clone, PartialEq, Default)]
struct Summary {
    #[serde(default)]
    true_positive: u32,
    #[serde(default)]
    false_negative: u32,
    #[serde(default)]
    true_negative: u32,
    #[serde(default)]
    false_accept: u32,
    #[serde(default)]
    average_time: Time,
    #[serde(default)]
    average_time_95pc: Time,
    #[serde(default)]
    average_position_of_correct: f32,
    #[serde(default)]
    average_suggestions_for_correct: f32,
}

// ===========================================================================
// Classification
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Tp,
    Fn_,
    Tn,
    Fp,
}

fn classify(r: &AccuracyResult) -> Class {
    if r.expected.is_some() {
        if !r.false_accept {
            Class::Tp
        } else {
            Class::Fn_
        }
    } else if r.false_accept {
        Class::Fp
    } else {
        Class::Tn
    }
}

/// The four classes in display order; `class_index` indexes into this.
const CLASSES: [Class; 4] = [Class::Tp, Class::Fn_, Class::Tn, Class::Fp];

fn class_index(c: Class) -> usize {
    match c {
        Class::Tp => 0,
        Class::Fn_ => 1,
        Class::Tn => 2,
        Class::Fp => 3,
    }
}

fn class_label(c: Class) -> &'static str {
    match c {
        Class::Tp => "True positive",
        Class::Fn_ => "False negative",
        Class::Tn => "True negative",
        Class::Fp => "False positive",
    }
}

fn result_class(r: &AccuracyResult) -> &'static str {
    match classify(r) {
        Class::Tp => match r.position {
            Some(0) => "indicator-tp-first",
            Some(_) => "indicator-tp-found",
            None => "indicator-true-positive",
        },
        Class::Fn_ => "indicator-false-negative",
        Class::Tn => "indicator-true-negative",
        Class::Fp => "indicator-false-positive",
    }
}

fn class_order(r: &AccuracyResult) -> u8 {
    match classify(r) {
        Class::Tp => 0,
        Class::Tn => 1,
        Class::Fp => 2,
        Class::Fn_ => 3,
    }
}

/// Worst-to-best sort key for a result's suggestion position.
fn position_key(r: &AccuracyResult) -> usize {
    match (r.position, r.suggestions.is_empty()) {
        (Some(p), _) => p,
        (None, false) => usize::MAX - 1,
        (None, true) => usize::MAX,
    }
}

// ===========================================================================
// Formatting helpers
// ===========================================================================

fn format_weight(w: f32) -> String {
    format!("{w:.5}")
}

fn weight_details_str(wd: &WeightDetails) -> String {
    let mid = if wd.reweight_mid < 0.0 {
        "-".to_string()
    } else {
        format!("{:.0}", wd.reweight_mid)
    };
    format!(
        "(lex: {:.5}, mut: {:.5}, rew: {:.0}/{}/{:.0})",
        wd.lexicon_weight, wd.mutator_weight, wd.reweight_start, mid, wd.reweight_end
    )
}

fn human_time(t: Time) -> String {
    let s = t.to_secs_f64();
    if s > 60.0 {
        let m = (s / 60.0).floor() as u64;
        let rem = s % 60.0;
        format!("{m}:{rem:.3}")
    } else {
        format!("00:{s:.3}")
    }
}

fn human_time_millis(t: Time) -> String {
    format!("{} ms", t.to_millis_f64())
}

fn words_per_second(t: Time, count: usize) -> String {
    let total = t.to_secs_f64();
    if total <= 0.0 {
        return "0.00".to_string();
    }
    format!("{:.2}", count as f64 / total)
}

/// Sum of per-word lookup times (estimated serial/CPU runtime).
fn total_cpu_time(results: &[AccuracyResult]) -> Time {
    let nanos: u128 = results.iter().map(|r| r.time.total_nanos()).sum();
    Time {
        secs: (nanos / 1_000_000_000) as u64,
        subsec_nanos: (nanos % 1_000_000_000) as u32,
    }
}

/// Percentage with one decimal over a total word count; "N/A" when empty.
fn pct1(num: u32, den: usize) -> String {
    if den == 0 {
        "N/A".to_string()
    } else {
        format!("{:.1}%", num as f64 / den as f64 * 100.0)
    }
}

/// Percentage with two decimals over the true-positive count; "0.00" when empty.
fn pct2(num: usize, den: usize) -> String {
    if den == 0 {
        "0.00".to_string()
    } else {
        format!("{:.2}", num as f64 / den as f64 * 100.0)
    }
}

fn fmt2(v: f64) -> String {
    format!("{v:.2}")
}

fn format_metric(value: &str) -> String {
    if value == "N/A" {
        value.to_string()
    } else {
        format!("{value}%")
    }
}

fn speller_title(report: &Report) -> String {
    let Some(info) = report.metadata.as_ref().and_then(|m| m.get("info")) else {
        return "Spellchecker Accuracy Report".to_string();
    };
    let locale = info.get("locale").and_then(|v| v.as_str()).unwrap_or("?");
    let title = info
        .get("title")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|t| t.get("$value"))
        .and_then(|v| v.as_str())
        .unwrap_or("Spellchecker");
    format!("{title} ({locale})")
}

// ===========================================================================
// Precomputed statistics (computed once per loaded report)
// ===========================================================================

#[derive(Clone, PartialEq)]
struct Stats {
    title: String,
    config_json: String,
    total_words: usize,
    // Runtime
    real_wps: String,
    real_total: String,
    cpu_wps: String,
    cpu_total: String,
    avg_per_word: String,
    avg_per_word_95: String,
    // Classifier counts + metrics
    tp: u32,
    fneg: u32,
    tn: u32,
    fp: u32,
    tp_pct: String,
    fn_pct: String,
    tn_pct: String,
    fp_pct: String,
    c_precision: String,
    c_recall: String,
    c_accuracy: String,
    c_fscore: String,
    // Suggestion stats (over true positives)
    first_count: usize,
    first_pct: String,
    top5_count: usize,
    top5_pct: String,
    anywhere_count: usize,
    anywhere_pct: String,
    nosugg_count: usize,
    nosugg_pct: String,
    onlywrong_count: usize,
    onlywrong_pct: String,
    avg_position: f32,
    avg_suggestions: f32,
    s_precision: String,
    s_recall: String,
    s_accuracy: String,
    s_fscore: String,
}

fn compute_stats(report: &Report) -> Stats {
    let total_words = report.results.len();
    let s = &report.summary;

    // Classifier metrics from summary counts.
    let tp = s.true_positive;
    let fneg = s.false_negative;
    let tn = s.true_negative;
    let fp = s.false_accept;

    let c_precision = if tp + fp == 0 {
        "N/A".to_string()
    } else {
        fmt2(tp as f64 / (tp + fp) as f64 * 100.0)
    };
    let c_recall = if tp + fneg == 0 {
        "N/A".to_string()
    } else {
        fmt2(tp as f64 / (tp + fneg) as f64 * 100.0)
    };
    let c_accuracy = {
        let total = tp + tn + fp + fneg;
        if total == 0 {
            "N/A".to_string()
        } else {
            fmt2((tp + tn) as f64 / total as f64 * 100.0)
        }
    };
    let c_fscore = if c_precision == "N/A" || c_recall == "N/A" {
        "N/A".to_string()
    } else {
        let p: f64 = c_precision.parse().unwrap_or(0.0);
        let r: f64 = c_recall.parse().unwrap_or(0.0);
        if p + r == 0.0 {
            "0.00".to_string()
        } else {
            fmt2(2.0 * p * r / (p + r))
        }
    };

    // Suggestion statistics over true-positive words.
    let tps: Vec<&AccuracyResult> = report
        .results
        .iter()
        .filter(|r| classify(r) == Class::Tp)
        .collect();
    let n = tps.len();

    let first_count = tps.iter().filter(|r| r.position == Some(0)).count();
    let top5_count = tps
        .iter()
        .filter(|r| matches!(r.position, Some(p) if p < 5))
        .count();
    let anywhere_count = tps.iter().filter(|r| r.position.is_some()).count();
    let nosugg_count = tps.iter().filter(|r| r.suggestions.is_empty()).count();
    let onlywrong_count = tps
        .iter()
        .filter(|r| r.position.is_none() && !r.suggestions.is_empty())
        .count();
    let with_suggestions = tps.iter().filter(|r| !r.suggestions.is_empty()).count();
    let total_suggestions: usize = tps.iter().map(|r| r.suggestions.len()).sum();

    let s_precision = if with_suggestions == 0 {
        "0.00".to_string()
    } else {
        fmt2(anywhere_count as f64 / with_suggestions as f64 * 100.0)
    };
    let s_recall = pct2(anywhere_count, n);
    let s_accuracy = if total_suggestions == 0 {
        "0.00".to_string()
    } else {
        fmt2(anywhere_count as f64 / total_suggestions as f64 * 100.0)
    };
    let s_fscore = {
        let p: f64 = s_precision.parse().unwrap_or(0.0);
        let r: f64 = s_recall.parse().unwrap_or(0.0);
        if p + r == 0.0 {
            "0.00".to_string()
        } else {
            fmt2(2.0 * p * r / (p + r))
        }
    };

    let cpu_total = total_cpu_time(&report.results);

    Stats {
        title: speller_title(report),
        config_json: serde_json::to_string_pretty(&report.config).unwrap_or_default(),
        total_words,
        real_wps: words_per_second(report.total_time, total_words),
        real_total: human_time(report.total_time),
        cpu_wps: words_per_second(cpu_total, total_words),
        cpu_total: human_time(cpu_total),
        avg_per_word: human_time_millis(s.average_time),
        avg_per_word_95: human_time_millis(s.average_time_95pc),
        tp,
        fneg,
        tn,
        fp,
        tp_pct: pct1(tp, total_words),
        fn_pct: pct1(fneg, total_words),
        tn_pct: pct1(tn, total_words),
        fp_pct: pct1(fp, total_words),
        c_precision: format_metric(&c_precision),
        c_recall: format_metric(&c_recall),
        c_accuracy: format_metric(&c_accuracy),
        c_fscore: format_metric(&c_fscore),
        first_count,
        first_pct: pct2(first_count, n),
        top5_count,
        top5_pct: pct2(top5_count, n),
        anywhere_count,
        anywhere_pct: pct2(anywhere_count, n),
        nosugg_count,
        nosugg_pct: pct2(nosugg_count, n),
        onlywrong_count,
        onlywrong_pct: pct2(onlywrong_count, n),
        avg_position: s.average_position_of_correct,
        avg_suggestions: s.average_suggestions_for_correct,
        s_precision,
        s_recall,
        s_accuracy,
        s_fscore,
    }
}

// ===========================================================================
// Loaded report + the derived row view
// ===========================================================================

/// A report as the UI holds it: stats computed once, results stored once,
/// plus per-row data for filtering. Everything downstream (sorting, filtering,
/// paging) works on indices into `results`, so no result is ever copied.
struct LoadedReport {
    stats: Stats,
    results: Vec<AccuracyResult>,
    /// Lowercased `input` and `expected`, newline-joined, per result: what the
    /// search box matches against, so typing doesn't re-lowercase every row.
    search_keys: Vec<String>,
    /// Number of results per classification, indexed by `class_index`.
    class_counts: [usize; 4],
}

impl LoadedReport {
    fn new(report: Report) -> Self {
        let stats = compute_stats(&report);
        let mut class_counts = [0; 4];
        let search_keys = report
            .results
            .iter()
            .map(|r| {
                class_counts[class_index(classify(r))] += 1;
                let mut key = r.input.to_lowercase();
                if let Some(exp) = &r.expected {
                    key.push('\n');
                    key.push_str(&exp.to_lowercase());
                }
                key
            })
            .collect();
        Self {
            stats,
            results: report.results,
            search_keys,
            class_counts,
        }
    }
}

/// Shared handle to the loaded report. Cloning it (e.g. into every row's
/// props) copies a pointer, and props diffing compares pointers rather than
/// walking every suggestion of every row.
#[derive(Clone)]
struct ReportRef(Rc<LoadedReport>);

impl PartialEq for ReportRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// Indices of the results to list: those whose class is enabled in
/// `classes` and whose words contain `query` (case-insensitive), ordered by
/// `mode` (`"<field>:asc"` / `"<field>:desc"`, `None` for input order). Ties
/// break on input order, so every sort is deterministic.
fn row_view(rep: &LoadedReport, classes: [bool; 4], query: &str, mode: Option<&str>) -> Vec<u32> {
    let query = query.trim().to_lowercase();
    let results = &rep.results;
    let r = |i: u32| &results[i as usize];
    let mut view: Vec<u32> = (0..results.len() as u32)
        .filter(|&i| classes[class_index(classify(r(i)))])
        .filter(|&i| query.is_empty() || rep.search_keys[i as usize].contains(&query))
        .collect();
    let Some((field, dir)) = mode.and_then(|m| m.split_once(':')) else {
        return view;
    };
    match field {
        // Slowest first.
        "time" => view.sort_by(|&a, &b| r(b).time.cmp(&r(a).time).then(a.cmp(&b))),
        "position" => view.sort_by_key(|&i| (position_key(r(i)), i)),
        "distance" => view.sort_by_key(|&i| (r(i).distance, i)),
        "classification" => view.sort_by_key(|&i| (class_order(r(i)), i)),
        _ => {}
    }
    if dir == "desc" {
        view.reverse();
    }
    view
}

/// `12345` → `"12,345"`.
fn group_digits(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// ===========================================================================
// Theme handling (light / dark / auto, persisted in localStorage)
// ===========================================================================

const THEMES: [&str; 3] = ["light", "dark", "auto"];

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn saved_theme() -> String {
    local_storage()
        .and_then(|s| s.get_item("theme").ok().flatten())
        .filter(|t| THEMES.contains(&t.as_str()))
        .unwrap_or_else(|| "auto".to_string())
}

fn prefers_dark() -> bool {
    web_sys::window()
        .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
        .map(|mq| mq.matches())
        .unwrap_or(false)
}

fn apply_theme(theme: &str) {
    let resolved = if theme == "auto" {
        if prefers_dark() { "dark" } else { "light" }
    } else {
        theme
    };
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let _ = el.set_attribute("data-theme", resolved);
    }
}

fn save_theme(theme: &str) {
    if let Some(s) = local_storage() {
        let _ = s.set_item("theme", theme);
    }
}

fn theme_icon(theme: &str) -> &'static str {
    match theme {
        "light" => "\u{2600}\u{fe0f}", // ☀️
        "dark" => "\u{1f319}",         // 🌙
        _ => "\u{1f4bb}",              // 💻
    }
}

fn theme_label(theme: &str) -> &'static str {
    match theme {
        "light" => "Light",
        "dark" => "Dark",
        _ => "Auto",
    }
}

// ===========================================================================
// Data fetch
// ===========================================================================

/// `window.__DOCS_DATA_BASE__`, when the theme layout sets it: the repo's
/// `generated/docs-data` branch served via `raw.githubusercontent.com` (see
/// jekyll-theme-giellalt's `_layouts/typosreport.html`). Falls back to `""`
/// (same-origin relative fetch) for local `trunk serve` testing, mirroring
/// the former Svelte bundle's `(window.__DOCS_DATA_BASE__||"")`.
fn docs_data_base() -> String {
    web_sys::window()
        .and_then(|w| {
            js_sys::Reflect::get(&w, &wasm_bindgen::JsValue::from_str("__DOCS_DATA_BASE__")).ok()
        })
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}

/// `speller-accuracy.json.gz`, or `speller-accuracy-<tag>.json.gz` for a
/// variant (dialect/area/orthography/writing system — see `fetch_variants`
/// below). The reports are published gzipped because the largest ones pass
/// GitHub's 100 MB file limit as plain JSON; raw.githubusercontent.com serves
/// the `.gz` as opaque bytes, so it is inflated here. Falls back to the plain
/// `.json` when there is no `.gz` (a repo not yet rebuilt, or local testing).
async fn fetch_report(variant: Option<&str>) -> Result<Report, String> {
    let file = match variant {
        Some(tag) => format!("speller-accuracy-{tag}.json"),
        None => "speller-accuracy.json".to_string(),
    };
    let base = docs_data_base();
    let gz_url = format!("{base}{file}.gz");
    let mut url = gz_url.clone();
    let mut resp = gloo_net::http::Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to load {url}: {e}"))?;
    if resp.status() == 404 {
        url = format!("{base}{file}");
        resp = gloo_net::http::Request::get(&url)
            .send()
            .await
            .map_err(|e| format!("Failed to load {url}: {e}"))?;
    }
    if !resp.ok() {
        let tried = if url == gz_url {
            url.clone()
        } else {
            format!("{gz_url} or {url}")
        };
        return Err(format!(
            "Failed to load {tried}: {} {}",
            resp.status(),
            resp.status_text()
        ));
    }
    let bytes = resp
        .binary()
        .await
        .map_err(|e| format!("Failed to load {url}: {e}"))?;
    parse_report(&bytes).map_err(|e| format!("Failed to parse {url}: {e}"))
}

/// Parse a report that may or may not be gzipped, going by the gzip magic
/// bytes rather than the file name: a host that sends the `.gz` with
/// `Content-Encoding: gzip` has the browser inflate it before we see it.
///
/// Inflates into a buffer before parsing rather than handing the decoder to
/// `serde_json::from_reader`, which reads a byte at a time and made a
/// sme-sized report (~100 MB inflated) take minutes in wasm.
fn parse_report(bytes: &[u8]) -> Result<Report, String> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        use std::io::Read;
        let mut json = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_end(&mut json)
            .map_err(|e| format!("gzip: {e}"))?;
        serde_json::from_slice(&json).map_err(|e| e.to_string())
    } else {
        serde_json::from_slice(bytes).map_err(|e| e.to_string())
    }
}

// ===========================================================================
// Variant selection (dialects / areas / orthographies / writing systems)
// ===========================================================================

/// One `<option>` in the variant selector. `tag: None` is the always-present
/// "Default" entry (`speller-accuracy.json.gz`); `Some(code)` fetches
/// `speller-accuracy-<code>.json.gz`.
#[derive(Clone, PartialEq)]
struct VariantOption {
    tag: Option<String>,
    label: String,
}

#[derive(Deserialize)]
struct VariantEntry {
    code: String,
}

/// Mirrors giella-core's `pkg-variants.json` shape: up to four independent
/// groups of variants, any of which may be absent.
#[derive(Deserialize, Default)]
struct VariantsFile {
    #[serde(default)]
    areas: Option<Vec<VariantEntry>>,
    #[serde(default)]
    dialects: Option<Vec<VariantEntry>>,
    #[serde(default)]
    orthographies: Option<Vec<VariantEntry>>,
    #[serde(default)]
    writing_systems: Option<Vec<VariantEntry>>,
}

/// Fetches `pkg-variants.json` and flattens it into a `Default` option plus
/// one option per variant, in areas/dialects/orthographies/writing_systems
/// order. Best-effort: any failure (missing file, bad JSON — most repos have
/// no variants at all) just yields `[Default]`, same as the Svelte bundle.
async fn fetch_variants() -> Vec<VariantOption> {
    let default_only = vec![VariantOption {
        tag: None,
        label: "Default".to_string(),
    }];

    let url = format!("{}pkg-variants.json", docs_data_base());
    let Ok(resp) = gloo_net::http::Request::get(&url).send().await else {
        return default_only;
    };
    if !resp.ok() {
        return default_only;
    }
    let Ok(vf) = resp.json::<VariantsFile>().await else {
        return default_only;
    };

    let mut out = default_only;
    for group in [vf.areas, vf.dialects, vf.orthographies, vf.writing_systems]
        .into_iter()
        .flatten()
    {
        for entry in group {
            out.push(VariantOption {
                tag: Some(entry.code.clone()),
                label: entry.code,
            });
        }
    }
    out
}

/// Classification codes for the `show` URL parameter, indexed by
/// `class_index`.
const CLASS_CODES: [&str; 4] = ["tp", "fn", "tn", "fp"];

/// Fields the results can be sorted by, as used in `sort_mode`
/// (`"<field>:asc"` / `"<field>:desc"`) and the `sort` URL parameter.
const SORT_FIELDS: [&str; 4] = ["time", "position", "distance", "classification"];

fn is_sort_mode(mode: &str) -> bool {
    mode.split_once(':')
        .is_some_and(|(f, d)| SORT_FIELDS.contains(&f) && (d == "asc" || d == "desc"))
}

/// The view a link carries in its query string, so the address bar is always
/// a shareable link to what is on screen:
/// `?variant=<tag>&q=<search>&show=fn,fp&sort=time:desc&page=3`. Values at
/// their defaults are left out, so a plain view keeps a plain URL. A row
/// permalink (`#<input>`) rides along in the fragment (see `anchor_from_url`).
#[derive(Clone, PartialEq)]
struct UrlState {
    variant: Option<String>,
    query: String,
    /// Which classes to list, indexed by `class_index`.
    classes: [bool; 4],
    sort: Option<String>,
    /// 0-based (the URL's `page` is 1-based).
    page: usize,
}

impl UrlState {
    /// The current page's URL state. Unknown or malformed values fall back to
    /// their defaults.
    fn from_url() -> Self {
        let params = current_url().map(|u| u.search_params());
        let get = |k: &str| params.as_ref().and_then(|p| p.get(k));
        let classes = match get("show") {
            None => [true; 4],
            Some(show) => {
                let mut classes = [false; 4];
                for code in show.split(',') {
                    if let Some(i) = CLASS_CODES.iter().position(|&c| c == code.trim()) {
                        classes[i] = true;
                    }
                }
                classes
            }
        };
        UrlState {
            variant: get("variant").filter(|v| !v.is_empty()),
            query: get("q").unwrap_or_default(),
            classes,
            sort: get("sort").filter(|s| is_sort_mode(s)),
            page: get("page")
                .and_then(|p| p.parse::<usize>().ok())
                .map_or(0, |p| p.saturating_sub(1)),
        }
    }

    /// Writes this state into `url`'s query string, replacing any previous
    /// values (in a fixed order, so equal states give equal URLs) and leaving
    /// other parameters alone.
    ///
    /// Built by hand rather than with `URLSearchParams`, which form-encodes
    /// `,` and `:` and would turn `show=fn,fp&sort=time:desc` into
    /// `show=fn%2Cfp&sort=time%3Adesc`; both are legal as-is in a query.
    fn write_search(&self, url: &web_sys::Url) {
        let params = url.search_params();
        for key in ["variant", "q", "show", "sort", "page"] {
            params.delete(key);
        }
        let mut parts: Vec<String> = Vec::new();
        let others = params.to_string().as_string().unwrap_or_default();
        if !others.is_empty() {
            parts.push(others);
        }
        let mut add = |key: &str, value: &str| parts.push(format!("{key}={}", encode_query_value(value)));
        if let Some(v) = &self.variant {
            add("variant", v);
        }
        if !self.query.is_empty() {
            add("q", &self.query);
        }
        if self.classes != [true; 4] {
            let show: Vec<&str> = CLASS_CODES
                .iter()
                .zip(self.classes)
                .filter_map(|(&code, on)| on.then_some(code))
                .collect();
            add("show", &show.join(","));
        }
        if let Some(s) = &self.sort {
            add("sort", s);
        }
        if self.page > 0 {
            add("page", &(self.page + 1).to_string());
        }
        url.set_search(&parts.join("&"));
    }
}

/// Percent-encodes a query parameter value, leaving `,` and `:` readable.
/// Anything that would end or split the value (`&`, `=`, `#`, `+`, spaces,
/// ...) is still escaped, as are non-ASCII letters (browsers show those
/// decoded in the address bar).
fn encode_query_value(value: &str) -> String {
    String::from(js_sys::encode_uri_component(value))
        .replace("%2C", ",")
        .replace("%3A", ":")
}

fn current_url() -> Option<web_sys::Url> {
    let href = web_sys::window()?.location().href().ok()?;
    web_sys::Url::new(&href).ok()
}

/// Sets `signal` to `value` only if that changes it, so re-applying the same
/// URL state doesn't wake everything subscribed to it.
fn set_if_changed<T: PartialEq + 'static>(mut signal: Signal<T>, value: T) {
    if *signal.peek() != value {
        signal.set(value);
    }
}

/// The word named by the URL fragment (`#<input>`, the rows' permalinks), if
/// any.
fn anchor_from_url() -> Option<String> {
    let hash = web_sys::window()?.location().hash().ok()?;
    let raw = hash.strip_prefix('#').filter(|h| !h.is_empty())?;
    js_sys::decode_uri_component(raw)
        .ok()
        .and_then(|s| s.as_string())
        .or_else(|| Some(raw.to_string()))
}

fn scroll_to_id(id: &str) {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
    {
        el.scroll_into_view();
    }
}

// ===========================================================================
// Components
// ===========================================================================

/// Suggestions listed per row until "Show all" is clicked. Lists can run to
/// ~100 entries, and rendering every one for every row is most of the page's
/// DOM on a large report.
const SUGGESTION_LIMIT: usize = 10;

#[component]
fn ResultRow(report: ReportRef, index: u32) -> Element {
    let mut expanded = use_signal(|| false);
    let result = &report.0.results[index as usize];
    let cls = classify(result);
    let n = result.suggestions.len();
    let collapsed = n > SUGGESTION_LIMIT && !expanded();
    // Collapsed: the top suggestions, plus the correct one wherever it ranks
    // (after a gap marker if it isn't right after them), so a row still shows
    // whether and where the speller found the correction.
    let correct_below = result.position.filter(|&p| collapsed && p >= SUGGESTION_LIMIT);
    let shown: Vec<usize> = if collapsed {
        (0..SUGGESTION_LIMIT).chain(correct_below).collect()
    } else {
        (0..n).collect()
    };
    let gap_before = correct_below.filter(|&p| p > SUGGESTION_LIMIT);
    let label_color = if cls == Class::Fp || cls == Class::Fn_ {
        "#d00"
    } else {
        "#080"
    };

    rsx! {
        tr { class: result_class(result), id: "{result.input}",
            td { class: "right",
                p {
                    a { href: "#{result.input}", class: "word", "{result.input}" }
                    if let Some(exp) = result.expected.as_ref() {
                        " \u{2192} "
                        span { class: "word", "{exp}" }
                    }
                }
                p {
                    strong { "Result: " }
                    span { style: "font-weight: bold; color: {label_color};", "{class_label(cls)}" }
                    if cls == Class::Tp {
                        {match result.position {
                            None => rsx! { br {} small { "Not in suggestions" } },
                            Some(0) => rsx! { br {} small { "Top suggestion" } },
                            Some(p) => rsx! { br {} small { "Suggestion {p + 1}" } },
                        }}
                    }
                }
                if cls == Class::Tp || cls == Class::Fn_ {
                    p {
                        strong { "Edit distance: " }
                        "{result.distance}"
                    }
                }
                if cls == Class::Tp {
                    p {
                        strong { "Time: " }
                        "{human_time_millis(result.time)}"
                    }
                }
            }
            td {
                if result.false_accept && cls == Class::Fn_ {
                    em { "Incorrectly accepted as correct" }
                } else if !result.suggestions.is_empty() {
                    ol {
                        for i in shown {
                            if gap_before == Some(i) {
                                li { class: "suggestion-gap", "\u{2026}" }
                            }
                            li { value: "{i + 1}",
                                span {
                                    class: if result.position == Some(i) { "word word-correct" } else { "word" },
                                    "{result.suggestions[i].value}"
                                }
                                small {
                                    "{format_weight(result.suggestions[i].weight)} "
                                    if let Some(wd) = result.suggestions[i].weight_details.as_ref() {
                                        span { class: "weight-details", "{weight_details_str(wd)}" }
                                    }
                                }
                            }
                        }
                    }
                    if n > SUGGESTION_LIMIT {
                        button {
                            class: "link-button",
                            onclick: move |_| expanded.set(!expanded()),
                            if collapsed {
                                "Show all {n} suggestions"
                            } else {
                                "Show fewer"
                            }
                        }
                    }
                } else if cls != Class::Tn {
                    em { "No suggestions" }
                }
            }
        }
    }
}

#[component]
fn StatsView(stats: Stats) -> Element {
    let s = &stats;
    rsx! {
        h1 { "{s.title} - Accuracy Report" }

        h2 { "Speller Configuration" }
        div { class: "config-block",
            pre { "{s.config_json}" }
        }

        h2 { "Performance Statistics" }
        div { class: "accuracy-stats-container",
            div {
                h3 { "Runtime" }
                table { class: "stats-table",
                    tr {
                        th {}
                        th { "Words per second" }
                        th { "Total runtime" }
                    }
                    tr {
                        th {
                            "Real"
                            br {}
                            small { "(clock time, parallelised processing)" }
                        }
                        td { "{s.real_wps}" }
                        td { "{s.real_total}" }
                    }
                    tr {
                        th {
                            "CPU"
                            br {}
                            small { "(estimated serial processing time)" }
                        }
                        td { "{s.cpu_wps}" }
                        td { "{s.cpu_total}" }
                    }
                    tr {
                        th { "Average per word" }
                        td { "-" }
                        td { "{s.avg_per_word}" }
                    }
                    tr {
                        th {
                            "Average per word (95%)"
                            br {}
                            small { "(excluding slowest 5%)" }
                        }
                        td { "-" }
                        td { "{s.avg_per_word_95}" }
                    }
                }
            }
            div {
                h3 { "Spell Checker Classification" }
                div { class: "accuracy-stats-container",
                    table { class: "stats-table",
                        tr {
                            th {
                                "True positive"
                                br {}
                                small { "(correctly flagged)" }
                            }
                            td { "{s.tp}" }
                            td { "{s.tp_pct}" }
                        }
                        tr {
                            th {
                                "False negative"
                                br {}
                                small { "(incorrectly accepted)" }
                            }
                            td { "{s.fneg}" }
                            td { "{s.fn_pct}" }
                        }
                        tr {
                            th {
                                "True negative"
                                br {}
                                small { "(correctly accepted)" }
                            }
                            td { "{s.tn}" }
                            td { "{s.tn_pct}" }
                        }
                        tr {
                            th {
                                "False positive"
                                br {}
                                small { "(incorrectly flagged)" }
                            }
                            td { "{s.fp}" }
                            td { "{s.fp_pct}" }
                        }
                        tr {
                            th { "Total words" }
                            td { "{s.total_words}" }
                            td { "100%" }
                        }
                    }
                    div { class: "metrics-box",
                        ul {
                            li {
                                strong { "Precision:" }
                                " {s.c_precision}"
                                small { "Of words flagged as incorrect, how many are actually incorrect" }
                            }
                            li {
                                strong { "Recall:" }
                                " {s.c_recall}"
                                small { "Of words that are actually incorrect, how many were flagged as incorrect" }
                            }
                            li {
                                strong { "Accuracy:" }
                                " {s.c_accuracy}"
                                small { "Correct classifications (TP+TN) out of all words" }
                            }
                            li {
                                strong { "F-score:" }
                                " {s.c_fscore}"
                                small { "Harmonic mean of precision and recall" }
                            }
                        }
                    }
                }
            }
        }

        h2 { "Suggestion Statistics" }
        p {
            em { "These statistics apply only to true positive words ({s.tp} words)." }
        }
        div { class: "accuracy-stats-container",
            div {
                table { class: "stats-table",
                    tr {
                        th { "In 1st position" }
                        td { "{s.first_count}" }
                        td { "{s.first_pct}%" }
                    }
                    tr {
                        th { "In top 5" }
                        td { "{s.top5_count}" }
                        td { "{s.top5_pct}%" }
                    }
                    tr {
                        th { "Anywhere" }
                        td { "{s.anywhere_count}" }
                        td { "{s.anywhere_pct}%" }
                    }
                    tr {
                        th { "No suggestions" }
                        td { "{s.nosugg_count}" }
                        td { "{s.nosugg_pct}%" }
                    }
                    tr {
                        th { "Only wrong" }
                        td { "{s.onlywrong_count}" }
                        td { "{s.onlywrong_pct}%" }
                    }
                }
                ul {
                    li { "Average position of correct: {s.avg_position:.2}" }
                    li { "Average suggestions for correct: {s.avg_suggestions:.2}" }
                }
            }
            div { class: "metrics-box",
                ul {
                    li {
                        strong { "Precision:" }
                        " {s.s_precision}%"
                        small { "Of words that got suggestions, how many got the correct one" }
                    }
                    li {
                        strong { "Recall:" }
                        " {s.s_recall}%"
                        small { "Of all misspelled words, how many got the correct suggestion" }
                    }
                    li {
                        strong { "Accuracy:" }
                        " {s.s_accuracy}%"
                        small { "Correct suggestions out of all suggestions (indicates noise level)" }
                    }
                    li {
                        strong { "F-score:" }
                        " {s.s_fscore}%"
                        small { "Harmonic mean of precision and recall; high only when both are good" }
                    }
                }
            }
        }
    }
}

fn sort_mode_label(mode: Option<&str>) -> &'static str {
    match mode {
        None => "Sorted by input order",
        Some("time:asc") => "Sorted by time, ascending (slowest first)",
        Some("time:desc") => "Sorted by time, descending (fastest first)",
        Some("position:asc") => "Sorted by position, ascending (best first)",
        Some("position:desc") => "Sorted by position, descending (worst first)",
        Some("distance:asc") => "Sorted by edit distance, ascending (smallest first)",
        Some("distance:desc") => "Sorted by edit distance, descending (largest first)",
        Some("classification:asc") => {
            "Sorted by classification (TP \u{2192} TN \u{2192} FP \u{2192} FN)"
        }
        Some("classification:desc") => {
            "Sorted by classification (FN \u{2192} FP \u{2192} TN \u{2192} TP)"
        }
        Some(_) => "Sorted in some unknown way (this is a bug)",
    }
}

/// Rows per results page. Rendering every row of a large report (sme: ~16k
/// rows) at once froze the page for minutes; a page's worth renders at once.
const PAGE_SIZE: usize = 100;

/// The app's state signals, bundled so they can be handed around as one
/// (signals are `Copy`).
#[derive(Clone, Copy)]
struct AppState {
    loaded: Signal<Option<ReportRef>>,
    load_error: Signal<Option<String>>,
    current_variant: Signal<Option<String>>,
    sort_mode: Signal<Option<String>>,
    /// Which classes to list, indexed by `class_index`.
    class_filter: Signal<[bool; 4]>,
    query: Signal<String>,
    page: Signal<usize>,
    /// A row permalink (`#<input>`) still to be brought into view: its page
    /// is selected, then it is scrolled to once rendered.
    pending_anchor: Signal<Option<String>>,
}

impl AppState {
    /// Takes on a URL's search, filter, sort and page (the variant is loaded
    /// separately, being a fetch).
    fn apply_url_state(self, u: &UrlState) {
        set_if_changed(self.query, u.query.clone());
        set_if_changed(self.class_filter, u.classes);
        set_if_changed(self.sort_mode, u.sort.clone());
        set_if_changed(self.page, u.page);
    }
}

/// Loads `variant`'s report into the shared signals — used for the initial
/// fetch, every variant-selector change, and Back/Forward across variants.
/// Search, filters and sort carry over, so the same view can be compared
/// across variants; the URL follows via the URL-sync effect in `App`.
async fn load_variant(variant: Option<String>, mut st: AppState) {
    st.loaded.set(None);
    st.load_error.set(None);
    match fetch_report(variant.as_deref()).await {
        Ok(rep) => {
            st.loaded.set(Some(ReportRef(Rc::new(LoadedReport::new(rep)))));
            st.current_variant.set(variant);
            st.pending_anchor.set(anchor_from_url());
        }
        Err(e) => st.load_error.set(Some(e)),
    }
}

/// `requested` if it names one of `variants`; unknown variants in a URL are
/// ignored, same as the Svelte bundle.
fn known_variant(requested: Option<String>, variants: &[VariantOption]) -> Option<String> {
    requested.filter(|t| variants.iter().any(|v| v.tag.as_deref() == Some(t.as_str())))
}

#[component]
fn Pager(
    page: usize,
    pages: usize,
    first: usize,
    last: usize,
    matching: usize,
    total: usize,
    on_change: EventHandler<usize>,
) -> Element {
    let of = if matching == total {
        group_digits(total)
    } else {
        format!("{} matching ({} total)", group_digits(matching), group_digits(total))
    };
    rsx! {
        div { class: "pager",
            button { disabled: page == 0, onclick: move |_| on_change.call(0), "\u{00ab} First" }
            button { disabled: page == 0, onclick: move |_| on_change.call(page - 1), "\u{2039} Prev" }
            span { class: "pager-status",
                "Page {page + 1} of {pages} \u{00b7} showing {group_digits(first)}\u{2013}{group_digits(last)} of {of}"
            }
            button { disabled: page + 1 >= pages, onclick: move |_| on_change.call(page + 1), "Next \u{203a}" }
            button { disabled: page + 1 >= pages, onclick: move |_| on_change.call(pages - 1), "Last \u{00bb}" }
        }
    }
}

#[component]
fn App() -> Element {
    let st = AppState {
        loaded: use_signal(|| None),
        load_error: use_signal(|| None),
        current_variant: use_signal(|| None),
        sort_mode: use_signal(|| None),
        class_filter: use_signal(|| [true; 4]),
        query: use_signal(String::new),
        page: use_signal(|| 0),
        pending_anchor: use_signal(|| None),
    };
    let AppState {
        loaded,
        load_error,
        current_variant,
        mut sort_mode,
        mut class_filter,
        mut query,
        mut page,
        mut pending_anchor,
    } = st;
    // A variant to load, requested from outside a Dioxus event handler (the
    // `popstate` listener), where `spawn` isn't available.
    let mut variant_request = use_signal(|| None::<Option<String>>);
    // Whether the URL-sync effect has run since the report loaded (see there).
    let mut url_written = use_signal(|| false);
    let mut theme = use_signal(saved_theme);
    let mut variants = use_signal(|| {
        vec![VariantOption {
            tag: None,
            label: "Default".to_string(),
        }]
    });

    // Discover variants (if any), take on the view the URL describes, then
    // fetch the report once on mount.
    use_future(move || async move {
        let vs = fetch_variants().await;
        variants.set(vs.clone());
        let u = UrlState::from_url();
        st.apply_url_state(&u);
        load_variant(known_variant(u.variant, &vs), st).await;
    });

    let select_variant = move |evt: dioxus::events::FormEvent| {
        let value = evt.value();
        let tag = if value.is_empty() { None } else { Some(value) };
        page.set(0);
        spawn(load_variant(tag, st));
    };

    use_effect(move || {
        let Some(v) = variant_request() else { return };
        variant_request.set(None);
        spawn(load_variant(v, st));
    });

    // The rows to list, as indices into the report's results: recomputed only
    // when the report, sort, filter or search changes (not on paging).
    let view = use_memo(move || {
        let Some(rep) = loaded() else {
            return Rc::new(Vec::new());
        };
        Rc::new(row_view(
            &rep.0,
            class_filter(),
            &query.read(),
            sort_mode.read().as_deref(),
        ))
    });

    // Bring a permalinked row into view: select its page, then scroll to it
    // once that page has rendered (this re-runs after the page change).
    use_effect(move || {
        let Some(word) = pending_anchor() else { return };
        let Some(rep) = loaded() else { return };
        let target = view
            .read()
            .iter()
            .position(|&i| rep.0.results[i as usize].input == word)
            .map(|pos| pos / PAGE_SIZE);
        match target {
            Some(p) if p != page() => page.set(p),
            Some(_) => {
                scroll_to_id(&word);
                pending_anchor.set(None);
            }
            // Filtered out, or not in this report.
            None => pending_anchor.set(None),
        }
    });

    // Keep the URL describing the current view (see `UrlState`), so the
    // address bar is always a shareable link. A search edit replaces the
    // history entry (Back shouldn't undo it a keystroke at a time); paging,
    // sorting, filtering and variant changes push one, so Back undoes them.
    use_effect(move || {
        let Some(rep) = loaded() else { return };
        // Let a permalink settle on its page first.
        if pending_anchor().is_some() {
            return;
        }
        let rows = view();
        let pages = rows.len().div_ceil(PAGE_SIZE).max(1);
        let p = page();
        if p >= pages {
            // e.g. `page=` past the end of a link, or of a narrower view.
            page.set(pages - 1);
            return;
        }
        let state = UrlState {
            variant: current_variant(),
            query: query(),
            classes: class_filter(),
            sort: sort_mode(),
            page: p,
        };
        let (Some(window), Some(url)) = (web_sys::window(), current_url()) else {
            return;
        };
        let before = UrlState::from_url();
        state.write_search(&url);
        // Keep a `#<input>` permalink only while its row is on the page shown;
        // otherwise it would pull the link's recipient to another page.
        let shown = &rows[p * PAGE_SIZE..((p + 1) * PAGE_SIZE).min(rows.len())];
        let keep_hash = anchor_from_url()
            .is_some_and(|w| shown.iter().any(|&i| rep.0.results[i as usize].input == w));
        if !keep_hash {
            url.set_hash("");
        }
        // The first run after load only tidies the URL the page was opened
        // with (even if that changes nothing), so it never adds an entry.
        let first = !*url_written.peek();
        if first {
            url_written.set(true);
        }
        let href = url.href();
        if window.location().href().ok().as_deref() == Some(href.as_str()) {
            return;
        }
        let replace = first || state.query != before.query;
        if let Ok(history) = window.history() {
            let null = wasm_bindgen::JsValue::NULL;
            let _ = if replace {
                history.replace_state_with_url(&null, "", Some(&href))
            } else {
                history.push_state_with_url(&null, "", Some(&href))
            };
        }
    });

    // One-time setup: apply the saved theme and react to OS theme changes
    // while in "auto" mode; follow permalink clicks/edits (`#<input>`) to rows
    // on other pages; restore the view on Back/Forward.
    use_hook(move || {
        apply_theme(&saved_theme());
        let Some(window) = web_sys::window() else { return };
        if let Ok(Some(mq)) = window.match_media("(prefers-color-scheme: dark)") {
            let cb = Closure::<dyn FnMut()>::new(move || {
                if theme.peek().as_str() == "auto" {
                    apply_theme("auto");
                }
            });
            let _ = mq.add_event_listener_with_callback("change", cb.as_ref().unchecked_ref());
            cb.forget();
        }
        let cb = Closure::<dyn FnMut()>::new(move || pending_anchor.set(anchor_from_url()));
        let _ = window.add_event_listener_with_callback("hashchange", cb.as_ref().unchecked_ref());
        cb.forget();
        let cb = Closure::<dyn FnMut()>::new(move || {
            let u = UrlState::from_url();
            st.apply_url_state(&u);
            set_if_changed(pending_anchor, anchor_from_url());
            let variant = known_variant(u.variant, &variants.peek());
            if variant != *current_variant.peek() {
                variant_request.set(Some(variant));
            }
        });
        let _ = window.add_event_listener_with_callback("popstate", cb.as_ref().unchecked_ref());
        cb.forget();
    });

    let cycle_theme = move |_| {
        let next = match theme().as_str() {
            "light" => "dark",
            "dark" => "auto",
            _ => "light",
        };
        save_theme(next);
        apply_theme(next);
        theme.set(next.to_string());
    };

    // Sort buttons toggle between a field's ascending and descending order.
    let mut sort_by = move |field: &str| {
        let asc = format!("{field}:asc");
        let next = if sort_mode.read().as_deref() == Some(asc.as_str()) {
            format!("{field}:desc")
        } else {
            asc
        };
        sort_mode.set(Some(next));
        page.set(0);
    };

    let theme_val = theme();
    let rep = loaded();
    let err = load_error();
    let mode = sort_mode();
    let variant_list = variants();
    let active_variant = current_variant();

    let rows = view();
    let matching = rows.len();
    let pages = matching.div_ceil(PAGE_SIZE).max(1);
    let cur = page().min(pages - 1);
    let start = cur * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(matching);
    let classes = class_filter();
    let q = query();

    rsx! {
        button {
            class: "theme-toggle",
            onclick: cycle_theme,
            "aria-label": "Toggle theme, current mode: {theme_label(&theme_val)}",
            title: "Switch between light, dark, and auto theme modes",
            span { "{theme_icon(&theme_val)}" }
            span { "{theme_label(&theme_val)}" }
        }

        if variant_list.len() > 1 {
            div { class: "variant-selector",
                label { r#for: "variant-select", "Variant:" }
                select {
                    id: "variant-select",
                    onchange: select_variant,
                    for v in variant_list.iter() {
                        option {
                            key: "{v.tag.clone().unwrap_or_default()}",
                            value: "{v.tag.clone().unwrap_or_default()}",
                            selected: active_variant == v.tag,
                            "{v.label}"
                        }
                    }
                }
            }
        }

        div { class: "container",
            if let Some(r) = rep.as_ref() {
                StatsView { stats: r.0.stats.clone() }
            }

            if let Some(e) = err {
                div { class: "error-message",
                    h2 { "Error Loading Report" }
                    p { "{e}" }
                    p {
                        strong { "For giellalt lang- repos:" }
                    }
                    ul {
                        li {
                            "Enable "
                            code { "spellers" }
                            " in "
                            code { ".build-config.yml" }
                            " so CI generates "
                            code { "speller-accuracy.json.gz" }
                        }
                        li {
                            "Check that the repo's "
                            code { "generated/docs-data" }
                            " branch has a "
                            code { "speller-accuracy.json.gz" }
                            " from a recent build (published by "
                            code { "divvun-actions run lang-docs-publish" }
                            ")"
                        }
                        li {
                            "This page loads it from "
                            code { "raw.githubusercontent.com" }
                            " via "
                            code { "window.__DOCS_DATA_BASE__" }
                            ", set by the theme's "
                            code { "typosreport" }
                            " layout"
                        }
                    }
                    p {
                        strong { "For local testing:" }
                    }
                    p { "Generate a report file:" }
                    pre { "divvunspell accuracy -o speller-accuracy.json typos.tsv language.zhfst" }
                    p {
                        "Then copy the speller-accuracy.json file (or a gzipped speller-accuracy.json.gz) next to the built "
                        code { "index.html" }
                        " (the Trunk "
                        code { "dist/" }
                        " directory)."
                    }
                }
            } else if let Some(r) = rep {
                h2 { id: "detailed-results", "Detailed Results" }

                div { class: "filter-bar",
                    input {
                        r#type: "search",
                        class: "search-input",
                        placeholder: "Search words\u{2026}",
                        "aria-label": "Search input and expected words",
                        value: "{q}",
                        oninput: move |e| {
                            query.set(e.value());
                            page.set(0);
                        },
                    }
                    for (i , c) in CLASSES.into_iter().enumerate() {
                        label { class: "class-filter",
                            input {
                                r#type: "checkbox",
                                checked: classes[i],
                                onchange: move |_| {
                                    class_filter.with_mut(|f| f[i] = !f[i]);
                                    page.set(0);
                                },
                            }
                            " {class_label(c)} ({group_digits(r.0.class_counts[i])})"
                        }
                    }
                }

                p { "{sort_mode_label(mode.as_deref())}" }
                button {
                    onclick: move |_| {
                        sort_mode.set(None);
                        page.set(0);
                    },
                    "Sort by Input Order"
                }
                button { onclick: move |_| sort_by("time"), "Sort by Time" }
                button { onclick: move |_| sort_by("position"), "Sort by Position" }
                button { onclick: move |_| sort_by("distance"), "Sort by Edit Distance" }
                button { onclick: move |_| sort_by("classification"), "Sort by Classification" }

                if matching == 0 {
                    p { class: "no-results", em { "No results match the current filters." } }
                } else {
                    Pager {
                        page: cur,
                        pages,
                        first: start + 1,
                        last: end,
                        matching,
                        total: r.0.results.len(),
                        on_change: move |p| page.set(p),
                    }
                    table { class: "table",
                        thead {
                            tr {
                                th { "Spelling error data" }
                                th { "Suggestion list" }
                            }
                        }
                        tbody {
                            // Keyed by index into `results` (never reordered),
                            // not by `input`, which can repeat in a corpus.
                            for &i in rows[start..end].iter() {
                                ResultRow { key: "{i}", report: r.clone(), index: i }
                            }
                        }
                    }
                    Pager {
                        page: cur,
                        pages,
                        first: start + 1,
                        last: end,
                        matching,
                        total: r.0.results.len(),
                        on_change: move |p| {
                            page.set(p);
                            scroll_to_id("detailed-results");
                        },
                    }
                }
            } else {
                div { class: "loading", "Loading..." }
            }
        }
    }
}
