use axum_server_unary::{HelloRequest, HelloResponse};
use connectrpc::{Error, Result, UnaryRequest, UnaryResponse};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug)]
struct State {
    cache: Arc<Mutex<BTreeMap<String, String>>>,
}

async fn say_hello(
    state: State,
    req: UnaryRequest<HelloRequest>,
) -> Result<UnaryResponse<HelloResponse>> {
    let name = req
        .into_message()
        .name
        .ok_or_else(|| Error::internal("Name is required"))?;

    let message = {
        let mut cache = state.cache.lock().unwrap();
        match cache.get(&name) {
            Some(cached_message) => cached_message.clone(),
            None => {
                let message = format!("Hello, {}!", name);
                cache.insert(name.clone(), message.clone());
                message
            }
        }
    };

    let response = HelloResponse { message };
    Ok(UnaryResponse::new(response))
}

#[tokio::main]
async fn main() {
    // Create the Axum router with the generated server and your handler
    //
    // There is a big reason why we don't use `axum::Router::new()` directly, but rather
    // as using the generated server struct:
    //   1. You can swap handlers easily, without using a single struct that implements
    //   the whole service trait.
    //
    //   2. It issues a compile-time error when you re-generate the code and forget to
    //   implement a new method.
    let router = axum_server_unary::HelloWorldServiceAxumServer {
        // The server uses fields to store state and handlers
        // Store the state directly in the server struct
        state: State {
            cache: Arc::new(Mutex::new(BTreeMap::new())),
        },
        // Provide the handler function for the SayHello RPC
        say_hello,
    }
    .into_router();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:50051")
        .await
        .unwrap();

    let sh = tokio::spawn(async move {
        println!("Axum server listening on 127.0.0.1:50051");
        axum::serve(listener, router).await.unwrap();
    });

    let mut cmd = tokio::process::Command::new("curl")
        .arg("-X")
        .arg("POST")
        .arg("--header")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg("{\"name\":\"World\"}")
        .arg("http://127.0.0.1:50051/hello.HelloWorldService/SayHello")
        .spawn()
        .expect("Failed to start curl");

    assert!(cmd.wait().await.expect("Failed to wait on curl").success());

    sh.abort();
}
