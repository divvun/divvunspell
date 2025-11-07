pub(crate) mod tokenizer;

#[doc(hidden)]
pub trait IntoFlatbuffer {
    fn into_flatbuffer(self) -> Vec<u8>;
}

impl IntoFlatbuffer for divvun_fst::tokenizer::WordContext {
    fn into_flatbuffer(self) -> Vec<u8> {
        use crate::fbs::tokenizer::*;

        macro_rules! add_indexed_word {
            ($fbb:expr, $data:expr) => {{
                if let Some((index, word)) = $data {
                    let s = $fbb.create_string(&word);
                    Some(IndexedWord::create(
                        &mut $fbb,
                        &IndexedWordArgs {
                            index: index as u64,
                            value: Some(s),
                        },
                    ))
                } else {
                    None
                }
            }};
        }

        let mut builder = flatbuffers::FlatBufferBuilder::with_capacity(1024);
        let current = add_indexed_word!(builder, Some(self.current));
        let first_before = add_indexed_word!(builder, self.first_before);
        let second_before = add_indexed_word!(builder, self.second_before);
        let first_after = add_indexed_word!(builder, self.first_after);
        let second_after = add_indexed_word!(builder, self.second_after);
        let word_context = WordContext::create(
            &mut builder,
            &WordContextArgs {
                current,
                first_before,
                second_before,
                first_after,
                second_after,
            },
        );
        builder.finish(word_context, None);
        builder.finished_data().to_vec()
    }
}
