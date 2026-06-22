use servant::prelude::*;
use servant_client::client;

fn main() {
    let api = path("resource", with_resource::<u32, _>(get::<(Json,), u32>()));

    let _endpoint = client(api);
}
