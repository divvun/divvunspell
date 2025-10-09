pub(crate) mod tokenizer;

#[doc(hidden)]
pub trait IntoFlatbuffer {
    fn into_flatbuffer(self) -> Vec<u8>;
}
