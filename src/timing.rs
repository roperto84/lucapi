//! Connection phase probe: DNS, TCP and TLS timings.
//!
//! `reqwest` resolves, connects and handshakes internally and does not expose
//! how long each step took, so we measure them on a side connection opened in
//! parallel with the real request. The numbers are real measurements of the
//! same host and port, not guesses.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls_platform_verifier::BuilderVerifierExt;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

#[derive(Clone, Copy, Debug, Default)]
pub struct Phases {
    pub dns_ms: Option<u64>,
    pub connect_ms: Option<u64>,
    pub tls_ms: Option<u64>,
}

/// Measures the phases needed to reach `host:port`.
pub async fn probe(host: &str, port: u16, https: bool, timeout: Duration) -> Phases {
    let mut phases = Phases::default();
    let deadline = Instant::now() + timeout;
    let remaining = || {
        deadline
            .saturating_duration_since(Instant::now())
            .max(Duration::from_millis(50))
    };

    let candidates: Vec<SocketAddr> = match host.parse::<IpAddr>() {
        Ok(ip) => {
            phases.dns_ms = Some(0);
            vec![SocketAddr::new(ip, port)]
        }
        Err(_) => {
            let started = Instant::now();
            let lookup = tokio::net::lookup_host((host, port));
            match tokio::time::timeout(remaining(), lookup).await {
                Ok(Ok(addresses)) => {
                    phases.dns_ms = Some(started.elapsed().as_millis() as u64);
                    addresses.collect()
                }
                _ => return phases,
            }
        }
    };
    if candidates.is_empty() {
        return phases;
    }

    let started = Instant::now();
    let mut stream: Option<TcpStream> = None;
    for address in &candidates {
        match tokio::time::timeout(remaining(), TcpStream::connect(address)).await {
            Ok(Ok(connection)) => {
                stream = Some(connection);
                break;
            }
            _ => continue,
        }
    }
    let Some(stream) = stream else {
        return phases;
    };
    phases.connect_ms = Some(started.elapsed().as_millis() as u64);

    if !https {
        return phases;
    }

    let config = match rustls::ClientConfig::builder().with_platform_verifier() {
        Ok(builder) => builder.with_no_client_auth(),
        Err(_) => return phases,
    };
    let Ok(server_name) = rustls::pki_types::ServerName::try_from(host.to_string()) else {
        return phases;
    };
    let connector = TlsConnector::from(Arc::new(config));
    let started = Instant::now();
    if let Ok(Ok(tls)) = tokio::time::timeout(remaining(), connector.connect(server_name, stream))
        .await
    {
        phases.tls_ms = Some(started.elapsed().as_millis() as u64);
        drop(tls);
    }
    phases
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn measures_dns_and_tcp_for_a_local_host() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("free port");
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 16];
                let _ = stream.read(&mut buffer);
            }
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let phases = runtime.block_on(probe(
            "127.0.0.1",
            port,
            false,
            Duration::from_secs(3),
        ));
        server.join().unwrap();
        assert_eq!(phases.dns_ms, Some(0));
        assert!(phases.connect_ms.is_some());
        assert!(phases.tls_ms.is_none());
    }

    #[test]
    fn a_closed_port_reports_no_connect_time() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("free port");
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let phases = runtime.block_on(probe(
            "127.0.0.1",
            port,
            false,
            Duration::from_millis(600),
        ));
        assert!(phases.connect_ms.is_none());
    }
}
