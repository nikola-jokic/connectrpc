use axum_reqwest::{HelloRequest, HelloResponse, HelloWorldServiceAsyncService};
use connectrpc::http::Uri;
use connectrpc::{
    ClientStreamingRequest, ClientStreamingResponse, Error, Result, UnaryRequest, UnaryResponse,
};
use futures_util::{StreamExt, stream};
use std::str::FromStr;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::task::JoinHandle;

const SERVER_ADDR: &str = "127.0.0.1:50051";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server_handle = spawn_server().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let client = axum_reqwest::HelloWorldServiceReqwestProtoClient::new(
        reqwest::Client::new(),
        Uri::from_str("http://127.0.0.1:50051").expect("Failed to create URI"),
    )
    .expect("Failed to create client");

    let response = client
        .say_hello(UnaryRequest::new(HelloRequest {
            name: Some("Axum".to_string()),
        }))
        .await
        .map_err(|e| anyhow::anyhow!("RPC failed: {:?}", e))?;

    let response_message = response.into_message();
    println!("Received response: {:?}", response_message);
    assert_eq!(response_message.message, "Hello, Axum!");

    let stream = client
        .say_hello_client_stream(ClientStreamingRequest::new(Box::pin(stream::iter(vec![
            Ok(HelloRequest {
                name: Some("Nikola".to_string()),
            }),
            Ok(HelloRequest {
                name: Some("John".to_string()),
            }),
        ]))))
        .await
        .map_err(|e| anyhow::anyhow!("RPC failed: {:?}", e))?;

    let response_message = stream.into_message();
    println!("Received stream response: {:?}", response_message);
    assert_eq!(response_message.message, "Hello Nikola, John");

    server_handle.abort();
    Ok(())
}

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

    println!("Generated message: {}", message);
    let response = HelloResponse { message };
    Ok(UnaryResponse::new(response))
}

async fn say_hello_client_stream(
    _state: State,
    request: ClientStreamingRequest<HelloRequest>,
) -> Result<ClientStreamingResponse<HelloResponse>> {
    let mut names = vec![];
    let mut messages = request.into_message_stream();
    while let Some(req) = messages.next().await {
        let req = req.expect("Failed to read message from stream");
        names.push(req.name.expect("Name is required"))
    }

    Ok(ClientStreamingResponse::new(HelloResponse {
        message: format!("Hello {}", names.join(", ")),
    }))
}

async fn spawn_server() -> anyhow::Result<JoinHandle<()>> {
    let router = axum_reqwest::HelloWorldServiceAxumServer {
        // The server uses fields to store state and handlers
        // Store the state directly in the server struct
        state: State {
            cache: Arc::new(Mutex::new(BTreeMap::new())),
        },
        // Provide the handler function for the SayHello RPC
        say_hello,
        say_hello_client_stream,
    }
    .into_router();

    let listener = tokio::net::TcpListener::bind(SERVER_ADDR).await.unwrap();

    Ok(tokio::spawn(async move {
        println!("Axum server listening on {SERVER_ADDR}");
        axum::serve(listener, router).await.unwrap();
    }))
}
