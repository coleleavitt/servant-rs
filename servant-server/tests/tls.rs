use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
use servant::prelude::*;
use servant_server::{RouterService, RustlsConfig, serve, serve_rustls_listener};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn rustls_listener_sets_secure_connection_info() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let CertifiedTls { cert, key } = certified_tls();
    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert.clone()], key)
        .unwrap();
    let api = path("tls", is_secure(remote_host(get::<(PlainText,), String>())));
    let router = serve(
        api,
        |secure: bool, peer: Option<std::net::SocketAddr>| async move {
            Ok::<_, ServerError>(format!("{secure}/{peer:?}"))
        },
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve_rustls_listener(
        listener,
        RouterService::new(router),
        RustlsConfig::new(Arc::new(server_config)),
    ));

    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert).unwrap();
    let client_config = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let server_name = ServerName::try_from("localhost").unwrap();
    let mut stream = tokio_rustls::TlsConnector::from(Arc::new(client_config))
        .connect(server_name, stream)
        .await
        .unwrap();

    stream
        .write_all(b"GET /tls HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.unwrap();
    let response = String::from_utf8_lossy(&response);

    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains("true/Some(127.0.0.1:"), "{response}");
}

struct CertifiedTls {
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
}

fn certified_tls() -> CertifiedTls {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    CertifiedTls {
        cert: certified.cert.der().clone(),
        key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der())),
    }
}
