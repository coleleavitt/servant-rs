use servant::prelude::*;
use servant_client::client;

fn main() {
    let api = path("vault", vault(get::<(PlainText,), String>()));

    let _endpoint = client(api);
}
