// SPDX-License-Identifier: 0BSD

use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();

    let proto = root.join("proto/pvault/economy/v1/economy.proto");
    let out = root.join("crates/pvault-proto/src/generated");
    std::fs::create_dir_all(&out).expect("create output directory");

    let files = protox::compile([&proto], [root.join("proto")]).expect("compile protos");

    let mut config = prost_build::Config::new();
    config.out_dir(&out);
    config.skip_protoc_run();
    config
        .compile_fds(files)
        .expect("generate rust from descriptors");

    println!("wrote {}", out.display());
}
