use servant::prelude::*;
use servant_client::client;

fn main() {
    let api = path("peer", remote_host(get::<(PlainText,), String>()));

    let _endpoint = client(api);
}
