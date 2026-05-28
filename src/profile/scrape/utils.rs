pub fn iter_json_scripts(html: &str) -> Vec<(&str, &str)> {
    crate::core::logging::trace_debug!(
        html_len = html.len(),
        "scanning profile HTML for JSON script blocks"
    );

    let mut res = Vec::new();
    let mut pos = 0usize;

    crate::core::logging::trace_only! {
        let mut total_scripts = 0usize;
        let mut total_json_scripts = 0usize;
        let mut total_svelte_fetched = 0usize;
    }

    while let Some(si) = html[pos..].find("<script") {
        let si = pos + si;

        crate::core::logging::trace_only! {
            total_scripts += 1;
        }

        let open_end = match html[si..].find('>') {
            Some(x) => si + x,
            None => break,
        };
        let tag_open = &html[si..=open_end];

        let is_json = tag_open.contains("type=\"application/json\"");
        if is_json {
            crate::core::logging::trace_only! {
                total_json_scripts += 1;
            }
            if tag_open.contains("data-sveltekit-fetched") {
                crate::core::logging::trace_only! {
                    total_svelte_fetched += 1;
                }
            }
        }

        let close = match html[open_end + 1..].find("</script>") {
            Some(x) => open_end + 1 + x,
            None => break,
        };
        let inner = &html[open_end + 1..close];

        if is_json {
            res.push((tag_open, inner));
        }
        pos = close + "</script>".len();
    }

    crate::core::logging::trace_only! {
        crate::core::logging::trace_debug!(
            total_scripts,
            total_json_scripts,
            total_svelte_fetched,
            "finished scanning profile HTML for JSON script blocks"
        );
        if let Some((attrs, body)) = res.first() {
            let a = attrs.get(..180).unwrap_or(attrs);
            let b = body.get(..120).unwrap_or(body);
            crate::core::logging::trace_debug!(
                attrs = a,
                body = b,
                "first profile JSON script preview"
            );
        }
    }
    res
}

/// Exposed for debug helpers as well.
pub fn find_matching_brace(s: &str, start: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    let i = start;
    if bytes.get(i).copied()? != b'{' {
        return None;
    }

    let mut depth = 0usize;
    let mut in_str = false;
    let mut j = i;

    while j < bytes.len() {
        let c = bytes[j];

        if in_str {
            if c == b'\\' {
                j += 2;
                continue;
            } else if c == b'"' {
                in_str = false;
            }
            j += 1;
            continue;
        }

        match c {
            b'"' => {
                in_str = true;
            }
            b'{' => {
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

/// Exposed for debug helpers as well.
#[cfg(feature = "debug-dumps")]
pub fn parse_jsonish_string(s: &str) -> Option<serde_json::Value> {
    let t = s.trim();
    if t.starts_with('{') || t.starts_with('[') {
        serde_json::from_str::<serde_json::Value>(t).ok()
    } else {
        None
    }
}

#[cfg(feature = "debug-dumps")]
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}
