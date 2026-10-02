//! Stages: parts of an error model that the stored automaton calls into,
//! held in the same DHFST file.
//!
//! An error model is assembled from components by union, concatenation and
//! repetition — the giella recipes build `words | (strings | edits)^{1..N}
//! final_strings?` — so a component that is computed rather than stored
//! cannot be one link of a linear chain: it sits inside the union, under the
//! repetition, possibly several times. It is instead *called*. The stored
//! automaton carries, where the component would be, an arc on a reserved pair
//! of call symbols (declared here, hidden from the alphabet) whose target is
//! the state to return to. The search sees that arc as an `ε:ε` move into the
//! stage; the stage runs, and from any of its final contexts an `ε:ε` move
//! returns to the stored state the call named. A search state inside a stage
//! is a *virtual* state packing the stage, the return state and the stage's
//! own substate, so the search state tuple does not change.
//!
//! # The edit-table stage
//!
//! An edit table computes single-character edits at search time from cost
//! tables instead of storing one arc per pair. The stage is a small automaton
//! of *contexts*: each has identity transitions (`x:x` at weight 0 for `x` in
//! a set, to a target context), optionally a final weight, and optionally an
//! edit table whose edits all lead to the table's target context:
//!
//! * substitution `x:y` at `sub(x, y)`,
//! * deletion `x:ε` at `del(x)`,
//! * insertion `ε:y` at `ins(y)`,
//! * transposition `x y → y x`, walked as `x:ε` charged `swap_entry(x)`
//!   (0 without an entry vector), then `y:y` charged `swap(x, y)`, then `ε:x`,
//!   through two pending substates per `x`.
//!
//! Where a stage is charged matters even though the total does not: the
//! search adds weights in `f32` as it goes, interleaved with the lexicon's.
//! A stage written from a weight-pushed model is pushed too — the call arc
//! carries the least cost of getting through the stage, the edits carry what
//! is left, and a transposition charges its row's least cost when it starts
//! — so that every partial sum equals the stored model's and the totals
//! agree to the bit.
//!
//! A matrix cell is its exception if one is listed, else — inside the
//! matrix's row and column sets — its column's weight if the column has one,
//! else the matrix default; anything else is no edit (`+inf`). A vector
//! entry is its exception if listed, else the default inside the vector's
//! set. Symbols are the alphabet's, markers included: `@_UNKNOWN_SYMBOL_@`
//! and `@_IDENTITY_SYMBOL_@` are labels like any other, exactly as the stored
//! arcs a table replaces would carry them.
//!
//! A substitution row reaches the search as one [`ArcGroup::Each`] per weight
//! (the default cells as one group), so the search meets it against the
//! lexicon the same way it meets a default arc.
//!
//! # Layout (`STAG` section)
//!
//! ```text
//! u32 n_alphabet   symbols 0..n_alphabet are the alphabet; the rest are
//!                  call symbols, used only on call arcs
//! u32 n_stages
//! { u16 call_input; u16 call_output; u32 kind (1 = edit table);
//!   u64 offset (from the section start); u64 length; u32 substates;
//!   u32 reserved }[n_stages]
//! ```
//!
//! An edit-table stage body:
//!
//! ```text
//! u32 n_sets, n_contexts, start_context, n_idents,
//!     n_tables, n_matrices, n_vectors, n_column_weights,
//!     n_cells, n_vector_cells, 0, 0
//! u64 sets[n_sets * words]            words = ceil(n_alphabet / 64)
//! { f32 final; u32 first_ident; u32 n_idents; u32 table }[n_contexts]
//! { u32 set; u32 target_context }[n_idents]
//! { u32 target_context; u32 sub; u32 del; u32 ins; u32 swap;
//!   u32 swap_entry }[n_tables]
//! { u32 row_set; u32 column_set; f32 default; u32 first_column_weight;
//!   u32 n_column_weights; u32 first_cell; u32 n_cells; u32 0 }[n_matrices]
//! { u32 set; f32 default; u32 first_cell; u32 n_cells }[n_vectors]
//! { u16 symbol; u16 0; f32 weight }[n_column_weights]   sorted per matrix
//! { u16 row; u16 column; f32 weight }[n_cells]           sorted per matrix
//! { u16 symbol; u16 0; f32 weight }[n_vector_cells]      sorted per vector
//! ```
//!
//! `0xFFFFFFFF` marks an absent table, matrix or vector; weights are finite
//! or `+inf`.

use std::borrow::Cow;
use std::path::Path;

use crate::transducer::{ArcGroup, SymbolSet, TransducerError};
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};

use super::{NONE, u16_at, u32_at, u64_at};

/// The stage kind of an edit table.
pub const STAGE_EDIT_TABLE: u32 = 1;
/// Bytes per stage header.
pub const STAGE_HEADER_LEN: usize = 32;
/// Bytes of an edit-table stage's counts.
pub const EDIT_TABLE_HEADER_LEN: usize = 48;

fn corrupt(path: &Path, detail: impl Into<Cow<'static, str>>) -> TransducerError {
    TransducerError::CorruptTables {
        path: path.to_path_buf(),
        detail: detail.into(),
    }
}

fn weight_ok(w: f32) -> bool {
    !w.is_nan() && w != f32::NEG_INFINITY
}

/// A context of an edit-table stage.
#[derive(Clone, Debug)]
pub(crate) struct Context {
    pub(crate) final_weight: f32,
    pub(crate) idents: (u32, u32),
    pub(crate) table: Option<u32>,
}

/// An edit table: what edits a context allows and where they lead.
#[derive(Clone, Debug)]
pub(crate) struct Table {
    pub(crate) target: u32,
    pub(crate) sub: Option<u32>,
    pub(crate) del: Option<u32>,
    pub(crate) ins: Option<u32>,
    pub(crate) swap: Option<u32>,
    /// What starting a transposition of `x` costs, by `x`; none for 0.
    pub(crate) swap_entry: Option<u32>,
    /// Substate of the first transposition-pending state: `x` pending after
    /// `x:ε` is `swap_base + 2x`, and after copying the middle symbol it is
    /// `swap_base + 2x + 1`.
    pub(crate) swap_base: u32,
}

/// A weight matrix over symbol pairs, in its compressed form, with the
/// lookup structures built at load.
#[derive(Clone, Debug)]
pub(crate) struct Matrix {
    rows: u32,
    cols: u32,
    default: f32,
    /// Column weight per symbol, `NaN` where the column has none.
    col_weight: Vec<f32>,
    /// Columns that have a column weight, as a bitset.
    col_weight_mask: Vec<u64>,
    /// Exceptions, sorted by `(row, column)`.
    cells: Vec<(u16, u16, f32)>,
    /// `cells[row_start[x]..row_start[x + 1]]` are row `x`'s exceptions.
    row_start: Vec<u32>,
    /// Rows with at least one finite cell, as a bitset.
    row_live: Vec<u64>,
}

/// A weight vector over symbols.
#[derive(Clone, Debug)]
pub(crate) struct Vector {
    set: u32,
    default: f32,
    /// Exceptions, sorted by symbol.
    cells: Vec<(u16, f32)>,
}

/// An edit-table stage.
#[derive(Clone, Debug)]
pub(crate) struct EditStage {
    pub(crate) call_input: u16,
    pub(crate) call_output: u16,
    /// First virtual state id of this stage.
    pub(crate) base: u64,
    /// Substates per return state.
    pub(crate) substates: u32,
    pub(crate) start: u32,
    words: usize,
    n_alphabet: u32,
    sets: Vec<u64>,
    pub(crate) contexts: Vec<Context>,
    idents: Vec<(u32, u32)>,
    pub(crate) tables: Vec<Table>,
    matrices: Vec<Matrix>,
    vectors: Vec<Vector>,
}

/// The stages a file declares.
#[derive(Clone, Debug)]
pub(crate) struct Stages {
    pub(crate) n_alphabet: u32,
    pub(crate) stages: Vec<EditStage>,
}

impl Stages {
    /// Parse and validate a `STAG` section. `n_states` is the stored
    /// automaton's state count, which every return target must be under and
    /// which sizes the virtual state space.
    pub(crate) fn parse(
        b: &[u8],
        start: usize,
        end: usize,
        n_symbols: u32,
        n_states: u32,
        path: &Path,
    ) -> Result<Stages, TransducerError> {
        if end - start < 8 {
            return Err(corrupt(path, "STAG is truncated"));
        }
        let n_alphabet = u32_at(b, start);
        let n_stages = u32_at(b, start + 4) as usize;
        if n_alphabet == 0 || n_alphabet > n_symbols {
            return Err(corrupt(path, "STAG alphabet size is out of range"));
        }
        let headers_end = n_stages
            .checked_mul(STAGE_HEADER_LEN)
            .and_then(|n| n.checked_add(start + 8))
            .filter(|e| *e <= end)
            .ok_or_else(|| corrupt(path, "STAG stage headers run past the section"))?;
        let mut stages = Vec::with_capacity(n_stages);
        let mut base = n_states as u64;
        for s in 0..n_stages {
            let at = start + 8 + STAGE_HEADER_LEN * s;
            let call_input = u16_at(b, at);
            let call_output = u16_at(b, at + 2);
            let kind = u32_at(b, at + 4);
            let offset = u64_at(b, at + 8);
            let length = u64_at(b, at + 16);
            let substates = u32_at(b, at + 24);
            if kind != STAGE_EDIT_TABLE {
                return Err(corrupt(
                    path,
                    format!("stage {s} is of unknown kind {kind}"),
                ));
            }
            for symbol in [call_input, call_output] {
                if (symbol as u32) < n_alphabet || symbol as u32 >= n_symbols {
                    return Err(corrupt(
                        path,
                        format!("stage {s} call symbol {symbol} is not a call symbol"),
                    ));
                }
            }
            let (Ok(offset), Ok(length)) = (usize::try_from(offset), usize::try_from(length))
            else {
                return Err(corrupt(path, "stage extent overflows"));
            };
            let body_start = start
                .checked_add(offset)
                .filter(|o| *o >= headers_end && o % 8 == 0)
                .ok_or_else(|| corrupt(path, format!("stage {s} is misplaced")))?;
            let body_end = body_start
                .checked_add(length)
                .filter(|e| *e <= end)
                .ok_or_else(|| corrupt(path, format!("stage {s} runs past the section")))?;
            let stage = EditStage::parse(
                b,
                body_start,
                body_end,
                n_alphabet,
                call_input,
                call_output,
                base,
                substates,
                path,
            )?;
            base = base
                .checked_add(n_states as u64 * substates as u64)
                .filter(|b| *b < NONE as u64)
                .ok_or_else(|| corrupt(path, "the stages' virtual states do not fit 32 bits"))?;
            stages.push(stage);
        }
        for (i, a) in stages.iter().enumerate() {
            for b2 in stages.iter().skip(i + 1) {
                if (a.call_input, a.call_output) == (b2.call_input, b2.call_output) {
                    return Err(corrupt(path, "two stages share a call pair"));
                }
            }
        }
        Ok(Stages { n_alphabet, stages })
    }

    /// The stage a virtual state belongs to, with its return state and
    /// substate.
    #[inline(always)]
    pub(crate) fn decode(&self, state: u32, n_states: u32) -> Option<(&EditStage, u32, u32)> {
        let state = state as u64;
        for stage in &self.stages {
            let span = n_states as u64 * stage.substates as u64;
            if state >= stage.base && state < stage.base + span {
                let local = state - stage.base;
                let ret = (local / stage.substates as u64) as u32;
                let sub = (local % stage.substates as u64) as u32;
                return Some((stage, ret, sub));
            }
        }
        None
    }
}

/// Whether row `x` of `matrix` has any finite cell, from its parts.
fn matrix_row_live(matrix: &Matrix, sets: &[u64], words: usize, n_alphabet: u32, x: u16) -> bool {
    let has = |set: u32, s: u16| {
        (s as u32) < n_alphabet
            && sets[set as usize * words + s as usize / 64] & (1u64 << (s % 64)) != 0
    };
    let row = &matrix.cells
        [matrix.row_start[x as usize] as usize..matrix.row_start[x as usize + 1] as usize];
    if row.iter().any(|c| c.2.is_finite()) {
        return true;
    }
    if !has(matrix.rows, x) {
        return false;
    }
    (0..n_alphabet as u16).any(|y| {
        has(matrix.cols, y) && row.binary_search_by_key(&y, |c| c.1).is_err() && {
            let w = matrix.col_weight[y as usize];
            if w.is_nan() {
                matrix.default.is_finite()
            } else {
                w.is_finite()
            }
        }
    })
}

impl EditStage {
    #[allow(clippy::too_many_arguments)]
    fn parse(
        b: &[u8],
        start: usize,
        end: usize,
        n_alphabet: u32,
        call_input: u16,
        call_output: u16,
        base: u64,
        substates: u32,
        path: &Path,
    ) -> Result<EditStage, TransducerError> {
        if end - start < EDIT_TABLE_HEADER_LEN {
            return Err(corrupt(path, "edit-table stage is truncated"));
        }
        let count = |i: usize| u32_at(b, start + 4 * i) as usize;
        let (n_sets, n_contexts, start_context, n_idents) =
            (count(0), count(1), count(2), count(3));
        let (n_tables, n_matrices, n_vectors, n_colw) = (count(4), count(5), count(6), count(7));
        let (n_cells, n_vcells) = (count(8), count(9));
        let words = (n_alphabet as usize).div_ceil(64);

        let mut at = start + EDIT_TABLE_HEADER_LEN;
        let mut take = |n: usize, size: usize, what: &str| -> Result<usize, TransducerError> {
            let here = at;
            at = n
                .checked_mul(size)
                .and_then(|len| len.checked_add(here))
                .filter(|e| *e <= end)
                .ok_or_else(|| corrupt(path, format!("edit-table {what} run past the stage")))?;
            Ok(here)
        };
        let sets_at = take(n_sets, 8 * words, "sets")?;
        let contexts_at = take(n_contexts, 16, "contexts")?;
        let idents_at = take(n_idents, 8, "identities")?;
        let tables_at = take(n_tables, 24, "tables")?;
        let matrices_at = take(n_matrices, 32, "matrices")?;
        let vectors_at = take(n_vectors, 16, "vectors")?;
        let colw_at = take(n_colw, 8, "column weights")?;
        let cells_at = take(n_cells, 8, "cells")?;
        let vcells_at = take(n_vcells, 8, "vector cells")?;

        let mut sets = Vec::with_capacity(n_sets * words);
        for i in 0..n_sets * words {
            sets.push(u64_at(b, sets_at + 8 * i));
        }
        for s in 0..n_sets {
            let last = &sets[s * words + words - 1];
            let tail = n_alphabet as usize % 64;
            if (tail != 0 && last >> tail != 0) || sets[s * words] & 1 != 0 {
                return Err(corrupt(
                    path,
                    format!("set {s} holds epsilon or a non-symbol"),
                ));
            }
        }
        if n_contexts == 0 || start_context >= n_contexts {
            return Err(corrupt(path, "edit-table stage has no valid start context"));
        }

        let opt = |v: u32, n: usize, what: &str| -> Result<Option<u32>, TransducerError> {
            match v {
                NONE => Ok(None),
                v if (v as usize) < n => Ok(Some(v)),
                _ => Err(corrupt(
                    path,
                    format!("edit-table {what} index is out of range"),
                )),
            }
        };

        let mut idents = Vec::with_capacity(n_idents);
        for i in 0..n_idents {
            let set = u32_at(b, idents_at + 8 * i);
            let target = u32_at(b, idents_at + 8 * i + 4);
            if set as usize >= n_sets || target as usize >= n_contexts {
                return Err(corrupt(
                    path,
                    format!("identity transition {i} is out of range"),
                ));
            }
            idents.push((set, target));
        }

        let mut matrices = Vec::with_capacity(n_matrices);
        for m in 0..n_matrices {
            let r = matrices_at + 32 * m;
            let rows = u32_at(b, r);
            let cols = u32_at(b, r + 4);
            let default = f32::from_bits(u32_at(b, r + 8));
            let (first_colw, n_colw_m) = (u32_at(b, r + 12) as usize, u32_at(b, r + 16) as usize);
            let (first_cell, n_cell) = (u32_at(b, r + 20) as usize, u32_at(b, r + 24) as usize);
            if rows as usize >= n_sets || cols as usize >= n_sets || !weight_ok(default) {
                return Err(corrupt(path, format!("matrix {m} is invalid")));
            }
            if first_colw.checked_add(n_colw_m).is_none_or(|e| e > n_colw)
                || first_cell.checked_add(n_cell).is_none_or(|e| e > n_cells)
            {
                return Err(corrupt(
                    path,
                    format!("matrix {m} ranges run past the stage"),
                ));
            }
            let mut col_weight = vec![f32::NAN; n_alphabet as usize];
            let mut col_weight_mask = vec![0u64; words];
            let mut previous: Option<u16> = None;
            for c in first_colw..first_colw + n_colw_m {
                let symbol = u16_at(b, colw_at + 8 * c);
                let w = f32::from_bits(u32_at(b, colw_at + 8 * c + 4));
                if symbol as u32 >= n_alphabet
                    || !weight_ok(w)
                    || previous.is_some_and(|p| p >= symbol)
                {
                    return Err(corrupt(
                        path,
                        format!("matrix {m} column weights are invalid"),
                    ));
                }
                previous = Some(symbol);
                col_weight[symbol as usize] = w;
                col_weight_mask[symbol as usize / 64] |= 1u64 << (symbol % 64);
            }
            let mut cells = Vec::with_capacity(n_cell);
            let mut previous: Option<(u16, u16)> = None;
            for c in first_cell..first_cell + n_cell {
                let row = u16_at(b, cells_at + 8 * c);
                let col = u16_at(b, cells_at + 8 * c + 2);
                let w = f32::from_bits(u32_at(b, cells_at + 8 * c + 4));
                if row as u32 >= n_alphabet
                    || col as u32 >= n_alphabet
                    || !weight_ok(w)
                    || previous.is_some_and(|p| p >= (row, col))
                {
                    return Err(corrupt(path, format!("matrix {m} cells are invalid")));
                }
                previous = Some((row, col));
                cells.push((row, col, w));
            }
            let mut row_start = vec![0u32; n_alphabet as usize + 1];
            for (row, _, _) in &cells {
                row_start[*row as usize + 1] += 1;
            }
            for x in 0..n_alphabet as usize {
                row_start[x + 1] += row_start[x];
            }
            let mut matrix = Matrix {
                rows,
                cols,
                default,
                col_weight,
                col_weight_mask,
                cells,
                row_start,
                row_live: vec![0u64; words],
            };
            for x in 0..n_alphabet as u16 {
                if matrix_row_live(&matrix, &sets, words, n_alphabet, x) {
                    matrix.row_live[x as usize / 64] |= 1u64 << (x % 64);
                }
            }
            matrices.push(matrix);
        }

        let mut vectors = Vec::with_capacity(n_vectors);
        for v in 0..n_vectors {
            let r = vectors_at + 16 * v;
            let set = u32_at(b, r);
            let default = f32::from_bits(u32_at(b, r + 4));
            let (first, n) = (u32_at(b, r + 8) as usize, u32_at(b, r + 12) as usize);
            if set as usize >= n_sets
                || !weight_ok(default)
                || first.checked_add(n).is_none_or(|e| e > n_vcells)
            {
                return Err(corrupt(path, format!("vector {v} is invalid")));
            }
            let mut cells = Vec::with_capacity(n);
            let mut previous: Option<u16> = None;
            for c in first..first + n {
                let symbol = u16_at(b, vcells_at + 8 * c);
                let w = f32::from_bits(u32_at(b, vcells_at + 8 * c + 4));
                if symbol as u32 >= n_alphabet
                    || symbol == 0
                    || !weight_ok(w)
                    || previous.is_some_and(|p| p >= symbol)
                {
                    return Err(corrupt(path, format!("vector {v} cells are invalid")));
                }
                previous = Some(symbol);
                cells.push((symbol, w));
            }
            vectors.push(Vector {
                set,
                default,
                cells,
            });
        }

        let mut tables = Vec::with_capacity(n_tables);
        let mut swap_base = n_contexts as u64;
        for t in 0..n_tables {
            let r = tables_at + 24 * t;
            let target = u32_at(b, r);
            if target as usize >= n_contexts {
                return Err(corrupt(
                    path,
                    format!("table {t} targets a missing context"),
                ));
            }
            let swap = opt(u32_at(b, r + 16), n_matrices, "swap")?;
            tables.push(Table {
                target,
                sub: opt(u32_at(b, r + 4), n_matrices, "substitution")?,
                del: opt(u32_at(b, r + 8), n_vectors, "deletion")?,
                ins: opt(u32_at(b, r + 12), n_vectors, "insertion")?,
                swap,
                swap_entry: opt(u32_at(b, r + 20), n_vectors, "transposition entry")?,
                swap_base: swap_base as u32,
            });
            if swap.is_some() {
                swap_base += 2 * n_alphabet as u64;
            }
        }
        if swap_base != substates as u64 {
            return Err(corrupt(
                path,
                "stage substate count does not match its tables",
            ));
        }

        let mut contexts = Vec::with_capacity(n_contexts);
        for c in 0..n_contexts {
            let r = contexts_at + 16 * c;
            let final_weight = f32::from_bits(u32_at(b, r));
            let (first, n) = (u32_at(b, r + 4), u32_at(b, r + 8));
            if !weight_ok(final_weight)
                || (first as usize)
                    .checked_add(n as usize)
                    .is_none_or(|e| e > n_idents)
            {
                return Err(corrupt(path, format!("context {c} is invalid")));
            }
            contexts.push(Context {
                final_weight,
                idents: (first, n),
                table: opt(u32_at(b, r + 12), n_tables, "table")?,
            });
        }

        Ok(EditStage {
            call_input,
            call_output,
            base,
            substates,
            start: start_context as u32,
            words,
            n_alphabet,
            sets,
            contexts,
            idents,
            tables,
            matrices,
            vectors,
        })
    }

    #[inline(always)]
    fn set_word(&self, set: u32, word: usize) -> u64 {
        self.sets[set as usize * self.words + word]
    }

    #[inline(always)]
    fn set_has(&self, set: u32, symbol: u16) -> bool {
        let s = symbol as usize;
        s < self.n_alphabet as usize && self.set_word(set, s / 64) & (1u64 << (s % 64)) != 0
    }

    /// The virtual state for `sub` of a call that returns to `ret`.
    #[inline(always)]
    pub(crate) fn virtual_state(&self, ret: u32, sub: u32) -> TransitionTableIndex {
        TransitionTableIndex((self.base + ret as u64 * self.substates as u64 + sub as u64) as u32)
    }

    /// A substitution or transposition cell.
    pub(crate) fn matrix_weight(&self, m: u32, x: u16, y: u16) -> f32 {
        let matrix = &self.matrices[m as usize];
        let row = &matrix.cells
            [matrix.row_start[x as usize] as usize..matrix.row_start[x as usize + 1] as usize];
        if let Ok(at) = row.binary_search_by_key(&y, |c| c.1) {
            return row[at].2;
        }
        if self.set_has(matrix.rows, x) && self.set_has(matrix.cols, y) {
            let w = matrix.col_weight[y as usize];
            if w.is_nan() { matrix.default } else { w }
        } else {
            f32::INFINITY
        }
    }

    /// A deletion or insertion entry.
    pub(crate) fn vector_weight(&self, v: u32, x: u16) -> f32 {
        let vector = &self.vectors[v as usize];
        if let Ok(at) = vector.cells.binary_search_by_key(&x, |c| c.0) {
            return vector.cells[at].1;
        }
        if self.set_has(vector.set, x) {
            vector.default
        } else {
            f32::INFINITY
        }
    }

    /// Whether row `x` of a matrix has any finite cell.
    #[inline(always)]
    fn matrix_row_live(&self, m: u32, x: u16) -> bool {
        let live = &self.matrices[m as usize].row_live;
        live.get(x as usize / 64)
            .is_some_and(|w| w & (1u64 << (x % 64)) != 0)
    }

    /// Every arc of virtual substate `sub` of a call returning to `ret`, on
    /// `input`.
    pub(crate) fn walk<V>(&self, ret: u32, sub: u32, input: u16, visit: &mut V)
    where
        V: FnMut(ArcGroup<'_>),
    {
        let n_contexts = self.contexts.len() as u32;
        if sub >= n_contexts {
            // A transposition in progress: which table, which symbol, which
            // half.
            let Some(table) = self
                .tables
                .iter()
                .rfind(|t| t.swap.is_some() && sub >= t.swap_base)
            else {
                return;
            };
            let Some(swap) = table.swap else {
                return;
            };
            let offset = sub - table.swap_base;
            let x = (offset / 2) as u16;
            if offset.is_multiple_of(2) {
                if input == 0 {
                    return;
                }
                let w = self.matrix_weight(swap, x, input);
                if w.is_finite() {
                    visit(ArcGroup::One {
                        output: SymbolNumber(input),
                        target: self.virtual_state(ret, sub + 1),
                        weight: Weight(w),
                    });
                }
            } else if input == 0 {
                visit(ArcGroup::One {
                    output: SymbolNumber(x),
                    target: self.virtual_state(ret, table.target),
                    weight: Weight::ZERO,
                });
            }
            return;
        }

        let context = &self.contexts[sub as usize];
        let table = context.table.map(|t| &self.tables[t as usize]);

        if input == 0 {
            if context.final_weight.is_finite() {
                visit(ArcGroup::One {
                    output: SymbolNumber::ZERO,
                    target: TransitionTableIndex(ret),
                    weight: Weight(context.final_weight),
                });
            }
            if let Some(table) = table
                && let Some(ins) = table.ins
            {
                self.walk_vector_outputs(ins, self.virtual_state(ret, table.target), visit);
            }
            return;
        }
        if input as u32 >= self.n_alphabet {
            return;
        }

        let (first, n) = context.idents;
        for &(set, target) in &self.idents[first as usize..(first + n) as usize] {
            if self.set_has(set, input) {
                visit(ArcGroup::One {
                    output: SymbolNumber(input),
                    target: self.virtual_state(ret, target),
                    weight: Weight::ZERO,
                });
            }
        }

        let Some(table) = table else {
            return;
        };
        let target = self.virtual_state(ret, table.target);
        if let Some(sub_m) = table.sub {
            self.walk_matrix_row(sub_m, input, target, visit);
        }
        if let Some(del) = table.del {
            let w = self.vector_weight(del, input);
            if w.is_finite() {
                visit(ArcGroup::One {
                    output: SymbolNumber::ZERO,
                    target,
                    weight: Weight(w),
                });
            }
        }
        if let Some(swap) = table.swap
            && self.matrix_row_live(swap, input)
        {
            let w = table
                .swap_entry
                .map_or(0.0, |v| self.vector_weight(v, input));
            if w.is_finite() {
                visit(ArcGroup::One {
                    output: SymbolNumber::ZERO,
                    target: self.virtual_state(ret, table.swap_base + 2 * input as u32),
                    weight: Weight(w),
                });
            }
        }
    }

    /// One row of a substitution matrix: the default cells as one group, then
    /// the column-weighted cells and the exceptions one by one.
    fn walk_matrix_row<V>(&self, m: u32, x: u16, target: TransitionTableIndex, visit: &mut V)
    where
        V: FnMut(ArcGroup<'_>),
    {
        let matrix = &self.matrices[m as usize];
        let row = &matrix.cells
            [matrix.row_start[x as usize] as usize..matrix.row_start[x as usize + 1] as usize];
        let in_rows = self.set_has(matrix.rows, x);

        if in_rows {
            let mut inline = [0u64; super::INLINE_WORDS];
            let mut heap: Vec<u64>;
            let offer: &mut [u64] = if self.words <= super::INLINE_WORDS {
                &mut inline[..self.words]
            } else {
                heap = vec![0u64; self.words];
                &mut heap
            };
            let mut any = 0u64;
            for (word, slot) in offer.iter_mut().enumerate() {
                *slot = self.set_word(matrix.cols, word) & !matrix.col_weight_mask[word];
            }
            for (_, y, _) in row {
                offer[*y as usize / 64] &= !(1u64 << (y % 64));
            }
            for word in offer.iter() {
                any |= *word;
            }
            if any != 0 && matrix.default.is_finite() {
                visit(ArcGroup::Each {
                    outputs: SymbolSet::new(offer),
                    target,
                    weight: Weight(matrix.default),
                });
            }
            for word in 0..self.words {
                let mut bits = matrix.col_weight_mask[word] & self.set_word(matrix.cols, word);
                while bits != 0 {
                    let y = (word * 64 + bits.trailing_zeros() as usize) as u16;
                    bits &= bits - 1;
                    if row.binary_search_by_key(&y, |c| c.1).is_ok() {
                        continue;
                    }
                    let w = matrix.col_weight[y as usize];
                    if w.is_finite() {
                        visit(ArcGroup::One {
                            output: SymbolNumber(y),
                            target,
                            weight: Weight(w),
                        });
                    }
                }
            }
        }
        for &(_, y, w) in row {
            if w.is_finite() {
                visit(ArcGroup::One {
                    output: SymbolNumber(y),
                    target,
                    weight: Weight(w),
                });
            }
        }
    }

    /// The insertions of a vector: the default entries as one group, then the
    /// exceptions one by one.
    fn walk_vector_outputs<V>(&self, v: u32, target: TransitionTableIndex, visit: &mut V)
    where
        V: FnMut(ArcGroup<'_>),
    {
        let vector = &self.vectors[v as usize];
        let mut inline = [0u64; super::INLINE_WORDS];
        let mut heap: Vec<u64>;
        let offer: &mut [u64] = if self.words <= super::INLINE_WORDS {
            &mut inline[..self.words]
        } else {
            heap = vec![0u64; self.words];
            &mut heap
        };
        for (word, slot) in offer.iter_mut().enumerate() {
            *slot = self.set_word(vector.set, word);
        }
        for (y, _) in &vector.cells {
            offer[*y as usize / 64] &= !(1u64 << (y % 64));
        }
        if offer.iter().any(|w| *w != 0) && vector.default.is_finite() {
            visit(ArcGroup::Each {
                outputs: SymbolSet::new(offer),
                target,
                weight: Weight(vector.default),
            });
        }
        for &(y, w) in &vector.cells {
            if w.is_finite() {
                visit(ArcGroup::One {
                    output: SymbolNumber(y),
                    target,
                    weight: Weight(w),
                });
            }
        }
    }
}
