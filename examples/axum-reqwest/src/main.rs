use axum_reqwest::{
    client,
};
use connectrpc::{Error, Result, UnaryRequest, UnaryResponse, http::Uri};
use std::{
    collections::BTreeMap, str::FromStr, sync::{Arc, Mutex}
};

const ADDR: &str = "127.0.0.1:50051";

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

async fn start_server() {
    // Create the Axum router with the generated server and your handler
    //
    // There is a big reason why we don't use `axum::Router::new()` directly, but rather
    // as using the generated server struct:
    //   1. You can swap handlers easily, without using a single struct that implements
    //   the whole service trait.
    //
    //   2. It issues a compile-time error when you re-generate the code and forget to
    //   implement a new method.
    let router = axum_reqwest::server::HelloWorldServiceAxumServer {
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

    println!("Axum server listening on 127.0.0.1:50051");
    axum::serve(listener, router).await.unwrap();
}

#[tokio::main]
async fn main() {
    let server_handle = tokio::spawn(async {
        start_server().await;
    });

    let reqwest = reqwest::Client::new(); // Create reqwest client. You can customize it as needed.
    let json_client = client::HelloWorldServiceReqwestJsonClient::new(
        reqwest,
        Uri::from_str(&format!("http://{}", ADDR)).unwrap(),
    )
    .expect("Failed to create client");
    
    json_client.say_hello(UnaryRequest::new())

    // Wait for the server to finish
    let _ = server_handle.await;
}
