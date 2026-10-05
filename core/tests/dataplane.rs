//! End-to-end check that packets pushed into the core reach the Shadowsocks
//! server, without a real NetworkExtension.
//!
//! A fake Shadowsocks server just accepts TCP connections. The test pushes a
//! hand-built IPv4/TCP SYN into the core and asserts that the core tries to
//! originate a connection to the server.

use std::{
    net::{Ipv4Addr, TcpListener},
    os::raw::{c_char, c_void},
    ptr,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use socks_core::{SocksPacket, socks_core_push, socks_core_start, socks_core_stop};

static OUTBOUND: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

unsafe extern "C" fn record_send(packets: *const SocksPacket, count: usize, _ctx: *mut c_void) {
    if packets.is_null() {
        return;
    }
    let mut outbound = OUTBOUND.lock().unwrap();
    for index in 0..count {
        let packet = &*packets.add(index);
        if packet.data.is_null() || packet.len == 0 {
            continue;
        }
        outbound.push(std::slice::from_raw_parts(packet.data, packet.len).to_vec());
    }
}

unsafe extern "C" fn ignore_event(_event_json: *const c_char, _ctx: *mut c_void) {}

fn checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    let mut chunks = data.chunks_exact(2);
    for chunk in &mut chunks {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    if let [last] = chunks.remainder() {
        sum += (*last as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// Returns whether `packet` is an IPv4/TCP segment with both SYN and ACK set.
fn is_tcp_syn_ack(packet: &[u8]) -> bool {
    if packet.len() < 20 || packet[0] >> 4 != 4 {
        return false;
    }
    let ihl = (packet[0] & 0x0f) as usize * 4;
    if ihl < 20 || packet.len() < ihl + 20 || packet[9] != 6 {
        return false;
    }
    packet[ihl + 13] & 0x12 == 0x12
}

fn ipv4_tcp_syn(src: Ipv4Addr, src_port: u16, dst: Ipv4Addr, dst_port: u16) -> Vec<u8> {
    let mut tcp = Vec::with_capacity(20);
    tcp.extend_from_slice(&src_port.to_be_bytes());
    tcp.extend_from_slice(&dst_port.to_be_bytes());
    tcp.extend_from_slice(&1u32.to_be_bytes()); // sequence number
    tcp.extend_from_slice(&0u32.to_be_bytes()); // acknowledgment
    tcp.push(0x50); // data offset = 5 words
    tcp.push(0x02); // SYN
    tcp.extend_from_slice(&64240u16.to_be_bytes()); // window
    tcp.extend_from_slice(&0u16.to_be_bytes()); // checksum placeholder
    tcp.extend_from_slice(&0u16.to_be_bytes()); // urgent pointer

    let mut pseudo = Vec::new();
    pseudo.extend_from_slice(&src.octets());
    pseudo.extend_from_slice(&dst.octets());
    pseudo.push(0);
    pseudo.push(6); // TCP
    pseudo.extend_from_slice(&(tcp.len() as u16).to_be_bytes());
    pseudo.extend_from_slice(&tcp);
    let tcp_checksum = checksum(&pseudo);
    tcp[16..18].copy_from_slice(&tcp_checksum.to_be_bytes());

    let total_len = 20 + tcp.len();
    let mut ip = Vec::with_capacity(total_len);
    ip.push(0x45); // IPv4, IHL = 5
    ip.push(0); // DSCP/ECN
    ip.extend_from_slice(&(total_len as u16).to_be_bytes());
    ip.extend_from_slice(&0u16.to_be_bytes()); // identification
    ip.extend_from_slice(&0x4000u16.to_be_bytes()); // flags: don't fragment
    ip.push(64); // TTL
    ip.push(6); // protocol: TCP
    ip.extend_from_slice(&0u16.to_be_bytes()); // checksum placeholder
    ip.extend_from_slice(&src.octets());
    ip.extend_from_slice(&dst.octets());
    let ip_checksum = checksum(&ip);
    ip[10..12].copy_from_slice(&ip_checksum.to_be_bytes());

    ip.extend_from_slice(&tcp);
    ip
}

#[test]
fn pushed_syn_connects_to_the_shadowsocks_server() {
    // Fake Shadowsocks server: accept one TCP connection.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake server");
    let server_port = listener.local_addr().unwrap().port();
    let accepted = std::sync::Arc::new(AtomicBool::new(false));
    {
        let accepted = accepted.clone();
        std::thread::spawn(move || {
            if listener.accept().is_ok() {
                accepted.store(true, Ordering::SeqCst);
            }
        });
    }

    let config = format!(
        r#"{{"server":"127.0.0.1","server_port":{server_port},"method":"aes-256-gcm","password":"test-password"}}"#
    );
    let config = std::ffi::CString::new(config).unwrap();
    let address = std::ffi::CString::new("10.111.222.0").unwrap();
    let netmask = std::ffi::CString::new("255.255.255.0").unwrap();
    let mut error = [0 as c_char; 1024];

    let status = unsafe {
        socks_core_start(
            config.as_ptr(),
            address.as_ptr(),
            netmask.as_ptr(),
            record_send,
            ignore_event,
            ptr::null_mut(),
            error.as_mut_ptr(),
            error.len(),
        )
    };
    assert_eq!(status, 0, "start failed: {}", String::from_utf8_lossy(&error.map(|c| c as u8)));

    let syn = ipv4_tcp_syn(
        Ipv4Addr::new(10, 111, 222, 1),
        40000,
        Ipv4Addr::new(93, 184, 216, 34), // example.com
        80,
    );
    let packet = SocksPacket {
        data: syn.as_ptr(),
        len: syn.len(),
    };
    let pushed = unsafe { socks_core_push(&packet, 1) };
    assert_eq!(pushed, 0, "push failed");

    let deadline = Instant::now() + Duration::from_secs(10);
    while !accepted.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }

    // The core must also have produced an outbound SYN-ACK for the tunnel.
    let saw_syn_ack = OUTBOUND.lock().unwrap().iter().any(|packet| is_tcp_syn_ack(packet));

    assert!(accepted.load(Ordering::SeqCst), "the Shadowsocks server never received a connection");
    assert!(saw_syn_ack, "the core never sent a SYN-ACK back to the tunnel");

    socks_core_stop();
}
