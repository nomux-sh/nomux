//! A TCP relay between a client and a server on this computer. A test cuts
//! the connections of the relay to make a network failure, and the
//! measurements give the relay a delay.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const LOCALHOST: &str = "127.0.0.1";
const ACCEPT_INTERVAL: Duration = Duration::from_millis(5);
const CHUNK: usize = 16 * 1024;

/// What the relay does to the bytes that go through it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Impairment {
    /// The time that each chunk of bytes waits, in each direction.
    pub delay: Duration,
    /// A model of a lost packet: this part of the chunks (0.0 to 1.0) waits
    /// for `stall` more, and the chunks after it wait behind it. This is
    /// what a TCP connection does when it sends a lost packet again.
    pub stall_probability: f64,
    /// The time of one stall.
    pub stall: Duration,
}

/// A relay that listens on a port of this computer and connects each client
/// to `upstream_port`.
#[derive(Debug)]
pub struct TcpRelay {
    port: u16,
    connections: Arc<Mutex<Vec<TcpStream>>>,
    stopped: Arc<AtomicBool>,
}

impl TcpRelay {
    /// Starts the relay.
    ///
    /// # Panics
    ///
    /// Panics if the relay cannot listen.
    #[must_use]
    pub fn start(upstream_port: u16, impairment: Impairment) -> Self {
        let listener = TcpListener::bind((LOCALHOST, 0)).expect("the relay can listen");
        listener
            .set_nonblocking(true)
            .expect("the listener can be polled");
        let port = listener.local_addr().expect("the relay has a port").port();
        let connections = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let relay = Self {
            port,
            connections: Arc::clone(&connections),
            stopped: Arc::clone(&stopped),
        };
        thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((client, _)) => connect(client, upstream_port, impairment, &connections),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(ACCEPT_INTERVAL);
                    }
                    Err(_) => return,
                }
            }
        });
        relay
    }

    /// The port on which the relay listens.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Closes each connection that goes through the relay now. The relay
    /// accepts new connections after that.
    ///
    /// # Panics
    ///
    /// Panics if a thread of the relay panicked.
    pub fn cut_connections(&self) {
        let mut connections = self.connections.lock().expect("the relay runs");
        for connection in connections.drain(..) {
            let _ = connection.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for TcpRelay {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Ok(mut connections) = self.connections.lock() {
            for connection in connections.drain(..) {
                let _ = connection.shutdown(Shutdown::Both);
            }
        }
    }
}

/// Connects one client to the upstream server and moves the bytes in the
/// two directions.
fn connect(
    client: TcpStream,
    upstream_port: u16,
    impairment: Impairment,
    connections: &Mutex<Vec<TcpStream>>,
) {
    let Ok(upstream) = TcpStream::connect((LOCALHOST, upstream_port)) else {
        return;
    };
    let prepared = (|| {
        client.set_nonblocking(false)?;
        client.set_nodelay(true)?;
        upstream.set_nodelay(true)?;
        let pairs = [
            (client.try_clone()?, upstream.try_clone()?),
            (upstream.try_clone()?, client.try_clone()?),
        ];
        io::Result::Ok(pairs)
    })();
    let Ok(pairs) = prepared else {
        return;
    };
    if let Ok(mut connections) = connections.lock() {
        connections.push(client);
        connections.push(upstream);
    }
    for (from, to) in pairs {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || read_chunks(from, impairment, &sender));
        thread::spawn(move || write_chunks(to, &receiver));
    }
}

/// Reads chunks and gives each one the time at which the other side gets
/// it.
fn read_chunks(
    mut from: TcpStream,
    impairment: Impairment,
    chunks: &mpsc::Sender<(Instant, Vec<u8>)>,
) {
    let mut random = Random::new();
    let mut last_delivery = Instant::now();
    let mut buffer = vec![0; CHUNK];
    loop {
        let length = match from.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(length) => length,
        };
        let mut delivery = Instant::now() + impairment.delay;
        if random.chance(impairment.stall_probability) {
            delivery += impairment.stall;
        }
        // The bytes of a connection stay in their sequence.
        last_delivery = delivery.max(last_delivery);
        if chunks
            .send((last_delivery, buffer[..length].to_vec()))
            .is_err()
        {
            return;
        }
    }
}

fn write_chunks(mut to: TcpStream, chunks: &Receiver<(Instant, Vec<u8>)>) {
    for (delivery, chunk) in chunks {
        thread::sleep(delivery.saturating_duration_since(Instant::now()));
        if to.write_all(&chunk).is_err() {
            break;
        }
    }
    let _ = to.shutdown(Shutdown::Both);
}

/// A small random number generator (xorshift): the relay needs no quality.
struct Random(u64);

impl Random {
    fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(1, |time| u64::from(time.subsec_nanos()) | 1);
        Self(seed)
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "a probability needs no exact value"
    )]
    fn chance(&mut self, probability: f64) -> bool {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % 1_000_000) as f64 / 1_000_000.0 < probability
    }
}
