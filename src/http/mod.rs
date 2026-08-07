use std::collections::HashMap;
use tokio::net::TcpStream;
use std::fs::File;
use std::io::Read;
use crate::config::{DEF_ASSETS, SERVER_CONFIG};

pub mod http_server;
mod routes;
pub mod request;
pub mod response;
pub mod cookie;
pub mod tls;
pub mod http_client;
mod tcp_socket;

#[derive(Debug)]
pub enum Method {
    Get,
    Post,
}

pub trait MethodStr {
    fn to_method(&self) -> Method;
}

impl MethodStr for str {
    fn to_method(&self) -> Method {
        match self {
            "GET" => Method::Get,
            "POST" => Method::Post,
            _ => Method::Get
        }
    }
}

pub async fn find_remote_ip_addr (conn: &mut TcpStream) -> String {
    let client_addr = conn.peer_addr().unwrap();
    client_addr.ip().to_string().replace("::ffff:","")
}

pub fn get_index_file_content(file_name : &str) -> Option<Vec<u8>> {
    if SERVER_CONFIG.get()?.assets_path.is_empty() {
        if file_name.contains("servers_list.js") {
            Some(generate_server_endpoint())
        } else {
            let file_name = &file_name[1..];
            let file = DEF_ASSETS.get_file(file_name)?;
            Some(Vec::from(file.contents()))
        }
    } else {
        let file_path = format!("{}{}",SERVER_CONFIG.get()?.assets_path,file_name);
        if let Ok(mut file) = File::open(file_path) {
            let mut file_bytes = Vec::new();
            if file.read_to_end(&mut file_bytes).is_ok() {
                Some(file_bytes)
            } else {
                None
            }
        } else {
            None
        }
    }
}

fn generate_server_endpoint() -> Vec<u8> {
    let base_url = SERVER_CONFIG.get().unwrap().base_url.clone();
    let base_url = if base_url.is_empty() {
        "".to_string()
    } else {
        format!("{}/",&base_url[1..])
    };
    let endpoint = format!(r#"function get_servers() {{
        return [
            {{
                name : "Simple Server",
                server : window.location.origin,
                dlURL: "{base_url}garbage",
                ulURL: "{base_url}empty",
                pingURL: "{base_url}empty",
                getIpURL: "{base_url}getIP"
            }}
        ]
    }}"#);
    Vec::from(endpoint.as_bytes())
}


pub fn get_chunk_count (query_params : &HashMap<String,String>) -> i32 {
    let mut chunks = 4;
    if let Some(ck_size) = query_params.get("ckSize") {
        if let Ok(parsed_ck_size) = ck_size.parse::<i32>() {
            //`ckSize` is client-supplied: clamp it at both ends. Without the lower bound a
            //zero or negative value yields a chunk count of zero, and the response is sent
            //with chunked headers and no body at all.
            chunks = parsed_ck_size.clamp(1,1024)
        }
    }
    chunks *= 2;
    chunks
}

#[macro_export]
macro_rules! make_route {
    ($a:expr) => {
        {
            let base_url = SERVER_CONFIG.get().unwrap().base_url.clone();
            format!("{}{}",base_url,$a)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ck_size(value : &str) -> i32 {
        let mut query_params = HashMap::new();
        query_params.insert("ckSize".to_string(),value.to_string());
        get_chunk_count(&query_params)
    }

    /// `ckSize` is client-supplied and must be clamped at both ends. Zero and negative values
    /// used to yield a chunk count of zero or less, producing a chunked response with no body.
    #[test]
    fn ck_size_is_clamped_to_at_least_one_chunk() {
        assert!(ck_size("0") > 0,"ckSize=0 must still produce chunks");
        assert!(ck_size("-1") > 0,"a negative ckSize must still produce chunks");
        assert!(ck_size("-2147483648") > 0,"i32::MIN must not underflow into a negative count");
    }

    /// The existing upper bound and the default are unchanged.
    #[test]
    fn ck_size_bounds_are_unchanged() {
        assert_eq!(ck_size("100"),200);
        assert_eq!(ck_size("1024"),2048);
        assert_eq!(ck_size("99999"),2048);
        assert_eq!(get_chunk_count(&HashMap::new()),8);
        assert_eq!(ck_size("not-a-number"),8);
    }
}
