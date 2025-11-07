use std::fs::File;
use std::io::{BufWriter, prelude::*};
use std::path::Path;

use byteorder::{LittleEndian, WriteBytesExt};

use super::hfst;
use super::thfst;
use crate::transducer::Transducer;
use crate::types::{SymbolNumber, TransitionTableIndex};

pub trait ConvertFile<T> {
    fn convert_file(transducer: &T, path: &Path) -> Result<(), std::io::Error>;
}

pub trait ConvertFrom<T> {
    fn convert_from<W: Write>(from: &T, writer: &mut W) -> Result<(), std::io::Error>;
}

impl ConvertFile<hfst::HfstTransducer<std::fs::File>>
    for thfst::MmapThfstTransducer<std::fs::File>
{
    fn convert_file(
        transducer: &hfst::HfstTransducer<std::fs::File>,
        path: &Path,
    ) -> Result<(), std::io::Error> {
        let thfst_path = path.with_extension("thfst");
        std::fs::create_dir_all(&thfst_path)?;

        let transition_path = thfst_path.join("transition");
        let index_path = thfst_path.join("index");
        let alphabet_path = thfst_path.join("alphabet");

        let mut writer = BufWriter::new(File::create(transition_path)?);
        thfst::transition_table::TransitionTable::<memmap2::Mmap>::convert_from(
            &transducer.transition_table,
            &mut writer,
        )?;

        let mut writer = BufWriter::new(File::create(index_path)?);
        thfst::index_table::IndexTable::<memmap2::Mmap>::convert_from(
            &transducer.index_table,
            &mut writer,
        )?;

        let writer = BufWriter::new(File::create(alphabet_path)?);
        serde_json::to_writer_pretty(writer, transducer.alphabet())?;

        Ok(())
    }
}

impl ConvertFrom<hfst::index_table::MappedIndexTable>
    for thfst::index_table::IndexTable<memmap2::Mmap>
{
    fn convert_from<W: Write>(
        table: &hfst::index_table::MappedIndexTable,
        writer: &mut W,
    ) -> Result<(), std::io::Error> {
        for index in 0..table.size.0 {
            let input_symbol = table
                .input_symbol(TransitionTableIndex(index))
                .unwrap_or(SymbolNumber::MAX);
            let targetish = table
                .target(TransitionTableIndex(index))
                .unwrap_or(TransitionTableIndex::MAX);

            writer.write_u16::<LittleEndian>(input_symbol.0).unwrap();
            writer.write_u16::<LittleEndian>(0).unwrap();
            writer.write_u32::<LittleEndian>(targetish.0).unwrap();
        }

        Ok(())
    }
}

impl ConvertFrom<hfst::transition_table::MappedTransitionTable>
    for thfst::transition_table::TransitionTable<memmap2::Mmap>
{
    fn convert_from<W: Write>(
        table: &hfst::transition_table::MappedTransitionTable,
        writer: &mut W,
    ) -> Result<(), std::io::Error> {
        for index in 0..table.size.0 {
            let index = TransitionTableIndex(index);
            let input_symbol = table.input_symbol(index).unwrap_or(SymbolNumber::MAX);
            let output_symbol = table.output_symbol(index).unwrap_or(SymbolNumber::MAX);
            let target = table.target(index).unwrap_or(TransitionTableIndex::MAX);
            let weight = table.weight(index).unwrap();

            writer.write_u16::<LittleEndian>(input_symbol.0).unwrap();
            writer.write_u16::<LittleEndian>(output_symbol.0).unwrap();
            writer.write_u32::<LittleEndian>(target.0).unwrap();
            writer
                .write_u32::<LittleEndian>(weight.0.to_bits())
                .unwrap();
        }

        Ok(())
    }
}
