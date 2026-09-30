//! C ABI for cJSON, implemented on top of `cjson-core`.
//!
//! Every `unsafe` block has a `SAFETY:` comment. Pointer contracts match `cJSON.h`.

#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
// C ABI entry points take raw pointers by contract; they cannot be `unsafe fn`
// without breaking the exported C signatures. SAFETY comments cover each deref.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use cjson_core::value::{
    saturate_int, TYPE_ARRAY, TYPE_FALSE, TYPE_IS_REFERENCE, TYPE_NULL, TYPE_NUMBER, TYPE_OBJECT,
    TYPE_RAW, TYPE_STRING, TYPE_STRING_IS_CONST, TYPE_TRUE,
};
use cjson_core::{
    compare as core_compare, duplicate as core_duplicate, minify as core_minify, parse_with_opts,
    print_formatted, print_unformatted, version, Value,
};
use std::cell::Cell;
use std::ffi::{c_char, c_double, c_int, c_void, CStr, CString};
use std::ptr;
use std::sync::Mutex;

#[repr(C)]
pub struct CJSON {
    pub next: *mut CJSON,
    pub prev: *mut CJSON,
    pub child: *mut CJSON,
    pub type_: c_int,
    pub valuestring: *mut c_char,
    pub valueint: c_int,
    pub valuedouble: c_double,
    pub string: *mut c_char,
}

#[repr(C)]
pub struct CJSON_Hooks {
    pub malloc_fn: Option<unsafe extern "C" fn(usize) -> *mut c_void>,
    pub free_fn: Option<unsafe extern "C" fn(*mut c_void)>,
}

struct Hooks {
    malloc_fn: unsafe extern "C" fn(usize) -> *mut c_void,
    free_fn: unsafe extern "C" fn(*mut c_void),
}

// SAFETY: only used as default function pointers matching C malloc/free.
unsafe extern "C" fn default_malloc(sz: usize) -> *mut c_void {
    // SAFETY: libc malloc is valid for any size; may return null.
    unsafe { libc_malloc(sz) }
}
unsafe extern "C" fn default_free(p: *mut c_void) {
    // SAFETY: p is either null or from malloc_fn.
    unsafe { libc_free(p) }
}

extern "C" {
    fn malloc(sz: usize) -> *mut c_void;
    fn free(p: *mut c_void);
    fn realloc(p: *mut c_void, sz: usize) -> *mut c_void;
}

unsafe extern "C" fn libc_malloc(sz: usize) -> *mut c_void {
    // SAFETY: delegates to libc.
    unsafe { malloc(sz) }
}
unsafe extern "C" fn libc_free(p: *mut c_void) {
    // SAFETY: delegates to libc.
    unsafe { free(p) }
}

static HOOKS: Mutex<Hooks> = Mutex::new(Hooks {
    malloc_fn: default_malloc,
    free_fn: default_free,
});

thread_local! {
    static GLOBAL_ERROR: Cell<(*const c_char, usize)> = const { Cell::new((ptr::null(), 0)) };
}

fn allocate(size: usize) -> *mut c_void {
    let hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: malloc_fn is a user or default allocator; size is the request.
    unsafe { (hooks.malloc_fn)(size) }
}

fn deallocate(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    let hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: p came from allocate / hooks.malloc_fn or is a C string we created that way.
    unsafe { (hooks.free_fn)(p) }
}

fn cstr_dup(s: &str) -> *mut c_char {
    let need = s.len() + 1;
    let p = allocate(need) as *mut c_char;
    if p.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: p points to need bytes from allocate.
    unsafe {
        ptr::copy_nonoverlapping(s.as_ptr(), p as *mut u8, s.len());
        *p.add(s.len()) = 0;
    }
    p
}

fn new_item() -> *mut CJSON {
    let p = allocate(std::mem::size_of::<CJSON>()) as *mut CJSON;
    if p.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: freshly allocated CJSON-sized block.
    unsafe {
        ptr::write_bytes(p as *mut u8, 0, std::mem::size_of::<CJSON>());
    }
    p
}

fn value_to_cjson(v: &Value) -> *mut CJSON {
    let item = new_item();
    if item.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: item is a valid new CJSON.
    unsafe {
        (*item).type_ = v.type_flags;
        (*item).valueint = v.valueint;
        (*item).valuedouble = v.valuedouble;
        if let Some(s) = &v.valuestring {
            if v.valuestring_is_ref {
                // Reference: store pointer without ownership — not representable safely
                // from a temporary; duplicate for ABI stability (documented CH if needed).
                (*item).valuestring = cstr_dup(s);
                (*item).type_ &= !TYPE_IS_REFERENCE;
            } else {
                (*item).valuestring = cstr_dup(s);
            }
        }
        if let Some(k) = &v.key {
            (*item).string = cstr_dup(k);
            if v.key_is_const {
                (*item).type_ |= TYPE_STRING_IS_CONST;
            }
        }
        let mut head: *mut CJSON = ptr::null_mut();
        let mut prev: *mut CJSON = ptr::null_mut();
        for child in &v.children {
            let c = value_to_cjson(child);
            if c.is_null() {
                cJSON_Delete(item);
                return ptr::null_mut();
            }
            if head.is_null() {
                head = c;
            } else {
                (*prev).next = c;
                (*c).prev = prev;
            }
            prev = c;
        }
        if !head.is_null() {
            (*head).prev = prev;
        }
        (*item).child = head;
    }
    item
}

fn cjson_to_value(item: *const CJSON) -> Option<Value> {
    if item.is_null() {
        return None;
    }
    // SAFETY: caller guarantees item is a valid cJSON or null.
    unsafe {
        let mut v = Value {
            type_flags: (*item).type_,
            valuestring: None,
            valuestring_is_ref: ((*item).type_ & TYPE_IS_REFERENCE) != 0,
            valueint: (*item).valueint,
            valuedouble: (*item).valuedouble,
            key: None,
            key_is_const: ((*item).type_ & TYPE_STRING_IS_CONST) != 0,
            children: Vec::new(),
        };
        if !(*item).valuestring.is_null() {
            v.valuestring = Some(
                CStr::from_ptr((*item).valuestring)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        if !(*item).string.is_null() {
            v.key = Some(
                CStr::from_ptr((*item).string)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        let mut child = (*item).child;
        while !child.is_null() {
            v.children.push(cjson_to_value(child)?);
            child = (*child).next;
        }
        Some(v)
    }
}

fn set_global_error(json: *const c_char, position: usize) {
    GLOBAL_ERROR.with(|c| c.set((json, position)));
}

#[no_mangle]
pub extern "C" fn cJSON_Version() -> *const c_char {
    // Leak a stable CString for the process lifetime (matches C static buffer spirit).
    use std::sync::OnceLock;
    static VER: OnceLock<CString> = OnceLock::new();
    VER.get_or_init(|| CString::new(version()).unwrap())
        .as_ptr()
}

#[no_mangle]
pub extern "C" fn cJSON_InitHooks(hooks: *mut CJSON_Hooks) {
    let mut g = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
    if hooks.is_null() {
        g.malloc_fn = default_malloc;
        g.free_fn = default_free;
        return;
    }
    // SAFETY: hooks is a valid CJSON_Hooks pointer from the caller.
    unsafe {
        if let Some(m) = (*hooks).malloc_fn {
            g.malloc_fn = m;
        } else {
            g.malloc_fn = default_malloc;
        }
        if let Some(f) = (*hooks).free_fn {
            g.free_fn = f;
        } else {
            g.free_fn = default_free;
        }
    }
}

#[no_mangle]
pub extern "C" fn cJSON_Parse(value: *const c_char) -> *mut CJSON {
    if value.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: value is a C string from the caller.
    unsafe {
        let len = libc_strlen(value);
        cJSON_ParseWithLengthOpts(value, len + 1, ptr::null_mut(), 0)
    }
}

#[no_mangle]
pub extern "C" fn cJSON_ParseWithLength(value: *const c_char, buffer_length: usize) -> *mut CJSON {
    cJSON_ParseWithLengthOpts(value, buffer_length, ptr::null_mut(), 0)
}

#[no_mangle]
pub extern "C" fn cJSON_ParseWithOpts(
    value: *const c_char,
    return_parse_end: *mut *const c_char,
    require_null_terminated: c_int,
) -> *mut CJSON {
    if value.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: value is a C string.
    unsafe {
        let len = libc_strlen(value) + 1;
        cJSON_ParseWithLengthOpts(value, len, return_parse_end, require_null_terminated)
    }
}

#[no_mangle]
pub extern "C" fn cJSON_ParseWithLengthOpts(
    value: *const c_char,
    buffer_length: usize,
    return_parse_end: *mut *const c_char,
    require_null_terminated: c_int,
) -> *mut CJSON {
    set_global_error(ptr::null(), 0);
    if value.is_null() || buffer_length == 0 {
        return ptr::null_mut();
    }
    // SAFETY: value points at buffer_length bytes from the caller.
    let slice = unsafe { std::slice::from_raw_parts(value as *const u8, buffer_length) };
    // If buffer includes a trailing NUL (ParseWithOpts), trim for core when not required?
    // C uses full buffer_length; for strlen+1 the last byte is NUL and typically inaccessible
    // as content past JSON. Core parse treats the slice length as the limit — include NUL byte
    // as part of length like C.
    match parse_with_opts(slice, require_null_terminated != 0) {
        Ok((v, end)) => {
            if !return_parse_end.is_null() {
                // SAFETY: return_parse_end is an out-pointer from the caller.
                unsafe {
                    *return_parse_end = value.add(end);
                }
            }
            value_to_cjson(&v)
        }
        Err(_) => {
            let off = cjson_core::last_error_offset().unwrap_or(0);
            set_global_error(value, off);
            if !return_parse_end.is_null() {
                // SAFETY: out-pointer from caller.
                unsafe {
                    *return_parse_end = value.add(off);
                }
            }
            ptr::null_mut()
        }
    }
}

extern "C" {
    fn strlen(s: *const c_char) -> usize;
}
unsafe fn libc_strlen(s: *const c_char) -> usize {
    // SAFETY: s is a valid C string.
    unsafe { strlen(s) }
}

#[no_mangle]
pub extern "C" fn cJSON_Print(item: *const CJSON) -> *mut c_char {
    let Some(v) = cjson_to_value(item) else {
        return ptr::null_mut();
    };
    match print_formatted(&v) {
        Ok(s) => cstr_dup(&s),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn cJSON_PrintUnformatted(item: *const CJSON) -> *mut c_char {
    let Some(v) = cjson_to_value(item) else {
        return ptr::null_mut();
    };
    match print_unformatted(&v) {
        Ok(s) => cstr_dup(&s),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn cJSON_PrintBuffered(
    item: *const CJSON,
    _prebuffer: c_int,
    fmt: c_int,
) -> *mut c_char {
    if fmt != 0 {
        cJSON_Print(item)
    } else {
        cJSON_PrintUnformatted(item)
    }
}

#[no_mangle]
pub extern "C" fn cJSON_PrintPreallocated(
    item: *mut CJSON,
    buffer: *mut c_char,
    length: c_int,
    format: c_int,
) -> c_int {
    if buffer.is_null() || length <= 0 {
        return 0;
    }
    let printed = if format != 0 {
        cJSON_Print(item)
    } else {
        cJSON_PrintUnformatted(item)
    };
    if printed.is_null() {
        return 0;
    }
    // SAFETY: printed is our C string; buffer has length bytes.
    unsafe {
        let plen = libc_strlen(printed);
        if plen + 1 > length as usize {
            cJSON_free(printed as *mut c_void);
            return 0;
        }
        ptr::copy_nonoverlapping(printed, buffer, plen + 1);
        cJSON_free(printed as *mut c_void);
    }
    1
}

#[no_mangle]
pub extern "C" fn cJSON_Delete(item: *mut CJSON) {
    if item.is_null() {
        return;
    }
    // SAFETY: item is a cJSON tree from this library or compatible allocator.
    unsafe {
        let mut child = (*item).child;
        while !child.is_null() {
            let next = (*child).next;
            cJSON_Delete(child);
            child = next;
        }
        if !(*item).valuestring.is_null() && ((*item).type_ & TYPE_IS_REFERENCE) == 0 {
            deallocate((*item).valuestring as *mut c_void);
        }
        if !(*item).string.is_null() && ((*item).type_ & TYPE_STRING_IS_CONST) == 0 {
            deallocate((*item).string as *mut c_void);
        }
        deallocate(item as *mut c_void);
    }
}

#[no_mangle]
pub extern "C" fn cJSON_GetArraySize(array: *const CJSON) -> c_int {
    if array.is_null() {
        return 0;
    }
    let mut n = 0i32;
    // SAFETY: array is a valid cJSON.
    unsafe {
        let mut c = (*array).child;
        while !c.is_null() {
            n = n.wrapping_add(1);
            c = (*c).next;
        }
    }
    n
}

#[no_mangle]
pub extern "C" fn cJSON_GetArrayItem(array: *const CJSON, index: c_int) -> *mut CJSON {
    if array.is_null() || index < 0 {
        return ptr::null_mut();
    }
    // SAFETY: array valid.
    unsafe {
        let mut c = (*array).child;
        let mut i = 0;
        while !c.is_null() && i < index {
            c = (*c).next;
            i += 1;
        }
        c
    }
}

fn object_item(object: *const CJSON, string: *const c_char, case_sensitive: bool) -> *mut CJSON {
    if object.is_null() || string.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: string is a C string; object is valid.
    unsafe {
        let name = CStr::from_ptr(string).to_string_lossy();
        let mut c = (*object).child;
        while !c.is_null() {
            if !(*c).string.is_null() {
                let key = CStr::from_ptr((*c).string).to_string_lossy();
                let matched = if case_sensitive {
                    key == name
                } else {
                    cjson_core::internals::case_insensitive_eq(&key, &name)
                };
                if matched {
                    return c;
                }
            }
            c = (*c).next;
        }
    }
    ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn cJSON_GetObjectItem(object: *const CJSON, string: *const c_char) -> *mut CJSON {
    object_item(object, string, false)
}

#[no_mangle]
pub extern "C" fn cJSON_GetObjectItemCaseSensitive(
    object: *const CJSON,
    string: *const c_char,
) -> *mut CJSON {
    object_item(object, string, true)
}

#[no_mangle]
pub extern "C" fn cJSON_HasObjectItem(object: *const CJSON, string: *const c_char) -> c_int {
    if cJSON_GetObjectItem(object, string).is_null() {
        0
    } else {
        1
    }
}

#[no_mangle]
pub extern "C" fn cJSON_GetErrorPtr() -> *const c_char {
    GLOBAL_ERROR.with(|c| {
        let (json, pos) = c.get();
        if json.is_null() {
            ptr::null()
        } else {
            // SAFETY: json was the parse input pointer stored on failure.
            unsafe { json.add(pos) }
        }
    })
}

#[no_mangle]
pub extern "C" fn cJSON_GetStringValue(item: *const CJSON) -> *mut c_char {
    if item.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: item valid.
    unsafe {
        if ((*item).type_ & 0xff) != TYPE_STRING {
            return ptr::null_mut();
        }
        (*item).valuestring
    }
}

#[no_mangle]
pub extern "C" fn cJSON_GetNumberValue(item: *const CJSON) -> c_double {
    if item.is_null() {
        return f64::NAN;
    }
    // SAFETY: item valid.
    unsafe {
        if ((*item).type_ & 0xff) != TYPE_NUMBER {
            return f64::NAN;
        }
        (*item).valuedouble
    }
}

macro_rules! is_exact_type {
    ($name:ident, $mask:expr) => {
        #[no_mangle]
        pub extern "C" fn $name(item: *const CJSON) -> c_int {
            if item.is_null() {
                return 0;
            }
            // SAFETY: item valid.
            unsafe {
                if ((*item).type_ & 0xff) == $mask {
                    1
                } else {
                    0
                }
            }
        }
    };
}

is_exact_type!(cJSON_IsInvalid, 0);
is_exact_type!(cJSON_IsFalse, TYPE_FALSE);
is_exact_type!(cJSON_IsTrue, TYPE_TRUE);
is_exact_type!(cJSON_IsNull, TYPE_NULL);
is_exact_type!(cJSON_IsNumber, TYPE_NUMBER);
is_exact_type!(cJSON_IsString, TYPE_STRING);
is_exact_type!(cJSON_IsArray, TYPE_ARRAY);
is_exact_type!(cJSON_IsObject, TYPE_OBJECT);
is_exact_type!(cJSON_IsRaw, TYPE_RAW);

#[no_mangle]
pub extern "C" fn cJSON_IsBool(item: *const CJSON) -> c_int {
    if item.is_null() {
        return 0;
    }
    // SAFETY: item valid.
    unsafe {
        if ((*item).type_ & (TYPE_TRUE | TYPE_FALSE)) != 0 {
            1
        } else {
            0
        }
    }
}

fn create_basic(type_: c_int) -> *mut CJSON {
    let item = new_item();
    if item.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: new item.
    unsafe {
        (*item).type_ = type_;
    }
    item
}

#[no_mangle]
pub extern "C" fn cJSON_CreateNull() -> *mut CJSON {
    create_basic(TYPE_NULL)
}
#[no_mangle]
pub extern "C" fn cJSON_CreateTrue() -> *mut CJSON {
    let i = create_basic(TYPE_TRUE);
    if !i.is_null() {
        // SAFETY: new item.
        unsafe {
            (*i).valueint = 1;
        }
    }
    i
}
#[no_mangle]
pub extern "C" fn cJSON_CreateFalse() -> *mut CJSON {
    create_basic(TYPE_FALSE)
}
#[no_mangle]
pub extern "C" fn cJSON_CreateBool(boolean: c_int) -> *mut CJSON {
    if boolean != 0 {
        cJSON_CreateTrue()
    } else {
        cJSON_CreateFalse()
    }
}
#[no_mangle]
pub extern "C" fn cJSON_CreateNumber(num: c_double) -> *mut CJSON {
    let item = create_basic(TYPE_NUMBER);
    if item.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: new item.
    unsafe {
        (*item).valuedouble = num;
        (*item).valueint = saturate_int(num);
    }
    item
}
#[no_mangle]
pub extern "C" fn cJSON_CreateString(string: *const c_char) -> *mut CJSON {
    if string.is_null() {
        return ptr::null_mut();
    }
    let item = create_basic(TYPE_STRING);
    if item.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: string is C string.
    unsafe {
        let s = CStr::from_ptr(string).to_string_lossy();
        (*item).valuestring = cstr_dup(&s);
        if (*item).valuestring.is_null() {
            cJSON_Delete(item);
            return ptr::null_mut();
        }
    }
    item
}
#[no_mangle]
pub extern "C" fn cJSON_CreateRaw(raw: *const c_char) -> *mut CJSON {
    let item = cJSON_CreateString(raw);
    if !item.is_null() {
        // SAFETY: item just created as string.
        unsafe {
            (*item).type_ = TYPE_RAW;
        }
    }
    item
}
#[no_mangle]
pub extern "C" fn cJSON_CreateArray() -> *mut CJSON {
    create_basic(TYPE_ARRAY)
}
#[no_mangle]
pub extern "C" fn cJSON_CreateObject() -> *mut CJSON {
    create_basic(TYPE_OBJECT)
}

#[no_mangle]
pub extern "C" fn cJSON_CreateStringReference(string: *const c_char) -> *mut CJSON {
    let item = create_basic(TYPE_STRING | TYPE_IS_REFERENCE);
    if item.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: store caller pointer by reference as C does.
    unsafe {
        (*item).valuestring = string as *mut c_char;
    }
    item
}

#[no_mangle]
pub extern "C" fn cJSON_CreateObjectReference(child: *const CJSON) -> *mut CJSON {
    let item = create_basic(TYPE_OBJECT | TYPE_IS_REFERENCE);
    if item.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: child is borrowed.
    unsafe {
        (*item).child = child as *mut CJSON;
    }
    item
}

#[no_mangle]
pub extern "C" fn cJSON_CreateArrayReference(child: *const CJSON) -> *mut CJSON {
    let item = create_basic(TYPE_ARRAY | TYPE_IS_REFERENCE);
    if item.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: child is borrowed.
    unsafe {
        (*item).child = child as *mut CJSON;
    }
    item
}

fn create_number_array(numbers: &[f64]) -> *mut CJSON {
    let a = cJSON_CreateArray();
    if a.is_null() {
        return ptr::null_mut();
    }
    for &n in numbers {
        let item = cJSON_CreateNumber(n);
        if item.is_null() || cJSON_AddItemToArray(a, item) == 0 {
            cJSON_Delete(a);
            return ptr::null_mut();
        }
    }
    a
}

#[no_mangle]
pub extern "C" fn cJSON_CreateIntArray(numbers: *const c_int, count: c_int) -> *mut CJSON {
    if numbers.is_null() || count < 0 {
        return ptr::null_mut();
    }
    // SAFETY: numbers has count ints.
    let slice = unsafe { std::slice::from_raw_parts(numbers, count as usize) };
    let vals: Vec<f64> = slice.iter().map(|&x| x as f64).collect();
    create_number_array(&vals)
}

#[no_mangle]
pub extern "C" fn cJSON_CreateFloatArray(numbers: *const f32, count: c_int) -> *mut CJSON {
    if numbers.is_null() || count < 0 {
        return ptr::null_mut();
    }
    // SAFETY: numbers points at `count` floats from the caller.
    let slice = unsafe { std::slice::from_raw_parts(numbers, count as usize) };
    let vals: Vec<f64> = slice.iter().map(|&x| x as f64).collect();
    create_number_array(&vals)
}

#[no_mangle]
pub extern "C" fn cJSON_CreateDoubleArray(numbers: *const c_double, count: c_int) -> *mut CJSON {
    if numbers.is_null() || count < 0 {
        return ptr::null_mut();
    }
    // SAFETY: numbers points at `count` doubles from the caller.
    let slice = unsafe { std::slice::from_raw_parts(numbers, count as usize) };
    create_number_array(slice)
}

#[no_mangle]
pub extern "C" fn cJSON_CreateStringArray(
    strings: *const *const c_char,
    count: c_int,
) -> *mut CJSON {
    if strings.is_null() || count < 0 {
        return ptr::null_mut();
    }
    let a = cJSON_CreateArray();
    if a.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: strings has count pointers.
    let slice = unsafe { std::slice::from_raw_parts(strings, count as usize) };
    for &s in slice {
        let item = cJSON_CreateString(s);
        if item.is_null() || cJSON_AddItemToArray(a, item) == 0 {
            cJSON_Delete(a);
            return ptr::null_mut();
        }
    }
    a
}

#[no_mangle]
pub extern "C" fn cJSON_AddItemToArray(array: *mut CJSON, item: *mut CJSON) -> c_int {
    if array.is_null() || item.is_null() || array == item {
        return 0;
    }
    // SAFETY: both valid nodes.
    unsafe {
        if (*array).child.is_null() {
            (*array).child = item;
            (*item).prev = item;
            (*item).next = ptr::null_mut();
        } else {
            let last = (*(*array).child).prev;
            (*last).next = item;
            (*item).prev = last;
            (*item).next = ptr::null_mut();
            (*(*array).child).prev = item;
        }
    }
    1
}

#[no_mangle]
pub extern "C" fn cJSON_AddItemToObject(
    object: *mut CJSON,
    string: *const c_char,
    item: *mut CJSON,
) -> c_int {
    if object.is_null() || string.is_null() || item.is_null() {
        return 0;
    }
    // SAFETY: string is C string; item valid.
    unsafe {
        if !(*item).string.is_null() && ((*item).type_ & TYPE_STRING_IS_CONST) == 0 {
            deallocate((*item).string as *mut c_void);
        }
        let s = CStr::from_ptr(string).to_string_lossy();
        (*item).string = cstr_dup(&s);
        (*item).type_ &= !TYPE_STRING_IS_CONST;
        if (*item).string.is_null() {
            return 0;
        }
    }
    cJSON_AddItemToArray(object, item)
}

#[no_mangle]
pub extern "C" fn cJSON_AddItemToObjectCS(
    object: *mut CJSON,
    string: *const c_char,
    item: *mut CJSON,
) -> c_int {
    if object.is_null() || string.is_null() || item.is_null() {
        return 0;
    }
    // SAFETY: const key stored by reference.
    unsafe {
        if !(*item).string.is_null() && ((*item).type_ & TYPE_STRING_IS_CONST) == 0 {
            deallocate((*item).string as *mut c_void);
        }
        (*item).string = string as *mut c_char;
        (*item).type_ |= TYPE_STRING_IS_CONST;
    }
    cJSON_AddItemToArray(object, item)
}

#[no_mangle]
pub extern "C" fn cJSON_AddItemReferenceToArray(array: *mut CJSON, item: *mut CJSON) -> c_int {
    if item.is_null() {
        return 0;
    }
    let reference = new_item();
    if reference.is_null() {
        return 0;
    }
    // SAFETY: item is a non-null cJSON; reference is a freshly allocated node.
    unsafe {
        (*reference).type_ = (*item).type_ | TYPE_IS_REFERENCE;
        (*reference).valueint = (*item).valueint;
        (*reference).valuedouble = (*item).valuedouble;
        (*reference).valuestring = (*item).valuestring;
        (*reference).string = (*item).string;
        (*reference).child = (*item).child;
    }
    cJSON_AddItemToArray(array, reference)
}

#[no_mangle]
pub extern "C" fn cJSON_AddItemReferenceToObject(
    object: *mut CJSON,
    string: *const c_char,
    item: *mut CJSON,
) -> c_int {
    if item.is_null() {
        return 0;
    }
    let reference = new_item();
    if reference.is_null() {
        return 0;
    }
    // SAFETY: item is a non-null cJSON; reference is a freshly allocated node.
    unsafe {
        (*reference).type_ = (*item).type_ | TYPE_IS_REFERENCE;
        (*reference).valueint = (*item).valueint;
        (*reference).valuedouble = (*item).valuedouble;
        (*reference).valuestring = (*item).valuestring;
        (*reference).child = (*item).child;
    }
    cJSON_AddItemToObject(object, string, reference)
}

#[no_mangle]
pub extern "C" fn cJSON_DetachItemViaPointer(parent: *mut CJSON, item: *mut CJSON) -> *mut CJSON {
    if parent.is_null() || item.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: parent/item in same list.
    unsafe {
        if !(*item).prev.is_null() && (*item).prev != item {
            (*(*item).prev).next = (*item).next;
        }
        if !(*item).next.is_null() {
            (*(*item).next).prev = (*item).prev;
        }
        if (*parent).child == item {
            (*parent).child = (*item).next;
            if !(*parent).child.is_null() {
                // fix circular prev on head
                let mut last = (*parent).child;
                while !(*last).next.is_null() {
                    last = (*last).next;
                }
                (*(*parent).child).prev = last;
            }
        } else if !(*parent).child.is_null() && (*(*parent).child).prev == item {
            (*(*parent).child).prev = (*item).prev;
        }
        (*item).prev = ptr::null_mut();
        (*item).next = ptr::null_mut();
    }
    item
}

#[no_mangle]
pub extern "C" fn cJSON_DetachItemFromArray(array: *mut CJSON, which: c_int) -> *mut CJSON {
    let item = cJSON_GetArrayItem(array, which);
    cJSON_DetachItemViaPointer(array, item)
}

#[no_mangle]
pub extern "C" fn cJSON_DeleteItemFromArray(array: *mut CJSON, which: c_int) {
    cJSON_Delete(cJSON_DetachItemFromArray(array, which));
}

#[no_mangle]
pub extern "C" fn cJSON_DetachItemFromObject(
    object: *mut CJSON,
    string: *const c_char,
) -> *mut CJSON {
    let item = cJSON_GetObjectItem(object, string);
    cJSON_DetachItemViaPointer(object, item)
}

#[no_mangle]
pub extern "C" fn cJSON_DetachItemFromObjectCaseSensitive(
    object: *mut CJSON,
    string: *const c_char,
) -> *mut CJSON {
    let item = cJSON_GetObjectItemCaseSensitive(object, string);
    cJSON_DetachItemViaPointer(object, item)
}

#[no_mangle]
pub extern "C" fn cJSON_DeleteItemFromObject(object: *mut CJSON, string: *const c_char) {
    cJSON_Delete(cJSON_DetachItemFromObject(object, string));
}

#[no_mangle]
pub extern "C" fn cJSON_DeleteItemFromObjectCaseSensitive(
    object: *mut CJSON,
    string: *const c_char,
) {
    cJSON_Delete(cJSON_DetachItemFromObjectCaseSensitive(object, string));
}

#[no_mangle]
pub extern "C" fn cJSON_InsertItemInArray(
    array: *mut CJSON,
    which: c_int,
    newitem: *mut CJSON,
) -> c_int {
    let after = cJSON_GetArrayItem(array, which);
    if after.is_null() {
        return cJSON_AddItemToArray(array, newitem);
    }
    if array.is_null() || newitem.is_null() {
        return 0;
    }
    // SAFETY: insert before after.
    unsafe {
        (*newitem).next = after;
        (*newitem).prev = (*after).prev;
        if (*array).child == after {
            (*array).child = newitem;
        } else if !(*after).prev.is_null() {
            (*(*after).prev).next = newitem;
        }
        (*after).prev = newitem;
    }
    1
}

#[no_mangle]
pub extern "C" fn cJSON_ReplaceItemViaPointer(
    parent: *mut CJSON,
    item: *mut CJSON,
    replacement: *mut CJSON,
) -> c_int {
    if parent.is_null() || item.is_null() || replacement.is_null() || item == replacement {
        return 0;
    }
    // SAFETY: list surgery.
    unsafe {
        (*replacement).next = (*item).next;
        (*replacement).prev = (*item).prev;
        if !(*replacement).next.is_null() {
            (*(*replacement).next).prev = replacement;
        }
        if (*parent).child == item {
            (*parent).child = replacement;
        } else if !(*item).prev.is_null() {
            (*(*item).prev).next = replacement;
        }
        if !(*parent).child.is_null() && (*(*parent).child).prev == item {
            (*(*parent).child).prev = replacement;
        }
        (*item).next = ptr::null_mut();
        (*item).prev = ptr::null_mut();
        cJSON_Delete(item);
    }
    1
}

#[no_mangle]
pub extern "C" fn cJSON_ReplaceItemInArray(
    array: *mut CJSON,
    which: c_int,
    newitem: *mut CJSON,
) -> c_int {
    cJSON_ReplaceItemViaPointer(array, cJSON_GetArrayItem(array, which), newitem)
}

#[no_mangle]
pub extern "C" fn cJSON_ReplaceItemInObject(
    object: *mut CJSON,
    string: *const c_char,
    newitem: *mut CJSON,
) -> c_int {
    let item = cJSON_GetObjectItem(object, string);
    if item.is_null() || newitem.is_null() || string.is_null() {
        return 0;
    }
    // SAFETY: copy key onto replacement like C.
    unsafe {
        if !(*newitem).string.is_null() && ((*newitem).type_ & TYPE_STRING_IS_CONST) == 0 {
            deallocate((*newitem).string as *mut c_void);
        }
        let s = CStr::from_ptr(string).to_string_lossy();
        (*newitem).string = cstr_dup(&s);
        (*newitem).type_ &= !TYPE_STRING_IS_CONST;
    }
    cJSON_ReplaceItemViaPointer(object, item, newitem)
}

#[no_mangle]
pub extern "C" fn cJSON_ReplaceItemInObjectCaseSensitive(
    object: *mut CJSON,
    string: *const c_char,
    newitem: *mut CJSON,
) -> c_int {
    let item = cJSON_GetObjectItemCaseSensitive(object, string);
    if item.is_null() || newitem.is_null() || string.is_null() {
        return 0;
    }
    // SAFETY: newitem and string are non-null caller pointers; key is duplicated.
    unsafe {
        if !(*newitem).string.is_null() && ((*newitem).type_ & TYPE_STRING_IS_CONST) == 0 {
            deallocate((*newitem).string as *mut c_void);
        }
        let s = CStr::from_ptr(string).to_string_lossy();
        (*newitem).string = cstr_dup(&s);
        (*newitem).type_ &= !TYPE_STRING_IS_CONST;
    }
    cJSON_ReplaceItemViaPointer(object, item, newitem)
}

#[no_mangle]
pub extern "C" fn cJSON_Duplicate(item: *const CJSON, recurse: c_int) -> *mut CJSON {
    let Some(v) = cjson_to_value(item) else {
        return ptr::null_mut();
    };
    match core_duplicate(&v, recurse != 0) {
        Ok(d) => value_to_cjson(&d),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn cJSON_Compare(a: *const CJSON, b: *const CJSON, case_sensitive: c_int) -> c_int {
    let (Some(av), Some(bv)) = (cjson_to_value(a), cjson_to_value(b)) else {
        return 0;
    };
    if core_compare(&av, &bv, case_sensitive != 0) {
        1
    } else {
        0
    }
}

#[no_mangle]
pub extern "C" fn cJSON_Minify(json: *mut c_char) {
    if json.is_null() {
        return;
    }
    // SAFETY: json is a mutable C string.
    unsafe {
        let len = libc_strlen(json);
        let slice = std::slice::from_raw_parts(json as *const u8, len);
        let mut s = String::from_utf8_lossy(slice).into_owned();
        core_minify(&mut s);
        if s.len() <= len {
            ptr::copy_nonoverlapping(s.as_ptr(), json as *mut u8, s.len());
            *json.add(s.len()) = 0;
        }
    }
}

#[no_mangle]
pub extern "C" fn cJSON_AddNullToObject(object: *mut CJSON, name: *const c_char) -> *mut CJSON {
    let item = cJSON_CreateNull();
    if item.is_null() || cJSON_AddItemToObject(object, name, item) == 0 {
        cJSON_Delete(item);
        return ptr::null_mut();
    }
    item
}
#[no_mangle]
pub extern "C" fn cJSON_AddTrueToObject(object: *mut CJSON, name: *const c_char) -> *mut CJSON {
    let item = cJSON_CreateTrue();
    if item.is_null() || cJSON_AddItemToObject(object, name, item) == 0 {
        cJSON_Delete(item);
        return ptr::null_mut();
    }
    item
}
#[no_mangle]
pub extern "C" fn cJSON_AddFalseToObject(object: *mut CJSON, name: *const c_char) -> *mut CJSON {
    let item = cJSON_CreateFalse();
    if item.is_null() || cJSON_AddItemToObject(object, name, item) == 0 {
        cJSON_Delete(item);
        return ptr::null_mut();
    }
    item
}
#[no_mangle]
pub extern "C" fn cJSON_AddBoolToObject(
    object: *mut CJSON,
    name: *const c_char,
    boolean: c_int,
) -> *mut CJSON {
    let item = cJSON_CreateBool(boolean);
    if item.is_null() || cJSON_AddItemToObject(object, name, item) == 0 {
        cJSON_Delete(item);
        return ptr::null_mut();
    }
    item
}
#[no_mangle]
pub extern "C" fn cJSON_AddNumberToObject(
    object: *mut CJSON,
    name: *const c_char,
    number: c_double,
) -> *mut CJSON {
    let item = cJSON_CreateNumber(number);
    if item.is_null() || cJSON_AddItemToObject(object, name, item) == 0 {
        cJSON_Delete(item);
        return ptr::null_mut();
    }
    item
}
#[no_mangle]
pub extern "C" fn cJSON_AddStringToObject(
    object: *mut CJSON,
    name: *const c_char,
    string: *const c_char,
) -> *mut CJSON {
    let item = cJSON_CreateString(string);
    if item.is_null() || cJSON_AddItemToObject(object, name, item) == 0 {
        cJSON_Delete(item);
        return ptr::null_mut();
    }
    item
}
#[no_mangle]
pub extern "C" fn cJSON_AddRawToObject(
    object: *mut CJSON,
    name: *const c_char,
    raw: *const c_char,
) -> *mut CJSON {
    let item = cJSON_CreateRaw(raw);
    if item.is_null() || cJSON_AddItemToObject(object, name, item) == 0 {
        cJSON_Delete(item);
        return ptr::null_mut();
    }
    item
}
#[no_mangle]
pub extern "C" fn cJSON_AddObjectToObject(object: *mut CJSON, name: *const c_char) -> *mut CJSON {
    let item = cJSON_CreateObject();
    if item.is_null() || cJSON_AddItemToObject(object, name, item) == 0 {
        cJSON_Delete(item);
        return ptr::null_mut();
    }
    item
}
#[no_mangle]
pub extern "C" fn cJSON_AddArrayToObject(object: *mut CJSON, name: *const c_char) -> *mut CJSON {
    let item = cJSON_CreateArray();
    if item.is_null() || cJSON_AddItemToObject(object, name, item) == 0 {
        cJSON_Delete(item);
        return ptr::null_mut();
    }
    item
}

#[no_mangle]
pub extern "C" fn cJSON_SetNumberHelper(object: *mut CJSON, number: c_double) -> c_double {
    if object.is_null() {
        return f64::NAN;
    }
    // SAFETY: object valid.
    unsafe {
        (*object).valueint = saturate_int(number);
        (*object).valuedouble = number;
        number
    }
}

#[no_mangle]
pub extern "C" fn cJSON_SetValuestring(
    object: *mut CJSON,
    valuestring: *const c_char,
) -> *mut c_char {
    if object.is_null() || valuestring.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: object/valuestring from caller.
    unsafe {
        if ((*object).type_ & TYPE_STRING) == 0 || ((*object).type_ & TYPE_IS_REFERENCE) != 0 {
            return ptr::null_mut();
        }
        if (*object).valuestring.is_null() {
            return ptr::null_mut();
        }
        let new_s = CStr::from_ptr(valuestring).to_string_lossy();
        let old_s = CStr::from_ptr((*object).valuestring).to_string_lossy();
        if new_s.len() <= old_s.len() {
            // Overlap guard approximating C.
            let o = (*object).valuestring as usize;
            let n = valuestring as usize;
            let o_end = o + old_s.len();
            let n_end = n + new_s.len();
            if !(n_end < o || o_end < n) {
                return ptr::null_mut();
            }
            ptr::copy_nonoverlapping(valuestring, (*object).valuestring, new_s.len() + 1);
            return (*object).valuestring;
        }
        let copy = cstr_dup(&new_s);
        if copy.is_null() {
            return ptr::null_mut();
        }
        deallocate((*object).valuestring as *mut c_void);
        (*object).valuestring = copy;
        copy
    }
}

#[no_mangle]
pub extern "C" fn cJSON_malloc(size: usize) -> *mut c_void {
    allocate(size)
}

#[no_mangle]
pub extern "C" fn cJSON_free(object: *mut c_void) {
    deallocate(object);
}

// Silence unused import warnings from macros / helpers.
#[allow(dead_code)]
fn _keep_realloc() {
    // SAFETY: unused placeholder referencing realloc symbol for link parity if needed.
    let _ = realloc as unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void;
}
