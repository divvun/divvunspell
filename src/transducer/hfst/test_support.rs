//! Small HFST optimized-lookup files built in memory, for tests of code that
//! reads real archive members.

/// One state: final weight, if final, and arcs as `(input, output, target
/// state, weight)`.
pub(crate) type OlState = (Option<f32>, Vec<(u16, u16, u32, f32)>);

const TARGET_TABLE: u32 = 2_147_483_648;

/// An optimized-lookup file for `states` over `symbols` (symbol 0 is
/// epsilon), state 0 being the start.
///
/// Every state goes in the index table, one slot per symbol, so that states
/// with arcs on several inputs are read correctly; each state's arcs sit in
/// the transition table sorted by input, closed by an entry with no input.
pub(crate) fn ol_bytes(symbols: &[&str], states: &[OlState]) -> Vec<u8> {
    ol_bytes_with(symbols, states, true)
}

/// [`ol_bytes`], but unweighted where `weighted` is false: transitions carry
/// no weight, and a final state's index entry targets 1 instead of storing a
/// final weight.
pub(crate) fn ol_bytes_with(symbols: &[&str], states: &[OlState], weighted: bool) -> Vec<u8> {
    let n = symbols.len() as u32;
    let slot = 1 + n;

    // Flag diacritic arcs are free arcs, which optimized lookup keeps in one
    // run with the epsilon arcs, under the epsilon slot.
    let free = |input: u16| {
        let name = symbols[input as usize];
        input == 0 || (name.starts_with('@') && name.as_bytes().get(2) == Some(&b'.'))
    };
    let mut transitions: Vec<(u16, u16, u32, f32)> = Vec::new();
    let mut index: Vec<(u16, u32)> = Vec::new();
    for (final_weight, arcs) in states {
        let mut arcs = arcs.clone();
        arcs.sort_by_key(|a| (!free(a.0), a.0, a.1, a.2));
        let base = index.len();
        index.push(match final_weight {
            Some(w) if weighted => (u16::MAX, w.to_bits()),
            Some(_) => (u16::MAX, 1),
            None => (u16::MAX, u32::MAX),
        });
        index.extend((0..n).map(|_| (u16::MAX, u32::MAX)));
        for arc in &arcs {
            let slot_input = if free(arc.0) { 0 } else { arc.0 };
            let at = base + 1 + slot_input as usize;
            if index[at].0 == u16::MAX {
                index[at] = (slot_input, TARGET_TABLE + transitions.len() as u32);
            }
            transitions.push((arc.0, arc.1, arc.2 * slot, arc.3));
        }
        transitions.push((u16::MAX, u16::MAX, u32::MAX, 0.0));
    }

    let mut out = Vec::new();
    out.extend_from_slice(b"HFST\0");
    let text = b"version\x003.3\x00type\x00HFST_OLW\x00";
    out.extend_from_slice(&(text.len() as u16).to_le_bytes());
    out.push(0);
    out.extend_from_slice(text);
    out.extend_from_slice(&(n as u16).to_le_bytes());
    out.extend_from_slice(&(n as u16).to_le_bytes());
    out.extend_from_slice(&(index.len() as u32).to_le_bytes());
    out.extend_from_slice(&(transitions.len() as u32).to_le_bytes());
    out.extend_from_slice(&(states.len() as u32).to_le_bytes());
    out.extend_from_slice(&(transitions.len() as u32).to_le_bytes());
    for property in 0..9u32 {
        out.extend_from_slice(&u32::from(property == 0 && weighted).to_le_bytes());
    }
    for s in symbols {
        out.extend_from_slice(s.as_bytes());
        out.push(0);
    }
    for (input, target) in &index {
        out.extend_from_slice(&input.to_le_bytes());
        out.extend_from_slice(&target.to_le_bytes());
    }
    for (input, output, target, weight) in &transitions {
        out.extend_from_slice(&input.to_le_bytes());
        out.extend_from_slice(&output.to_le_bytes());
        out.extend_from_slice(&target.to_le_bytes());
        if weighted {
            out.extend_from_slice(&weight.to_bits().to_le_bytes());
        }
    }
    out
}

/// Symbols of the test speller: epsilon, the identity marker and five
/// letters.
pub(crate) const SYMBOLS: [&str; 7] = [
    "@_EPSILON_SYMBOL_@",
    "@_IDENTITY_SYMBOL_@",
    "c",
    "a",
    "t",
    "r",
    "e",
];

/// A lexicon of `cat`, `car`, `cart` (weight 1) and `care`.
pub(crate) fn lexicon() -> Vec<u8> {
    let (c, a, t, r, e) = (2, 3, 4, 5, 6);
    ol_bytes(
        &SYMBOLS,
        &[
            (None, vec![(c, c, 1, 0.0)]),
            (None, vec![(a, a, 2, 0.0)]),
            (None, vec![(t, t, 3, 0.0), (r, r, 4, 0.0)]),
            (Some(0.0), vec![]),
            (Some(0.0), vec![(t, t, 5, 1.0), (e, e, 6, 0.0)]),
            (Some(0.0), vec![]),
            (Some(0.0), vec![]),
        ],
    )
}

/// An error model shaped like a determinised one: identity on every letter,
/// a fan of substitutions at weight 5 (but `e:a` at 2), deletions at 7 and
/// insertions at 9, at most two edits.
pub(crate) fn errmodel() -> Vec<u8> {
    let letters = [2u16, 3, 4, 5, 6];
    let mut states: Vec<OlState> = Vec::new();
    for edits in 0..3u32 {
        let mut arcs = Vec::new();
        for &x in &letters {
            arcs.push((x, x, edits, 0.0));
            if edits < 2 {
                for &y in &letters {
                    if x != y {
                        let w = if (x, y) == (6, 3) { 2.0 } else { 5.0 };
                        arcs.push((x, y, edits + 1, w));
                    }
                }
                arcs.push((x, 0, edits + 1, 7.0));
                arcs.push((0, x, edits + 1, 9.0));
            }
        }
        states.push((Some(0.0), arcs));
    }
    ol_bytes(&SYMBOLS, &states)
}
