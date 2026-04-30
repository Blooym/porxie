use anyhow::bail;
use core::{net::SocketAddr, str::FromStr};

#[cfg(unix)]
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Address {
    /// An IP socket address.
    Ip(SocketAddr),

    /// A UNIX socket path.
    #[cfg(unix)]
    Unix(PathBuf),
}

impl FromStr for Address {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        #[cfg(unix)]
        if let Some(path) = s.strip_prefix("unix:") {
            return Ok(Address::Unix(PathBuf::from(path)));
        }
        if let Some(ip) = s.strip_prefix("ip:") {
            return Ok(ip.parse::<SocketAddr>().map(Address::Ip)?);
        }

        #[cfg(unix)]
        bail!("unknown address binding type, expected 'ip:<addr>' or 'unix:<path>'".to_string(),);
        #[cfg(not(unix))]
        bail!("unknown address binding type, expected 'ip:<addr>'".to_string());
    }
}
