//! A request to a server of this computer (an engine here, or the end of a tunnel to one on another computer) whose answer
//! is read as it comes and can be given up at any moment (C224). The socket is read a fifth of a second at a time, and
//! once the chat is told to stop, it is closed: llama.cpp's server stops working on a request whose connection closed,
//! even while it still reads the conversation, and so does Ollama. A client that looks at the stop only when the next
//! piece of the answer comes leaves the engine busy until it comes.
//!
//! It speaks the little of HTTP/1.1 the chat needs: a POST of JSON, then the answer's status, its headers, and its body,
//! chunked or of a known length or to the end of the connection.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// How long a read waits before it looks at the stop again.
const LOOK: Duration = Duration::from_millis(200);
/// How long the connection and the sending of the request may take.
const CONNECT: Duration = Duration::from_secs(10);
const SEND: Duration = Duration::from_secs(30);
/// The longest head of an answer that is read: a server that says more is not one the chat talks to.
const LONGEST_HEAD: usize = 64 * 1024;

/// The answer to a request: its status, and its body as it comes (the chunks of a chunked one put together).
pub(super) struct Answer<'a> {
    pub status: u16,
    pub body: Box<dyn BufRead + 'a>,
}

/// The socket, read a little at a time and closed once `stop` is set: a read then fails, as on a connection that was
/// aborted.
struct Stoppable<'a> {
    stream: TcpStream,
    stop: &'a AtomicBool,
}

impl Read for Stoppable<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.stop.load(Ordering::SeqCst) {
                let _ = self.stream.shutdown(Shutdown::Both);
                return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "stopped"));
            }
            match self.stream.read(buf) {
                // the fifth of a second went by with nothing (a timed-out read is WouldBlock on Unix, TimedOut on Windows)
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) => {}
                other => return other,
            }
        }
    }
}

impl Drop for Stoppable<'_> {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}

/// A chunked body (`Transfer-Encoding: chunked`), as the bytes it carries: each chunk is its size in hexadecimal on a
/// line, the bytes, and a line's end; a chunk of size 0, then the trailer to an empty line, ends it.
struct Chunked<R: BufRead> {
    inner: R,
    /// What is left of the chunk being read.
    left: usize,
    done: bool,
}

impl<R: BufRead> Read for Chunked<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        if self.left == 0 {
            // the size of the next chunk (what follows a `;` is an extension, and is passed over)
            let mut line = String::new();
            loop {
                line.clear();
                if self.inner.read_line(&mut line)? == 0 {
                    // the connection ended without the last chunk: what came is all there is
                    self.done = true;
                    return Ok(0);
                }
                if !line.trim().is_empty() {
                    break;
                }
            }
            let size = line.trim().split(';').next().unwrap_or_default().trim();
            let size = usize::from_str_radix(size, 16).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("not the size of a chunk: {size}"),
                )
            })?;
            if size == 0 {
                loop {
                    line.clear();
                    if self.inner.read_line(&mut line)? == 0 || line.trim().is_empty() {
                        break;
                    }
                }
                self.done = true;
                return Ok(0);
            }
            self.left = size;
        }
        let n = {
            let available = self.inner.fill_buf()?;
            if available.is_empty() {
                self.done = true;
                return Ok(0);
            }
            let n = available.len().min(self.left).min(buf.len());
            buf[..n].copy_from_slice(&available[..n]);
            n
        };
        self.inner.consume(n);
        self.left -= n;
        if self.left == 0 {
            // the end of the chunk's line
            let mut end = String::new();
            self.inner.read_line(&mut end)?;
        }
        Ok(n)
    }
}

/// POSTs `body` (JSON) to `path` of the server on `port` of this computer, and gives its answer as it comes. Once `stop`
/// is set, the connection is closed and what reads it fails.
pub(super) fn post<'a>(
    port: u16,
    path: &str,
    body: &str,
    stop: &'a AtomicBool,
) -> io::Result<Answer<'a>> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, CONNECT)?;
    let _ = stream.set_nodelay(true);
    stream.set_write_timeout(Some(SEND))?;
    stream.set_read_timeout(Some(LOOK))?;
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nAccept: text/event-stream\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()?;
    let mut reader = BufReader::new(Stoppable { stream, stop });
    // the status line, then the headers to the empty line
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|_| line.starts_with("HTTP/"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("not an answer of HTTP: {}", line.trim()),
            )
        })?;
    let (mut chunked, mut length) = (false, None);
    let mut read = line.len();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        read += n;
        if n == 0 || line.trim_end().is_empty() {
            break;
        }
        if read > LONGEST_HEAD {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the head of the answer is too long",
            ));
        }
        if let Some((name, value)) = line.split_once(':') {
            let (name, value) = (name.trim().to_ascii_lowercase(), value.trim());
            if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
                chunked = true;
            } else if name == "content-length" {
                length = value.parse::<u64>().ok();
            }
        }
    }
    let body: Box<dyn BufRead + 'a> = if chunked {
        Box::new(BufReader::new(Chunked {
            inner: reader,
            left: 0,
            done: false,
        }))
    } else if let Some(n) = length {
        Box::new(reader.take(n))
    } else {
        Box::new(reader)
    };
    Ok(Answer { status, body })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Instant;

    /// A server of one connection: it reads the request's head and body, then writes `answer`.
    fn serve(answer: &'static [u8]) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut asked = Vec::new();
            let mut buf = [0u8; 4096];
            while !String::from_utf8_lossy(&asked).contains("{\"a\":1}") {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                asked.extend_from_slice(&buf[..n]);
            }
            let _ = stream.write_all(answer);
        });
        port
    }

    fn all_of(answer: Answer) -> String {
        let mut text = String::new();
        let mut body = answer.body;
        body.read_to_string(&mut text).unwrap();
        text
    }

    /// The bodies the chat meets: chunked (llama-server's and Ollama's streams), of a known length (an error), and to the
    /// end of the connection.
    #[test]
    fn a_body_is_read_chunked_of_a_length_or_to_the_end() {
        let stop = AtomicBool::new(false);
        let port = serve(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n\
              f\r\ndata: {\"a\":1}\n\n\r\n6;ext=1\r\ndata: \r\n8\r\n[DONE]\n\n\r\n0\r\n\r\n",
        );
        let answer = post(port, "/v1/chat/completions", "{\"a\":1}", &stop).unwrap();
        assert_eq!(answer.status, 200);
        assert_eq!(all_of(answer), "data: {\"a\":1}\n\ndata: [DONE]\n\n");
        let port = serve(
            b"HTTP/1.1 400 Bad Request\r\ncontent-length: 27\r\n\r\n{\"error\":\"too long\"}  extra",
        );
        let answer = post(port, "/v1/chat/completions", "{\"a\":1}", &stop).unwrap();
        assert_eq!(answer.status, 400);
        assert_eq!(all_of(answer), "{\"error\":\"too long\"}  extra");
        let port =
            serve(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\ndata: x\n\ndata: [DONE]\n\n");
        let answer = post(port, "/", "{\"a\":1}", &stop).unwrap();
        assert_eq!(all_of(answer), "data: x\n\ndata: [DONE]\n\n");
        let port = serve(b"SSH-2.0-OpenSSH\r\n");
        assert!(post(port, "/", "{\"a\":1}", &stop).is_err(), "not HTTP");
    }

    /// A stop is heard while nothing comes (an engine still reading the conversation): the read ends within a fraction of
    /// a second, and the server sees its connection closed, which is what makes it stop.
    #[test]
    fn a_stop_closes_the_connection_while_nothing_comes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (closed_tx, closed_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // the request is read, the head of the answer is sent, and then nothing more: the model is thinking
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
            let started = Instant::now();
            let closed = loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break true,
                    Ok(_) if started.elapsed() > Duration::from_secs(10) => break false,
                    Ok(_) => {}
                }
            };
            let _ = closed_tx.send(closed);
        });
        let stop = AtomicBool::new(false);
        let answer = post(port, "/v1/chat/completions", "{}", &stop).unwrap();
        let mut body = answer.body;
        std::thread::scope(|s| {
            s.spawn(|| {
                std::thread::sleep(Duration::from_millis(300));
                stop.store(true, Ordering::SeqCst);
            });
            let asked = Instant::now();
            let mut line = String::new();
            let read = body.read_line(&mut line);
            assert!(read.is_err(), "{read:?}");
            assert!(
                asked.elapsed() < Duration::from_secs(2),
                "{:?}",
                asked.elapsed()
            );
        });
        drop(body);
        assert_eq!(
            closed_rx.recv_timeout(Duration::from_secs(5)),
            Ok(true),
            "the server saw its connection closed"
        );
    }
}
