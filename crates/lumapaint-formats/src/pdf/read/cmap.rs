//! Bounded PDF Encoding CMaps: source bytes -> CID, not Unicode -> glyph guesses.
use super::*;
use std::collections::HashMap;
#[derive(Clone)]
pub(super) struct CMap {
    spaces: Vec<(u8, u32, u32)>,
    mapping: HashMap<(u8, u32), u16>,
    notdef: HashMap<(u8, u32), u16>,
    identity: bool,
    pub vertical: bool,
}
impl CMap {
    pub fn identity(vertical: bool) -> Self {
        Self {
            spaces: vec![(2, 0, 65535)],
            mapping: HashMap::new(),
            notdef: HashMap::new(),
            identity: true,
            vertical,
        }
    }
    pub fn memory_bytes(&self) -> usize {
        (self.mapping.capacity() + self.notdef.capacity()) * 32 + self.spaces.capacity() * 16
    }
    pub fn decode(&self, bytes: &[u8]) -> Result<Vec<(u16, bool)>, ImportError> {
        let mut output = Vec::new();
        let mut offset = 0;
        while offset < bytes.len() {
            let mut matched = None;
            for length in 1..=4u8 {
                let Some(code) = bytes.get(offset..offset + usize::from(length)) else {
                    break;
                };
                let value = code.iter().fold(0u32, |v, b| (v << 8) | u32::from(*b));
                if self.spaces.iter().any(|&(n, lo, hi)| {
                    n == length
                        && (0..length).all(|i| {
                            let shift = (length - i - 1) * 8;
                            let c = (value >> shift) & 255;
                            c >= ((lo >> shift) & 255) && c <= ((hi >> shift) & 255)
                        })
                }) {
                    matched = Some((length, value));
                    break;
                }
            }
            let (length, value) = matched.ok_or(ImportError::Malformed("pdf.font_mapping"))?;
            let cid = self
                .mapping
                .get(&(length, value))
                .copied()
                .or_else(|| self.identity.then(|| u16::try_from(value).ok()).flatten())
                .or_else(|| self.notdef.get(&(length, value)).copied())
                .unwrap_or(0);
            output.push((cid, length == 1 && value == 32));
            if output.len() > 65536 {
                return Err(ImportError::LimitExceeded("pdf.text_glyphs"));
            }
            offset += usize::from(length);
        }
        Ok(output)
    }
}
#[derive(Clone, Copy)]
enum Token<'a> {
    Hex(u8, u32),
    Word(&'a [u8]),
    Opaque,
}
fn tokens(data: &[u8]) -> Result<Vec<Token<'_>>, ImportError> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b if b.is_ascii_whitespace() => {
                i += 1;
                continue;
            }
            b'%' => {
                while i < data.len() && !b"\r\n".contains(&data[i]) {
                    i += 1;
                }
                continue;
            }
            b'(' => {
                i += 1;
                let mut depth = 1;
                while i < data.len() && depth > 0 {
                    match data[i] {
                        b'\\' => {
                            i += 1;
                        }
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                if depth != 0 {
                    return Err(malformed());
                }
                out.push(Token::Opaque);
            }
            b'<' | b'>' if data.get(i + 1) == Some(&data[i]) => {
                i += 2;
                out.push(Token::Opaque);
            }
            b'<' if data.get(i + 1) != Some(&b'<') => {
                i += 1;
                let mut digits = Vec::new();
                while i < data.len() && data[i] != b'>' {
                    if !data[i].is_ascii_whitespace() {
                        digits.push(data[i]);
                    }
                    i += 1;
                }
                if i == data.len() || digits.is_empty() || digits.len() > 8 {
                    return Err(malformed());
                }
                i += 1;
                if !digits.len().is_multiple_of(2) {
                    digits.push(b'0');
                }
                let mut value = 0u32;
                for digit in &digits {
                    value = (value << 4) | (*digit as char).to_digit(16).ok_or_else(malformed)?;
                }
                out.push(Token::Hex((digits.len() / 2) as u8, value));
            }
            b'<' | b'>' | b'[' | b']' | b'{' | b'}' => {
                i += 1;
                out.push(Token::Opaque);
            }
            _ => {
                let start = i;
                i += 1;
                while i < data.len()
                    && !data[i].is_ascii_whitespace()
                    && !b"%()<>[]{}".contains(&data[i])
                {
                    i += 1;
                }
                out.push(Token::Word(&data[start..i]));
            }
        }
        if out.len() > 262144 {
            return Err(ImportError::LimitExceeded("pdf.font_mapping"));
        }
    }
    Ok(out)
}
fn number(t: Token<'_>) -> Result<u32, ImportError> {
    if let Token::Word(w) = t {
        std::str::from_utf8(w)
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(malformed)
    } else {
        Err(malformed())
    }
}
fn hex(t: Token<'_>) -> Result<(u8, u32), ImportError> {
    if let Token::Hex(n, v) = t {
        Ok((n, v))
    } else {
        Err(malformed())
    }
}
pub(super) fn parse(data: &[u8], parent: Option<&[u8]>) -> Result<CMap, ImportError> {
    let tokens = tokens(data)?;
    let mut result = match parent {
        None => CMap {
            spaces: vec![],
            mapping: HashMap::new(),
            notdef: HashMap::new(),
            identity: false,
            vertical: false,
        },
        Some(b"Identity-H") => CMap::identity(false),
        Some(b"Identity-V") => CMap::identity(true),
        _ => return Err(ImportError::Unsupported("pdf.font_encoding")),
    };
    let mut assignments = 0usize;
    let mut i = 0;
    while i < tokens.len() {
        if let Token::Word(word) = tokens[i] {
            match word {
                b"/WMode" => {
                    let mode = number(*tokens.get(i + 1).ok_or_else(malformed)?)?;
                    if mode > 1 {
                        return Err(malformed());
                    }
                    result.vertical = mode == 1;
                }
                b"usecmap" => {
                    let Token::Word(name) = *tokens.get(i.wrapping_sub(1)).ok_or_else(malformed)?
                    else {
                        return Err(malformed());
                    };
                    if ![b"/Identity-H".as_slice(), b"/Identity-V"].contains(&name) {
                        return Err(ImportError::Unsupported("pdf.font_encoding"));
                    }
                    result.identity = true;
                    result.vertical = name == b"/Identity-V";
                    if !result.spaces.contains(&(2, 0, 65535)) {
                        result.spaces.push((2, 0, 65535));
                    }
                }
                b"begincodespacerange"
                | b"begincidchar"
                | b"begincidrange"
                | b"beginnotdefchar"
                | b"beginnotdefrange" => {
                    let count =
                        number(*tokens.get(i.wrapping_sub(1)).ok_or_else(malformed)?)? as usize;
                    if count > 65536 {
                        return Err(ImportError::LimitExceeded("pdf.font_mapping"));
                    }
                    let range = word.ends_with(b"range");
                    let space = word == b"begincodespacerange";
                    let notdef = word.starts_with(b"beginnotdef");
                    for _ in 0..count {
                        i += 1;
                        let (n, lo) = hex(*tokens.get(i).ok_or_else(malformed)?)?;
                        let hi = if range {
                            i += 1;
                            let (m, hi) = hex(*tokens.get(i).ok_or_else(malformed)?)?;
                            if m != n || hi < lo {
                                return Err(malformed());
                            }
                            hi
                        } else {
                            lo
                        };
                        if space {
                            if result.spaces.len() >= 64 {
                                return Err(ImportError::LimitExceeded("pdf.font_mapping"));
                            }
                            result.spaces.push((n, lo, hi));
                        } else {
                            i += 1;
                            let cid = number(*tokens.get(i).ok_or_else(malformed)?)?;
                            let count = u64::from(hi) - u64::from(lo) + 1;
                            assignments =
                                assignments
                                    .checked_add(usize::try_from(count).map_err(|_| {
                                        ImportError::LimitExceeded("pdf.font_mapping")
                                    })?)
                                    .ok_or_else(malformed)?;
                            if assignments > 65536 {
                                return Err(ImportError::LimitExceeded("pdf.font_mapping"));
                            }
                            if cid > 65535 || (!notdef && u64::from(cid) + count - 1 > 65535) {
                                return Err(malformed());
                            }
                            let target = if notdef {
                                &mut result.notdef
                            } else {
                                &mut result.mapping
                            };
                            for v in lo..=hi {
                                target
                                    .insert((n, v), (cid + if notdef { 0 } else { v - lo }) as u16);
                            }
                        }
                    }
                    i += 1;
                    let Token::Word(end) = *tokens.get(i).ok_or_else(malformed)? else {
                        return Err(malformed());
                    };
                    let expected = [b"end".as_slice(), &word[5..]].concat();
                    if end != expected {
                        return Err(malformed());
                    }
                }
                b"beginbfchar" | b"beginbfrange" | b"beginrearrangedfont" | b"beginusematrix" => {
                    return Err(ImportError::Unsupported("pdf.font_encoding"))
                }
                _ => {}
            }
        }
        i += 1;
    }
    if result.spaces.len() > 64 {
        return Err(ImportError::LimitExceeded("pdf.font_mapping"));
    }
    if result.spaces.is_empty() {
        return Err(ImportError::Unsupported("pdf.font_encoding"));
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_lengths_ranges_vertical_and_notdef() {
        let c=parse(b"% ignored\n/WMode 1 def 3 begincodespacerange <00> <7f> <8140> <9ffc> <ff010000> <ff01ffff> endcodespacerange 1 begincidchar <20> 8 endcidchar 2 begincidrange <8140> <8142> 10 <ff010000> <ff010001> 20 endcidrange 1 beginnotdefrange <00> <1f> 7 endnotdefrange",None).unwrap();
        assert!(c.vertical);
        assert_eq!(
            c.decode(&[32, 0x81, 0x41, 0xff, 1, 0, 1, 1]).unwrap(),
            [(8, true), (11, false), (21, false), (7, false)]
        );
        assert!(c.decode(&[0x81]).is_err());
        assert!(c.decode(&[0x81, 0x20]).is_err()); // Byte-wise codespace bounds.
    }
    #[test]
    fn invalid_and_oversized_maps_do_not_guess() {
        for bytes in [
            b"1 begincidrange <00> <ffff> 1 endcidrange".as_slice(),
            b"1 begincodespacerange <00> <ff> endcidchar",
            b"1 begincidrange <00000000> <ffffffff> 0 endcidrange",
        ] {
            assert!(parse(bytes, None).is_err());
        }
        assert!(parse(b"/Unknown usecmap", None).is_err());
        assert_eq!(
            parse(
                b"1 begincodespacerange <0> <f> endcodespacerange 1 begincidchar <1> 9 endcidchar",
                None
            )
            .unwrap()
            .decode(&[0x10])
            .unwrap(),
            [(9, false)]
        );
        assert_eq!(
            parse(b"/Identity-V usecmap", None)
                .unwrap()
                .decode(&[0, 32])
                .unwrap(),
            [(32, false)]
        );
    }
}
