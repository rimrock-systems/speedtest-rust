use std::fs::File;
use std::io::{Error};
use std::pin::Pin;
use std::task::{Context, Poll};
use serde_json::Value;
use tokio::io::{split, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use std::io::Write;
use indicatif::{ProgressBar, ProgressStyle};
use log::error;
use crate::http::request::header_parser;
use crate::http::tls::setup_tls_connector;

pub struct HttpClient {
    pub host : String,
    pub path : String,
    pub stream : ClientStream
}

#[derive(Debug)]
pub enum ClientStream {
    Tcp(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
}

impl AsyncRead for ClientStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ClientStream::Tcp(tcp) => Pin::new(tcp).poll_read(cx, buf),
            ClientStream::Tls(tls) => Pin::new(tls).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ClientStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize, Error>> {
        match self.get_mut() {
            ClientStream::Tcp(tcp) => Pin::new(tcp).poll_write(cx, buf),
            ClientStream::Tls(tls) => Pin::new(tls).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match self.get_mut() {
            ClientStream::Tcp(tcp) => Pin::new(tcp).poll_flush(cx),
            ClientStream::Tls(tls) => Pin::new(tls).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match self.get_mut() {
            ClientStream::Tcp(tcp) => Pin::new(tcp).poll_flush(cx),
            ClientStream::Tls(tls) => Pin::new(tls).poll_flush(cx),
        }
    }
}

impl HttpClient {

    pub async fn open(url : &str) -> std::io::Result<Self> {
        let pared_url = Self::parse_url(url)?;
        let tcp_stream = TcpStream::connect(format!("{}:{}",pared_url.1,pared_url.2)).await?;
        let stream = if pared_url.2 == 443 {
            let tls_stream = setup_tls_connector(pared_url.1.clone(),tcp_stream).await;
            ClientStream::Tls(Box::from(tls_stream))
        } else {
            ClientStream::Tcp(tcp_stream)
        };
        Ok(HttpClient {
            host : pared_url.1,
            path : pared_url.3,
            stream
        })
    }

    pub async fn send_request_json(&mut self,packet : &[u8]) -> std::io::Result<Option<Value>> {
        self.stream.write_all(packet).await?;
        let read_data = Self::read_response(&mut self.stream).await?;
        let parser = String::from_utf8_lossy(&read_data);
        let response = parser.trim_matches(char::from(0));
        if response.starts_with("HTTP/1.1 200") {
            let mut split_body = response.splitn(2,"\r\n\r\n");
            let resp_body = split_body.nth(1).unwrap_or("");
            if let Ok(parsed_json_body) = serde_json::from_str::<Value>(resp_body) {
                Ok(Some(parsed_json_body))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    /// Read a whole HTTP response off `stream`.
    ///
    /// A read that returns fewer bytes than the buffer holds is a normal short read, not end
    /// of stream — over TLS it is the common case — so the loop runs until the announced
    /// `Content-Length` is satisfied, or until a genuine EOF (`read == 0`) for a response that
    /// does not announce one. Treating a short read as EOF truncated every response that
    /// arrived in more than one segment, and the truncated body then failed to parse.
    async fn read_response<R>(stream : &mut R) -> std::io::Result<Vec<u8>>
    where
        R : AsyncReadExt + Unpin
    {
        let mut read_data = Vec::new();
        let mut buffer = [0u8; 1024];
        let mut body_start = None;
        let mut content_length = None;
        loop {
            let read = stream.read(&mut buffer).await?;
            if read == 0 { //EOF
                break;
            }
            //only the bytes actually read: the rest of the buffer is padding
            read_data.extend_from_slice(&buffer[..read]);
            if body_start.is_none() {
                body_start = find_subslice(&read_data,b"\r\n\r\n").map(|at| at + 4);
                if let Some(body_start) = body_start {
                    content_length = parse_content_length(&read_data[..body_start]);
                }
            }
            if let (Some(body_start),Some(content_length)) = (body_start,content_length) {
                if read_data.len() >= body_start + content_length {
                    break;
                }
            }
        }
        Ok(read_data)
    }

    pub async fn download_file(&mut self,file_name : &str) {
        let request = format!("GET /{} HTTP/1.1\r\n\
        accept-encoding: gzip, deflate, br, zstd\r\n\
        Host: {}\r\n\r\n",self.path,self.host);
        self.stream.write_all(request.as_bytes()).await.unwrap();
        let (socket_r, _) = split(&mut self.stream);
        let mut buf_reader = BufReader::with_capacity(8 * 1024, socket_r);
        Self::download_stream(&mut buf_reader,file_name).await
    }

    async fn download_stream<R>(buf_reader: &mut BufReader<R>,file_name : &str)
    where
        R : AsyncReadExt + Unpin
    {
        //read status line
        if let Ok(Some(status_line)) = buf_reader.lines().next_line().await {
            if status_line.to_lowercase().contains("200 ok") {
                //headers
                let parsed_headers = header_parser(buf_reader).await;
                //read body
                let body_len = parsed_headers.get("Content-Length");
                if let Some(body_len) = body_len {
                    let body_len = body_len.parse::<usize>().unwrap();
                    let pb = ProgressBar::new(body_len as u64);
                    pb.set_style(ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                        .unwrap()
                        .progress_chars("#>-"));
                    let mut file = File::create(file_name).unwrap();
                    let mut buffer = [0; 1024];
                    let mut read_buff = 0;
                    'body_loop:loop {
                        let n = match buf_reader.read(&mut buffer).await {
                            Ok(n) => n,
                            Err(e) => {
                                pb.abandon();
                                error!("Download failed : {e}");
                                return
                            }
                        };
                        //a read of 0 is end of stream. Without this the loop spins at 100% CPU
                        //forever whenever the connection ends before `Content-Length` is
                        //reached, because `read_buff` stops advancing but never reaches it.
                        if n == 0 {
                            pb.abandon();
                            error!("Download ended after {read_buff} of {body_len} bytes");
                            return
                        }
                        read_buff += n;
                        pb.set_position(read_buff as u64);
                        if let Err(e) = file.write_all(&buffer[..n]) {
                            pb.abandon();
                            error!("Download write failed : {e}");
                            return
                        }
                        if read_buff >= body_len {
                            break 'body_loop;
                        }
                    }
                    pb.finish_with_message("Download completed");
                }
            }
        }
    }

    fn parse_url(url: &str) -> std::io::Result<(String,String,i32,String)> {
        if let Some((scheme, rest)) = url.split_once("://") {
            let (host, path) = if rest.contains('/') {
                rest.split_once('/').unwrap()
            } else {
                (rest,"")
            };
            let port = if scheme == "https" { 443 } else { 80 };
            Ok((scheme.to_string(),host.to_string(),port,path.to_string()))
        } else {
            Err(Error::other("Error parsing input url"))
        }
    }

}

fn find_subslice(haystack : &[u8],needle : &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None
    }
    (0..=haystack.len() - needle.len()).find(|&at| &haystack[at..at + needle.len()] == needle)
}

fn parse_content_length(headers : &[u8]) -> Option<usize> {
    String::from_utf8_lossy(headers)
        .lines()
        .find_map(|line| {
            let (key,value) = line.split_once(':')?;
            if key.trim().eq_ignore_ascii_case("Content-Length") {
                value.trim().parse::<usize>().ok()
            } else {
                None
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, AsyncWriteExt};

    async fn read_response_from(segments : &[&[u8]]) -> Vec<u8> {
        let (mut client,mut server) = duplex(64);
        let owned : Vec<Vec<u8>> = segments.iter().map(|s| s.to_vec()).collect();
        let writer = tokio::spawn(async move {
            for segment in owned {
                server.write_all(&segment).await.unwrap();
            }
            server.shutdown().await.unwrap();
        });
        let read = HttpClient::read_response(&mut client).await.unwrap();
        writer.await.unwrap();
        read
    }

    /// A response that arrives in several segments — the normal case over TLS — must be
    /// assembled whole. The old loop treated any read shorter than its buffer as end of
    /// stream, so it cut the response short and the JSON body then failed to parse, silently
    /// disabling the ipinfo lookup.
    #[tokio::test]
    async fn multi_segment_response_is_assembled_without_truncation() {
        let body = format!("{{\"org\":\"AS64500 {}\",\"country\":\"US\"}}","x".repeat(4096));
        let head = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",body.len());
        let read = read_response_from(&[head.as_bytes(),body.as_bytes()]).await;
        let text = String::from_utf8(read).unwrap();
        assert!(text.starts_with("HTTP/1.1 200"));
        let (_,read_body) = text.split_once("\r\n\r\n").unwrap();
        assert_eq!(read_body,body,"the whole body must be read, not the first segment of it");
    }

    /// The read must not append the unused tail of its buffer as trailing NUL bytes.
    #[tokio::test]
    async fn short_reads_do_not_pad_the_response() {
        let body = "{\"country\":\"US\"}";
        let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",body.len());
        let read = read_response_from(&[head.as_bytes(),body.as_bytes()]).await;
        assert_eq!(read.len(),head.len() + body.len());
        assert!(!read.contains(&0),"no NUL padding may be appended");
    }

    /// A response without `Content-Length` is read to genuine EOF.
    #[tokio::test]
    async fn response_without_content_length_is_read_to_eof() {
        let head = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n";
        let body = b"{\"country\":\"US\"}";
        let read = read_response_from(&[head,body]).await;
        assert_eq!(read,[head.as_slice(),body.as_slice()].concat());
    }

    /// A download that ends before `Content-Length` must stop. The old loop only broke once
    /// the running count reached `body_len`, so an EOF short of that left it spinning at 100%
    /// CPU forever. If this regresses the test does not fail, it hangs.
    #[tokio::test]
    async fn truncated_download_stops_instead_of_spinning() {
        let mut response = Vec::new();
        response.extend_from_slice(b"HTTP/1.1 200 OK\r\nContent-Length: 4096\r\n\r\n");
        response.extend_from_slice(&vec![b'x'; 512]); //far short of the announced length
        let mut buf_reader = BufReader::new(std::io::Cursor::new(response));
        let file_name = std::env::temp_dir().join("librespeed-rs-truncated-download.bin");
        HttpClient::download_stream(&mut buf_reader,file_name.to_str().unwrap()).await;
        assert_eq!(
            std::fs::metadata(&file_name).unwrap().len(),512,
            "what did arrive is written, and the loop stops at EOF"
        );
        let _ = std::fs::remove_file(&file_name);
    }

    #[test]
    fn content_length_is_parsed_case_insensitively() {
        assert_eq!(parse_content_length(b"HTTP/1.1 200 OK\r\ncontent-length: 42\r\n\r\n"),Some(42));
        assert_eq!(parse_content_length(b"HTTP/1.1 200 OK\r\nContent-Length:7\r\n\r\n"),Some(7));
        assert_eq!(parse_content_length(b"HTTP/1.1 200 OK\r\n\r\n"),None);
    }
}
