//! Owned JSON value tree. FFI reconstitutes the C linked-list layout.

use crate::error::{Error, Result};

pub const TYPE_INVALID: i32 = 0;
pub const TYPE_FALSE: i32 = 1 << 0;
pub const TYPE_TRUE: i32 = 1 << 1;
pub const TYPE_NULL: i32 = 1 << 2;
pub const TYPE_NUMBER: i32 = 1 << 3;
pub const TYPE_STRING: i32 = 1 << 4;
pub const TYPE_ARRAY: i32 = 1 << 5;
pub const TYPE_OBJECT: i32 = 1 << 6;
pub const TYPE_RAW: i32 = 1 << 7;
pub const TYPE_IS_REFERENCE: i32 = 256;
pub const TYPE_STRING_IS_CONST: i32 = 512;

pub const NESTING_LIMIT: usize = 1000;
pub const CIRCULAR_LIMIT: usize = 10000;

#[derive(Clone, Debug)]
pub struct Value {
    pub type_flags: i32,
    pub valuestring: Option<String>,
    /// When `TYPE_IS_REFERENCE` is set on a string, the valuestring is borrowed
    /// and must not be freed by Delete — modeled as `valuestring_is_ref`.
    pub valuestring_is_ref: bool,
    pub valueint: i32,
    pub valuedouble: f64,
    pub key: Option<String>,
    pub key_is_const: bool,
    pub children: Vec<Value>,
}

impl Default for Value {
    fn default() -> Self {
        Self {
            type_flags: TYPE_INVALID,
            valuestring: None,
            valuestring_is_ref: false,
            valueint: 0,
            valuedouble: 0.0,
            key: None,
            key_is_const: false,
            children: Vec::new(),
        }
    }
}

impl Value {
    pub fn new_null() -> Self {
        Self {
            type_flags: TYPE_NULL,
            ..Self::default()
        }
    }

    pub fn new_bool(b: bool) -> Self {
        if b {
            Self {
                type_flags: TYPE_TRUE,
                valueint: 1,
                ..Self::default()
            }
        } else {
            Self {
                type_flags: TYPE_FALSE,
                ..Self::default()
            }
        }
    }

    pub fn new_number(num: f64) -> Self {
        let mut v = Self {
            type_flags: TYPE_NUMBER,
            valuedouble: num,
            ..Self::default()
        };
        v.valueint = saturate_int(num);
        v
    }

    pub fn new_string(s: &str) -> Self {
        Self {
            type_flags: TYPE_STRING,
            valuestring: Some(s.to_owned()),
            ..Self::default()
        }
    }

    pub fn new_raw(s: &str) -> Self {
        Self {
            type_flags: TYPE_RAW,
            valuestring: Some(s.to_owned()),
            ..Self::default()
        }
    }

    pub fn new_array() -> Self {
        Self {
            type_flags: TYPE_ARRAY,
            ..Self::default()
        }
    }

    pub fn new_object() -> Self {
        Self {
            type_flags: TYPE_OBJECT,
            ..Self::default()
        }
    }

    pub fn base_type(&self) -> i32 {
        self.type_flags & 0xff
    }

    pub fn is_bool(&self) -> bool {
        (self.type_flags & (TYPE_TRUE | TYPE_FALSE)) != 0
    }

    pub fn array_size(&self) -> i32 {
        let n = self.children.len();
        if n > i32::MAX as usize {
            // Match C GetArraySize potential overflow quirks conservatively.
            i32::MAX
        } else {
            n as i32
        }
    }

    pub fn get_array_item(&self, index: i32) -> Option<&Value> {
        if index < 0 {
            return None;
        }
        self.children.get(index as usize)
    }

    pub fn get_array_item_mut(&mut self, index: i32) -> Option<&mut Value> {
        if index < 0 {
            return None;
        }
        self.children.get_mut(index as usize)
    }

    pub fn get_object_item(&self, name: &str, case_sensitive: bool) -> Option<&Value> {
        self.children.iter().find(|c| match &c.key {
            Some(k) if case_sensitive => k == name,
            Some(k) => case_insensitive_eq(k, name),
            None => false,
        })
    }

    pub fn get_object_item_mut(&mut self, name: &str, case_sensitive: bool) -> Option<&mut Value> {
        self.children.iter_mut().find(|c| match &c.key {
            Some(k) if case_sensitive => k == name,
            Some(k) => case_insensitive_eq(k, name),
            None => false,
        })
    }

    pub fn add_item(&mut self, mut item: Value) -> Result<()> {
        let base = self.base_type();
        if base != TYPE_ARRAY && base != TYPE_OBJECT {
            return Err(Error::InvalidArgument);
        }
        // cJSON rejects adding an item to itself.
        // Pointer identity is not available for owned values; skip self-check here.
        // FFI layer can still enforce pointer identity for C callers.
        let _ = &mut item;
        self.children.push(item);
        Ok(())
    }

    pub fn add_item_to_object(
        &mut self,
        key: &str,
        mut item: Value,
        constant_key: bool,
    ) -> Result<()> {
        if self.base_type() != TYPE_OBJECT {
            return Err(Error::InvalidArgument);
        }
        item.key = Some(key.to_owned());
        item.key_is_const = constant_key;
        if constant_key {
            item.type_flags |= TYPE_STRING_IS_CONST;
        }
        self.children.push(item);
        Ok(())
    }
}

pub fn saturate_int(number: f64) -> i32 {
    if number >= f64::from(i32::MAX) {
        i32::MAX
    } else if number <= f64::from(i32::MIN) {
        i32::MIN
    } else {
        number as i32
    }
}

pub fn case_insensitive_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        // C's case_insensitive_strcmp compares until mismatch; unequal lengths can still
        // compare equal only if one is a prefix — but object lookup uses full-key equality
        // via walking until tolower differs including the NUL. So lengths must match.
    }
    let mut i = 0;
    loop {
        let ca = ab.get(i).copied().unwrap_or(0);
        let cb = bb.get(i).copied().unwrap_or(0);
        let ta = (ca as char).to_ascii_lowercase() as u8;
        let tb = (cb as char).to_ascii_lowercase() as u8;
        if ta != tb {
            return false;
        }
        if ca == 0 {
            return true;
        }
        i += 1;
    }
}

pub fn compare_double(a: f64, b: f64) -> bool {
    let max_val = a.abs().max(b.abs());
    (a - b).abs() <= max_val * f64::EPSILON
}
