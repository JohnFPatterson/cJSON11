//! Libc `snprintf` boundary for `%1.Ng` matching cJSON's `print_number`.
//!
//! This is the only place that talks to libc for float formatting so
//! `cjson-core` can stay `forbid(unsafe_code)` while matching glibc output.

/// Format `d` like C `sprintf(buf, "%1.<prec>g", d)`.
pub fn sprintf_g(d: f64, prec: usize) -> String {
    // "%1.15g" style; prec is the significant-digit count.
    let fmt = std::ffi::CString::new(format!("%1.{prec}g")).expect("format string");
    let mut buf = [0i8; 64];
    // SAFETY: fmt is a valid CString; buf is a writable stack array;
    // snprintf writes at most buf.len() bytes and NUL-terminates on success.
    let n = unsafe { libc::snprintf(buf.as_mut_ptr(), buf.len(), fmt.as_ptr(), d) };
    if n < 0 || n as usize >= buf.len() {
        return "null".to_owned();
    }
    let bytes = buf[..n as usize]
        .iter()
        .map(|b| *b as u8)
        .collect::<Vec<_>>();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Parse like C `sscanf(s, "%lg", &out)`.
pub fn sscanf_lg(s: &str) -> Option<f64> {
    let mut out = 0f64;
    let c = std::ffi::CString::new(s).ok()?;
    // SAFETY: c is a valid CString; out is a stack f64; format string is a static CStr.
    let n = unsafe { libc::sscanf(c.as_ptr(), c"%lg".as_ptr(), &mut out as *mut f64) };
    if n == 1 {
        Some(out)
    } else {
        None
    }
}

/// Parse like C `strtod`, returning the value and bytes consumed.
pub fn strtod(s: &str) -> Option<(f64, usize)> {
    let c = std::ffi::CString::new(s).ok()?;
    let mut end: *mut libc::c_char = std::ptr::null_mut();
    // SAFETY: c is a valid CString; end receives a pointer into that buffer.
    let v = unsafe { libc::strtod(c.as_ptr(), &mut end) };
    if end.is_null() || end == c.as_ptr() as *mut libc::c_char {
        return None;
    }
    // SAFETY: end points into c's buffer.
    let consumed = unsafe { end.offset_from(c.as_ptr()) as usize };
    Some((v, consumed))
}
