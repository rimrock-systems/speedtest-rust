use std::net::IpAddr;
use crate::ip::mmdb::mmdb_record::MMDBResult;
use log::warn;
use maxminddb::Reader;

pub struct MMDBReader {
    reader: Reader<Vec<u8>>,
}

impl MMDBReader {
    pub fn from(path: &str) -> Option<Self> {
        if let Ok(custom_reader) = maxminddb::Reader::open_readfile(path) {
            Some(MMDBReader {
                reader: custom_reader,
            })
        } else {
            None
        }
    }

    pub fn lookup(&mut self, address: &str) -> Option<MMDBResult> {
        //the address reaches here from the request's remote address, which may have been
        //taken from a client-supplied forwarding header and need not be an IP address at all
        let Ok(address) = address.parse::<IpAddr>() else {
            warn!("Geo IP lookup for a value that is not an IP address");
            return None
        };
        match self.reader.lookup(address) {
            Err(e) => {
                warn!("Geo IP database error: {}", e);
                None
            }
            Ok(o) => {
                if let Ok(Some(result)) = o.decode() {
                    Some(result)
                } else {
                    warn!("Failed to deserialise Geo IP data {:?}", o);
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The looked-up address originates in the client-controlled `X-Real-IP` /
    /// `X-Forwarded-For` headers, so it may be any string at all. Parsing it with `.unwrap()`
    /// panicked the request handler for anything that is not an IP address.
    #[test]
    fn non_ip_address_returns_none_instead_of_panicking() {
        let Some(mut reader) = MMDBReader::from("country_asn.mmdb") else {
            return //no database shipped alongside the test run
        };
        for address in ["", "notanip", "1.2.3", "999.999.999.999", "127.0.0.1:8080", "../etc"] {
            assert!(
                reader.lookup(address).is_none(),
                "lookup of {address:?} must return None"
            );
        }
    }

    /// A real address still resolves.
    #[test]
    fn valid_address_still_looks_up() {
        let Some(mut reader) = MMDBReader::from("country_asn.mmdb") else {
            return
        };
        assert!(reader.lookup("8.8.8.8").is_some(),"a valid public address must still resolve");
    }
}
