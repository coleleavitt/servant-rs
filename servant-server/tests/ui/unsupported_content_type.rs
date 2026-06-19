use servant::prelude::*;
use servant_server::serve;

struct NotAMime;

fn main() {
    // Content-type lists must be built from supported MIME marker types before
    // an API can be interpreted as a server.
    let _router = serve(get::<(NotAMime,), String>(), || async {
        Ok::<_, ServerError>("ok".to_string())
    });
}