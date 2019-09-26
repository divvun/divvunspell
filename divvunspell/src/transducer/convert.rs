use std::fs::File;
use std::io::{prelude::*, BufWriter};
use std::path::Path;

use byteorder::{LittleEndian, WriteBytesExt};

use super::hfst;
use super::thfst;
use crate::transducer::Transducer;

pub trait ConvertFile<T> {
    fn convert_file(transducer: &T, path: &Path) -> Result<(), std::io::Error>;
}

pub trait ConvertFrom<T> {
    fn convert_from<W: Write>(from: &T, writer: &mut W) -> Result<(), std::io::Error>;
}

impl ConvertFile<hfst::HfstTransducer<std::fs::File>>
    for thfst::MemmapThfstTransducer<std::fs::File>
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
        thfst::MemmapTransitionTable::convert_from(&transducer.transition_table, &mut writer)?;

        let mut writer = BufWriter::new(File::create(index_path)?);
        thfst::MemmapIndexTable::convert_from(&transducer.index_table, &mut writer)?;

        let writer = BufWriter::new(File::create(alphabet_path)?);
        serde_json::to_writer_pretty(writer, transducer.alphabet())?;

        Ok(())
    }
}

impl ConvertFrom<hfst::MappedIndexTable> for thfst::MemmapIndexTable<std::fs::File> {
    fn convert_from<W: Write>(
        table: &hfst::MappedIndexTable,
        writer: &mut W,
    ) -> Result<(), std::io::Error> {
        use std::{u16, u32};

        // eprintln!(
        //     "size: {}, len: {}, offset: {}",
        //     table.size, table.len, table.offset
        // );

        for index in 0..table.size {
            let input_symbol = table.input_symbol(index).unwrap_or(u16::MAX);
            let targetish = table.target(index).unwrap_or(u32::MAX);

            writer.write_u16::<LittleEndian>(input_symbol).unwrap();
            writer.write_u16::<LittleEndian>(0).unwrap();
            writer.write_u32::<LittleEndian>(targetish).unwrap();
        }

        Ok(())
    }
}

impl ConvertFrom<hfst::MappedTransitionTable> for thfst::MemmapTransitionTable<std::fs::File> {
    fn convert_from<W: Write>(
        table: &hfst::MappedTransitionTable,
        writer: &mut W,
    ) -> Result<(), std::io::Error> {
        use std::{u16, u32};

        // eprintln!(
        //     "size: {}, len: {}, offset: {}",
        //     table.size, table.len, table.offset
        // );

        for index in 0..table.size {
            let input_symbol = table.input_symbol(index).unwrap_or(u16::MAX);
            let output_symbol = table.output_symbol(index).unwrap_or(u16::MAX);
            let target = table.target(index).unwrap_or(u32::MAX);
            let weight = table.weight(index).unwrap();

            writer.write_u16::<LittleEndian>(input_symbol).unwrap();
            writer.write_u16::<LittleEndian>(output_symbol).unwrap();
            writer.write_u32::<LittleEndian>(target).unwrap();
            writer
                .write_u32::<LittleEndian>(unsafe { std::mem::transmute::<f32, u32>(weight) })
                .unwrap();
        }

        Ok(())
    }
}
