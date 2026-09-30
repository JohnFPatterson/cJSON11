//! JSON parser matching cJSON parse_* behavior and error offsets.

use crate::error::{Error, Result};
use crate::value::{
    saturate_int, Value, NESTING_LIMIT, TYPE_ARRAY, TYPE_FALSE, TYPE_NULL, TYPE_NUMBER,
    TYPE_OBJECT, TYPE_STRING, TYPE_TRUE,
};

#[derive(Debug)]
struct ParseBuffer<'a> {
    content: &'a [u8],
    offset: usize,
    depth: usize,
}

impl<'a> ParseBuffer<'a> {
    fn new(content: &'a [u8]) -> Self {
        Self {
            content,
            offset: 0,
            depth: 0,
        }
    }

    fn can_access(&self, index: usize) -> bool {
        self.offset + index < self.content.len()
    }

    fn can_read(&self, n: usize) -> bool {
        n > 0 && self.offset + n - 1 < self.content.len()
    }

    fn at(&self) -> &[u8] {
        &self.content[self.offset..]
    }

    fn byte0(&self) -> Option<u8> {
        self.content.get(self.offset).copied()
    }

    fn skip_whitespace(&mut self) {
        while self.can_access(0) && self.content[self.offset] <= 32 {
            self.offset += 1;
        }
        if self.offset == self.content.len() && !self.content.is_empty() {
            self.offset -= 1;
        }
    }

    fn skip_utf8_bom(&mut self) -> bool {
        if self.offset != 0 {
            return false;
        }
        if self.can_access(4)
            && self.content.get(self.offset..self.offset + 3) == Some(&[0xef, 0xbb, 0xbf])
        {
            self.offset += 3;
        }
        true
    }
}

/// Thread-local error offset matching `cJSON_GetErrorPtr` semantics for the last
/// failed parse on this thread. FFI maintains the process-global pointer separately.
use std::cell::Cell;
thread_local! {
    static LAST_ERROR_OFFSET: Cell<Option<usize>> = const { Cell::new(None) };
}

pub fn last_error_offset() -> Option<usize> {
    LAST_ERROR_OFFSET.with(|c| c.get())
}

pub fn clear_error() {
    LAST_ERROR_OFFSET.with(|c| c.set(None));
}

fn set_error(offset: usize) {
    LAST_ERROR_OFFSET.with(|c| c.set(Some(offset)));
}

pub fn parse(input: &[u8]) -> Result<Value> {
    parse_with_opts(input, false).map(|(v, _)| v)
}

pub fn parse_with_opts(input: &[u8], require_null_terminated: bool) -> Result<(Value, usize)> {
    clear_error();
    if input.is_empty() {
        set_error(0);
        return Err(Error::Parse { offset: 0 });
    }

    let mut buffer = ParseBuffer::new(input);
    if !buffer.skip_utf8_bom() {
        set_error(0);
        return Err(Error::Parse { offset: 0 });
    }
    buffer.skip_whitespace();

    let mut item = Value::default();
    if !parse_value(&mut item, &mut buffer) {
        let offset = if buffer.offset < buffer.content.len() {
            buffer.offset
        } else if !buffer.content.is_empty() {
            buffer.content.len() - 1
        } else {
            0
        };
        set_error(offset);
        return Err(Error::Parse { offset });
    }

    if require_null_terminated {
        buffer.skip_whitespace();
        if buffer.offset >= buffer.content.len() || buffer.content[buffer.offset] != 0 {
            let offset = if buffer.offset < buffer.content.len() {
                buffer.offset
            } else if !buffer.content.is_empty() {
                buffer.content.len() - 1
            } else {
                0
            };
            set_error(offset);
            return Err(Error::Parse { offset });
        }
    }

    Ok((item, buffer.offset))
}

fn parse_value(item: &mut Value, buf: &mut ParseBuffer<'_>) -> bool {
    if buf.content.is_empty() {
        return false;
    }

    if buf.can_read(4) && buf.at().starts_with(b"null") {
        item.type_flags = TYPE_NULL;
        buf.offset += 4;
        return true;
    }
    if buf.can_read(5) && buf.at().starts_with(b"false") {
        item.type_flags = TYPE_FALSE;
        buf.offset += 5;
        return true;
    }
    if buf.can_read(4) && buf.at().starts_with(b"true") {
        item.type_flags = TYPE_TRUE;
        item.valueint = 1;
        buf.offset += 4;
        return true;
    }
    if buf.byte0() == Some(b'"') {
        return parse_string(item, buf);
    }
    if matches!(buf.byte0(), Some(b'-') | Some(b'0'..=b'9')) {
        return parse_number(item, buf);
    }
    if buf.byte0() == Some(b'[') {
        return parse_array(item, buf);
    }
    if buf.byte0() == Some(b'{') {
        return parse_object(item, buf);
    }
    false
}

fn parse_number(item: &mut Value, buf: &mut ParseBuffer<'_>) -> bool {
    if buf.content.is_empty() {
        return false;
    }
    let mut number_string_length = 0usize;
    let mut has_decimal_point = false;
    let mut i = 0usize;
    while buf.can_access(i) {
        match buf.content[buf.offset + i] {
            b'0'..=b'9' | b'+' | b'-' | b'e' | b'E' => number_string_length += 1,
            b'.' => {
                number_string_length += 1;
                has_decimal_point = true;
            }
            _ => break,
        }
        i += 1;
    }

    let slice = &buf.content[buf.offset..buf.offset + number_string_length];
    let tmp = String::from_utf8_lossy(slice);
    let _ = has_decimal_point; // locale decimal swap is a no-op under "C"/UTF-8 '.' locales

    let Some((number, consumed)) = cjson_cfmt::strtod(&tmp) else {
        return false;
    };
    if consumed == 0 {
        return false;
    }

    item.valuedouble = number;
    item.valueint = saturate_int(number);
    item.type_flags = TYPE_NUMBER;
    buf.offset += consumed;
    true
}

pub fn parse_hex4(input: &[u8]) -> u32 {
    if input.len() < 4 {
        return 0;
    }
    let mut h = 0u32;
    for (i, &c) in input.iter().take(4).enumerate() {
        h += match c {
            b'0'..=b'9' => u32::from(c - b'0'),
            b'A'..=b'F' => 10 + u32::from(c - b'A'),
            b'a'..=b'f' => 10 + u32::from(c - b'a'),
            _ => return 0,
        };
        if i < 3 {
            h <<= 4;
        }
    }
    h
}

fn utf16_literal_to_utf8(input: &[u8]) -> Option<(Vec<u8>, usize)> {
    if input.len() < 6 || input[0] != b'\\' || input[1] != b'u' {
        return None;
    }
    let first = parse_hex4(&input[2..6]);
    // parse_hex4 returns 0 for invalid — but 0000 is also valid. Check digits manually.
    for &c in &input[2..6] {
        if !c.is_ascii_hexdigit() {
            return None;
        }
    }
    let mut sequence_length = 6usize;
    let mut codepoint: u32 = first;

    if (0xDC00..=0xDFFF).contains(&first) {
        return None;
    }
    if (0xD800..=0xDBFF).contains(&first) {
        if input.len() < 12 || input[6] != b'\\' || input[7] != b'u' {
            return None;
        }
        for &c in &input[8..12] {
            if !c.is_ascii_hexdigit() {
                return None;
            }
        }
        let second = parse_hex4(&input[8..12]);
        if !(0xDC00..=0xDFFF).contains(&second) {
            return None;
        }
        codepoint = 0x10000 + (((first & 0x3FF) << 10) | (second & 0x3FF));
        sequence_length = 12;
    }

    let mut out = Vec::new();
    if codepoint < 0x80 {
        out.push(codepoint as u8);
    } else if codepoint < 0x800 {
        out.push((codepoint >> 6) as u8 | 0xc0);
        out.push((codepoint & 0x3f) as u8 | 0x80);
    } else if codepoint < 0x10000 {
        out.push((codepoint >> 12) as u8 | 0xe0);
        out.push(((codepoint >> 6) & 0x3f) as u8 | 0x80);
        out.push((codepoint & 0x3f) as u8 | 0x80);
    } else {
        out.push((codepoint >> 18) as u8 | 0xf0);
        out.push(((codepoint >> 12) & 0x3f) as u8 | 0x80);
        out.push(((codepoint >> 6) & 0x3f) as u8 | 0x80);
        out.push((codepoint & 0x3f) as u8 | 0x80);
    }
    Some((out, sequence_length))
}

fn parse_string(item: &mut Value, buf: &mut ParseBuffer<'_>) -> bool {
    if buf.byte0() != Some(b'"') {
        return false;
    }
    let mut input_end = buf.offset + 1;
    let mut skipped_bytes = 0usize;
    while input_end < buf.content.len() && buf.content[input_end] != b'"' {
        if buf.content[input_end] == b'\\' {
            if input_end + 1 >= buf.content.len() {
                buf.offset = input_end;
                return false;
            }
            skipped_bytes += 1;
            input_end += 1;
        }
        input_end += 1;
    }
    if input_end >= buf.content.len() || buf.content[input_end] != b'"' {
        buf.offset += 1;
        return false;
    }

    let mut output: Vec<u8> = Vec::new();
    let mut input_pointer = buf.offset + 1;
    while input_pointer < input_end {
        if buf.content[input_pointer] != b'\\' {
            output.push(buf.content[input_pointer]);
            input_pointer += 1;
        } else {
            if input_end - input_pointer < 1 {
                buf.offset = input_pointer;
                return false;
            }
            match buf.content[input_pointer + 1] {
                b'b' => {
                    output.push(0x08);
                    input_pointer += 2;
                }
                b'f' => {
                    output.push(0x0c);
                    input_pointer += 2;
                }
                b'n' => {
                    output.push(b'\n');
                    input_pointer += 2;
                }
                b'r' => {
                    output.push(b'\r');
                    input_pointer += 2;
                }
                b't' => {
                    output.push(b'\t');
                    input_pointer += 2;
                }
                b'"' | b'\\' | b'/' => {
                    output.push(buf.content[input_pointer + 1]);
                    input_pointer += 2;
                }
                b'u' => match utf16_literal_to_utf8(&buf.content[input_pointer..input_end]) {
                    Some((bytes, seq_len)) => {
                        output.extend_from_slice(&bytes);
                        input_pointer += seq_len;
                    }
                    None => {
                        buf.offset = input_pointer;
                        return false;
                    }
                },
                _ => {
                    buf.offset = input_pointer;
                    return false;
                }
            }
        }
    }

    let _ = skipped_bytes;
    item.type_flags = TYPE_STRING;
    item.valuestring = Some(String::from_utf8_lossy(&output).into_owned());
    buf.offset = input_end + 1;
    true
}

fn parse_array(item: &mut Value, buf: &mut ParseBuffer<'_>) -> bool {
    if buf.depth >= NESTING_LIMIT {
        return false;
    }
    buf.depth += 1;
    if buf.byte0() != Some(b'[') {
        buf.depth -= 1;
        return false;
    }
    buf.offset += 1;
    buf.skip_whitespace();
    if buf.byte0() == Some(b']') {
        item.type_flags = TYPE_ARRAY;
        buf.offset += 1;
        buf.depth -= 1;
        return true;
    }
    if !buf.can_access(0) {
        buf.offset = buf.offset.saturating_sub(1);
        buf.depth -= 1;
        return false;
    }
    buf.offset = buf.offset.saturating_sub(1);

    let mut children = Vec::new();
    loop {
        buf.offset += 1;
        buf.skip_whitespace();
        let mut child = Value::default();
        if !parse_value(&mut child, buf) {
            buf.depth -= 1;
            return false;
        }
        children.push(child);
        buf.skip_whitespace();
        if buf.byte0() != Some(b',') {
            break;
        }
    }
    if buf.byte0() != Some(b']') {
        buf.depth -= 1;
        return false;
    }
    item.type_flags = TYPE_ARRAY;
    item.children = children;
    buf.offset += 1;
    buf.depth -= 1;
    true
}

fn parse_object(item: &mut Value, buf: &mut ParseBuffer<'_>) -> bool {
    if buf.depth >= NESTING_LIMIT {
        return false;
    }
    buf.depth += 1;
    if buf.byte0() != Some(b'{') {
        buf.depth -= 1;
        return false;
    }
    buf.offset += 1;
    buf.skip_whitespace();
    if buf.byte0() == Some(b'}') {
        item.type_flags = TYPE_OBJECT;
        buf.offset += 1;
        buf.depth -= 1;
        return true;
    }
    if !buf.can_access(0) {
        buf.offset = buf.offset.saturating_sub(1);
        buf.depth -= 1;
        return false;
    }
    buf.offset = buf.offset.saturating_sub(1);

    let mut children = Vec::new();
    loop {
        if !buf.can_access(1) {
            buf.depth -= 1;
            return false;
        }
        buf.offset += 1;
        buf.skip_whitespace();
        let mut child = Value::default();
        if !parse_string(&mut child, buf) {
            buf.depth -= 1;
            return false;
        }
        buf.skip_whitespace();
        // swap valuestring -> key
        child.key = child.valuestring.take();
        if buf.byte0() != Some(b':') {
            buf.depth -= 1;
            return false;
        }
        buf.offset += 1;
        buf.skip_whitespace();
        if !parse_value(&mut child, buf) {
            buf.depth -= 1;
            return false;
        }
        children.push(child);
        buf.skip_whitespace();
        if buf.byte0() != Some(b',') {
            break;
        }
    }
    if buf.byte0() != Some(b'}') {
        buf.depth -= 1;
        return false;
    }
    item.type_flags = TYPE_OBJECT;
    item.children = children;
    buf.offset += 1;
    buf.depth -= 1;
    true
}

/// Exposed for white-box / internals tests (buffer helpers).
pub mod internals {
    use super::*;

    pub fn parse_number_raw(input: &str) -> Option<(f64, i32)> {
        let mut item = Value::default();
        let mut buf = ParseBuffer::new(input.as_bytes());
        if parse_number(&mut item, &mut buf) {
            Some((item.valuedouble, item.valueint))
        } else {
            None
        }
    }

    pub fn parse_hex4_raw(input: &str) -> u32 {
        parse_hex4(input.as_bytes())
    }

    pub fn parse_string_raw(input: &str) -> Option<String> {
        let mut item = Value::default();
        let mut buf = ParseBuffer::new(input.as_bytes());
        if parse_string(&mut item, &mut buf) {
            item.valuestring
        } else {
            None
        }
    }

    pub fn parse_value_raw(input: &str) -> Option<Value> {
        let mut item = Value::default();
        let mut buf = ParseBuffer::new(input.as_bytes());
        if parse_value(&mut item, &mut buf) {
            Some(item)
        } else {
            None
        }
    }

    pub fn skip_utf8_bom_offset(input: &[u8], start_offset: usize) -> Option<usize> {
        let mut buf = ParseBuffer {
            content: input,
            offset: start_offset,
            depth: 0,
        };
        if buf.skip_utf8_bom() {
            Some(buf.offset)
        } else {
            None
        }
    }
}
