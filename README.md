# ConnectRPC - Rust implementation of Connect protocol

[![Crates.io](https://img.shields.io/crates/v/connectrpc)](https://crates.io/crates/connectrpc)
[![Docs.rs](https://docs.rs/connectrpc/badge.svg)](https://docs.rs/connectrpc)

## About

ConnectRPC is a Rust implementation of the Connect protocol, a modern and efficient RPC framework designed for building scalable and high-performance applications.
It supports multiple transport protocols, including HTTP/1.1, HTTP/2, and gRPC, making it versatile for various use cases.

⚠️ This project is in early development. It only supports unary and unary get requests at the moment, and is heavily being tested and iterated on. Expect breaking changes.

## Features

The library offers multiple components and features to facilitate the development of RPC clients and servers:

### Components:

- [x] **Reqwest Client**: Built on top of the popular Reqwest HTTP client for ease of use and reliability
- [x] **Axum Server**: Implementation used for axum servers.
- [x] **Code Generation**: Tools to generate client and server code from protobuf definitions.

### Features:

- [x] **Unary Calls**: Support for unary RPC calls.
- [x] **Unary Get Calls**: Support for unary GET RPC calls.
- [ ] **Streaming Calls**: (Planned) Support for server-side, client-side, and bidirectional streaming RPC calls.
- [ ] **Middleware Support**: (Planned) Ability to add middleware for logging, authentication, etc. MTLs is supported since reqwest supports it.

## Usage

Define services and messages in a `proto` file:

```proto
syntax = "proto3";

package hello;

message HelloRequest { optional string name = 1; }

message GoodbyeRequest { optional string name = 1; }

message HelloResponse { string message = 1; }

message GoodbyeResponse { string message = 1; }

service HelloWorldService {
  rpc SayHello(HelloRequest) returns (HelloResponse) {}
  rpc SayGoodbye(GoodbyeRequest) returns (GoodbyeResponse) {}
}
```

You need to add `connectrpc-build` to your `build-dependencies` in `Cargo.toml`:

```toml
[build-dependencies]
connectrpc-build = "0.1"
protoc-fetcher = "0.1.2"
```

Then, create a `build.rs` file to generate the necessary code. More complex examples will be included in [the examples directory](./connectrpc-examples), but here is a simple snippet:

```rust
use std::{fs, path::PathBuf};
use connectrpc_build::Settings;

fn main() {
    let settings = Settings::from_directory_recursive("./proto")
        .expect("failed to create settings from directory");

    let mut result = settings.generate().expect("failed to generate code");

    // The difference between other generators is that **YOU** pick where the generated code goes, you can add more code to include potentially other things, such as `use your_module`;
    let package = result.remove("hello").expect("no hello package found"); // package name
    let out_file = PathBuf::from("src/lib.rs"); // where to put the generated code
    fs::write(&out_file, package).expect("failed to write generated code");
}
```

This will generate the necessary client and server code in `src/lib.rs`. If you are building a client only, you can specify the generation features in the `Settings`:

```rust
use std::{fs, path::PathBuf};
use connectrpc_build::{GeneratorFeatures, GeneratorReqwestFeatures, Settings};

fn main() {
    let settings = Settings::from_directory_recursive("./proto")
        .expect("failed to create settings from directory");

    let features = GeneratorFeatures::new().reqwest(GeneratorReqwestFeatures {
        proto: true, // Only generate the protobuf client
        json: false, // Do not generate the JSON client
    });
    settings.features = features;

    let mut result = settings.generate().expect("failed to generate code");

    // The difference between other generators is that **YOU** pick where the generated code goes, you can add more code to include potentially other things, such as `use your_module`;
    let package = result.remove("hello").expect("no hello package found"); // package name
    let out_file = PathBuf::from("src/client.rs"); // where to put the generated code
    fs::write(&out_file, package).expect("failed to write generated code");

    // Generate server code to a separate file
    let features = GeneratorFeatures::new().axum(); // Only generate the axum server
    settings.features = features;
    let mut result = settings.generate().expect("failed to generate code");
    let package = result.remove("hello").expect("no hello package found"); // package name
    let out_file = PathBuf::from("src/server.rs"); // where to put the generated code
    fs::write(&out_file, package).expect("failed to write generated code");
}

Note: Depending on the features you enable, you might need to include additional dependencies in your `Cargo.toml`. For example, if you are using the Reqwest client, ensure that you have the `reqwest` crate included.

```toml
[package]
name = "your_crate_name"
version = "0.1.0"
edition = "2024"

[dependencies]
# Required dependencies for ConnectRPC
pbjson = "0.8.0"
pbjson-types = "0.8.0"
prost = "0.14"
serde = { version = "1", features = ["derive"] }
connectrpc = { version = "0.1", features = ["reqwest"] }
# Dependencies you need
reqwest = { version = "0.12", features = ["json", "gzip"] }

[build-dependencies]
# Required for code generation
connectrpc-build = "0.1"
protoc-fetcher = "0.1.2"
```
