use servant::prelude::*;
use servant_client::client;

fn main() {
    let api = path("users", capture::<u64, _>("id", get::<(Json,), String>()));
    let endpoint = client(api);

    // The generated client consumes the same capture type as the server handler.
    let _future = endpoint.call(&(), servant::hlist!["not-a-u64".to_string()]);
}