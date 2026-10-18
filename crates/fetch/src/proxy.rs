//! The proxy of Options → Network (`menus-and-shortcuts.md`, `data-model.md` §4 `network`):
//! "system" reads `HTTPS_PROXY` and `NO_PROXY` (on every OS; the lower-case names too), "manual"
//! is a host and a port. Only an HTTP proxy is used, and only through `CONNECT`: the TLS session
//! runs end to end inside the tunnel, so the proxy sees the host and port asked for, never the
//! request or the file, and the certificate is still checked against the server's own name. No
//! proxy authentication: a proxy address with a user name or password is refused, and a proxy that
//! asks for one fails the connection with a message saying so.

use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream};
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::transport::{Limits, TransportError};

/// The longest proxy answer to `CONNECT` (status line and headers).
const MAX_ANSWER: usize = 16 * 1024;
/// The longest proxy setting accepted, in bytes.
const MAX_SETTING: usize = 2048;

/// Why a proxy setting cannot be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the proxy setting {setting} is not usable: {why}")]
pub struct ProxyError {
    /// The setting (`HTTPS_PROXY`, or the manual host).
    pub setting: String,
    pub why: String,
}

/// Where connections go: straight to the server, or through an HTTP proxy.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Proxy {
    via: Option<(String, u16)>,
    /// The hosts reached directly (`NO_PROXY`), lower case.
    bypass: Vec<String>,
}

impl Proxy {
    /// No proxy: every connection goes straight to the server.
    pub fn direct() -> Proxy {
        Proxy::default()
    }

    /// The manual setting: every connection goes through `host:port`.
    pub fn manual(host: &str, port: u16) -> Result<Proxy, ProxyError> {
        let bad = |why: &str| ProxyError { setting: "network.proxyHost".into(), why: why.into() };
        if port == 0 {
            return Err(bad("the port is 0"));
        }
        let host = host.trim();
        let host = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
        Ok(Proxy { via: Some((valid_host(host).ok_or_else(|| bad("not a host name or address"))?, port)), bypass: Vec::new() })
    }

    /// The system setting, from this process's environment (`HTTPS_PROXY`, `NO_PROXY`).
    pub fn system() -> Result<Proxy, ProxyError> {
        Proxy::from_vars(|name| std::env::var(name).ok())
    }

    /// The system setting from `var` (the environment, or a test's map): `HTTPS_PROXY` (else
    /// `https_proxy`) as `[http://]host[:port][/]` (port 80 when left out), and `NO_PROXY` (else
    /// `no_proxy`) as a comma-separated list of hosts, domains (`example.com` also covers
    /// `a.example.com`; `.example.com` and `*.example.com` too), addresses, address ranges
    /// (`10.0.0.0/8`) or `*`. Unset or empty: no proxy.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Proxy, ProxyError> {
        let first = |names: [&'static str; 2]| names.into_iter().filter_map(|n| var(n).map(|v| (n, v))).find(|(_, v)| !v.trim().is_empty());
        let Some((name, value)) = first(["HTTPS_PROXY", "https_proxy"]) else { return Ok(Proxy::direct()) };
        let via = parse_proxy_url(&value).map_err(|why| ProxyError { setting: name.to_string(), why: why.to_string() })?;
        let bypass = first(["NO_PROXY", "no_proxy"])
            .map(|(_, v)| v.split(',').map(|e| e.trim().to_ascii_lowercase()).filter(|e| !e.is_empty()).take(1024).collect())
            .unwrap_or_default();
        Ok(Proxy { via: Some(via), bypass })
    }

    /// The proxy to reach `host` through, or `None` to connect directly.
    pub fn route(&self, host: &str) -> Option<(&str, u16)> {
        let (phost, pport) = self.via.as_ref()?;
        if bypassed(&self.bypass, host) { None } else { Some((phost.as_str(), *pport)) }
    }
}

/// `[http://]host[:port][/]`: an HTTP proxy's address, port 80 by default.
fn parse_proxy_url(value: &str) -> Result<(String, u16), &'static str> {
    let v = value.trim();
    if v.len() > MAX_SETTING || v.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("it is too long or contains spaces or control characters");
    }
    let rest = match v.split_once("://") {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("http") => rest,
        Some(_) => return Err("only an http:// proxy is supported"),
        None => v,
    };
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.contains('@') {
        return Err("proxy user names and passwords are not supported");
    }
    if rest.contains(['/', '?', '#']) {
        return Err("a proxy address has no path");
    }
    let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
        let (host, after) = v6.split_once(']').ok_or("unterminated IPv6 address")?;
        let port = match after {
            "" => None,
            p => Some(p.strip_prefix(':').ok_or("bad port")?),
        };
        (host, port)
    } else {
        match rest.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (rest, None),
        }
    };
    let port = match port {
        Some(p) => p.parse::<u16>().ok().filter(|p| *p != 0).ok_or("bad port")?,
        None => 80,
    };
    Ok((valid_host(host).ok_or("not a host name or address")?, port))
}

/// `host` lower-cased when it is a host name or an IP address.
fn valid_host(host: &str) -> Option<String> {
    let ok = !host.is_empty()
        && host.len() <= 253
        && (host.parse::<IpAddr>().is_ok() || host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_')));
    ok.then(|| host.to_ascii_lowercase())
}

/// Whether `NO_PROXY`'s `entries` name `host`.
fn bypassed(entries: &[String], host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let ip = host.parse::<IpAddr>().ok();
    entries.iter().any(|entry| {
        if entry == "*" {
            return true;
        }
        if let Some((net, bits)) = entry.split_once('/') {
            return ip.is_some_and(|ip| in_range(ip, net, bits));
        }
        // `[v6]`, `[v6]:port`, `host:port` → the host part
        let e = if let Some(v6) = entry.strip_prefix('[') {
            v6.split(']').next().unwrap_or_default()
        } else if entry.matches(':').count() == 1 {
            entry.split(':').next().unwrap_or_default()
        } else {
            entry.as_str()
        };
        if let (Some(ip), Ok(eip)) = (ip, e.parse::<IpAddr>()) {
            return ip == eip;
        }
        let domain = e.trim_start_matches("*.").trim_start_matches('.').trim_end_matches('.');
        !domain.is_empty() && (host == domain || host.strip_suffix(domain).is_some_and(|sub| sub.ends_with('.')))
    })
}

/// Whether `ip` is inside `net/bits`.
fn in_range(ip: IpAddr, net: &str, bits: &str) -> bool {
    let (Ok(net), Ok(bits)) = (net.trim_matches(['[', ']']).parse::<IpAddr>(), bits.parse::<u32>()) else { return false };
    let masked = |a: u128, width: u32| if bits == 0 { 0 } else { a >> width.saturating_sub(bits) };
    match (ip, net) {
        (IpAddr::V4(a), IpAddr::V4(n)) if bits <= 32 => masked(u128::from(u32::from(a)), 32) == masked(u128::from(u32::from(n)), 32),
        (IpAddr::V6(a), IpAddr::V6(n)) if bits <= 128 => masked(u128::from(a), 128) == masked(u128::from(n), 128),
        _ => false,
    }
}

/// Asks the proxy on `sock` for a tunnel to `host:port` (`CONNECT`) and reads its answer by
/// `deadline`; on success the socket carries the tunnel.
pub(crate) fn tunnel(sock: &mut TcpStream, host: &str, port: u16, deadline: Instant, limits: &Limits<'_>) -> Result<(), TransportError> {
    let authority = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
    let ask = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n");
    let fail = |why: String| TransportError::Connect(format!("proxy: {why}"));
    let wait = |e: &std::io::Error| -> Result<(), TransportError> {
        if limits.cancel.load(Ordering::Relaxed) {
            return Err(TransportError::Cancelled);
        }
        if !matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted) {
            return Err(fail(e.to_string()));
        }
        if Instant::now() >= deadline {
            return Err(fail("the proxy did not answer in time".into()));
        }
        Ok(())
    };
    let mut rest = ask.as_bytes();
    while !rest.is_empty() {
        match sock.write(rest) {
            Ok(0) => return Err(fail("the proxy closed the connection".into())),
            Ok(n) => rest = rest.get(n..).unwrap_or_default(),
            Err(e) => wait(&e)?,
        }
    }
    // read up to the end of the answer's head; a proxy sends nothing after it before our TLS hello
    let mut answer = Vec::new();
    let mut buf = [0u8; 1024];
    while !answer.ends_with(b"\r\n\r\n") && !answer.ends_with(b"\n\n") {
        if answer.len() > MAX_ANSWER {
            return Err(fail("the proxy's answer is too large".into()));
        }
        if limits.cancel.load(Ordering::Relaxed) {
            return Err(TransportError::Cancelled);
        }
        match sock.read(&mut buf) {
            Ok(0) => return Err(fail("the proxy closed the connection".into())),
            Ok(n) => {
                answer.extend_from_slice(buf.get(..n).unwrap_or_default());
                if let Some(end) = find_end(&answer)
                    && end != answer.len()
                {
                    return Err(fail("the proxy sent data before the tunnel opened".into()));
                }
            }
            Err(e) => wait(&e)?,
        }
    }
    let line = answer.split(|b| *b == b'\n').next().unwrap_or_default();
    let line = String::from_utf8_lossy(line);
    let mut parts = line.split_whitespace();
    let status = parts.nth(1).and_then(|s| s.parse::<u16>().ok());
    if !line.starts_with("HTTP/1.") {
        return Err(fail("not an HTTP proxy".into()));
    }
    match status {
        Some(200..=299) => Ok(()),
        Some(407) => Err(fail("the proxy asks for a password; proxy authentication is not supported".into())),
        Some(s) => Err(fail(format!("the proxy refused the tunnel (status {s})"))),
        None => Err(fail("not an HTTP proxy".into())),
    }
}

/// Where the head in `bytes` ends (just past its empty line), if it does.
fn find_end(bytes: &[u8]) -> Option<usize> {
    let crlf = bytes.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i.saturating_add(4));
    let lf = bytes.windows(2).position(|w| w == b"\n\n").map(|i| i.saturating_add(2));
    match (crlf, lf) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |name| pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    #[test]
    fn reads_the_environment() {
        assert_eq!(Proxy::from_vars(vars(&[])).unwrap(), Proxy::direct());
        assert_eq!(Proxy::from_vars(vars(&[("HTTPS_PROXY", "  ")])).unwrap(), Proxy::direct());
        let p = Proxy::from_vars(vars(&[("HTTPS_PROXY", "http://Proxy.Corp:3128/")])).unwrap();
        assert_eq!(p.route("example.com"), Some(("proxy.corp", 3128)));
        let p = Proxy::from_vars(vars(&[("https_proxy", "10.0.0.1")])).unwrap();
        assert_eq!(p.route("example.com"), Some(("10.0.0.1", 80)));
        let p = Proxy::from_vars(vars(&[("HTTPS_PROXY", "[::1]:8080")])).unwrap();
        assert_eq!(p.route("example.com"), Some(("::1", 8080)));
        // the upper-case name wins
        let p = Proxy::from_vars(vars(&[("HTTPS_PROXY", "a:1"), ("https_proxy", "b:2")])).unwrap();
        assert_eq!(p.route("x"), Some(("a", 1)));
        for bad in ["socks5://h:1", "https://h:1", "http://u:p@h:1", "h:0", "h:99999", "h/x", "h :1", "[::1", "http://"] {
            let r = Proxy::from_vars(vars(&[("HTTPS_PROXY", bad)]));
            assert!(r.as_ref().is_err_and(|e| e.setting == "HTTPS_PROXY"), "{bad}: {r:?}");
        }
    }

    #[test]
    fn no_proxy_lists_hosts_domains_addresses_and_ranges() {
        let p = Proxy::from_vars(vars(&[
            ("HTTPS_PROXY", "proxy:1"),
            ("NO_PROXY", "localhost, .internal.example ,Example.org:443,*.lan,192.168.0.0/16,[::1],fd00::/8,10.1.2.3"),
        ]))
        .unwrap();
        for direct in [
            "localhost",
            "a.internal.example",
            "internal.example",
            "example.org",
            "www.example.org",
            "x.lan",
            "192.168.4.5",
            "::1",
            "fd12::1",
            "10.1.2.3",
        ] {
            assert_eq!(p.route(direct), None, "{direct}");
        }
        for proxied in ["notexample.org", "example.com", "lan.example", "192.169.0.1", "::2", "10.1.2.4", "localhost.evil"] {
            assert_eq!(p.route(proxied), Some(("proxy", 1)), "{proxied}");
        }
        let all = Proxy::from_vars(vars(&[("HTTPS_PROXY", "proxy:1"), ("no_proxy", "*")])).unwrap();
        assert_eq!(all.route("anything"), None);
        let none = Proxy::from_vars(vars(&[("HTTPS_PROXY", "proxy:1"), ("NO_PROXY", "0.0.0.0/0")])).unwrap();
        assert_eq!(none.route("1.2.3.4"), None);
        assert_eq!(none.route("name.example"), Some(("proxy", 1)));
    }

    #[test]
    fn the_manual_setting() {
        assert_eq!(Proxy::manual("Proxy.Corp", 8080).unwrap().route("x"), Some(("proxy.corp", 8080)));
        assert_eq!(Proxy::manual("[::1]", 8080).unwrap().route("x"), Some(("::1", 8080)));
        assert!(Proxy::manual("", 8080).is_err());
        assert!(Proxy::manual("h", 0).is_err());
        assert!(Proxy::manual("u@h", 1).is_err());
        assert_eq!(Proxy::direct().route("x"), None);
    }

    #[test]
    fn the_end_of_a_head() {
        assert_eq!(find_end(b"HTTP/1.1 200 OK\r\n\r\n"), Some(19));
        assert_eq!(find_end(b"HTTP/1.1 200 OK\n\nx"), Some(17));
        assert_eq!(find_end(b"HTTP/1.1 200 OK\r\n"), None);
    }
}
