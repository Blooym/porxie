use anyhow::bail;
use core::str::FromStr;

#[derive(Debug, Clone)]
pub enum SocketAddress {
    /// An IP socket address.
    Ip(std::net::SocketAddr),

    /// A UNIX socket path.
    #[cfg(unix)]
    Unix(std::path::PathBuf),
}

impl FromStr for SocketAddress {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        #[cfg(unix)]
        if let Some(path) = s.strip_prefix("unix:") {
            if path.ends_with("/") {
                bail!("unix socket path cannot be a directory")
            }
            return Ok(SocketAddress::Unix(std::path::PathBuf::from(path)));
        }
        if let Some(ip) = s.strip_prefix("ip:") {
            return Ok(ip.parse::<std::net::SocketAddr>().map(SocketAddress::Ip)?);
        }

        #[cfg(unix)]
        bail!("unknown address binding type, expected 'ip:<addr>' or 'unix:<path>'".to_string(),);
        #[cfg(not(unix))]
        bail!("unknown address binding type, expected 'ip:<addr>'".to_string());
    }
}

impl From<std::net::SocketAddr> for SocketAddress {
    fn from(value: std::net::SocketAddr) -> Self {
        Self::Ip(value)
    }
}

#[cfg(unix)]
impl From<std::path::PathBuf> for SocketAddress {
    fn from(value: std::path::PathBuf) -> Self {
        Self::Unix(value)
    }
}

#[cfg(unix)]
impl From<&std::path::Path> for SocketAddress {
    fn from(value: &std::path::Path) -> Self {
        Self::Unix(value.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use crate::server::socket_address::SocketAddress;
    use core::str::FromStr;

    #[test]
    fn parse_ip_address() {
        assert!(SocketAddress::from_str("ip:127.0.0.1:3000").is_ok());
        assert!(SocketAddress::from_str("ip:1.1.1.1:80").is_ok());
        assert!(SocketAddress::from_str("ip:1.1.1.1").is_err());
        assert!(SocketAddress::from_str("ip:1.1.1.1.2").is_err());
    }

    #[test]
    #[cfg(unix)]
    fn parse_unix_path() {
        assert!(SocketAddress::from_str("unix:/run/porxie/porxie.sock").is_ok());
        assert!(SocketAddress::from_str("unix:/just/a/directory/").is_err());
    }
}
