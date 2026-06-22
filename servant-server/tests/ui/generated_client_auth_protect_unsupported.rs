use servant::prelude::*;
use servant_client::client;

struct User;

fn main() {
    let api = path("auth", auth_protect::<User, _>(get::<(PlainText,), String>()));

    let _endpoint = client(api);
}
