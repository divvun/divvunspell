use std::path::Path;

use memmap2::Mmap;

use super::ThfstTransducer;
use super::index_table::{INDEX_TABLE_SIZE, IndexTable};
use super::transition_table::{TRANS_TABLE_SIZE, TransitionTable};
use crate::transducer::symbol_transition::SymbolTransition;
use crate::transducer::{
    IndexTableLoader, IndexTableTrait, TransducerError, TransitionTableLoader, TransitionTableTrait,
};
use crate::types::{SymbolNumber, TransitionTableIndex, Weight};
use crate::vfs::{self, Filesystem};

/// Tromsø-Helsinki Finite State Transducer format with chunked memory mapping.
///
/// This variant maps each table in as few chunks as the system allows, which
/// is useful for very large transducers that might exceed system memory
/// mapping limits or address space. It walks its tables exactly as
/// [`MmapThfstTransducer`](super::MmapThfstTransducer) does: only the tables
/// differ.
pub type ThfstChunkedTransducer =
    ThfstTransducer<ChunkedTable<IndexTable<Mmap>>, ChunkedTable<TransitionTable<Mmap>>>;

/// Type alias for memory-mapped chunked THFST transducer.
pub type MmapThfstChunkedTransducer = ThfstChunkedTransducer;

/// The most chunks a table is mapped in before loading gives up.
const MAX_CHUNKS: u64 = 16;

/// A THFST table mapped in chunks of whole records and read as one table.
///
/// Every chunk but the last holds `per_chunk` records and the last holds the
/// rest, so record `i` is record `i % per_chunk` of chunk `i / per_chunk`. A
/// record past the end of the table reads as no record, as it does in a table
/// mapped whole. The cursor walk depends on that: it reads the record after a
/// run of arcs to find where the run ends, and the last run in a table ends at
/// the table's end.
#[derive(Debug)]
pub struct ChunkedTable<T> {
    chunks: Vec<T>,
    /// Records in each chunk but the last. Zero only for an empty table, which
    /// has no chunks.
    per_chunk: u32,
    records: u32,
}

impl<T> ChunkedTable<T> {
    /// The chunk holding record `i`, and the record's place in it.
    #[inline(always)]
    fn locate(&self, i: TransitionTableIndex) -> Option<(&T, TransitionTableIndex)> {
        let chunk = self.chunks.get(i.0.checked_div(self.per_chunk)? as usize)?;
        Some((chunk, TransitionTableIndex(i.0 % self.per_chunk)))
    }

    /// Map the table at `path`, of records `record_size` bytes long, in the
    /// fewest chunks the system will map. `map` maps a count of records from a
    /// first record.
    fn load<FS, F, M>(
        fs: &FS,
        path: &Path,
        record_size: usize,
        map: M,
    ) -> Result<Self, TransducerError>
    where
        FS: Filesystem<File = F>,
        F: vfs::File,
        M: Fn(&F, &Path, u64, u64) -> Result<T, TransducerError>,
    {
        let io = |source| TransducerError::Io {
            path: path.to_path_buf(),
            source,
        };
        let file = fs.open_file(path).map_err(io)?;
        let records = file.len().map_err(io)? / record_size as u64;

        let mut chunk_count = 1;
        loop {
            match Self::in_chunks(&file, path, records, chunk_count, &map) {
                Err(TransducerError::Memmap { .. }) if chunk_count < MAX_CHUNKS => {
                    chunk_count *= 2;
                }
                Err(TransducerError::Memmap { .. }) => {
                    return Err(TransducerError::CorruptTables {
                        path: path.to_path_buf(),
                        detail: format!("could not memory-map the table in {MAX_CHUNKS} chunks")
                            .into(),
                    });
                }
                result => return result,
            }
        }
    }

    /// Map the first `records` records of `file` in `chunk_count` chunks, or
    /// fewer where there are fewer records than chunks.
    fn in_chunks<F, M>(
        file: &F,
        path: &Path,
        records: u64,
        chunk_count: u64,
        map: &M,
    ) -> Result<Self, TransducerError>
    where
        M: Fn(&F, &Path, u64, u64) -> Result<T, TransducerError>,
    {
        let too_long = |_| TransducerError::CorruptTables {
            path: path.to_path_buf(),
            detail: format!("{records} records are more than a THFST table can index").into(),
        };
        let per_chunk = records.div_ceil(chunk_count);
        let mut table = ChunkedTable {
            chunks: Vec::new(),
            per_chunk: u32::try_from(per_chunk).map_err(too_long)?,
            records: u32::try_from(records).map_err(too_long)?,
        };

        let mut first = 0;
        while first < records {
            let count = per_chunk.min(records - first);
            table.chunks.push(map(file, path, first, count)?);
            first += count;
        }
        Ok(table)
    }
}

impl<T: IndexTableTrait> IndexTableTrait for ChunkedTable<T> {
    #[inline(always)]
    fn len(&self) -> TransitionTableIndex {
        TransitionTableIndex(self.records)
    }

    #[inline(always)]
    fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        let (chunk, at) = self.locate(i)?;
        chunk.input_symbol(at)
    }

    #[inline(always)]
    fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        let (chunk, at) = self.locate(i)?;
        chunk.target(at)
    }

    #[inline(always)]
    fn final_weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        let (chunk, at) = self.locate(i)?;
        chunk.final_weight(at)
    }

    #[inline(always)]
    fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.locate(i).is_some_and(|(chunk, at)| chunk.is_final(at))
    }
}

impl<T: TransitionTableTrait> TransitionTableTrait for ChunkedTable<T> {
    #[inline(always)]
    fn len(&self) -> TransitionTableIndex {
        TransitionTableIndex(self.records)
    }

    #[inline(always)]
    fn input_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        let (chunk, at) = self.locate(i)?;
        chunk.input_symbol(at)
    }

    #[inline(always)]
    fn output_symbol(&self, i: TransitionTableIndex) -> Option<SymbolNumber> {
        let (chunk, at) = self.locate(i)?;
        chunk.output_symbol(at)
    }

    #[inline(always)]
    fn target(&self, i: TransitionTableIndex) -> Option<TransitionTableIndex> {
        let (chunk, at) = self.locate(i)?;
        chunk.target(at)
    }

    #[inline(always)]
    fn weight(&self, i: TransitionTableIndex) -> Option<Weight> {
        let (chunk, at) = self.locate(i)?;
        chunk.weight(at)
    }

    #[inline(always)]
    fn is_final(&self, i: TransitionTableIndex) -> bool {
        self.locate(i).is_some_and(|(chunk, at)| chunk.is_final(at))
    }

    #[inline(always)]
    fn symbol_transition(&self, i: TransitionTableIndex) -> SymbolTransition {
        match self.locate(i) {
            Some((chunk, at)) => chunk.symbol_transition(at),
            None => SymbolTransition::new(None, None, None),
        }
    }
}

impl<F: vfs::File> IndexTableLoader<F> for ChunkedTable<IndexTable<Mmap>> {
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<Path>,
        FS: Filesystem<File = F>,
    {
        ChunkedTable::load(
            fs,
            path.as_ref(),
            INDEX_TABLE_SIZE,
            IndexTable::from_file_partial,
        )
    }
}

impl<F: vfs::File> TransitionTableLoader<F> for ChunkedTable<TransitionTable<Mmap>> {
    fn from_path<P, FS>(fs: &FS, path: P) -> Result<Self, TransducerError>
    where
        P: AsRef<Path>,
        FS: Filesystem<File = F>,
    {
        ChunkedTable::load(
            fs,
            path.as_ref(),
            TRANS_TABLE_SIZE,
            TransitionTable::from_file_partial,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::constants::TARGET_TABLE;
    use crate::transducer::thfst::MmapThfstTransducer;
    use crate::transducer::{Transducer, TransducerLoader};
    use crate::vfs::Fs;

    /// Chunk counts from one chunk up to more chunks than most tables here
    /// have records.
    const CHUNK_COUNTS: &[u64] = &[1, 2, 3, 4, 5, 7, 8, 16, 40];

    fn fixtures() -> Vec<PathBuf> {
        let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"));
        let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
            .expect("fixture directory")
            .map(|entry| entry.expect("fixture entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "thfst"))
            .collect();
        paths.sort();
        assert!(!paths.is_empty(), "no THFST fixtures found");
        paths
    }

    fn chunked_table<T, M>(
        path: &Path,
        record_size: usize,
        chunk_count: u64,
        map: M,
    ) -> ChunkedTable<T>
    where
        M: Fn(&std::fs::File, &Path, u64, u64) -> Result<T, TransducerError>,
    {
        let file = std::fs::File::open(path).expect("open table");
        let records = vfs::File::len(&file).expect("table length") / record_size as u64;
        ChunkedTable::in_chunks(&file, path, records, chunk_count, &map).expect("map table")
    }

    fn chunked_transducer(dir: &Path, chunk_count: u64) -> ThfstChunkedTransducer {
        let mut t = ThfstChunkedTransducer::from_path(&Fs, dir).expect("load chunked");
        t.index_table = chunked_table(
            &dir.join("index"),
            INDEX_TABLE_SIZE,
            chunk_count,
            IndexTable::from_file_partial,
        );
        t.transition_table = chunked_table(
            &dir.join("transition"),
            TRANS_TABLE_SIZE,
            chunk_count,
            TransitionTable::from_file_partial,
        );
        t
    }

    fn arc(t: Option<SymbolTransition>) -> Option<(Option<u32>, Option<u16>, Option<u32>)> {
        t.map(|t| {
            (
                t.target().map(|x| x.0),
                t.symbol().map(|x| x.0),
                t.weight().map(|x| x.0.to_bits()),
            )
        })
    }

    /// Every question the speller asks of a transducer, at every index of both
    /// tables and a few past their ends, gets the same answer from both.
    fn assert_reads_alike(
        whole: &MmapThfstTransducer,
        chunked: &ThfstChunkedTransducer,
        label: &str,
    ) {
        let symbols = whole.alphabet().key_table().len() as u16 + 1;
        let index_len = whole.index_table.len().0;
        let trans_len = whole.transition_table.len().0;
        assert_eq!(
            chunked.index_table.len().0,
            index_len,
            "{label}: index length"
        );
        assert_eq!(
            chunked.transition_table.len().0,
            trans_len,
            "{label}: transition length"
        );

        let states = (0..index_len + u32::from(symbols) + 2)
            .chain((0..trans_len + 2).map(|j| TARGET_TABLE.0 + j))
            .map(TransitionTableIndex);
        for i in states {
            let at = format!("{label}: state {:#x}", i.0);
            assert_eq!(chunked.is_final(i), whole.is_final(i), "{at} is_final");
            assert_eq!(
                chunked.final_weight(i).map(|w| w.0.to_bits()),
                whole.final_weight(i).map(|w| w.0.to_bits()),
                "{at} final_weight"
            );
            assert_eq!(
                chunked.has_epsilons_or_flags(i),
                whole.has_epsilons_or_flags(i),
                "{at} has_epsilons_or_flags"
            );
            assert_eq!(
                chunked.distance_to_final(i).0.to_bits(),
                whole.distance_to_final(i).0.to_bits(),
                "{at} distance_to_final"
            );
            for symbol in (0..symbols).map(SymbolNumber) {
                assert_eq!(
                    chunked.has_transitions(i, Some(symbol)),
                    whole.has_transitions(i, Some(symbol)),
                    "{at} has_transitions {}",
                    symbol.0
                );
                // An index-table state is only asked for its next arc once the
                // slot one past it says the arc exists; asked anyway, both
                // readers subtract past zero on a slot holding a final weight.
                let cursor = i.incr();
                let asked = i >= TARGET_TABLE
                    || whole.has_transitions(cursor, Some(symbol))
                    || (symbol == SymbolNumber::ZERO && whole.has_epsilons_or_flags(cursor));
                if asked {
                    assert_eq!(
                        chunked.next(i, symbol),
                        whole.next(i, symbol),
                        "{at} next {}",
                        symbol.0
                    );
                }
            }
        }

        for j in (0..trans_len + 2).map(TransitionTableIndex) {
            let at = format!("{label}: transition {}", j.0);
            assert_eq!(
                chunked.transition_input_symbol(j),
                whole.transition_input_symbol(j),
                "{at} input symbol"
            );
            assert_eq!(
                arc(chunked.take_epsilons(j)),
                arc(whole.take_epsilons(j)),
                "{at} take_epsilons"
            );
            assert_eq!(
                arc(chunked.take_epsilons_and_flags(j)),
                arc(whole.take_epsilons_and_flags(j)),
                "{at} take_epsilons_and_flags"
            );
            for symbol in (0..symbols).map(SymbolNumber) {
                assert_eq!(
                    arc(chunked.take_non_epsilons(j, symbol)),
                    arc(whole.take_non_epsilons(j, symbol)),
                    "{at} take_non_epsilons {}",
                    symbol.0
                );
            }
        }

        assert_eq!(
            chunked.least_arc_weight().map(|w| w.0.to_bits()),
            whole.least_arc_weight().map(|w| w.0.to_bits()),
            "{label}: least_arc_weight"
        );
    }

    #[test]
    fn every_fixture_reads_as_it_does_mapped_whole() {
        for dir in fixtures() {
            let whole = MmapThfstTransducer::from_path(&Fs, &dir).expect("load whole");
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let loaded = ThfstChunkedTransducer::from_path(&Fs, &dir).expect("load chunked");
            assert_reads_alike(&whole, &loaded, &name);
            for &chunk_count in CHUNK_COUNTS {
                let chunked = chunked_transducer(&dir, chunk_count);
                assert_reads_alike(&whole, &chunked, &format!("{name} in {chunk_count} chunks"));
            }
        }
    }

    /// An index table of `records` records, each different from the next,
    /// with final, empty and arc records among them, then `stray` bytes too
    /// few to make another record.
    fn index_bytes(records: u32, stray: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        for i in 0..records {
            let (symbol, target) = match i % 4 {
                0 => (u16::MAX, 1.5f32.to_bits() + i),
                1 => (u16::MAX, u32::MAX),
                _ => (i as u16, TARGET_TABLE.0 + i),
            };
            bytes.extend_from_slice(&symbol.to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(&target.to_le_bytes());
        }
        bytes.resize(bytes.len() + stray, 0xab);
        bytes
    }

    /// A transition table of `records` records, each different from the
    /// next, with final and end-of-run records among them, then `stray`
    /// bytes too few to make another record.
    fn transition_bytes(records: u32, stray: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        for i in 0..records {
            let (input, output, target) = match i % 5 {
                0 => (u16::MAX, u16::MAX, 1),
                1 => (u16::MAX, u16::MAX, u32::MAX),
                _ => (i as u16, i as u16 + 1, 1000 + i),
            };
            bytes.extend_from_slice(&input.to_le_bytes());
            bytes.extend_from_slice(&output.to_le_bytes());
            bytes.extend_from_slice(&target.to_le_bytes());
            bytes.extend_from_slice(&(i as f32 + 0.25).to_bits().to_le_bytes());
        }
        bytes.resize(bytes.len() + stray, 0xab);
        bytes
    }

    /// Indexes into a table of `records` records: all of them, the first
    /// several past the end, and the last a table can name.
    fn probes(records: u32) -> impl Iterator<Item = TransitionTableIndex> {
        (0..records + 20)
            .chain([u32::MAX - 1, u32::MAX])
            .map(TransitionTableIndex)
    }

    /// Both tables of `records` records, with `stray` bytes after the last,
    /// read alike mapped whole and in `chunk_count` chunks.
    fn assert_tables_read_alike(records: u32, stray: usize, chunk_count: u64) {
        let label = format!("{records} records + {stray} bytes in {chunk_count} chunks");
        let dir = tempfile::tempdir().expect("tempdir");

        let path = dir.path().join("index");
        std::fs::write(&path, index_bytes(records, stray)).expect("write index");
        let whole = IndexTable::from_path(&Fs, &path).expect("map index whole");
        let chunked = chunked_table(
            &path,
            INDEX_TABLE_SIZE,
            chunk_count,
            IndexTable::from_file_partial,
        );
        assert_eq!(
            IndexTableTrait::len(&chunked),
            whole.len(),
            "{label}: index length"
        );
        assert!(
            chunked.chunks.len() as u64 <= chunk_count,
            "{label}: index chunks"
        );
        for i in probes(records) {
            let at = format!("{label}: index record {}", i.0);
            assert_eq!(
                IndexTableTrait::input_symbol(&chunked, i),
                whole.input_symbol(i),
                "{at}"
            );
            assert_eq!(
                IndexTableTrait::target(&chunked, i),
                whole.target(i),
                "{at}"
            );
            assert_eq!(
                IndexTableTrait::final_weight(&chunked, i).map(|w| w.0.to_bits()),
                whole.final_weight(i).map(|w| w.0.to_bits()),
                "{at}"
            );
            assert_eq!(
                IndexTableTrait::is_final(&chunked, i),
                whole.is_final(i),
                "{at}"
            );
        }

        let path = dir.path().join("transition");
        std::fs::write(&path, transition_bytes(records, stray)).expect("write transitions");
        let whole = TransitionTable::from_path(&Fs, &path).expect("map transitions whole");
        let chunked = chunked_table(
            &path,
            TRANS_TABLE_SIZE,
            chunk_count,
            TransitionTable::from_file_partial,
        );
        assert_eq!(
            TransitionTableTrait::len(&chunked),
            whole.len(),
            "{label}: transition length"
        );
        assert!(
            chunked.chunks.len() as u64 <= chunk_count,
            "{label}: transition chunks"
        );
        for i in probes(records) {
            let at = format!("{label}: transition record {}", i.0);
            assert_eq!(
                TransitionTableTrait::input_symbol(&chunked, i),
                whole.input_symbol(i),
                "{at}"
            );
            assert_eq!(
                TransitionTableTrait::output_symbol(&chunked, i),
                whole.output_symbol(i),
                "{at}"
            );
            assert_eq!(
                TransitionTableTrait::target(&chunked, i),
                whole.target(i),
                "{at}"
            );
            assert_eq!(
                TransitionTableTrait::weight(&chunked, i).map(|w| w.0.to_bits()),
                whole.weight(i).map(|w| w.0.to_bits()),
                "{at}"
            );
            assert_eq!(
                TransitionTableTrait::is_final(&chunked, i),
                whole.is_final(i),
                "{at}"
            );
            assert_eq!(
                arc(Some(TransitionTableTrait::symbol_transition(&chunked, i))),
                arc(Some(whole.symbol_transition(i))),
                "{at}"
            );
        }
    }

    #[test]
    fn an_empty_table_has_no_records() {
        for &chunk_count in CHUNK_COUNTS {
            assert_tables_read_alike(0, 0, chunk_count);
            assert_tables_read_alike(0, 5, chunk_count);
        }
    }

    /// The record one past the end of a table in one chunk falls in a second
    /// chunk, which does not exist.
    #[test]
    fn a_table_in_one_chunk_ends_where_the_table_ends() {
        for records in [1, 2, 5, 12] {
            assert_tables_read_alike(records, 0, 1);
        }
    }

    #[test]
    fn a_table_with_fewer_records_than_chunks_maps_one_record_a_chunk() {
        for (records, chunk_count) in [(1, 2), (3, 4), (5, 16), (15, 16)] {
            assert_tables_read_alike(records, 0, chunk_count);
        }
    }

    #[test]
    fn a_short_last_chunk_ends_where_the_table_ends() {
        for (records, chunk_count) in [(5, 2), (10, 4), (17, 8), (100, 16)] {
            assert_tables_read_alike(records, 0, chunk_count);
        }
    }

    /// Chunks split on record boundaries: five 12-byte transitions in two
    /// chunks is 30 bytes a chunk, which would start the second chunk inside
    /// a record.
    #[test]
    fn chunks_hold_whole_records() {
        for (records, chunk_count) in [(5, 2), (7, 3), (9, 4), (12, 8)] {
            assert_tables_read_alike(records, 0, chunk_count);
        }
    }

    #[test]
    fn bytes_after_the_last_record_are_not_a_record() {
        for (records, chunk_count) in [(1, 1), (4, 2), (9, 4), (16, 16)] {
            assert_tables_read_alike(records, 5, chunk_count);
        }
    }

    #[test]
    fn every_size_reads_alike_in_every_chunk_count() {
        for records in 0..=40 {
            for &chunk_count in CHUNK_COUNTS {
                assert_tables_read_alike(records, 0, chunk_count);
            }
        }
    }
}
