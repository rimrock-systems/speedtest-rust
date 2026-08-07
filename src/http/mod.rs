use std::collections::HashMap;
use tokio::net::TcpStream;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
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
        let file_path = resolve_asset_path(&SERVER_CONFIG.get()?.assets_path,file_name)?;
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

/// Resolve a request path against the configured assets directory, returning `None` for
/// anything that does not stay inside it.
///
/// The request path is attacker-controlled and is not normalised anywhere upstream of here,
/// so joining it onto the assets directory lets `..` segments walk out of that directory and
/// read files elsewhere on the host. Both sides are canonicalised and compared, which is the
/// check that actually holds — `res_200_fs`'s extension table is a content-type lookup, not
/// an access control, and it only incidentally narrows what a traversal can reach.
fn resolve_asset_path(assets_path : &str,file_name : &str) -> Option<PathBuf> {
    //belt and braces: a legitimate asset request never contains a `..` segment
    if Path::new(file_name).components().any(|c| matches!(c,Component::ParentDir)) {
        return None
    }
    let assets_root = std::fs::canonicalize(assets_path).ok()?;
    let resolved = std::fs::canonicalize(format!("{}{}",assets_path,file_name)).ok()?;
    if resolved.starts_with(&assets_root) {
        Some(resolved)
    } else {
        None
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
            if parsed_ck_size > 1024 {
                chunks = 1024
            } else {
                chunks = parsed_ck_size
            }
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

    /// A request path that stays inside the assets directory resolves normally.
    #[test]
    fn asset_inside_the_root_resolves() {
        let resolved = resolve_asset_path("./assets","/index.html");
        assert!(resolved.is_some(),"a normal asset request must resolve");
        assert!(resolved.unwrap().ends_with("assets/index.html"));
    }

    /// `..` segments must not walk out of the assets directory. `Cargo.toml` exists one level
    /// above `./assets`, so an unfixed handler resolves and serves it.
    #[test]
    fn traversal_out_of_the_root_is_rejected() {
        for path in [
            "/../Cargo.toml",
            "/../../speedtest-rust/Cargo.toml",
            "/./../Cargo.toml",
            "/subdir/../../Cargo.toml",
        ] {
            assert!(
                resolve_asset_path("./assets",path).is_none(),
                "traversal path {path:?} must be rejected"
            );
        }
    }

    /// A path that does not exist resolves to nothing rather than to something outside.
    #[test]
    fn missing_asset_resolves_to_none() {
        assert!(resolve_asset_path("./assets","/no-such-file.js").is_none());
    }
}
