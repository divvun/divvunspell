//! Dioxus web viewer for divvunspell accuracy reports.
//!
//! Fetches `report.json` (served alongside the page, e.g. on GitHub Pages) and
//! renders the speller configuration, performance/classification/suggestion
//! statistics, and a sortable, colour-coded results table. This is a Rust/WASM
//! reimplementation of the former Svelte app — no Node toolchain required.

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

async fn fetch_report() -> Result<Report, String> {
    let resp = gloo_net::http::Request::get("report.json")
        .send()
        .await
        .map_err(|e| format!("Failed to load report.json: {e}"))?;
    if !resp.ok() {
        return Err(format!(
            "Failed to load report.json: {} {}",
            resp.status(),
            resp.status_text()
        ));
    }
    resp.json::<Report>()
        .await
        .map_err(|e| format!("Failed to parse report.json: {e}"))
}

// ===========================================================================
// Components
// ===========================================================================

#[component]
fn ResultRow(result: AccuracyResult) -> Element {
    let cls = classify(&result);
    let label_color = if cls == Class::Fp || cls == Class::Fn_ {
        "#d00"
    } else {
        "#080"
    };

    rsx! {
        tr { class: result_class(&result), id: "{result.input}",
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
                        for (i , sugg) in result.suggestions.iter().enumerate() {
                            li {
                                span {
                                    class: if result.position == Some(i) { "word word-correct" } else { "word" },
                                    "{sugg.value}"
                                }
                                small {
                                    "{format_weight(sugg.weight)} "
                                    if let Some(wd) = sugg.weight_details.as_ref() {
                                        span { class: "weight-details", "{weight_details_str(wd)}" }
                                    }
                                }
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

#[component]
fn App() -> Element {
    let mut report = use_signal(|| None::<Report>);
    let mut results = use_signal(Vec::<AccuracyResult>::new);
    let mut original_results = use_signal(Vec::<AccuracyResult>::new);
    let mut load_error = use_signal(|| None::<String>);
    let mut sort_mode = use_signal(|| None::<String>);
    let mut theme = use_signal(saved_theme);

    // Fetch the report once on mount.
    use_future(move || async move {
        match fetch_report().await {
            Ok(rep) => {
                original_results.set(rep.results.clone());
                results.set(rep.results.clone());
                report.set(Some(rep));
            }
            Err(e) => load_error.set(Some(e)),
        }
    });

    // One-time theme setup: apply the saved theme and react to OS theme changes
    // while in "auto" mode.
    use_hook(move || {
        apply_theme(&saved_theme());
        if let Some(mq) = web_sys::window()
            .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok().flatten())
        {
            let cb = Closure::<dyn FnMut()>::new(move || {
                if theme.peek().as_str() == "auto" {
                    apply_theme("auto");
                }
            });
            let _ = mq.add_event_listener_with_callback("change", cb.as_ref().unchecked_ref());
            cb.forget();
        }
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

    let theme_val = theme();
    let stats = report.read().as_ref().map(compute_stats);
    let loaded = stats.is_some();
    let err = load_error();
    let rows = results.read().clone();
    let mode = sort_mode();

    rsx! {
        button {
            class: "theme-toggle",
            onclick: cycle_theme,
            "aria-label": "Toggle theme, current mode: {theme_label(&theme_val)}",
            title: "Switch between light, dark, and auto theme modes",
            span { "{theme_icon(&theme_val)}" }
            span { "{theme_label(&theme_val)}" }
        }

        div { class: "container",
            if let Some(s) = stats {
                StatsView { stats: s }
            }

            if let Some(e) = err {
                div { class: "error-message",
                    h2 { "Error Loading Report" }
                    p { "{e}" }
                    p {
                        strong { "For GitHub Pages usage:" }
                    }
                    ul {
                        li {
                            "Run "
                            code { "make check" }
                            " in your language repository, with "
                            strong { "spellcheckers" }
                            " enabled"
                        }
                        li {
                            "Verify that "
                            code { "docs/typosreport/report.json" }
                            " was generated or updated"
                        }
                        li {
                            "Commit and push the "
                            code { "report.json" }
                            " file to your repository"
                        }
                    }
                    p {
                        strong { "For local testing:" }
                    }
                    p { "Generate a report file:" }
                    pre { "divvunspell accuracy -o report.json typos.tsv language.zhfst" }
                    p {
                        "Then copy the report.json file next to the built "
                        code { "index.html" }
                        " (the Trunk "
                        code { "dist/" }
                        " directory)."
                    }
                }
            } else if !loaded {
                div { class: "loading", "Loading..." }
            } else {
                h2 { "Detailed Results" }
                p { "{sort_mode_label(mode.as_deref())}" }

                button {
                    onclick: move |_| {
                        results.set(original_results.read().clone());
                        sort_mode.set(None);
                    },
                    "Sort by Input Order"
                }
                button {
                    onclick: move |_| {
                        if sort_mode().as_deref() == Some("time:asc") {
                            results.write().reverse();
                            sort_mode.set(Some("time:desc".to_string()));
                        } else {
                            results.write().sort_by(|a, b| b.time.cmp(&a.time));
                            sort_mode.set(Some("time:asc".to_string()));
                        }
                    },
                    "Sort by Time"
                }
                button {
                    onclick: move |_| {
                        if sort_mode().as_deref() == Some("position:asc") {
                            results.write().reverse();
                            sort_mode.set(Some("position:desc".to_string()));
                        } else {
                            results.write().sort_by_key(position_key);
                            sort_mode.set(Some("position:asc".to_string()));
                        }
                    },
                    "Sort by Position"
                }
                button {
                    onclick: move |_| {
                        if sort_mode().as_deref() == Some("distance:asc") {
                            results.write().reverse();
                            sort_mode.set(Some("distance:desc".to_string()));
                        } else {
                            results.write().sort_by_key(|r| r.distance);
                            sort_mode.set(Some("distance:asc".to_string()));
                        }
                    },
                    "Sort by Edit Distance"
                }
                button {
                    onclick: move |_| {
                        if sort_mode().as_deref() == Some("classification:asc") {
                            results.write().reverse();
                            sort_mode.set(Some("classification:desc".to_string()));
                        } else {
                            results.write().sort_by_key(class_order);
                            sort_mode.set(Some("classification:asc".to_string()));
                        }
                    },
                    "Sort by Classification"
                }

                table { class: "table",
                    thead {
                        tr {
                            th { "Spelling error data" }
                            th { "Suggestion list" }
                        }
                    }
                    tbody {
                        for result in rows.iter() {
                            ResultRow { key: "{result.input}", result: result.clone() }
                        }
                    }
                }
            }
        }
    }
}
