# Packet Capture Crate

This crate provides a lightweight, asynchronous library for reading and writing pcap (packet capture) files. It is designed to be simple, efficient, and easy to integrate into networking applications in Rust.

## Features

- **Asynchronous:** Built with `tokio` for non-blocking I/O operations.
- **Zero-Copy Deserialization:** Uses the `zerocopy` crate for efficient, zero-cost parsing of pcap headers and records.
- **Simple API:** Offers a minimal set of functions to read and write the essential components of a pcap file:
    - `read_file_header` / `write_file_header`
    - `read_record` / `write_record`
- **Standard Format:** Adheres to the standard pcap file format, ensuring compatibility with common tools like Wireshark and `tcpdump`.

## Usage

### Reading a pcap file

```rust
use capture::pcap;
use std::io::Cursor;
use tokio::io::BufReader;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Sample pcap data
    const DATA: &[u8] = include_bytes!("data/dns.cap");
    let mut reader = BufReader::new(Cursor::new(DATA));

    // 1. Read the file header
    let header = pcap::read_file_header(&mut reader).await?;
    println!("LinkType: {:?}, SnapLen: {}", pcap::LinkType::from(header.linktype), header.snaplen);

    // 2. Read each packet record in a loop
    while let Ok((record_header, _record_data)) = pcap::read_record(&mut reader).await {
        println!(
            "Record: timestamp={}.{:06}, length={}",
            record_header.tv_sec, record_header.tv_usec, record_header.len
        );
    }

    Ok(())
}
```

### Writing a pcap file

```rust
use capture::pcap;
use std::time::Duration;
use tokio::io::BufWriter;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut buffer = BufWriter::new(Vec::new());

    // 1. Write the global file header
    pcap::write_file_header(pcap::LinkType::Ethernet, &mut buffer).await?;

    // 2. Write a packet record
    let packet_data = vec![0xDE, 0xAD, 0xBE, 0xEF];
    let timestamp = Duration::new(1678886400, 0); // Example timestamp
    pcap::write_record(timestamp, &mut buffer, &packet_data).await?;

    // The buffer now contains a valid pcap file.
    let pcap_data = buffer.into_inner();
    println!("Generated pcap file with {} bytes", pcap_data.len());

    Ok(())
}
```
