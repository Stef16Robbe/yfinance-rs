use std::io::{Error, Result};

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=src/stream/yaticker.proto");

    let protoc = protoc_bin_vendored::protoc_bin_path()
        .map_err(|e| Error::other(format!("failed to locate vendored protoc: {e}")))?;

    let mut config = prost_build::Config::new();
    config.protoc_executable(protoc);
    config
        .compile_protos(&["src/stream/yaticker.proto"], &["src/stream/"])
        .map_err(|e| {
            eprintln!("failed to compile protos with vendored protoc: {e}");
            e
        })?;

    Ok(())
}
