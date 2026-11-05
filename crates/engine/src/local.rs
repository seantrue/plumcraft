//! The local binding's transport (mode 2, spec 16.3): a Unix socket in a private directory on
//! macOS and Linux, a loopback TCP port on Windows (std has no Unix sockets there). Either way the
//! engine presents the 256-bit token in `hello`, and the frames are the same.
//!
//! An endpoint is written as a locator: `unix:<path>` or `tcp:127.0.0.1:<port>`. A bare path,
//! as earlier hosts set `PLUMCRAFT_ENGINE_SOCKET`, is a Unix socket.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

/// Where an engine connects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Unix(PathBuf),
    /// Loopback only: a locator naming another address is refused.
    Tcp(SocketAddr),
}

impl Endpoint {
    /// Read a locator: `unix:<path>`, `tcp:<loopback address>:<port>`, or a bare path.
    pub fn parse(s: &str) -> Result<Endpoint, String> {
        if let Some(addr) = s.strip_prefix("tcp:") {
            let a: SocketAddr = addr.parse().map_err(|e| format!("{s:?} is not an address: {e}"))?;
            if !a.ip().is_loopback() {
                return Err(format!("{s:?}: the local binding is loopback only"));
            }
            return Ok(Endpoint::Tcp(a));
        }
        let path = s.strip_prefix("unix:").unwrap_or(s);
        if path.is_empty() {
            return Err("an empty socket path".into());
        }
        Ok(Endpoint::Unix(PathBuf::from(path)))
    }

    /// The locator, as [`Endpoint::parse`] reads it.
    pub fn locator(&self) -> String {
        match self {
            Endpoint::Unix(p) => format!("unix:{}", p.display()),
            Endpoint::Tcp(a) => format!("tcp:{a}"),
        }
    }

    /// Connect to it.
    pub fn connect(&self) -> std::io::Result<Stream> {
        match self {
            #[cfg(unix)]
            Endpoint::Unix(p) => Ok(Stream::Unix(UnixStream::connect(p)?)),
            #[cfg(not(unix))]
            Endpoint::Unix(p) => Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                format!("{}: Unix sockets aren't available here; the host must offer tcp:", p.display()),
            )),
            Endpoint::Tcp(a) => {
                let s = TcpStream::connect(a)?;
                s.set_nodelay(true)?;
                Ok(Stream::Tcp(s))
            }
        }
    }
}

/// One connection, either kind.
#[derive(Debug)]
pub enum Stream {
    #[cfg(unix)]
    Unix(UnixStream),
    Tcp(TcpStream),
}

impl Stream {
    pub fn try_clone(&self) -> std::io::Result<Stream> {
        Ok(match self {
            #[cfg(unix)]
            Stream::Unix(s) => Stream::Unix(s.try_clone()?),
            Stream::Tcp(s) => Stream::Tcp(s.try_clone()?),
        })
    }

    pub fn shutdown(&self, how: Shutdown) -> std::io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::Unix(s) => s.shutdown(how),
            Stream::Tcp(s) => s.shutdown(how),
        }
    }

    pub fn set_nonblocking(&self, on: bool) -> std::io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::Unix(s) => s.set_nonblocking(on),
            Stream::Tcp(s) => s.set_nonblocking(on),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(unix)]
            Stream::Unix(s) => s.read(buf),
            Stream::Tcp(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(unix)]
            Stream::Unix(s) => s.write(buf),
            Stream::Tcp(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            #[cfg(unix)]
            Stream::Unix(s) => s.flush(),
            Stream::Tcp(s) => s.flush(),
        }
    }
}

/// The host's side: where engines connect.
#[derive(Debug)]
pub enum Listener {
    #[cfg(unix)]
    Unix(UnixListener),
    Tcp(TcpListener),
}

impl Listener {
    /// Listen for one engine: a socket `e.sock` in `dir` (a private directory) on Unix, a free
    /// loopback port elsewhere. Returns the listener and the endpoint to give the engine.
    pub fn bind_private(dir: &Path) -> std::io::Result<(Listener, Endpoint)> {
        #[cfg(unix)]
        {
            let socket = dir.join("e.sock");
            let l = UnixListener::bind(&socket)?;
            Ok((Listener::Unix(l), Endpoint::Unix(socket)))
        }
        #[cfg(not(unix))]
        {
            let _ = dir;
            Listener::bind_tcp()
        }
    }

    /// Listen on a free loopback port (Windows' binding; tests use it on Unix too).
    pub fn bind_tcp() -> std::io::Result<(Listener, Endpoint)> {
        let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let at = l.local_addr()?;
        Ok((Listener::Tcp(l), Endpoint::Tcp(at)))
    }

    pub fn set_nonblocking(&self, on: bool) -> std::io::Result<()> {
        match self {
            #[cfg(unix)]
            Listener::Unix(l) => l.set_nonblocking(on),
            Listener::Tcp(l) => l.set_nonblocking(on),
        }
    }

    /// Accept one connection. A TCP peer that isn't on loopback is refused (the listener is bound
    /// to 127.0.0.1, so this is belt and braces).
    pub fn accept(&self) -> std::io::Result<Stream> {
        match self {
            #[cfg(unix)]
            Listener::Unix(l) => Ok(Stream::Unix(l.accept()?.0)),
            Listener::Tcp(l) => {
                let (s, peer) = l.accept()?;
                if !peer.ip().is_loopback() {
                    return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("a connection from {peer}")));
                }
                s.set_nodelay(true)?;
                Ok(Stream::Tcp(s))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locators_round_trip_and_bare_paths_are_unix() {
        for s in ["unix:/tmp/x/e.sock", "tcp:127.0.0.1:4100", "tcp:[::1]:9"] {
            assert_eq!(Endpoint::parse(s).unwrap().locator(), s);
        }
        assert_eq!(Endpoint::parse("/tmp/x/e.sock").unwrap(), Endpoint::Unix("/tmp/x/e.sock".into()));
        assert!(Endpoint::parse("tcp:192.168.2.5:80").unwrap_err().contains("loopback"));
        assert!(Endpoint::parse("tcp:nonsense").is_err());
        assert!(Endpoint::parse("unix:").is_err());
    }

    #[test]
    fn a_tcp_listener_accepts_a_loopback_engine() {
        let (l, at) = Listener::bind_tcp().unwrap();
        let mut c = at.connect().unwrap();
        let mut s = l.accept().unwrap();
        c.write_all(b"hi\n").unwrap();
        let mut b = [0u8; 3];
        s.read_exact(&mut b).unwrap();
        assert_eq!(&b, b"hi\n");
    }
}
