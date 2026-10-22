//! Exercise the patched XML parser through object_store's public HTTP API.
use futures_util::TryStreamExt;
use object_store::{ClientOptions, ObjectStore, http::HttpBuilder};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

async fn list_response(xml: &'static str) -> object_store::Result<Vec<object_store::ObjectMeta>> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "HTTP client made no request");
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("{e}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut buf = [0; 1024];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0);
            request.extend_from_slice(&buf[..n]);
        }
        assert!(request.starts_with(b"PROPFIND "));
        write!(stream, "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", xml.len(), xml).unwrap();
    });
    let store = HttpBuilder::new()
        .with_url(format!("http://{address}/"))
        .with_client_options(ClientOptions::new().with_allow_http(true))
        .build()
        .unwrap();
    let result = store.list(None).try_collect().await;
    server.join().unwrap();
    result
}

#[tokio::test]
async fn namespaced_webdav_listing_preserves_metadata() {
    let rows = list_response(
        r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:"><d:response><d:href>/fixture.csv</d:href>
<d:propstat><d:prop><d:getlastmodified>Wed, 21 Oct 2015 07:28:00 GMT</d:getlastmodified>
<d:getcontentlength>42</d:getcontentlength><d:resourcetype/>
<d:getetag>fixture-etag</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status>
</d:propstat></d:response></d:multistatus>"#,
    )
    .await
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].location.as_ref(), "fixture.csv");
    assert_eq!(rows[0].size, 42);
    assert_eq!(rows[0].e_tag.as_deref(), Some("fixture-etag"));
}

#[tokio::test]
async fn malformed_xml_is_rejected() {
    assert!(
        list_response("<multistatus><response></multistatus>")
            .await
            .is_err()
    );
}
