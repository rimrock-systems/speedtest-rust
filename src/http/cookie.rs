use std::collections::HashMap;
use std::sync::OnceLock;
use chrono::Utc;
use log::{error, warn};
use sha2::{Digest, Sha256};
use crate::config::SERVER_CONFIG;
use crate::database::generate_uuid;

const ID_PREFIX : &str = "_session";
const COOKIE_MA : i64 = 3600;

// Environment variable that supplies the stats session signing key, overriding
// `stats_secret_key` in configs.toml. There is deliberately no CLI flag for it:
// argv is world-readable through `ps`, so a flag would leak the key to every
// local user on the host.
const SECRET_ENV : &str = "LIBRESPEED_STATS_SECRET";

// The signing key that used to be hardcoded in this file. It is published in the
// git history of this repository and every fork of it, so it is retained here for
// exactly one purpose: to be recognised and refused. A deployment that supplies it
// is no better protected than one that supplies nothing at all.
const BURNED_SECRET : &str = "BxKoUjZB6Pg6ymaeITnXudFa3QwNS1yz7JveTb8IGzFGh1xy98QdUChTy4K7P5Of";

static SECRET_KEY : OnceLock<String> = OnceLock::new();

/// Resolve the signing key once per process.
/// Precedence: `LIBRESPEED_STATS_SECRET` env > `stats_secret_key` in configs.toml
/// > a random key generated at startup.
fn secret_key() -> &'static str {
    SECRET_KEY.get_or_init(|| {
        let server_config = SERVER_CONFIG.get();
        let supplied = std::env::var(SECRET_ENV).ok()
            .or_else(|| server_config.map(|config| config.stats_secret_key.clone()));
        let stats_password_set = server_config.is_some_and(|config| !config.stats_password.is_empty());
        resolve_secret(supplied, stats_password_set)
    })
}

fn resolve_secret(supplied : Option<String>, stats_password_set : bool) -> String {
    match supplied.filter(|secret| !secret.is_empty()) {
        Some(secret) if secret == BURNED_SECRET => {
            error!("Configured stats secret key is the burned key that shipped hardcoded in this project. \
                    Refusing it and using an ephemeral key instead — set {} to a fresh random value.",SECRET_ENV);
            generate_secret()
        }
        Some(secret) => secret,
        None => {
            if stats_password_set {
                warn!("No stats secret key configured; generating an ephemeral one. \
                       Stats sessions will be invalidated on every restart. \
                       Set {} or stats_secret_key in configs.toml to keep sessions across restarts.",SECRET_ENV);
            }
            generate_secret()
        }
    }
}

/// Random key for deployments that supply none. `uuid` v4 is CSPRNG-backed, so
/// three of them carry well over 256 bits of entropy; the hash folds them into a
/// fixed-width key. This is the fail-safe default: an unconfigured deployment gets
/// an unguessable key, never a constant.
fn generate_secret() -> String {
    sha256_hash(&format!("{}{}{}",generate_uuid(),generate_uuid(),generate_uuid()))
}

fn sign(cookie_id : &str, expires_at : i64) -> String {
    sha256_hash(&format!("{}{}{}",cookie_id,expires_at,secret_key()))
}

/// Byte-wise constant-time equality, so a mismatching signature costs the same
/// time wherever it diverges. Hand-rolled to keep the dependency set unchanged.
fn constant_time_eq(left : &[u8], right : &[u8]) -> bool {
    if left.len() != right.len() {
        return false
    }
    let mut diff = 0u8;
    for (l,r) in left.iter().zip(right.iter()) {
        diff |= l ^ r;
    }
    diff == 0
}

pub fn make_cookie(path : &str) -> String {
    let cookie_id = format!("{}{}",ID_PREFIX,generate_uuid());
    let expires_at = Utc::now().timestamp() + COOKIE_MA;
    let cookie_sign = sign(&cookie_id,expires_at);
    format!("token={},{},{}; Path={}; Max-Age={}; HttpOnly; SameSite=Strict",cookie_id,expires_at,cookie_sign,path,COOKIE_MA)
}

pub fn make_discard_cookie (path : &str) -> String {
    format!("token=deleted; path={}; expires=Thu, 01 Jan 1970 00:00:00 GMT",path)
}

pub fn validate_cookie(cookie_data : Option<&String>) -> bool {
    if let Some(cookie_data) = cookie_data {
        // A Cookie header segment without '=' is malformed but perfectly sendable,
        // so skip those rather than indexing past the end of the segment.
        let cookie_parts : HashMap<&str,&str> = cookie_data.split(';').filter_map(|s| s.split_once('=')).map(|(key, val)| (key.trim(), val)).collect();
        let cookie_token = cookie_parts.get("token").unwrap_or(&"");
        let mut split_token = cookie_token.splitn(3,',');
        let cookie_id = split_token.next().unwrap_or("");
        let expires_at = split_token.next().unwrap_or("");
        let signature = split_token.next().unwrap_or("");
        // The expiry is part of the signed payload, so a captured cookie cannot outlive
        // it: Max-Age alone is enforced by the client and means nothing to the server.
        let Ok(expires_at) = expires_at.parse::<i64>() else {
            return false
        };
        if expires_at <= Utc::now().timestamp() {
            return false
        }
        constant_time_eq(signature.as_bytes(),sign(cookie_id,expires_at).as_bytes())
    } else {
        false
    }
}

fn sha256_hash(input : &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    let hash_string = format!("{:x}",result);
    hash_string
}

#[cfg(test)]
mod tests {
    use super::*;

    // `make_cookie` builds a Set-Cookie value; a request sends back only the pair.
    fn request_header(set_cookie : &str) -> String {
        let token = set_cookie.split(';').next().unwrap();
        format!("locale=en; {}",token)
    }

    #[test]
    fn round_trip_cookie_validates() {
        let header = request_header(&make_cookie("/backend/stats"));
        assert!(validate_cookie(Some(&header)));
    }

    #[test]
    fn forged_signature_is_rejected() {
        let expires_at = Utc::now().timestamp() + COOKIE_MA;
        let header = format!("token=_sessionforged,{},{}",expires_at,sha256_hash("not the secret"));
        assert!(!validate_cookie(Some(&header)));
    }

    #[test]
    fn expired_cookie_is_rejected() {
        let cookie_id = "_sessionexpired";
        let expired_at = Utc::now().timestamp() - 1;
        let header = format!("token={},{},{}",cookie_id,expired_at,sign(cookie_id,expired_at));
        assert!(!validate_cookie(Some(&header)));

        // control: the same id and signing path, still in date
        let valid_until = Utc::now().timestamp() + COOKIE_MA;
        let header = format!("token={},{},{}",cookie_id,valid_until,sign(cookie_id,valid_until));
        assert!(validate_cookie(Some(&header)));
    }

    #[test]
    fn unparseable_expiry_is_rejected() {
        let header = format!("token=_sessionbad,soon,{}",sign("_sessionbad",0));
        assert!(!validate_cookie(Some(&header)));
    }

    #[test]
    fn malformed_cookie_header_is_rejected_not_fatal() {
        for header in ["junk","junk; token=nonsense","","; ;",";=;"] {
            assert!(!validate_cookie(Some(&header.to_string())),"header {:?} should be rejected",header);
        }
        // a well formed cookie still validates when a malformed segment precedes it
        let token = make_cookie("/backend/stats").split(';').next().unwrap().to_string();
        let header = format!("junk; {}",token);
        assert!(validate_cookie(Some(&header)));
    }

    #[test]
    fn burned_secret_is_never_used() {
        let resolved = resolve_secret(Some(BURNED_SECRET.to_string()),true);
        assert_ne!(resolved,BURNED_SECRET);
        assert_eq!(resolved.len(),64);
    }

    #[test]
    fn missing_secret_generates_a_random_one() {
        let first = resolve_secret(None,true);
        let second = resolve_secret(Some(String::new()),false);
        assert_ne!(first,BURNED_SECRET);
        assert_ne!(first,second);
    }

    #[test]
    fn supplied_secret_is_used_verbatim() {
        assert_eq!(resolve_secret(Some("a-rotated-key".to_string()),true),"a-rotated-key");
    }
}
