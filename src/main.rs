use std::io::{self, Cursor, Read};
use std::net::SocketAddr;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const LISTEN_ADDR: &str = "0.0.0.0:25565";
const TARGET_HOST: &str = "mc.hypixel.net";
const TARGET_PORT: u16 = 25565;
const HANDSHAKE_PACKET_ID: i32 = 0;
const MAX_HANDSHAKE_SIZE: usize = 16 * 1024;
const MAX_HOST_SIZE: usize = 255;

fn encode_varint(mut value: i32, output: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value = ((value as u32) >> 7) as i32;

        if value != 0 {
            byte |= 0x80;
        }

        output.push(byte);
        if value == 0 {
            return;
        }
    }
}

fn read_varint<R: Read>(reader: &mut R) -> io::Result<i32> {
    let mut value = 0_u32;

    for shift in (0..35).step_by(7) {
        let mut byte = [0_u8; 1];
        reader.read_exact(&mut byte)?;
        value |= u32::from(byte[0] & 0x7f) << shift;

        if byte[0] & 0x80 == 0 {
            return Ok(value as i32);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "VarInt is longer than 5 bytes",
    ))
}

async fn read_varint_async<R>(reader: &mut R) -> io::Result<i32>
where
    R: AsyncRead + Unpin,
{
    let mut value = 0_u32;

    for shift in (0..35).step_by(7) {
        let byte = reader.read_u8().await?;
        value |= u32::from(byte & 0x7f) << shift;

        if byte & 0x80 == 0 {
            return Ok(value as i32);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "VarInt is longer than 5 bytes",
    ))
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn rewrite_handshake(packet: &[u8]) -> io::Result<(Vec<u8>, i32, i32, bool)> {
    let mut reader = Cursor::new(packet);

    if read_varint(&mut reader)? != HANDSHAKE_PACKET_ID {
        return Err(invalid_data("first packet is not a handshake"));
    }

    let protocol_version = read_varint(&mut reader)?;
    let host_length = read_varint(&mut reader)?;
    let host_length =
        usize::try_from(host_length).map_err(|_| invalid_data("negative host length"))?;

    if host_length > MAX_HOST_SIZE {
        return Err(invalid_data("handshake host is too long"));
    }

    let mut original_host = vec![0_u8; host_length];
    Read::read_exact(&mut reader, &mut original_host)?;

    let mut original_port = [0_u8; 2];
    Read::read_exact(&mut reader, &mut original_port)?;
    let next_state = read_varint(&mut reader)?;

    if next_state != 1 && next_state != 2 {
        return Err(invalid_data("invalid handshake next state"));
    }
    if reader.position() != packet.len() as u64 {
        return Err(invalid_data("trailing data outside the handshake host"));
    }

    let suffix = original_host
        .iter()
        .position(|byte| *byte == 0)
        .map_or(&[][..], |position| &original_host[position..]);
    let has_fml_suffix = !suffix.is_empty();

    let mut target_host = TARGET_HOST.as_bytes().to_vec();
    target_host.extend_from_slice(suffix);
    if target_host.len() > MAX_HOST_SIZE {
        return Err(invalid_data("rewritten handshake host is too long"));
    }

    let mut payload = Vec::with_capacity(packet.len());
    encode_varint(HANDSHAKE_PACKET_ID, &mut payload);
    encode_varint(protocol_version, &mut payload);
    encode_varint(target_host.len() as i32, &mut payload);
    payload.extend_from_slice(&target_host);
    payload.extend_from_slice(&TARGET_PORT.to_be_bytes());
    encode_varint(next_state, &mut payload);

    let mut framed_packet = Vec::with_capacity(payload.len() + 3);
    encode_varint(payload.len() as i32, &mut framed_packet);
    framed_packet.extend_from_slice(&payload);

    Ok((framed_packet, protocol_version, next_state, has_fml_suffix))
}

async fn handle_client(mut client: TcpStream, peer: SocketAddr) -> io::Result<()> {
    println!("[*] New connection: {peer}");

    let packet_length = read_varint_async(&mut client).await?;
    let packet_length =
        usize::try_from(packet_length).map_err(|_| invalid_data("negative handshake length"))?;
    if packet_length == 0 || packet_length > MAX_HANDSHAKE_SIZE {
        return Err(invalid_data("invalid handshake length"));
    }

    let mut packet = vec![0_u8; packet_length];
    client.read_exact(&mut packet).await?;
    let (handshake, protocol_version, next_state, has_fml_suffix) = rewrite_handshake(&packet)?;

    let mode = if next_state == 1 { "status" } else { "login" };
    println!(
        "[*] Handshake: peer={peer}, protocol={protocol_version}, mode={mode}, fml={has_fml_suffix}"
    );

    let mut target = TcpStream::connect((TARGET_HOST, TARGET_PORT)).await?;
    target.write_all(&handshake).await?;

    let (client_to_server, server_to_client) =
        tokio::io::copy_bidirectional(&mut client, &mut target).await?;
    println!(
        "[*] Connection closed: peer={peer}, sent={client_to_server} B, received={server_to_client} B"
    );

    Ok(())
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let listener = TcpListener::bind(LISTEN_ADDR).await?;
    println!("=== HypForward Rust proxy started ===");
    println!("Listening on: {LISTEN_ADDR}");
    println!("Target server: {TARGET_HOST}:{TARGET_PORT}");

    loop {
        let (socket, peer) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(error) = handle_client(socket, peer).await {
                eprintln!("[-] Connection failed: peer={peer}, error={error}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handshake(host: &[u8], protocol: i32, state: i32) -> Vec<u8> {
        let mut packet = Vec::new();
        encode_varint(HANDSHAKE_PACKET_ID, &mut packet);
        encode_varint(protocol, &mut packet);
        encode_varint(host.len() as i32, &mut packet);
        packet.extend_from_slice(host);
        packet.extend_from_slice(&12345_u16.to_be_bytes());
        encode_varint(state, &mut packet);
        packet
    }

    fn decode_framed_handshake(packet: &[u8]) -> (i32, Vec<u8>, u16, i32) {
        let mut reader = Cursor::new(packet);
        let frame_length = read_varint(&mut reader).unwrap();
        assert_eq!(
            frame_length as usize,
            packet.len() - reader.position() as usize
        );
        assert_eq!(read_varint(&mut reader).unwrap(), HANDSHAKE_PACKET_ID);
        let protocol = read_varint(&mut reader).unwrap();
        let host_length = read_varint(&mut reader).unwrap() as usize;
        let mut host = vec![0; host_length];
        Read::read_exact(&mut reader, &mut host).unwrap();
        let mut port = [0; 2];
        Read::read_exact(&mut reader, &mut port).unwrap();
        let state = read_varint(&mut reader).unwrap();
        (protocol, host, u16::from_be_bytes(port), state)
    }

    #[test]
    fn rewrites_host_and_port() {
        let packet = handshake(b"proxy.example.com", 47, 2);

        let (rewritten, protocol, state, has_fml) = rewrite_handshake(&packet).unwrap();
        let decoded = decode_framed_handshake(&rewritten);

        assert_eq!(protocol, 47);
        assert_eq!(state, 2);
        assert!(!has_fml);
        assert_eq!(decoded, (47, TARGET_HOST.as_bytes().to_vec(), 25565, 2));
    }

    #[test]
    fn preserves_forge_suffix() {
        let packet = handshake(b"proxy.example.com\0FML\0", 47, 2);

        let (rewritten, _, _, has_fml) = rewrite_handshake(&packet).unwrap();
        let (_, host, _, _) = decode_framed_handshake(&rewritten);

        assert!(has_fml);
        assert_eq!(host, b"mc.hypixel.net\0FML\0");
    }

    #[test]
    fn rejects_invalid_next_state() {
        let packet = handshake(b"proxy.example.com", 47, 3);

        let error = rewrite_handshake(&packet).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut packet = handshake(b"proxy.example.com", 47, 1);
        packet.push(0);

        let error = rewrite_handshake(&packet).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
