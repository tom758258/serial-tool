use std::{io, time::Duration};

use serial_tool_core::{
    DataBits, Error, FlowControl, Parity, SerialSession, SerialSettings, SerialTransport,
    SimulationTransport, StopBits, Transport,
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
