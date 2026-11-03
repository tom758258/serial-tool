use std::{
    io::{self, Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

use serial_tool_core::{
    DataBits, Error, FlowControl, Parity, SerialSession, SerialSettings, SerialTransport,
    SimulationTransport, StopBits, TcpSettings, TcpTransport, Transport,
};

fn settings(port: &str, baud_rate: u32) -> SerialSettings {
    SerialSettings {
        port: port.into(),
        baud_rate,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
        flow_control: FlowControl::None,
        timeout: Duration::from_millis(100),
    }
}

#[test]
fn validates_settings_before_open() {
    assert!(settings("COM3", 115_200).validate().is_ok());
    assert!(matches!(
        SerialTransport::open(&settings("  ", 115_200)),
        Err(Error::InvalidConfiguration(_))
    ));
    assert!(matches!(
        SerialTransport::open(&settings("COM3", 0)),
        Err(Error::InvalidConfiguration(_))
    ));
}

#[test]
fn simulation_captures_exact_tx_and_reads_chunks_in_order() {
    let mut session = SerialSession::new(SimulationTransport::with_rx_chunks([
        b"A".to_vec(),
        b"BC".to_vec(),
    ]));
    session.write_all(&[0, 0xff, b'?']).unwrap();
    session.flush().unwrap();
    assert_eq!(session.transport().captured_tx(), &[0, 0xff, b'?']);

    let mut buffer = [0; 3];
    assert_eq!(session.read(&mut buffer).unwrap(), 1);
    assert_eq!(&buffer[..1], b"A");
    assert_eq!(session.read(&mut buffer).unwrap(), 2);
    assert_eq!(&buffer[..2], b"BC");
    assert!(
        matches!(session.read(&mut buffer), Err(Error::Timeout { partial }) if partial.is_empty())
    );
}

#[test]
fn periodic_rx_emits_unsolicited_counter_without_echo_or_burst() {
    let mut session = SerialSession::new(SimulationTransport::periodic_rx());
    assert_eq!(session.transport().bytes_to_read(), 0);

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while session.transport().bytes_to_read() == 0 && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(session.transport().bytes_to_read(), 4);

    let mut first = [0; 2];
    assert_eq!(session.read(&mut first).unwrap(), 2);
    assert_eq!(first, [0, 0]);
    assert_eq!(session.transport().bytes_to_read(), 2);

    let mut second = [0; 2];
    assert_eq!(session.read(&mut second).unwrap(), 2);
    assert_eq!(second, [0, 1]);

    session.write_all(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee]).unwrap();
    assert_eq!(session.transport().bytes_to_read(), 0);

    std::thread::sleep(Duration::from_millis(350));
    assert_eq!(session.transport().bytes_to_read(), 4);
    let mut next = [0; 4];
    assert_eq!(session.read(&mut next).unwrap(), 4);
    assert_eq!(next, [0, 0, 0, 2]);
    assert_eq!(session.transport().bytes_to_read(), 0);

    session.write_all(&[0xff; 5]).unwrap();
    assert_eq!(session.transport().bytes_to_read(), 0);
}

#[test]
fn simulation_splits_a_chunk_for_small_reads() {
    let mut session = SerialSession::new(SimulationTransport::with_rx_chunks([b"ABC".to_vec()]));
    let mut buffer = [0; 2];
    assert_eq!(session.read(&mut buffer).unwrap(), 2);
    assert_eq!(&buffer, b"AB");
    assert_eq!(session.read(&mut buffer).unwrap(), 1);
    assert_eq!(buffer[0], b'C');
}

#[test]
fn simulation_reports_pending_rx_bytes() {
    let mut session = SerialSession::new(SimulationTransport::loopback());
    session.write_all(&[0xaa, 0xbb]).unwrap();
    assert_eq!(session.transport().bytes_to_read(), 2);
    session.read(&mut [0; 1]).unwrap();
    assert_eq!(session.transport().bytes_to_read(), 1);
}

#[test]
fn session_reports_bytes_buffered_after_delimiter() {
    let mut session =
        SerialSession::new(SimulationTransport::with_rx_chunks([b"A\r\nBC".to_vec()]));
    assert_eq!(session.read_until(b"\r\n", 16).unwrap(), b"A\r\n");
    assert_eq!(session.buffered_len(), 2);
    let mut bytes = [0; 2];
    assert_eq!(session.read(&mut bytes).unwrap(), 2);
    assert_eq!(&bytes, b"BC");
}

#[test]
fn read_until_handles_split_and_binary_delimiters() {
    let mut session = SerialSession::new(SimulationTransport::with_rx_chunks([
        vec![0xff, 0],
        vec![b'A', 0x00],
        vec![0xfe],
    ]));
    assert_eq!(
        session.read_until(&[0x00, 0xfe], 8).unwrap(),
        vec![0xff, 0, b'A', 0, 0xfe]
    );
}

#[test]
fn read_until_preserves_trailing_bytes_for_next_read() {
    let mut session = SerialSession::new(SimulationTransport::with_rx_chunks([
        b"OK\r\nREADY\r\n".to_vec()
    ]));
    assert_eq!(session.read_until(b"\r\n", 32).unwrap(), b"OK\r\n");
    assert_eq!(session.read_until(b"\r\n", 32).unwrap(), b"READY\r\n");
}

#[test]
fn raw_read_consumes_shared_buffer_first() {
    let mut session =
        SerialSession::new(SimulationTransport::with_rx_chunks([b"OK\nNEXT".to_vec()]));
    assert_eq!(session.read_until(b"\n", 16).unwrap(), b"OK\n");
    let mut buffer = [0; 4];
    assert_eq!(session.read(&mut buffer).unwrap(), 4);
    assert_eq!(&buffer, b"NEXT");
}

#[test]
fn timeout_returns_partial_and_preserves_it_for_retry() {
    let mut session = SerialSession::new(SimulationTransport::with_rx_chunks([b"ABC".to_vec()]));
    assert!(matches!(
        session.read_until(b"\n", 8),
        Err(Error::Timeout { partial }) if partial == b"ABC"
    ));
    let mut buffer = [0; 3];
    assert_eq!(session.read(&mut buffer).unwrap(), 3);
    assert_eq!(&buffer, b"ABC");
}

#[test]
fn max_bytes_stops_reading_and_reports_partial() {
    let mut session = SerialSession::new(SimulationTransport::with_rx_chunks([
        b"ABCDE".to_vec(),
        b"FG\n".to_vec(),
    ]));
    assert!(matches!(
        session.read_until(b"\n", 4),
        Err(Error::ReadLimitExceeded { max_bytes: 4, partial }) if partial == b"ABCD"
    ));
    assert_eq!(session.read_until(b"\n", 8).unwrap(), b"ABCDEFG\n");
}

#[test]
fn empty_delimiter_fails_before_transport_read() {
    let mut session = SerialSession::new(FailingTransport::new(Failure::Read));
    assert!(matches!(
        session.read_until(b"", 8),
        Err(Error::InvalidConfiguration(_))
    ));
}

#[test]
fn transport_failures_map_to_core_errors() {
    let mut read = SerialSession::new(FailingTransport::new(Failure::Read));
    assert!(matches!(read.read(&mut [0; 1]), Err(Error::ReadFailed(_))));

    let mut write = SerialSession::new(FailingTransport::new(Failure::Write));
    assert!(matches!(write.write_all(b"X"), Err(Error::WriteFailed(_))));

    let mut flush = SerialSession::new(FailingTransport::new(Failure::Flush));
    assert!(matches!(flush.flush(), Err(Error::FlushFailed(_))));
}

enum Failure {
    Read,
    Write,
    Flush,
}

struct FailingTransport(Failure);

impl FailingTransport {
    fn new(failure: Failure) -> Self {
        Self(failure)
    }
}

impl Transport for FailingTransport {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        if matches!(self.0, Failure::Read) {
            Err(io::Error::other("read failure"))
        } else {
            unreachable!()
        }
    }

    fn write_all(&mut self, _: &[u8]) -> io::Result<()> {
        if matches!(self.0, Failure::Write) {
            Err(io::Error::other("write failure"))
        } else {
            unreachable!()
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if matches!(self.0, Failure::Flush) {
            Err(io::Error::other("flush failure"))
        } else {
            unreachable!()
        }
    }
}

fn invalid_tcp_settings(settings: &TcpSettings) -> bool {
    matches!(settings.validate(), Err(Error::InvalidConfiguration(_)))
}

#[test]
fn tcp_settings_validate_without_dns_or_socket() {
    let mut settings = TcpSettings::default();
    assert_eq!(settings.connect_timeout, Duration::from_millis(3000));
    assert_eq!(settings.io_timeout, Duration::from_millis(1000));
    assert!(invalid_tcp_settings(&settings));
    settings.host = "localhost".into();
    assert!(invalid_tcp_settings(&settings));
    settings.port = 5025;
    assert!(settings.validate().is_ok());
    settings.host = "::1".into();
    assert!(settings.validate().is_ok());
    settings.host = "http://localhost".into();
    assert!(invalid_tcp_settings(&settings));
    settings.host = "127.0.0.1".into();
    settings.io_timeout = Duration::ZERO;
    assert!(invalid_tcp_settings(&settings));
    settings.io_timeout = Duration::from_micros(500);
    assert!(invalid_tcp_settings(&settings));
    settings.io_timeout = Duration::from_millis(1000);
    settings.connect_timeout = Duration::from_secs(u64::MAX);
    assert!(invalid_tcp_settings(&settings));
}

#[test]
fn tcp_socket_roundtrip_timeout_recovery_and_remote_eof() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (received, accepted) = mpsc::channel();
    let (release, send) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut bytes = [0u8; 3];
        peer.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [0, 0xaa, 0xff]);
        received.send(()).unwrap();
        send.recv_timeout(Duration::from_secs(2)).unwrap();
        peer.write_all(&[0x42, 0xff]).unwrap();
    });

    let settings = TcpSettings {
        host: "localhost".into(),
        port,
        io_timeout: Duration::from_millis(75),
        ..TcpSettings::default()
    };
    let mut session = SerialSession::new(TcpTransport::connect(&settings).unwrap());
    session.write_all(&[0, 0xaa, 0xff]).unwrap();
    session.flush().unwrap();
    accepted.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        session.read(&mut [0u8; 8]),
        Err(Error::Timeout { partial }) if partial.is_empty()
    ));
    release.send(()).unwrap();
    assert_eq!(session.read_until(&[0xff], 2).unwrap(), vec![0x42, 0xff]);
    assert!(matches!(
        session.read(&mut [0u8; 1]),
        Err(Error::ReadFailed(error)) if error.kind() == io::ErrorKind::UnexpectedEof
    ));
    server.join().unwrap();
}
