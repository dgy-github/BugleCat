//! Deterministic text decoding shared by file and process-output tools.
//!
//! Coding workspaces on Windows commonly mix UTF-8, UTF-16, and legacy
//! GB18030/GBK files. Treating every byte stream as UTF-8 either hides files
//! from search or replaces useful text with mojibake. This module recognizes
//! explicit BOMs first, accepts strict UTF-8, then performs a conservative
//! legacy-text fallback while rejecting obvious binary data.

use encoding_rs::{GB18030, UTF_16BE, UTF_16LE, WINDOWS_1252};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Gb18030,
    Windows1252,
}

impl TextEncoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 BOM",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Gb18030 => "GB18030/GBK",
            Self::Windows1252 => "Windows-1252",
        }
    }

    pub fn is_plain_utf8(self) -> bool {
        matches!(self, Self::Utf8)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedText {
    pub text: String,
    pub encoding: TextEncoding,
}

/// Decode a source-like byte buffer without silently replacing malformed data.
pub fn decode_text(bytes: &[u8]) -> Result<DecodedText, &'static str> {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        let text = std::str::from_utf8(rest).map_err(|_| "invalid UTF-8 after BOM")?;
        return Ok(decoded(text, TextEncoding::Utf8Bom));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return decode_with(UTF_16LE, rest, TextEncoding::Utf16Le);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return decode_with(UTF_16BE, rest, TextEncoding::Utf16Be);
    }

    if let Some(encoding) = likely_bomless_utf16(bytes) {
        let decoder = match encoding {
            TextEncoding::Utf16Le => UTF_16LE,
            TextEncoding::Utf16Be => UTF_16BE,
            _ => unreachable!(),
        };
        return decode_with(decoder, bytes, encoding);
    }
    if looks_binary(bytes) {
        return Err("binary data");
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(decoded(text, TextEncoding::Utf8));
    }

    let gb18030 = GB18030
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|text| text.into_owned());
    if let Some(text) = gb18030.as_ref().filter(|text| contains_cjk(text)) {
        return Ok(DecodedText {
            text: text.clone(),
            encoding: TextEncoding::Gb18030,
        });
    }
    if let Some(text) = WINDOWS_1252
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|text| text.into_owned())
    {
        return Ok(DecodedText {
            text,
            encoding: TextEncoding::Windows1252,
        });
    }
    if let Some(text) = gb18030 {
        return Ok(DecodedText {
            text,
            encoding: TextEncoding::Gb18030,
        });
    }
    Err("unsupported or malformed text encoding")
}

/// Decode human-facing process output, preserving the old lossy fallback for
/// genuinely malformed byte streams instead of dropping all output.
pub fn decode_text_lossy(bytes: &[u8]) -> String {
    decode_text(bytes)
        .map(|decoded| decoded.text)
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned())
}

/// Stateful UTF-8 decoder for incremental terminal/process chunks. Commands
/// spawned by ncx use UTF-8 mode on Windows, and this decoder keeps an
/// incomplete multibyte sequence until the next chunk instead of emitting a
/// replacement character at arbitrary read boundaries.
#[derive(Debug, Default)]
pub struct Utf8StreamDecoder {
    pending: Vec<u8>,
}

impl Utf8StreamDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.decode(bytes, false)
    }

    pub fn finish(&mut self) -> String {
        self.decode(&[], true)
    }

    fn decode(&mut self, bytes: &[u8], finish: bool) -> String {
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(bytes);
        let mut output = String::new();
        let mut cursor = 0usize;
        while cursor < data.len() {
            match std::str::from_utf8(&data[cursor..]) {
                Ok(text) => {
                    output.push_str(text);
                    cursor = data.len();
                }
                Err(error) => {
                    let valid_end = cursor + error.valid_up_to();
                    if valid_end > cursor {
                        // SAFETY: `valid_up_to` identifies a valid UTF-8 prefix.
                        output.push_str(unsafe {
                            std::str::from_utf8_unchecked(&data[cursor..valid_end])
                        });
                    }
                    match error.error_len() {
                        Some(length) => {
                            output.push('\u{FFFD}');
                            cursor = valid_end + length;
                        }
                        None if finish => {
                            output.push_str(&String::from_utf8_lossy(&data[valid_end..]));
                            cursor = data.len();
                        }
                        None => {
                            self.pending.extend_from_slice(&data[valid_end..]);
                            cursor = data.len();
                        }
                    }
                }
            }
        }
        output
    }
}

fn decoded(text: &str, encoding: TextEncoding) -> DecodedText {
    DecodedText {
        text: text.to_string(),
        encoding,
    }
}

fn decode_with(
    encoding: &'static encoding_rs::Encoding,
    bytes: &[u8],
    label: TextEncoding,
) -> Result<DecodedText, &'static str> {
    encoding
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|text| DecodedText {
            text: text.into_owned(),
            encoding: label,
        })
        .ok_or("malformed encoded text")
}

fn likely_bomless_utf16(bytes: &[u8]) -> Option<TextEncoding> {
    if bytes.len() < 8 || !bytes.len().is_multiple_of(2) {
        return None;
    }
    let pairs = bytes.len() / 2;
    let even_nuls = bytes.iter().step_by(2).filter(|byte| **byte == 0).count();
    let odd_nuls = bytes
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|byte| **byte == 0)
        .count();
    if odd_nuls * 3 >= pairs && even_nuls * 10 <= pairs {
        Some(TextEncoding::Utf16Le)
    } else if even_nuls * 3 >= pairs && odd_nuls * 10 <= pairs {
        Some(TextEncoding::Utf16Be)
    } else {
        None
    }
}

fn looks_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let sample = &bytes[..bytes.len().min(8_192)];
    if sample.contains(&0) {
        return true;
    }
    let controls = sample
        .iter()
        .filter(|byte| {
            **byte < 0x20 && !matches!(**byte, b'\n' | b'\r' | b'\t' | 0x07 | 0x08 | 0x0C | 0x1B)
        })
        .count();
    controls * 100 > sample.len()
}

fn contains_cjk(text: &str) -> bool {
    text.chars()
        .any(|ch| matches!(ch as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_bom_utf16_and_gb18030() {
        assert_eq!(
            decode_text(b"\xEF\xBB\xBFhello").unwrap().encoding,
            TextEncoding::Utf8Bom
        );

        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend("中文".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode_text(&utf16).unwrap().text, "中文");

        let gb = [0xD6, 0xD0, 0xCE, 0xC4];
        let decoded = decode_text(&gb).unwrap();
        assert_eq!(decoded.text, "中文");
        assert_eq!(decoded.encoding, TextEncoding::Gb18030);
    }

    #[test]
    fn rejects_obvious_binary_data() {
        assert_eq!(decode_text(&[0, 1, 2, 3, 4]), Err("binary data"));
    }

    #[test]
    fn stream_decoder_preserves_utf8_split_at_every_byte() {
        let source = "终端输出：中文路径/文件.rs\n";
        for split in 0..=source.len() {
            let mut decoder = Utf8StreamDecoder::default();
            let mut output = decoder.push(&source.as_bytes()[..split]);
            output.push_str(&decoder.push(&source.as_bytes()[split..]));
            output.push_str(&decoder.finish());
            assert_eq!(output, source, "split at byte {split}");
        }
    }
}
