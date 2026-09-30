//! In-place minify matching `cJSON_Minify`.

/// Minify `json` in place: strip whitespace and `//` / `/* */` comments outside strings.
pub fn minify(json: &mut String) {
    let bytes = json.as_bytes().to_vec();
    let mut into: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'/' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == b'\n' {
                            i += 1;
                            break;
                        }
                        i += 1;
                    }
                } else if i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    i += 2;
                    while i < bytes.len() {
                        if bytes[i] == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            b'"' => {
                into.push(b'"');
                i += 1;
                while i < bytes.len() {
                    into.push(bytes[i]);
                    if bytes[i] == b'"' {
                        i += 1;
                        break;
                    } else if bytes[i] == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                        into.push(bytes[i + 1]);
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            c => {
                into.push(c);
                i += 1;
            }
        }
    }
    // Keep only the minified prefix; C writes a NUL at `into`.
    *json = String::from_utf8_lossy(&into).into_owned();
}
