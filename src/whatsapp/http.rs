//! Async buffered transfers allow an in-flight image upload to be canceled.
//! Upstream history downloads retain its streaming implementation.
use std::time::Duration;
use whatsapp_rust::{
    http::{HttpClient, HttpRequest, HttpResponse, UreqHttpClient},
    wacore::net::StreamingHttpResponse,
};
pub(super) struct Http {
    client: reqwest::Client,
    streaming: UreqHttpClient,
}
impl Http {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::limited(3))
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(60))
                .pool_max_idle_per_host(2)
                .build()?,
            streaming: UreqHttpClient::new(),
        })
    }
}
#[async_trait::async_trait]
impl HttpClient for Http {
    async fn execute(&self, request: HttpRequest) -> anyhow::Result<HttpResponse> {
        let mut builder = self.client.request(request.method.parse()?, request.url);
        for (key, value) in request.headers {
            builder = builder.header(key, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        let mut response = builder.send().await.map_err(reqwest::Error::without_url)?;
        let status_code = response.status().as_u16();
        let success = response.status().is_success();
        let limit = if success { 64 * 1024 * 1024 } else { 64 * 1024 };
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(reqwest::Error::without_url)?
        {
            if body.len() + chunk.len() > limit {
                if success {
                    anyhow::bail!("HTTP response exceeds the memory limit");
                }
                body.extend_from_slice(&chunk[..limit - body.len()]);
                break;
            }
            body.extend_from_slice(&chunk);
        }
        Ok(HttpResponse { status_code, body })
    }
    fn supports_streaming(&self) -> bool {
        true
    }
    fn execute_streaming(&self, request: HttpRequest) -> anyhow::Result<StreamingHttpResponse> {
        self.streaming.execute_streaming(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[test]
    fn history_streaming_retains_support_above_64_mib() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/history", listener.local_addr().unwrap());
        let length = 64 * 1024 * 1024 + 1;
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = [0; 1024];
            assert!(stream.read(&mut request).unwrap() > 0);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            // Fixed-size buffers: this tests streaming without allocating a history blob.
            let _ = std::io::copy(&mut std::io::repeat(42).take(length), &mut stream);
        });
        let mut response = Http::new()
            .unwrap()
            .execute_streaming(HttpRequest::get(url))
            .unwrap();
        let copied = std::io::copy(&mut response.body, &mut std::io::sink()).unwrap();
        drop(response);
        server.join().unwrap();
        assert_eq!(copied, length);
    }
    #[tokio::test]
    async fn http_preserves_rejection_status_and_cancels_waiting_upload() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/upload", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            assert!(String::from_utf8_lossy(&buf[..n]).contains("POST /upload"));
            stream
                .write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 5\r\n\r\nstale")
                .await
                .unwrap();
            let (_stalled, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let http = Http {
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            streaming: UreqHttpClient::new(),
        };
        let response = http
            .execute(HttpRequest::post(&url).with_body(vec![1, 2, 3]))
            .await
            .unwrap();
        assert_eq!(response.status_code, 401);
        assert_eq!(response.body, b"stale");
        assert!(
            tokio::time::timeout(
                Duration::from_millis(50),
                http.execute(HttpRequest::post(url).with_body(vec![4, 5]))
            )
            .await
            .is_err()
        );
        server.abort();
    }
}
