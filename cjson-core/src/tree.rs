//! Tree mutation, duplicate, compare, and helpers.

use crate::error::{Error, Result};
use crate::value::{
    case_insensitive_eq, compare_double, saturate_int, Value, CIRCULAR_LIMIT, TYPE_ARRAY,
    TYPE_FALSE, TYPE_IS_REFERENCE, TYPE_NULL, TYPE_NUMBER, TYPE_OBJECT, TYPE_RAW, TYPE_STRING,
    TYPE_STRING_IS_CONST, TYPE_TRUE,
};

pub fn set_number(item: &mut Value, number: f64) -> f64 {
    item.valueint = saturate_int(number);
    item.valuedouble = number;
    number
}

pub fn set_valuestring(item: &mut Value, valuestring: &str) -> Result<()> {
    if item.base_type() != TYPE_STRING || (item.type_flags & TYPE_IS_REFERENCE) != 0 {
        return Err(Error::InvalidArgument);
    }
    let Some(existing) = item.valuestring.as_mut() else {
        return Err(Error::InvalidArgument);
    };
    if valuestring.len() <= existing.len() {
        // Overlap check: in Rust owned strings, aliasing is not possible the C way.
        existing.clear();
        existing.push_str(valuestring);
        return Ok(());
    }
    item.valuestring = Some(valuestring.to_owned());
    Ok(())
}

pub fn duplicate(item: &Value, recurse: bool) -> Result<Value> {
    duplicate_rec(item, 0, recurse)
}

fn duplicate_rec(item: &Value, depth: usize, recurse: bool) -> Result<Value> {
    let mut newitem = Value {
        type_flags: item.type_flags & !TYPE_IS_REFERENCE,
        valuestring: item.valuestring.clone(),
        valuestring_is_ref: false,
        valueint: item.valueint,
        valuedouble: item.valuedouble,
        key: item.key.clone(),
        key_is_const: item.key_is_const,
        children: Vec::new(),
    };
    if item.key_is_const {
        newitem.type_flags |= TYPE_STRING_IS_CONST;
    }
    if !recurse {
        return Ok(newitem);
    }
    for child in &item.children {
        if depth >= CIRCULAR_LIMIT {
            return Err(Error::CircularTooDeep);
        }
        newitem
            .children
            .push(duplicate_rec(child, depth + 1, true)?);
    }
    Ok(newitem)
}

pub fn compare(a: &Value, b: &Value, case_sensitive: bool) -> bool {
    if (a.type_flags & 0xff) != (b.type_flags & 0xff) {
        return false;
    }
    match a.base_type() {
        TYPE_FALSE | TYPE_TRUE | TYPE_NULL | TYPE_NUMBER | TYPE_STRING | TYPE_RAW | TYPE_ARRAY
        | TYPE_OBJECT => {}
        _ => return false,
    }
    // Identical objects: in C, pointer equality. Owned trees compare by value.
    match a.base_type() {
        TYPE_FALSE | TYPE_TRUE | TYPE_NULL => true,
        TYPE_NUMBER => compare_double(a.valuedouble, b.valuedouble),
        TYPE_STRING | TYPE_RAW => match (&a.valuestring, &b.valuestring) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
        TYPE_ARRAY => {
            if a.children.len() != b.children.len() {
                return false;
            }
            a.children
                .iter()
                .zip(b.children.iter())
                .all(|(x, y)| compare(x, y, case_sensitive))
        }
        TYPE_OBJECT => compare_objects(a, b, case_sensitive),
        _ => false,
    }
}

fn compare_objects(a: &Value, b: &Value, case_sensitive: bool) -> bool {
    // cJSON: each key in a must exist in b with equal value, and vice versa.
    for child in &a.children {
        let Some(key) = child.key.as_deref() else {
            return false;
        };
        let Some(other) = b.get_object_item(key, case_sensitive) else {
            return false;
        };
        if !compare(child, other, case_sensitive) {
            return false;
        }
    }
    for child in &b.children {
        let Some(key) = child.key.as_deref() else {
            return false;
        };
        if a.get_object_item(key, case_sensitive).is_none() {
            return false;
        }
    }
    true
}

pub fn estimate_compare_cost(item: &Value) -> u64 {
    const LIMIT: u64 = 1_000_000;
    let mut cost = 1u64;
    if matches!(item.base_type(), TYPE_ARRAY | TYPE_OBJECT) {
        for child in &item.children {
            let child_cost = estimate_compare_cost(child);
            if item.base_type() == TYPE_OBJECT {
                if child_cost > LIMIT {
                    return LIMIT + 1;
                }
                cost = cost.saturating_add(child_cost.saturating_mul(2));
            } else {
                cost = cost.saturating_add(child_cost);
            }
            if cost > LIMIT {
                return LIMIT + 1;
            }
        }
    }
    cost
}

pub fn detach_item_from_array(array: &mut Value, which: i32) -> Option<Value> {
    if array.base_type() != TYPE_ARRAY || which < 0 {
        return None;
    }
    let idx = which as usize;
    if idx >= array.children.len() {
        return None;
    }
    Some(array.children.remove(idx))
}

pub fn detach_item_from_object(
    object: &mut Value,
    name: &str,
    case_sensitive: bool,
) -> Option<Value> {
    if object.base_type() != TYPE_OBJECT {
        return None;
    }
    let pos = object.children.iter().position(|c| match &c.key {
        Some(k) if case_sensitive => k == name,
        Some(k) => case_insensitive_eq(k, name),
        None => false,
    })?;
    Some(object.children.remove(pos))
}

pub fn replace_item_in_array(array: &mut Value, which: i32, newitem: Value) -> Result<()> {
    if array.base_type() != TYPE_ARRAY || which < 0 {
        return Err(Error::InvalidArgument);
    }
    let idx = which as usize;
    if idx >= array.children.len() {
        return Err(Error::InvalidArgument);
    }
    array.children[idx] = newitem;
    Ok(())
}

pub fn replace_item_in_object(
    object: &mut Value,
    name: &str,
    mut newitem: Value,
    case_sensitive: bool,
) -> Result<()> {
    if object.base_type() != TYPE_OBJECT {
        return Err(Error::InvalidArgument);
    }
    let pos = object
        .children
        .iter()
        .position(|c| match &c.key {
            Some(k) if case_sensitive => k == name,
            Some(k) => case_insensitive_eq(k, name),
            None => false,
        })
        .ok_or(Error::InvalidArgument)?;
    newitem.key = Some(name.to_owned());
    object.children[pos] = newitem;
    Ok(())
}

pub fn insert_item_in_array(array: &mut Value, which: i32, newitem: Value) -> Result<()> {
    if array.base_type() != TYPE_ARRAY || which < 0 {
        return Err(Error::InvalidArgument);
    }
    let idx = which as usize;
    if idx > array.children.len() {
        return Err(Error::InvalidArgument);
    }
    array.children.insert(idx, newitem);
    Ok(())
}

pub fn create_string_reference(s: &str) -> Value {
    Value {
        type_flags: TYPE_STRING | TYPE_IS_REFERENCE,
        valuestring: Some(s.to_owned()),
        valuestring_is_ref: true,
        ..Value::default()
    }
}

pub fn create_array_reference(child: Value) -> Value {
    let mut v = Value {
        type_flags: TYPE_ARRAY | TYPE_IS_REFERENCE,
        ..Value::default()
    };
    v.children.push(child);
    v
}

pub fn create_object_reference(child: Value) -> Value {
    let mut v = Value {
        type_flags: TYPE_OBJECT | TYPE_IS_REFERENCE,
        ..Value::default()
    };
    v.children.push(child);
    v
}
