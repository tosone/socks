//! Virtual TUN device bridged to a native packet flow.
//!
//! `NEPacketTunnelFlow` does not expose a file descriptor, so the
//! `shadowsocks-service` TUN service cannot use a real `utun`. Instead we
//! implement [`TunStream`] on top of two directions:
//!
//! * inbound: the extension pushes packets with [`crate::socks_core_push`],
//! * outbound: the core calls the native send callback.

use std::{
    ffi::CString,
    io,
    net::IpAddr,
    os::raw::{c_char, c_void},
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use shadowsocks_service::local::tun::TunStream;
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::mpsc,
};

/// One raw IP packet, as seen by the native side.
#[repr(C)]
pub struct SocksPacket {
    /// Pointer to the packet bytes. Valid only for the duration of the call.
    pub data: *const u8,
    /// Length of the packet in bytes.
    pub len: usize,
}

/// Hands packets produced by the core back to the tunnel.
///
/// Called from a core worker thread; implementations must be thread-safe.
pub type SocksSendFn = unsafe extern "C" fn(packets: *const SocksPacket, count: usize, ctx: *mut c_void);

/// Reports a JSON event (`{"type":"status"|"traffic"|"error", ...}`).
pub type SocksEventFn = unsafe extern "C" fn(event_json: *const c_char, ctx: *mut c_void);

/// Owns a [`SocksSendFn`] and its opaque context.
///
/// # Safety
///
/// The caller guarantees that `ctx` stays valid until the core is stopped and
/// that the callback itself is safe to call from any thread.
#[derive(Clone, Copy)]
pub(crate) struct SendCallback {
    f: SocksSendFn,
    ctx: *mut c_void,
}

unsafe impl Send for SendCallback {}
unsafe impl Sync for SendCallback {}

impl SendCallback {
    pub(crate) fn new(f: SocksSendFn, ctx: *mut c_void) -> Self {
        Self { f, ctx }
    }

    pub(crate) fn send(&self, packet: &[u8]) {
        if packet.is_empty() {
            return;
        }
        let c_packet = SocksPacket {
            data: packet.as_ptr(),
            len: packet.len(),
        };
        // SAFETY: the caller guaranteed the callback and context are valid.
        unsafe { (self.f)(&c_packet, 1, self.ctx) };
    }
}

/// Same ownership rules as [`SendCallback`].
#[derive(Clone, Copy)]
pub(crate) struct EventCallback {
    f: SocksEventFn,
    ctx: *mut c_void,
}

unsafe impl Send for EventCallback {}
unsafe impl Sync for EventCallback {}

impl EventCallback {
    pub(crate) fn new(f: SocksEventFn, ctx: *mut c_void) -> Self {
        Self { f, ctx }
    }

    pub(crate) fn emit(&self, json: &str) {
        let Ok(message) = CString::new(json) else {
            log::warn!("dropping event containing NUL bytes");
            return;
        };
        // SAFETY: the caller guaranteed the callback and context are valid.
        unsafe { (self.f)(message.as_ptr(), self.ctx) };
    }
}

/// A [`TunStream`] whose packets come from and go to the host extension.
pub(crate) struct VirtualDevice {
    address: IpAddr,
    netmask: IpAddr,
    rx: mpsc::UnboundedReceiver<Bytes>,
    send: SendCallback,
    /// Tail of a packet that did not fit into the caller's read buffer.
    pending: Option<Bytes>,
    pending_offset: usize,
}

impl VirtualDevice {
    pub(crate) fn new(
        address: IpAddr,
        netmask: IpAddr,
        rx: mpsc::UnboundedReceiver<Bytes>,
        send: SendCallback,
    ) -> Self {
        Self {
            address,
            netmask,
            rx,
            send,
            pending: None,
            pending_offset: 0,
        }
    }
}

impl TunStream for VirtualDevice {
    fn address(&self) -> IpAddr {
        self.address
    }

    fn netmask(&self) -> IpAddr {
        self.netmask
    }

    fn tun_name(&self) -> String {
        "nepacketflow".to_owned()
    }
}

impl AsyncRead for VirtualDevice {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();

        if let Some(packet) = this.pending.take() {
            let len = packet.len() - this.pending_offset;
            let n = len.min(buf.remaining());
            buf.put_slice(&packet[this.pending_offset..this.pending_offset + n]);
            this.pending_offset += n;
            if this.pending_offset < packet.len() {
                this.pending = Some(packet);
            } else {
                this.pending_offset = 0;
            }
            return Poll::Ready(Ok(()));
        }

        match this.rx.poll_recv(cx) {
            Poll::Ready(Some(packet)) => {
                let n = packet.len().min(buf.remaining());
                buf.put_slice(&packet[..n]);
                if n < packet.len() {
                    this.pending = Some(packet);
                    this.pending_offset = n;
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(None) => Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for VirtualDevice {
    fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        this.send.send(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use tokio::io::AsyncReadExt;

    unsafe extern "C" fn noop_send(_: *const SocksPacket, _: usize, _: *mut c_void) {}

    #[tokio::test]
    async fn reads_pushed_packets_in_order() {
        let (tx, rx) = mpsc::unbounded_channel();
        let send: SocksSendFn = noop_send;
        let mut device = VirtualDevice::new(
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(255, 255, 255, 0)),
            rx,
            SendCallback::new(send, std::ptr::null_mut()),
        );

        tx.send(Bytes::from_static(&[1, 2, 3])).unwrap();
        tx.send(Bytes::from_static(&[4, 5])).unwrap();

        let mut buf = [0u8; 8];
        let n = device.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], &[1, 2, 3]);
        let n = device.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], &[4, 5]);
    }

    #[tokio::test]
    async fn splits_packets_larger_than_the_read_buffer() {
        let (tx, rx) = mpsc::unbounded_channel();
        let send: SocksSendFn = noop_send;
        let mut device = VirtualDevice::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            rx,
            SendCallback::new(send, std::ptr::null_mut()),
        );

        tx.send(Bytes::from_static(&[1, 2, 3, 4])).unwrap();

        let mut buf = [0u8; 2];
        let n = device.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], &[1, 2]);
        let n = device.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], &[3, 4]);
    }
}
