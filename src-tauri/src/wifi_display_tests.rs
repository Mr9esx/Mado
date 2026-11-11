use super::*;
use rustls::{server::WebPkiClientVerifier, ServerConfig, ServerConnection};
use std::{
    io::{Read, Write},
    net::TcpListener,
};

#[test]
fn wifi_tls_requires_the_provisioned_identity_on_both_ends() {
    let provisioned = Provisioning::new().unwrap();
    let unrelated = Provisioning::new().unwrap();
    let mut roots = RootCertStore::empty();
    roots.add(provisioned.ca.der().clone()).unwrap();
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .unwrap();
    let config = Arc::new(
        ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_client_cert_verifier(verifier)
            .with_single_cert(
                vec![provisioned.certificate.der().clone()],
                PrivatePkcs8KeyDer::from(provisioned.key.serialize_der()).into(),
            )
            .unwrap(),
    );
    for (credentials, authenticated) in [
        (provisioned.client.clone(), true),
        (unrelated.client.clone(), false),
        (
            Credentials {
                ca: provisioned.client.ca.clone(),
                ..unrelated.client
            },
            false,
        ),
    ] {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let config = config.clone();
        let server = thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(NETWORK_TIMEOUT)).unwrap();
            let mut tls = StreamOwned::new(ServerConnection::new(config).unwrap(), socket);
            let mut input = [0; 4];
            tls.read_exact(&mut input)
                .and_then(|()| tls.write_all(&input))
        });
        let exchange = open_socket(address, &credentials).and_then(|mut tls| {
            tls.write_all(b"mado").map_err(|e| e.to_string())?;
            let mut answer = [0; 4];
            tls.read_exact(&mut answer).map_err(|e| e.to_string())?;
            assert_eq!(&answer, b"mado");
            Ok(())
        });
        assert_eq!(exchange.is_ok(), authenticated);
        assert_eq!(server.join().unwrap().is_ok(), authenticated);
    }
}

#[test]
fn keeps_the_device_confirmed_pairing_when_starting_another_provision() {
    let joined = Provisioning::new().unwrap();
    let request = Uuid::new_v4();
    let mut stored = Stored {
        active: None,
        pending: Some(joined.client.clone()),
        pending_id: Some(request),
    };
    stored.reconcile(true, Some(Uuid::new_v4()));
    assert!(stored.active.is_none());
    stored.reconcile(true, Some(request));
    assert!(stored.pending.is_none());
    assert_eq!(stored.active.unwrap().ca, joined.client.ca);
}
