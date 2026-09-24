use ghv_parse::{Extractor, Lang};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Default)]
pub struct Parser(Extractor);

#[wasm_bindgen]
impl Parser {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Parser {
        Parser::default()
    }

    /// Extracts imports for a batch: file `i` is `bytes[offsets[i]..offsets[i + 1]]`, its language
    /// inferred from `names[i]`. Returns one newline-joined record list per file.
    pub fn extract(&mut self, names: Vec<String>, bytes: &[u8], offsets: &[u32]) -> Vec<String> {
        names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let src = &bytes[offsets[i] as usize..offsets[i + 1] as usize];
                Lang::from_name(name).map(|lang| self.0.extract(lang, src).join("\n")).unwrap_or_default()
            })
            .collect()
    }
}
