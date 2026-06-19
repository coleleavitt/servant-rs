use servant::prelude::*;
use servant_server::serve;

fn main() {
    let api = path("users", capture::<u64, _>("id", get::<(Json,), String>()));

    // The handler must accept the capture produced by the API description.
    let _router = serve(api, || async { Ok::<_, ServerError>("missing id".to_string()) });
}