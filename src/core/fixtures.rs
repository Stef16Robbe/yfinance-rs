//! Test/recording helpers for persisting HTTP fixtures.
//! Compiled only when the `test-mode` feature is enabled.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub fn get_fixture_dir() -> PathBuf {
    env::var("YF_FIXDIR").map_or_else(
        |_| Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
        PathBuf::from,
    )
}

pub fn record_fixture(
    endpoint: &str,
    symbol: &str,
    ext: &str,
    body: &str,
) -> Result<(), std::io::Error> {
    let dir = get_fixture_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    let filename = format!("{endpoint}_{symbol}.{ext}");
    let path = dir.join(filename);

    let mut file = fs::File::create(&path)?;
    write_fixture_body(&mut file, ext, body)?;

    if env::var("YF_DEBUG").ok().as_deref() == Some("1") {
        eprintln!("YF_RECORD: wrote fixture to {}", path.display());
    }
    Ok(())
}

fn write_fixture_body<W: Write>(writer: &mut W, ext: &str, body: &str) -> io::Result<()> {
    if ext.eq_ignore_ascii_case("json")
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(body)
    {
        serde_json::to_writer_pretty(&mut *writer, &value).map_err(io::Error::other)?;
        writer.write_all(b"\n")?;
        return Ok(());
    }

    writer.write_all(body.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::write_fixture_body;

    fn fixture_body(ext: &str, body: &str) -> String {
        let mut bytes = Vec::new();
        write_fixture_body(&mut bytes, ext, body).unwrap();
        String::from_utf8(bytes).unwrap()
    }

    #[test]
    fn fixture_body_prettifies_valid_json() {
        assert_eq!(
            fixture_body("json", r#"{"a":1,"b":[true]}"#),
            "{\n  \"a\": 1,\n  \"b\": [\n    true\n  ]\n}\n"
        );
    }

    #[test]
    fn fixture_body_leaves_invalid_json_raw() {
        assert_eq!(
            fixture_body("json", "<html>oops</html>"),
            "<html>oops</html>"
        );
    }

    #[test]
    fn fixture_body_leaves_non_json_raw() {
        assert_eq!(fixture_body("b64", "abc123"), "abc123");
    }
}
