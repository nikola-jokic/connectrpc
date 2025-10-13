use connectrpc_build::Settings;
use std::env;
use std::fs;

#[test]
fn test_generate() {
    // Create a temporary output directory for the test
    let temp_dir = env::temp_dir().join("connectrpc_test_output");
    fs::create_dir_all(&temp_dir).expect("failed to create temp directory");
    unsafe {
        env::set_var("OUT_DIR", &temp_dir);
    }

    let settings = Settings::from_directory_recursive("tests/proto")
        .expect("failed to create settings from directory");

    let result = settings.generate().expect("failed to generate code");

    assert!(
        result.contains_key("hello"),
        "result {result:?} missing 'hello' package"
    );
    assert!(
        result.len() == 1,
        "expected 1 package, got {}",
        result.len()
    );

    println!("Generated code for {} packages:", result.len());
    for (package, code) in &result {
        println!("\n=== Package: {} ===", package);
        println!("{}", code);
    }
}
