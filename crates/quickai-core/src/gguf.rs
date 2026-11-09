//! What QuickAI reads of a GGUF model file's header: the longest context the model was trained for, how many layers it
//! has, and how much memory each token of context takes. The panel's context grid says from these where each length
//! fits (C212, P1 of `the mockups`).
//!
//! The header is a list of keys and values before the tensors: `GGUF`, a version, the number of tensors and of keys,
//! then each key (a text) with its type and value. Only a few numbers are kept; the rest (the tokenizer's lists, which
//! are most of the header) is stepped over without being read into memory.

use std::collections::HashMap;
use std::io::{self, BufReader, Read, Seek};
use std::path::Path;

/// "GGUF", as the first four bytes read as a little-endian number.
const MAGIC: u32 = 0x4655_4747;

/// The most keys a header is believed to have, and the longest text kept: a file that says more is not read further.
const MOST_KEYS: u64 = 1_000_000;
const LONGEST_TEXT: u64 = 1 << 20;
/// The longest list of numbers kept (a number for each layer).
const LONGEST_LIST: u64 = 65_536;

/// What a model file says of the memory it needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelFacts {
    /// `general.architecture` (`llama`, `qwen3`, `lfm2`...).
    pub architecture: Option<String>,
    /// The longest context it was trained for, in tokens.
    pub context_length: Option<u64>,
    /// Its layers (`block_count`).
    pub layers: Option<u64>,
    /// The bytes one token of context takes, keys and values at 16 bits (llama.cpp's own default).
    pub kv_bytes_per_token: Option<u64>,
}

impl ModelFacts {
    /// The bytes `tokens` of context take, when that is known.
    pub fn context_bytes(&self, tokens: u64) -> Option<u64> {
        self.kv_bytes_per_token.map(|b| b.saturating_mul(tokens))
    }
}

/// Reads the facts of the model file at `path` (the first file of a model in parts has the header).
pub fn read(path: &Path) -> io::Result<ModelFacts> {
    parse(BufReader::new(std::fs::File::open(path)?))
}

/// A value of the header that is kept.
#[derive(Clone, Debug, PartialEq)]
enum Kept {
    Number(u64),
    Text(String),
    Numbers(Vec<u64>),
}

/// Reads the facts from a header.
pub fn parse<R: Read + Seek>(mut r: BufReader<R>) -> io::Result<ModelFacts> {
    if u32_of(&mut r)? != MAGIC {
        return Err(bad("not a GGUF file"));
    }
    let version = u32_of(&mut r)?;
    if !(2..=3).contains(&version) {
        return Err(bad(&format!(
            "GGUF version {version} is not one QuickAI reads"
        )));
    }
    let _tensors = u64_of(&mut r)?;
    let keys = u64_of(&mut r)?;
    if keys > MOST_KEYS {
        return Err(bad("the header says it has too many keys"));
    }
    let mut kept: HashMap<String, Kept> = HashMap::new();
    for _ in 0..keys {
        let key = text_of(&mut r)?;
        let kind = u32_of(&mut r)?;
        if let Some(value) = value_of(&mut r, kind, wanted(&key))? {
            kept.insert(key, value);
        }
    }
    Ok(facts(&kept))
}

/// Whether a key is one of those the facts come from, and so worth keeping.
fn wanted(key: &str) -> bool {
    key == "general.architecture"
        || [
            ".context_length",
            ".block_count",
            ".embedding_length",
            ".attention.head_count",
            ".attention.head_count_kv",
            ".attention.key_length",
            ".attention.value_length",
            ".attention.kv_lora_rank",
            ".rope.dimension_count",
        ]
        .iter()
        .any(|end| key.ends_with(end))
}

/// The facts, from the values kept.
fn facts(kept: &HashMap<String, Kept>) -> ModelFacts {
    let architecture = match kept.get("general.architecture") {
        Some(Kept::Text(a)) => Some(a.clone()),
        _ => None,
    };
    let Some(arch) = architecture.clone() else {
        return ModelFacts::default();
    };
    let get = |name: &str| kept.get(&format!("{arch}.{name}"));
    let number = |name: &str| match get(name) {
        Some(Kept::Number(n)) => Some(*n),
        _ => None,
    };
    // a number for the whole model, or one for each layer
    let per_layer = |name: &str, layer: usize| match get(name) {
        Some(Kept::Number(n)) => Some(*n),
        Some(Kept::Numbers(list)) => list.get(layer).or(list.last()).copied(),
        _ => None,
    };
    let layers = number("block_count");
    let kv_bytes_per_token = layers.and_then(|layers| {
        // DeepSeek's attention keeps one compressed vector a token (and the part that is turned), not keys and values
        if let Some(rank) = number("attention.kv_lora_rank") {
            let rope = number("rope.dimension_count").unwrap_or(64);
            return Some(layers * (rank + rope) * 2);
        }
        // the width of a head: as the file says, or the model's width shared among its heads
        let heads = (0..layers as usize)
            .find_map(|l| per_layer("attention.head_count", l).filter(|h| *h > 0));
        let width = number("embedding_length");
        let key = number("attention.key_length").or_else(|| Some(width? / heads?))?;
        let value = number("attention.value_length").unwrap_or(key);
        let mut total = 0u64;
        for layer in 0..layers as usize {
            // a layer with no heads of keys (the convolution or recurrent layers of a hybrid) keeps nothing a token
            let kv_heads = per_layer("attention.head_count_kv", layer)
                .or_else(|| per_layer("attention.head_count", layer))?;
            total += kv_heads * (key + value) * 2;
        }
        Some(total)
    });
    ModelFacts {
        architecture,
        context_length: number("context_length"),
        layers,
        kv_bytes_per_token,
    }
}

/// Reads a value of type `kind`: kept when it is `wanted` and a number, a text or a short list of numbers, else stepped
/// over.
fn value_of<R: Read + Seek>(
    r: &mut BufReader<R>,
    kind: u32,
    wanted: bool,
) -> io::Result<Option<Kept>> {
    match kind {
        // the numbers, by their size; a negative one is not a count of anything, and is kept as 0
        0 | 7 => Ok(number_kept(u8_of(r)? as u64, wanted)),
        1 => Ok(number_kept((u8_of(r)? as i8).max(0) as u64, wanted)),
        2 | 3 => {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            let n = u16::from_le_bytes(b);
            let n = if kind == 3 {
                (n as i16).max(0) as u64
            } else {
                n as u64
            };
            Ok(number_kept(n, wanted))
        }
        4 => Ok(number_kept(u32_of(r)? as u64, wanted)),
        5 => Ok(number_kept((u32_of(r)? as i32).max(0) as u64, wanted)),
        6 => {
            u32_of(r)?;
            Ok(None)
        }
        10 => Ok(number_kept(u64_of(r)?, wanted)),
        11 => Ok(number_kept((u64_of(r)? as i64).max(0) as u64, wanted)),
        12 => {
            u64_of(r)?;
            Ok(None)
        }
        8 => {
            if wanted {
                Ok(Some(Kept::Text(text_of(r)?)))
            } else {
                skip_text(r)?;
                Ok(None)
            }
        }
        9 => {
            let inner = u32_of(r)?;
            let count = u64_of(r)?;
            let numeric = matches!(inner, 0..=5 | 10 | 11);
            if wanted && numeric && count <= LONGEST_LIST {
                let mut list = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    if let Some(Kept::Number(n)) = value_of(r, inner, true)? {
                        list.push(n);
                    } else {
                        list.push(0);
                    }
                }
                return Ok(Some(Kept::Numbers(list)));
            }
            match size_of(inner) {
                Some(size) => r.seek_relative(
                    i64::try_from(size.saturating_mul(count))
                        .map_err(|_| bad("a list is too long"))?,
                )?,
                None => {
                    for _ in 0..count {
                        value_of(r, inner, false)?;
                    }
                }
            }
            Ok(None)
        }
        other => Err(bad(&format!("a value of an unknown type ({other})"))),
    }
}

fn number_kept(n: u64, wanted: bool) -> Option<Kept> {
    wanted.then_some(Kept::Number(n))
}

/// The bytes of one value of a type of a fixed size; `None` for a text or a list.
fn size_of(kind: u32) -> Option<u64> {
    match kind {
        0 | 1 | 7 => Some(1),
        2 | 3 => Some(2),
        4..=6 => Some(4),
        10..=12 => Some(8),
        _ => None,
    }
}

fn u8_of<R: Read>(r: &mut R) -> io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

fn u32_of<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn u64_of<R: Read>(r: &mut R) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn text_of<R: Read>(r: &mut R) -> io::Result<String> {
    let len = u64_of(r)?;
    if len > LONGEST_TEXT {
        return Err(bad("a text of the header is too long"));
    }
    let mut b = vec![0u8; len as usize];
    r.read_exact(&mut b)?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

fn skip_text<R: Read + Seek>(r: &mut BufReader<R>) -> io::Result<()> {
    let len = u64_of(r)?;
    r.seek_relative(i64::try_from(len).map_err(|_| bad("a text is too long"))?)
}

fn bad(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A header with the given keys, as a GGUF file begins.
    fn header(keys: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(MAGIC.to_le_bytes());
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend((keys.len() as u64).to_le_bytes());
        for (key, value) in keys {
            b.extend((key.len() as u64).to_le_bytes());
            b.extend(key.as_bytes());
            b.extend(value);
        }
        b
    }

    fn u32_value(n: u32) -> Vec<u8> {
        let mut b = 4u32.to_le_bytes().to_vec();
        b.extend(n.to_le_bytes());
        b
    }

    fn text_value(t: &str) -> Vec<u8> {
        let mut b = 8u32.to_le_bytes().to_vec();
        b.extend((t.len() as u64).to_le_bytes());
        b.extend(t.as_bytes());
        b
    }

    fn texts_value(list: &[&str]) -> Vec<u8> {
        let mut b = 9u32.to_le_bytes().to_vec();
        b.extend(8u32.to_le_bytes());
        b.extend((list.len() as u64).to_le_bytes());
        for t in list {
            b.extend((t.len() as u64).to_le_bytes());
            b.extend(t.as_bytes());
        }
        b
    }

    fn u32s_value(list: &[u32]) -> Vec<u8> {
        let mut b = 9u32.to_le_bytes().to_vec();
        b.extend(4u32.to_le_bytes());
        b.extend((list.len() as u64).to_le_bytes());
        for n in list {
            b.extend(n.to_le_bytes());
        }
        b
    }

    fn read_bytes(b: Vec<u8>) -> ModelFacts {
        parse(BufReader::new(Cursor::new(b))).unwrap()
    }

    #[test]
    fn a_model_with_grouped_heads_takes_its_layers_times_its_kv_heads_times_both_widths() {
        // 28 layers, 16 heads of 128 (width 2048), 8 of them for keys and values: 28 x 8 x 256 x 2 bytes a token
        let facts = read_bytes(header(&[
            ("general.architecture", text_value("qwen3")),
            ("tokenizer.ggml.tokens", texts_value(&["a", "bb", "ccc"])),
            ("qwen3.context_length", u32_value(40960)),
            ("qwen3.block_count", u32_value(28)),
            ("qwen3.embedding_length", u32_value(2048)),
            ("qwen3.attention.head_count", u32_value(16)),
            ("qwen3.attention.head_count_kv", u32_value(8)),
        ]));
        assert_eq!(facts.architecture.as_deref(), Some("qwen3"));
        assert_eq!(facts.context_length, Some(40960));
        assert_eq!(facts.layers, Some(28));
        assert_eq!(facts.kv_bytes_per_token, Some(28 * 8 * 256 * 2));
        assert_eq!(facts.context_bytes(1024), Some(28 * 8 * 256 * 2 * 1024));
    }

    #[test]
    fn the_layers_of_a_hybrid_that_keep_no_keys_take_nothing_a_token() {
        // four layers, two of them convolutions (no heads of keys), and the widths said by the file
        let facts = read_bytes(header(&[
            ("general.architecture", text_value("lfm2")),
            ("lfm2.block_count", u32_value(4)),
            ("lfm2.context_length", u32_value(128_000)),
            ("lfm2.attention.head_count", u32_value(32)),
            ("lfm2.attention.head_count_kv", u32s_value(&[0, 8, 0, 8])),
            ("lfm2.attention.key_length", u32_value(64)),
            ("lfm2.attention.value_length", u32_value(64)),
        ]));
        assert_eq!(facts.kv_bytes_per_token, Some(2 * 8 * 128 * 2));
    }

    #[test]
    fn a_file_that_is_not_gguf_is_said_to_be_one() {
        let err = parse(BufReader::new(Cursor::new(b"PK\x03\x04rest".to_vec()))).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn without_the_numbers_of_its_heads_what_a_token_takes_is_not_known() {
        let facts = read_bytes(header(&[
            ("general.architecture", text_value("odd")),
            ("odd.block_count", u32_value(10)),
        ]));
        assert_eq!(facts.layers, Some(10));
        assert_eq!(facts.kv_bytes_per_token, None);
    }
}
