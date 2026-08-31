use std::env;
use std::io::{self, Cursor, Read};
use std::net::{IpAddr, SocketAddr};
use std::process;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const DEFAULT_SERVERS: &str = "0.0.0.0:25565=mc.hypixel.net:25565";
const SERVERS_ENV: &str = "HYPFORWARD_SERVERS";
const HANDSHAKE_PACKET_ID: i32 = 0;
const MAX_HANDSHAKE_SIZE: usize = 16 * 1024;
const MAX_HOST_SIZE: usize = 255;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Forward {
    listen: SocketAddr,
    target_host: String,
    target_port: u16,
}

impl Forward {
    fn parse(value: &str) -> Result<Self, String> {
        let (listen, target) = value
            .split_once('=')
            .ok_or_else(|| format!("invalid forwarding rule '{value}': expected LISTEN=TARGET"))?;
        let listen = listen
            .parse()
            .map_err(|_| format!("invalid listen address '{listen}'"))?;
        let (target_host, target_port) = parse_target(target)?;

        Ok(Self {
            listen,
            target_host,
            target_port,
        })
    }

    fn target_address(&self) -> String {
        if self
            .target_host
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_ipv6())
        {
            format!("[{}]:{}", self.target_host, self.target_port)
        } else {
            format!("{}:{}", self.target_host, self.target_port)
        }
    }
}

fn parse_target(value: &str) -> Result<(String, u16), String> {
    let (host, port) = value
        .rsplit_once(':')
        .ok_or_else(|| format!("invalid target address '{value}': port is required"))?;
    let host = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);

    if host.is_empty() || host.as_bytes().contains(&0) || host.len() > MAX_HOST_SIZE {
        return Err(format!("invalid target host '{host}'"));
    }

    let port = port
        .parse::<u16>()
        .map_err(|_| format!("invalid target port '{port}'"))?;
    if port == 0 {
        return Err("target port must not be zero".to_owned());
    }

    Ok((host.to_owned(), port))
}

fn parse_forwards<I, S>(args: I, environment: Option<&str>) -> Result<Vec<Forward>, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut forwards = Vec::new();
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_ref() {
            "--forward" | "-f" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("{} requires LISTEN=TARGET", arg.as_ref()))?;
                forwards.push(Forward::parse(value.as_ref())?);
            }
            "--help" | "-h" => return Err(String::new()),
            unknown => return Err(format!("unknown argument '{unknown}'")),
        }
    }

    if forwards.is_empty() {
        let rules = environment.unwrap_or(DEFAULT_SERVERS);
        for rule in rules.split(',').filter(|rule| !rule.is_empty()) {
            forwards.push(Forward::parse(rule)?);
        }
    }

    if forwards.is_empty() {
        return Err("no forwarding rules configured".to_owned());
    }

    for (index, forward) in forwards.iter().enumerate() {
        if forwards[..index]
            .iter()
            .any(|other| other.listen == forward.listen)
        {
            return Err(format!("duplicate listen address '{}'", forward.listen));
        }
    }

    Ok(forwards)
}

fn print_usage() {
    println!("HypForward - asynchronous Minecraft forwarding proxy");
    println!();
    println!("Usage:");
    println!("  hypforward [--forward LISTEN=TARGET]...");
    println!();
    println!("Environment:");
    println!("  {SERVERS_ENV}=LISTEN=TARGET[,LISTEN=TARGET...]");
    println!();
    println!("Example:");
    println!("  hypforward -f 0.0.0.0:25565=mc.hypixel.net:25565 \\");
    println!("             -f 0.0.0.0:25566=example.com:25565");
}

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

fn rewrite_handshake(
    packet: &[u8],
    target_host: &str,
    target_port: u16,
) -> io::Result<(Vec<u8>, i32, i32, bool)> {
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

    let mut rewritten_host = target_host.as_bytes().to_vec();
    rewritten_host.extend_from_slice(suffix);
    if rewritten_host.len() > MAX_HOST_SIZE {
        return Err(invalid_data("rewritten handshake host is too long"));
    }

    let mut payload = Vec::with_capacity(packet.len());
    encode_varint(HANDSHAKE_PACKET_ID, &mut payload);
    encode_varint(protocol_version, &mut payload);
    encode_varint(rewritten_host.len() as i32, &mut payload);
    payload.extend_from_slice(&rewritten_host);
    payload.extend_from_slice(&target_port.to_be_bytes());
    encode_varint(next_state, &mut payload);

    let mut framed_packet = Vec::with_capacity(payload.len() + 3);
    encode_varint(payload.len() as i32, &mut framed_packet);
    framed_packet.extend_from_slice(&payload);

    Ok((framed_packet, protocol_version, next_state, has_fml_suffix))
}

async fn handle_client(
    mut client: TcpStream,
    peer: SocketAddr,
    forward: Forward,
) -> io::Result<()> {
    client.set_nodelay(true)?;
    println!("[*] New connection: {peer}");

    let packet_length = read_varint_async(&mut client).await?;
    let packet_length =
        usize::try_from(packet_length).map_err(|_| invalid_data("negative handshake length"))?;
    if packet_length == 0 || packet_length > MAX_HANDSHAKE_SIZE {
        return Err(invalid_data("invalid handshake length"));
    }

    let mut packet = vec![0_u8; packet_length];
    client.read_exact(&mut packet).await?;
    let (handshake, protocol_version, next_state, has_fml_suffix) =
        rewrite_handshake(&packet, &forward.target_host, forward.target_port)?;

    let mode = if next_state == 1 { "status" } else { "login" };
    println!(
        "[*] Handshake: peer={peer}, protocol={protocol_version}, mode={mode}, fml={has_fml_suffix}"
    );

    let mut target = TcpStream::connect(forward.target_address()).await?;
    target.set_nodelay(true)?;
    let target_peer = target.peer_addr()?;
    println!(
        "[+] Connected: peer={peer}, target={}:{} ({target_peer})",
        forward.target_host, forward.target_port
    );
    target.write_all(&handshake).await?;

    let (client_to_server, server_to_client) =
        tokio::io::copy_bidirectional(&mut client, &mut target).await?;
    println!(
        "[*] Connection closed: peer={peer}, sent={client_to_server} B, received={server_to_client} B"
    );

    Ok(())
}

async fn run_listener(listener: TcpListener, forward: Forward) -> io::Result<()> {
    println!(
        "[*] Forwarding {} -> {}",
        forward.listen,
        forward.target_address()
    );

    loop {
        let (socket, peer) = listener.accept().await?;
        let forward = forward.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_client(socket, peer, forward).await {
                eprintln!("[-] Connection failed: peer={peer}, error={error}");
            }
        });
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let forwards = match parse_forwards(env::args().skip(1), env::var(SERVERS_ENV).ok().as_deref())
    {
        Ok(forwards) => forwards,
        Err(error) => {
            if !error.is_empty() {
                eprintln!("error: {error}\n");
            }
            print_usage();
            process::exit(if error.is_empty() { 0 } else { 2 });
        }
    };

    let mut listeners = Vec::with_capacity(forwards.len());
    for forward in forwards {
        let listener = TcpListener::bind(forward.listen).await?;
        listeners.push((listener, forward));
    }

    println!("=== HypForward started ===");
    let mut tasks = tokio::task::JoinSet::new();
    for (listener, forward) in listeners {
        tasks.spawn(run_listener(listener, forward));
    }

    match tasks.join_next().await {
        Some(result) => result.map_err(io::Error::other)?,
        None => Err(io::Error::other("no listeners started")),
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

        let (rewritten, protocol, state, has_fml) =
            rewrite_handshake(&packet, "play.example.com", 25566).unwrap();
        let decoded = decode_framed_handshake(&rewritten);

        assert_eq!(protocol, 47);
        assert_eq!(state, 2);
        assert!(!has_fml);
        assert_eq!(decoded, (47, b"play.example.com".to_vec(), 25566, 2));
    }

    #[test]
    fn preserves_forge_suffix() {
        let packet = handshake(b"proxy.example.com\0FML\0", 47, 2);

        let (rewritten, _, _, has_fml) =
            rewrite_handshake(&packet, "play.example.com", 25565).unwrap();
        let (_, host, _, _) = decode_framed_handshake(&rewritten);

        assert!(has_fml);
        assert_eq!(host, b"play.example.com\0FML\0");
    }

    #[test]
    fn rejects_invalid_next_state() {
        let packet = handshake(b"proxy.example.com", 47, 3);

        let error = rewrite_handshake(&packet, "play.example.com", 25565).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut packet = handshake(b"proxy.example.com", 47, 1);
        packet.push(0);

        let error = rewrite_handshake(&packet, "play.example.com", 25565).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn parses_multiple_forwarding_rules() {
        let forwards = parse_forwards(
            [
                "--forward",
                "0.0.0.0:25565=mc.hypixel.net:25565",
                "-f",
                "127.0.0.1:25566=example.com:25567",
            ],
            None,
        )
        .unwrap();

        assert_eq!(forwards.len(), 2);
        assert_eq!(forwards[0].target_host, "mc.hypixel.net");
        assert_eq!(forwards[1].listen, "127.0.0.1:25566".parse().unwrap());
        assert_eq!(forwards[1].target_port, 25567);
    }

    #[test]
    fn parses_rules_from_environment() {
        let forwards = parse_forwards(
            std::iter::empty::<&str>(),
            Some("0.0.0.0:25565=one.example:25565,0.0.0.0:25566=two.example:25565"),
        )
        .unwrap();

        assert_eq!(forwards.len(), 2);
        assert_eq!(forwards[1].target_host, "two.example");
    }

    #[test]
    fn rejects_duplicate_listeners() {
        let error = parse_forwards(
            std::iter::empty::<&str>(),
            Some("0.0.0.0:25565=one.example:25565,0.0.0.0:25565=two.example:25565"),
        )
        .unwrap_err();

        assert!(error.contains("duplicate listen address"));
    }

    #[test]
    fn supports_ipv6_targets() {
        let forward = Forward::parse("[::]:25565=[2001:db8::1]:25565").unwrap();

        assert_eq!(forward.target_host, "2001:db8::1");
        assert_eq!(forward.target_address(), "[2001:db8::1]:25565");
    }
}
