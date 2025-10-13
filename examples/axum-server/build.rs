use connectrpc_build::{GeneratorFeatures, Settings};
use std::{fs, path::PathBuf};

const PROTO_DIR: &str = "proto";
const PROTO_FILE: &str = "hello.proto";
const PACKAGE_NAME: &str = "hello";

fn main() {
    // Generate axum server only.
    let features = GeneratorFeatures::new().axum();

    // Manually select what to generate
    let settings = Settings {
        includes: vec![PROTO_DIR.into()],
        inputs: vec![PathBuf::from(PROTO_DIR).join(PROTO_FILE)],
        features,
        ..Default::default()
    };

    // Output is a map of package name to generated code
    // As opposed to other generators, this one returns a string, so you can
    // decide where to write it, and if you want to add more code to it.
    let mut output = match settings.generate() {
        Ok(o) => o,
        Err(e) => panic!("connectrpc_build failed: {:?}", e),
    };

    // In this example we just write the generated code to src/lib.rs
    // You can of course write it to any file you want.
    let v = output
        .remove(PACKAGE_NAME)
        .expect("package is not found in the output");
    let out_file = PathBuf::from("src/lib.rs");
    fs::write(&out_file, v).expect("Unable to write file");
}
