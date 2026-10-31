//! `SpellerConfig` as the wrapper writes it: the same fields under camelCase
//! names, every one of them optional on the way in and present on the way
//! out.
//!
//! The conversions destructure and build divvun-fst's structs with every
//! field named and no `..` rest, so a field gained, lost or renamed there
//! stops this crate compiling until the mirror says what it does with it.
//! That mirror is checked against `js/fst/config.ts` by the tests below and
//! by the JavaScript suite, so the config the wrapper takes cannot silently
//! drop a setting.

use divvun_fst::speller::{ReweightingConfig, SpellerConfig};
use divvun_fst::types::Weight;
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::GlueError;

/// A field of a config as the wrapper hands it over: left out, null, or a
/// value. A field left out takes the built-in default, as it does in a
/// config file; null means none, where the field can be none.
type Nullable<T> = Option<Option<T>>;

/// Reads a field that is present, null included, as `Some`. With
/// `#[serde(default)]` beside it, a field left out stays `None`.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// A config as a call takes it. Unknown fields are refused, so a misspelt
/// one is an error rather than a setting that does nothing.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConfigInput {
    #[serde(default, deserialize_with = "present")]
    n_best: Nullable<usize>,
    #[serde(default, deserialize_with = "present")]
    max_weight: Nullable<f32>,
    #[serde(default, deserialize_with = "present")]
    beam: Nullable<f32>,
    #[serde(default, deserialize_with = "present")]
    reweight: Nullable<ReweightInput>,
    #[serde(default, deserialize_with = "present")]
    recase: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    completion_marker: Nullable<String>,
    #[serde(default, deserialize_with = "present")]
    astar_lookahead: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    search_dedup: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    mutator_subsets: Option<bool>,
    #[serde(default, deserialize_with = "present")]
    search_budget: Nullable<u64>,
    #[serde(default, deserialize_with = "present")]
    word_split_weight: Nullable<f32>,
    #[serde(default, deserialize_with = "present")]
    boundary_edit_weight: Nullable<f32>,
    #[serde(default, deserialize_with = "present")]
    initial_case_weight: Nullable<f32>,
    #[serde(default, deserialize_with = "present")]
    verbose: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReweightInput {
    #[serde(default, deserialize_with = "present")]
    start_penalty: Option<f32>,
    #[serde(default, deserialize_with = "present")]
    end_penalty: Option<f32>,
    #[serde(default, deserialize_with = "present")]
    mid_penalty: Option<f32>,
    #[serde(default, deserialize_with = "present")]
    curve: Nullable<f32>,
}

fn weight(value: Nullable<f32>, default: Option<Weight>) -> Option<Weight> {
    value.map_or(default, |value| value.map(Weight))
}

impl ConfigInput {
    /// The config a call with this input runs with: what it sets, and the
    /// built-in defaults for the rest. As with a config file, a field left
    /// out never comes from an archive's bundled config.
    fn resolve(self) -> SpellerConfig {
        let SpellerConfig {
            n_best: default_n_best,
            max_weight: default_max_weight,
            beam: default_beam,
            reweight: default_reweight,
            recase: default_recase,
            completion_marker: default_completion_marker,
            astar_lookahead: default_astar_lookahead,
            search_dedup: default_search_dedup,
            mutator_subsets: default_mutator_subsets,
            search_budget: default_search_budget,
            word_split_weight: default_word_split_weight,
            boundary_edit_weight: default_boundary_edit_weight,
            initial_case_weight: default_initial_case_weight,
            verbose: default_verbose,
        } = SpellerConfig::default();
        let ConfigInput {
            n_best,
            max_weight,
            beam,
            reweight,
            recase,
            completion_marker,
            astar_lookahead,
            search_dedup,
            mutator_subsets,
            search_budget,
            word_split_weight,
            boundary_edit_weight,
            initial_case_weight,
            verbose,
        } = self;
        SpellerConfig {
            n_best: n_best.unwrap_or(default_n_best),
            max_weight: weight(max_weight, default_max_weight),
            beam: weight(beam, default_beam),
            reweight: reweight.map_or(default_reweight, |reweight| {
                reweight.map(ReweightInput::resolve)
            }),
            recase: recase.unwrap_or(default_recase),
            completion_marker: completion_marker.unwrap_or(default_completion_marker),
            astar_lookahead: astar_lookahead.unwrap_or(default_astar_lookahead),
            search_dedup: search_dedup.unwrap_or(default_search_dedup),
            mutator_subsets: mutator_subsets.unwrap_or(default_mutator_subsets),
            search_budget: search_budget.unwrap_or(default_search_budget),
            word_split_weight: weight(word_split_weight, default_word_split_weight),
            boundary_edit_weight: weight(boundary_edit_weight, default_boundary_edit_weight),
            initial_case_weight: weight(initial_case_weight, default_initial_case_weight),
            verbose: verbose.unwrap_or(default_verbose),
        }
    }
}

impl ReweightInput {
    /// The penalties this sets, and the built-in defaults for the rest, as
    /// a `reweight` object in a config file has them.
    fn resolve(self) -> ReweightingConfig {
        let ReweightingConfig {
            start_penalty: default_start_penalty,
            end_penalty: default_end_penalty,
            mid_penalty: default_mid_penalty,
            curve: default_curve,
        } = ReweightingConfig::default();
        let ReweightInput {
            start_penalty,
            end_penalty,
            mid_penalty,
            curve,
        } = self;
        ReweightingConfig {
            start_penalty: start_penalty.unwrap_or(default_start_penalty),
            end_penalty: end_penalty.unwrap_or(default_end_penalty),
            mid_penalty: mid_penalty.unwrap_or(default_mid_penalty),
            curve: curve.unwrap_or(default_curve),
        }
    }
}

/// A config as the wrapper reads one back, every field present. Numbers are
/// the f32 values divvun-fst holds, written in the shortest form that reads
/// back as the same f32, so a value a caller wrote comes back as written.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigOutput {
    n_best: Option<usize>,
    max_weight: Option<f32>,
    beam: Option<f32>,
    reweight: Option<ReweightOutput>,
    recase: bool,
    completion_marker: Option<String>,
    astar_lookahead: bool,
    search_dedup: bool,
    mutator_subsets: bool,
    search_budget: Option<u64>,
    word_split_weight: Option<f32>,
    boundary_edit_weight: Option<f32>,
    initial_case_weight: Option<f32>,
    verbose: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReweightOutput {
    start_penalty: f32,
    end_penalty: f32,
    mid_penalty: f32,
    curve: Option<f32>,
}

impl From<&SpellerConfig> for ConfigOutput {
    fn from(config: &SpellerConfig) -> Self {
        let SpellerConfig {
            n_best,
            max_weight,
            beam,
            reweight,
            recase,
            completion_marker,
            astar_lookahead,
            search_dedup,
            mutator_subsets,
            search_budget,
            word_split_weight,
            boundary_edit_weight,
            initial_case_weight,
            verbose,
        } = config;
        let number = |weight: &Option<Weight>| weight.map(|weight| weight.0);
        ConfigOutput {
            n_best: *n_best,
            max_weight: number(max_weight),
            beam: number(beam),
            reweight: reweight.as_ref().map(ReweightOutput::from),
            recase: *recase,
            completion_marker: completion_marker.clone(),
            astar_lookahead: *astar_lookahead,
            search_dedup: *search_dedup,
            mutator_subsets: *mutator_subsets,
            search_budget: *search_budget,
            word_split_weight: number(word_split_weight),
            boundary_edit_weight: number(boundary_edit_weight),
            initial_case_weight: number(initial_case_weight),
            verbose: *verbose,
        }
    }
}

impl From<&ReweightingConfig> for ReweightOutput {
    fn from(reweight: &ReweightingConfig) -> Self {
        let ReweightingConfig {
            start_penalty,
            end_penalty,
            mid_penalty,
            curve,
        } = reweight;
        ReweightOutput {
            start_penalty: *start_penalty,
            end_penalty: *end_penalty,
            mid_penalty: *mid_penalty,
            curve: *curve,
        }
    }
}

/// The config a call runs with, given the JSON text of the config it was
/// passed. The wrapper writes that text from a plain object and refuses
/// numbers JSON cannot carry, so the errors left are fields this config does
/// not have and values of the wrong type.
pub fn parse(json: &str) -> Result<SpellerConfig, GlueError> {
    serde_json::from_str::<ConfigInput>(json)
        .map(ConfigInput::resolve)
        .map_err(|error| {
            // The text is the wrapper's, not the caller's, so its line and
            // column would point nowhere the caller can see.
            let message = error.to_string();
            let message = match message.rfind(" at line ") {
                Some(at) => &message[..at],
                None => &message,
            };
            GlueError::new("invalid-argument", format!("a speller config: {message}"))
        })
}

/// `config` as the JSON text of the object the wrapper reads back.
pub fn to_json(config: &SpellerConfig) -> String {
    serde_json::to_string(&ConfigOutput::from(config))
        .expect("a config of numbers, strings and booleans serialises")
}

/// The built-in defaults, as the JSON text of the object the wrapper reads
/// back. The wrapper declares them itself, so that they are there before the
/// module is; the test suite holds the two side by side.
#[cfg(any(test, feature = "test-hooks"))]
pub fn default_json() -> String {
    to_json(&SpellerConfig::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn resolved(json: &str) -> Value {
        let config = parse(json).expect("a valid config");
        serde_json::from_str(&to_json(&config)).expect("JSON")
    }

    fn json(text: &str) -> Value {
        serde_json::from_str(text).expect("JSON")
    }

    /// `name` in camelCase, as the wrapper writes divvun-fst's kebab-case
    /// config keys.
    fn camel(kebab: &str) -> String {
        let mut out = String::new();
        let mut upper = false;
        for c in kebab.chars() {
            match c {
                '-' => upper = true,
                c if upper => {
                    out.extend(c.to_uppercase());
                    upper = false;
                }
                c => out.push(c),
            }
        }
        out
    }

    /// The field names of `value`, an object, and of the objects in it,
    /// nested names after their parent's and a dot.
    fn field_names(value: &Value, prefix: &str, out: &mut Vec<String>) {
        if let Value::Object(fields) = value {
            for (name, inner) in fields {
                let name = format!("{prefix}{name}");
                field_names(inner, &format!("{name}."), out);
                out.push(name);
            }
        }
    }

    #[test]
    fn an_empty_config_is_the_built_in_defaults() {
        let defaults = serde_json::to_value(SpellerConfig::default()).expect("serialises");
        let empty = parse("{}").expect("an empty config");
        assert_eq!(
            serde_json::to_value(&empty).expect("serialises"),
            defaults,
            "every serialised field"
        );
        assert_eq!(empty.verbose, SpellerConfig::default().verbose);
        assert_eq!(resolved("{}"), json(&default_json()));
    }

    #[test]
    fn the_mirror_has_every_field_divvun_fst_serialises_and_verbose() {
        // divvun-fst's own serialisation names every field it reads from a
        // config file, in kebab-case. `verbose` is never serialised, which
        // is why the mirror is built from the struct and not from that.
        let mut native = Vec::new();
        let value = serde_json::to_value(SpellerConfig::default()).expect("serialises");
        let mut with_reweight = value.clone();
        with_reweight["reweight"] =
            serde_json::to_value(ReweightingConfig::default()).expect("serialises");
        field_names(&with_reweight, "", &mut native);
        let mut native: Vec<String> = native.iter().map(|name| camel(name)).collect();
        native.push("verbose".into());
        native.sort();

        let mut mirror = Vec::new();
        field_names(&json(&default_json()), "", &mut mirror);
        mirror.sort();
        assert_eq!(mirror, native);
    }

    #[test]
    fn the_typescript_defaults_name_every_field() {
        let config_ts = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../js/fst/config.ts"
        ))
        .expect("js/fst/config.ts is in the repository");
        let defaults = config_ts
            .split_once("export const defaultSpellerConfig")
            .map(|(_, rest)| rest)
            .expect("config.ts declares defaultSpellerConfig");
        let mut names = Vec::new();
        field_names(&json(&default_json()), "", &mut names);
        for name in names {
            let field = name.rsplit('.').next().unwrap_or(&name);
            assert!(
                defaults.contains(&format!("{field}:")),
                "defaultSpellerConfig in js/fst/config.ts sets {name}"
            );
        }
    }

    #[test]
    fn every_field_set_comes_back_as_set() {
        // Weights are written as floats because they come back as f32s, and
        // JSON's 15 and 15.0 are different values to serde_json.
        let every = r#"{
            "nBest": 3,
            "maxWeight": 40.5,
            "beam": 15.0,
            "reweight": {"startPenalty": 2.0, "endPenalty": 3.0, "midPenalty": 1.25, "curve": 5.0},
            "recase": false,
            "completionMarker": "+",
            "astarLookahead": true,
            "searchDedup": false,
            "mutatorSubsets": false,
            "searchBudget": 5000,
            "wordSplitWeight": 20.0,
            "boundaryEditWeight": 15.0,
            "initialCaseWeight": 8.5,
            "verbose": true
        }"#;
        assert_eq!(resolved(every), json(every));
    }

    #[test]
    fn null_means_none_and_a_field_left_out_means_the_default() {
        let config = parse(r#"{"nBest": null, "reweight": null, "beam": null}"#).expect("valid");
        assert_eq!(config.n_best, None);
        assert!(config.reweight.is_none());
        assert_eq!(config.max_weight, SpellerConfig::default().max_weight);

        let reweight = parse(r#"{"reweight": {"midPenalty": 1}}"#)
            .expect("valid")
            .reweight
            .expect("a reweight config");
        assert_eq!(
            (reweight.start_penalty, reweight.mid_penalty, reweight.curve),
            (10.0, 1.0, None)
        );
    }

    #[test]
    fn a_wrong_field_or_value_is_refused_by_name() {
        for (config, said) in [
            (r#"{"nbest": 3}"#, "unknown field `nbest`"),
            (r#"{"n-best": 3}"#, "unknown field `n-best`"),
            (r#"{"reweight": {"start": 1}}"#, "unknown field `start`"),
            (r#"{"recase": null}"#, "expected a boolean"),
            (r#"{"nBest": 2.5}"#, "expected usize"),
            (r#"{"nBest": -1}"#, "expected usize"),
            (r#"{"maxWeight": "10"}"#, "expected f32"),
            (r#"{"reweight": {"midPenalty": null}}"#, "expected f32"),
        ] {
            let error = parse(config).expect_err(config);
            assert_eq!(error.code(), "invalid-argument");
            assert!(
                error.message().contains(said),
                "{config}: {}",
                error.message()
            );
            assert!(!error.message().contains(" line "), "{}", error.message());
        }
    }

    #[test]
    fn numbers_come_back_as_the_shortest_text_of_their_f32() {
        let config = resolved(r#"{"maxWeight": 0.1, "beam": 16777217}"#);
        assert_eq!(config["maxWeight"], json("0.1"));
        assert_eq!(config["beam"], json("16777216.0"));
    }
}
