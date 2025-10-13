use connectrpc::http::Uri;
use connectrpc::{Codec, ReqwestClient, UnaryRequest};
use reqwest_client::{HelloRequest, HelloWorldServiceAsyncService};

#[tokio::main]
async fn main() {
    // Instantiate the reqwest client with constraints you have
    // For example, you might want to set timeouts, proxies, etc.
    let reqwest_client = reqwest::Client::new();
    // Base URI is the prefix before the service path.
    // Replace with your server's address.
    let base_uri = Uri::from_static("http://localhost:50051");

    // Since we generated a proto client, we can use it directly.
    let mut client = reqwest_client::HelloWorldServiceReqwestProtoClient::new(
        reqwest_client.clone(),
        base_uri.clone(),
    )
    .expect("Failed to create client");

    // Make a request
    let res = client
        .say_hello(UnaryRequest::new(HelloRequest {
            name: Some("World".to_string()),
        }))
        .await
        .expect("RPC failed");

    // Print the response message
    println!("Response message: {:?}", res.into_message().message);

    // If you want to swap the client implementation, you can do so easily.
    // In this case, we would instantiate a JSON client only for the say_hello method.
    client.say_hello = ReqwestClient::new(reqwest_client, base_uri, Codec::Json)
        .expect("Failed to create JSON client");

    // Make a request with the JSON client
    let res = client
        .say_hello(UnaryRequest::new(HelloRequest {
            name: Some("World".to_string()),
        }))
        .await
        .expect("RPC failed");

    // Print the response message
    println!("Response message (JSON): {:?}", res.into_message().message);
}
