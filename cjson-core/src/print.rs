//! Printer matching cJSON compact/pretty output (tabs, number formats).

use crate::error::{Error, Result};
use crate::value::{
    compare_double, Value, NESTING_LIMIT, TYPE_ARRAY, TYPE_FALSE, TYPE_NULL, TYPE_NUMBER,
    TYPE_OBJECT, TYPE_RAW, TYPE_STRING, TYPE_TRUE,
};

pub fn print(item: &Value, format: bool) -> Result<String> {
    let mut out = String::with_capacity(256);
    let mut depth = 0usize;
    print_value(item, format, &mut depth, &mut out)?;
    Ok(out)
}

pub fn print_unformatted(item: &Value) -> Result<String> {
    print(item, false)
}

pub fn print_formatted(item: &Value) -> Result<String> {
    print(item, true)
}

fn print_value(item: &Value, format: bool, depth: &mut usize, out: &mut String) -> Result<()> {
    match item.base_type() {
        TYPE_NULL => {
            out.push_str("null");
            Ok(())
        }
        TYPE_FALSE => {
            out.push_str("false");
            Ok(())
        }
        TYPE_TRUE => {
            out.push_str("true");
            Ok(())
        }
        TYPE_NUMBER => {
            out.push_str(&print_number(item));
            Ok(())
        }
        TYPE_RAW => {
            let Some(s) = item.valuestring.as_deref() else {
                return Err(Error::Print);
            };
            out.push_str(s);
            Ok(())
        }
        TYPE_STRING => {
            print_string_ptr(item.valuestring.as_deref(), out);
            Ok(())
        }
        TYPE_ARRAY => print_array(item, format, depth, out),
        TYPE_OBJECT => print_object(item, format, depth, out),
        _ => Err(Error::Print),
    }
}

/// Match cJSON `print_number`: null for NaN/Inf; `%d` when valuedouble == (double)valueint;
/// else `%1.15g` with fallback to `%1.17g` when round-trip fails.
pub fn print_number(item: &Value) -> String {
    let d = item.valuedouble;
    if d.is_nan() || d.is_infinite() {
        return "null".to_owned();
    }
    if d == f64::from(item.valueint) {
        return format!("{}", item.valueint);
    }
    // Use libc snprintf via the cjson-cfmt boundary so output matches glibc
    // exactly (cJSON.c print_number).
    let s15 = cjson_cfmt::sprintf_g(d, 15);
    if let Some(test) = cjson_cfmt::sscanf_lg(&s15) {
        if compare_double(test, d) {
            return s15;
        }
    }
    cjson_cfmt::sprintf_g(d, 17)
}

pub fn print_string_ptr(input: Option<&str>, out: &mut String) {
    let Some(input) = input else {
        out.push_str("\"\"");
        return;
    };
    let bytes = input.as_bytes();
    let mut escape_characters = 0usize;
    for &c in bytes {
        match c {
            b'"' | b'\\' | 0x08 | 0x0c | b'\n' | b'\r' | b'\t' => escape_characters += 1,
            c if c < 32 => escape_characters += 5,
            _ => {}
        }
    }
    out.push('"');
    if escape_characters == 0 {
        out.push_str(input);
        out.push('"');
        return;
    }
    for &c in bytes {
        if c > 31 && c != b'"' && c != b'\\' {
            out.push(c as char);
        } else {
            out.push('\\');
            match c {
                b'\\' => out.push('\\'),
                b'"' => out.push('"'),
                0x08 => out.push('b'),
                0x0c => out.push('f'),
                b'\n' => out.push('n'),
                b'\r' => out.push('r'),
                b'\t' => out.push('t'),
                _ => {
                    out.push('u');
                    out.push_str(&format!("{c:04x}"));
                }
            }
        }
    }
    out.push('"');
}

fn print_array(item: &Value, format: bool, depth: &mut usize, out: &mut String) -> Result<()> {
    if *depth >= NESTING_LIMIT {
        return Err(Error::NestingTooDeep);
    }
    out.push('[');
    *depth += 1;
    for (i, child) in item.children.iter().enumerate() {
        print_value(child, format, depth, out)?;
        if i + 1 < item.children.len() {
            out.push(',');
            if format {
                out.push(' ');
            }
        }
    }
    out.push(']');
    *depth -= 1;
    Ok(())
}

fn print_object(item: &Value, format: bool, depth: &mut usize, out: &mut String) -> Result<()> {
    if *depth >= NESTING_LIMIT {
        return Err(Error::NestingTooDeep);
    }
    out.push('{');
    *depth += 1;
    if format {
        out.push('\n');
    }
    for (idx, child) in item.children.iter().enumerate() {
        if format {
            for _ in 0..*depth {
                out.push('\t');
            }
        }
        print_string_ptr(child.key.as_deref(), out);
        out.push(':');
        if format {
            out.push('\t');
        }
        print_value(child, format, depth, out)?;
        if idx + 1 < item.children.len() {
            out.push(',');
        }
        if format {
            out.push('\n');
        }
    }
    if format {
        for _ in 0..(*depth - 1) {
            out.push('\t');
        }
    }
    out.push('}');
    *depth -= 1;
    Ok(())
}

pub mod internals {
    use super::*;

    pub fn print_number_raw(valuedouble: f64, valueint: i32) -> String {
        let item = Value {
            type_flags: TYPE_NUMBER,
            valuedouble,
            valueint,
            ..Value::default()
        };
        print_number(&item)
    }

    pub fn print_string_raw(s: &str) -> String {
        let mut out = String::new();
        print_string_ptr(Some(s), &mut out);
        out
    }
}
