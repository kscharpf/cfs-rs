//! OSAL wrappers, including `TcpStream`, `UdpSocket`, `local_time_us`, and task delay.

use crate::status::Status;

#[expect(
    clippy::as_conversions,
    reason = "OS_SOCKADDR_MAX_LEN is a C bindgen u32 constant used as buffer size"
)]
const OS_SOCKADDR_MAX_LEN: usize = cfe_sys::OS_SOCKADDR_MAX_LEN as usize;

/// Initializes an OSAL IPv4 socket address from a dotted-decimal `host` and `port`.
fn make_inet_addr(host: &str, port: u16) -> Result<cfe_sys::OS_SockAddr_t, Status> {
    if host.is_empty() || host.len() >= 64 || host.as_bytes().contains(&0) {
        return Err(Status::from_raw(cfe_sys::OS_ERR_BAD_ADDRESS));
    }

    let mut addr = cfe_sys::OS_SockAddr_t {
        ActualLength: 0,
        AddrData: cfe_sys::OS_SockAddrData_t {
            Buffer: [0; OS_SOCKADDR_MAX_LEN],
        },
    };

    // SAFETY: addr is a valid out-pointer.
    let init_status = unsafe {
        cfe_sys::OS_SocketAddrInit(
            &raw mut addr,
            cfe_sys::OS_SocketDomain_t_OS_SocketDomain_INET,
        )
    };
    if init_status != cfe_sys::OS_SUCCESS.cast_signed() {
        return Err(Status::from_raw(init_status));
    }

    let mut host_buf = [0u8; 64];
    let len = host.len();
    if let (Some(dest), Some(src)) = (host_buf.get_mut(..len), host.as_bytes().get(..len)) {
        dest.copy_from_slice(src);
    }

    // SAFETY: addr and null-terminated host_buf are valid.
    let addr_status =
        unsafe { cfe_sys::OS_SocketAddrFromString(&raw mut addr, host_buf.as_ptr().cast()) };
    if addr_status != cfe_sys::OS_SUCCESS.cast_signed() {
        return Err(Status::from_raw(addr_status));
    }

    // SAFETY: addr is valid.
    let port_status = unsafe { cfe_sys::OS_SocketAddrSetPort(&raw mut addr, port) };
    if port_status != cfe_sys::OS_SUCCESS.cast_signed() {
        return Err(Status::from_raw(port_status));
    }

    Ok(addr)
}

fn write_u8_ascii(mut val: u8, buf: &mut [u8], pos: &mut usize) {
    let hundreds = val / 100;
    val %= 100;
    let tens = val / 10;
    let ones = val % 10;
    if hundreds > 0 {
        if let Some(slot) = buf.get_mut(*pos) {
            *slot = b'0'.saturating_add(hundreds);
        }
        *pos = pos.saturating_add(1);
    }
    if hundreds > 0 || tens > 0 {
        if let Some(slot) = buf.get_mut(*pos) {
            *slot = b'0'.saturating_add(tens);
        }
        *pos = pos.saturating_add(1);
    }
    if let Some(slot) = buf.get_mut(*pos) {
        *slot = b'0'.saturating_add(ones);
    }
    *pos = pos.saturating_add(1);
}

/// Formats an IPv4 address `[a, b, c, d]` as dotted-decimal ASCII into `buf` and returns its byte length.
fn format_ipv4(ip: [u8; 4], buf: &mut [u8; 16]) -> usize {
    let mut pos = 0usize;
    for (idx, octet) in ip.iter().enumerate() {
        if idx > 0 {
            if let Some(slot) = buf.get_mut(pos) {
                *slot = b'.';
            }
            pos = pos.saturating_add(1);
        }
        write_u8_ascii(*octet, buf, &mut pos);
    }
    pos
}

/// A TCP stream connected via OSAL stream sockets.
#[derive(Debug)]
pub struct TcpStream {
    id: cfe_sys::osal_id_t,
}

impl TcpStream {
    /// Connects to a remote IPv4 host and port via OSAL.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if socket creation, address resolution, or connection fails.
    pub fn connect(host: &str, port: u16) -> Result<Self, Status> {
        let mut sock_id: cfe_sys::osal_id_t = cfe_sys::OS_OBJECT_ID_UNDEFINED;
        // SAFETY: CFE/OSAL opens an INET STREAM socket into sock_id.
        let status = unsafe {
            cfe_sys::OS_SocketOpen(
                &raw mut sock_id,
                cfe_sys::OS_SocketDomain_t_OS_SocketDomain_INET,
                cfe_sys::OS_SocketType_t_OS_SocketType_STREAM,
            )
        };
        if status != cfe_sys::OS_SUCCESS.cast_signed() {
            return Err(Status::from_raw(status));
        }

        let addr = match make_inet_addr(host, port) {
            Ok(addr) => addr,
            Err(err) => {
                // SAFETY: close socket on error.
                unsafe { cfe_sys::OS_close(sock_id) };
                return Err(err);
            }
        };

        // Connect with timeout: 5000 ms.
        // SAFETY: sock_id and addr are valid.
        let conn_status = unsafe { cfe_sys::OS_SocketConnect(sock_id, &raw const addr, 5000) };
        if conn_status != cfe_sys::OS_SUCCESS.cast_signed() {
            // SAFETY: close socket on error.
            unsafe { cfe_sys::OS_close(sock_id) };
            return Err(Status::from_raw(conn_status));
        }

        Ok(Self { id: sock_id })
    }

    /// Receives bytes from the socket into `buf`.
    ///
    /// Pass timeout in ms (or -1 for indefinite wait).
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if OSAL read fails.
    pub fn recv(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, Status> {
        // SAFETY: buf is a valid buffer of length buf.len().
        let nbytes = unsafe {
            if timeout_ms == -1 {
                cfe_sys::OS_read(self.id, buf.as_mut_ptr().cast(), buf.len())
            } else {
                cfe_sys::OS_TimedRead(self.id, buf.as_mut_ptr().cast(), buf.len(), timeout_ms)
            }
        };
        usize::try_from(nbytes).map_err(|_| Status::from_raw(nbytes))
    }

    /// Reads the exact number of bytes required to fill `buf`.
    ///
    /// Blocks until `buf.len()` bytes are read, or returns an error if EOF or an OSAL error occurs.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` with `OS_ERR_STREAM_DISCONNECTED` if EOF occurs before filling `buf`,
    /// or with the corresponding OSAL error code.
    pub fn read_exact(&mut self, mut buf: &mut [u8]) -> Result<(), Status> {
        while !buf.is_empty() {
            let n = self.recv(buf, -1)?;
            if n == 0 {
                return Err(Status::from_raw(cfe_sys::OS_ERR_STREAM_DISCONNECTED));
            }
            let Some(rest) = buf.get_mut(n..) else {
                return Err(Status::from_raw(cfe_sys::OS_ERROR));
            };
            buf = rest;
        }
        Ok(())
    }

    /// Sends bytes to the socket.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if OSAL write fails.
    pub fn send(&mut self, buf: &[u8]) -> Result<usize, Status> {
        // SAFETY: buf is a valid buffer of length buf.len().
        let nbytes = unsafe { cfe_sys::OS_write(self.id, buf.as_ptr().cast(), buf.len()) };
        usize::try_from(nbytes).map_err(|_| Status::from_raw(nbytes))
    }

    /// Writes all bytes in `buf` to the stream.
    ///
    /// Loops until all bytes are transmitted, or returns an error.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if OSAL send fails or stream disconnects.
    pub fn write_all(&mut self, mut buf: &[u8]) -> Result<(), Status> {
        while !buf.is_empty() {
            let n = self.send(buf)?;
            if n == 0 {
                return Err(Status::from_raw(cfe_sys::OS_ERR_STREAM_DISCONNECTED));
            }
            let Some(rest) = buf.get(n..) else {
                return Err(Status::from_raw(cfe_sys::OS_ERROR));
            };
            buf = rest;
        }
        Ok(())
    }

    /// Closes the socket explicitly.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if OSAL close fails.
    pub fn close(mut self) -> Result<(), Status> {
        let id = self.id;
        self.id = cfe_sys::OS_OBJECT_ID_UNDEFINED;
        // SAFETY: close valid socket id.
        let status = unsafe { cfe_sys::OS_close(id) };
        Status::from_raw(status).result()
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        if self.id != cfe_sys::OS_OBJECT_ID_UNDEFINED {
            // SAFETY: Clean up open socket resource if not already closed.
            unsafe {
                cfe_sys::OS_close(self.id);
            }
        }
    }
}

/// Timeout for OSAL socket receive operations.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Timeout {
    /// Wait forever until data arrives (`OS_PEND`).
    PendForever,
    /// Do not wait; poll the socket (`OS_CHECK`).
    Poll,
    /// Wait up to the specified number of milliseconds.
    Millis(u32),
}

impl Timeout {
    /// Convert to the raw `i32` millisecond timeout expected by OSAL socket APIs.
    #[must_use]
    pub const fn to_raw(self) -> i32 {
        match self {
            Self::PendForever => cfe_sys::OS_PEND,
            Self::Poll => cfe_sys::OS_CHECK.cast_signed(),
            Self::Millis(ms) => {
                if ms > i32::MAX.cast_unsigned() {
                    i32::MAX
                } else {
                    ms.cast_signed()
                }
            }
        }
    }
}

/// A UDP datagram socket bound via OSAL datagram sockets.
#[derive(Debug)]
pub struct UdpSocket {
    id: cfe_sys::osal_id_t,
}

impl UdpSocket {
    /// Opens an IPv4 datagram socket and binds it to `host:port`.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if socket creation, address parsing, or binding fails.
    pub fn bind(host: &str, port: u16) -> Result<Self, Status> {
        let mut sock_id: cfe_sys::osal_id_t = cfe_sys::OS_OBJECT_ID_UNDEFINED;
        // SAFETY: CFE/OSAL opens an INET DATAGRAM socket into sock_id.
        let status = unsafe {
            cfe_sys::OS_SocketOpen(
                &raw mut sock_id,
                cfe_sys::OS_SocketDomain_t_OS_SocketDomain_INET,
                cfe_sys::OS_SocketType_t_OS_SocketType_DATAGRAM,
            )
        };
        if status != cfe_sys::OS_SUCCESS.cast_signed() {
            return Err(Status::from_raw(status));
        }

        let addr = match make_inet_addr(host, port) {
            Ok(addr) => addr,
            Err(err) => {
                // SAFETY: close socket on error.
                unsafe { cfe_sys::OS_close(sock_id) };
                return Err(err);
            }
        };

        // SAFETY: sock_id and addr are valid.
        let bind_status = unsafe { cfe_sys::OS_SocketBind(sock_id, &raw const addr) };
        if bind_status != cfe_sys::OS_SUCCESS.cast_signed() {
            // SAFETY: close socket on error.
            unsafe { cfe_sys::OS_close(sock_id) };
            return Err(Status::from_raw(bind_status));
        }

        Ok(Self { id: sock_id })
    }

    /// Receives a datagram into `buf`, waiting according to `timeout`.
    ///
    /// Returns `Ok(None)` if no datagram is available (`OS_ERROR_TIMEOUT` or `OS_QUEUE_EMPTY`).
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if OSAL socket receive fails with another error.
    pub fn recv_from(&self, buf: &mut [u8], timeout: Timeout) -> Result<Option<usize>, Status> {
        // SAFETY: buf is a valid buffer of length buf.len(); RemoteAddr may be NULL per OSAL API.
        let nbytes = unsafe {
            cfe_sys::OS_SocketRecvFrom(
                self.id,
                buf.as_mut_ptr().cast(),
                buf.len(),
                core::ptr::null_mut(),
                timeout.to_raw(),
            )
        };
        if nbytes == cfe_sys::OS_ERROR_TIMEOUT || nbytes == cfe_sys::OS_QUEUE_EMPTY {
            return Ok(None);
        }
        usize::try_from(nbytes)
            .map(Some)
            .map_err(|_| Status::from_raw(nbytes))
    }

    /// Sends a datagram to `host:port`.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if address resolution or OSAL `OS_SocketSendTo` fails.
    pub fn send_to(&self, buf: &[u8], host: &str, port: u16) -> Result<usize, Status> {
        let addr = make_inet_addr(host, port)?;
        // SAFETY: self.id, buf, and addr are valid.
        let nbytes = unsafe {
            cfe_sys::OS_SocketSendTo(self.id, buf.as_ptr().cast(), buf.len(), &raw const addr)
        };
        usize::try_from(nbytes).map_err(|_| Status::from_raw(nbytes))
    }

    /// Sends a datagram to an IPv4 address `[a, b, c, d]` and `port` without heap allocation.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if OSAL `OS_SocketSendTo` fails.
    pub fn send_to_ipv4(&self, buf: &[u8], ip: [u8; 4], port: u16) -> Result<usize, Status> {
        let mut ip_buf = [0u8; 16];
        let len = format_ipv4(ip, &mut ip_buf);
        let ip_slice = ip_buf
            .get(..len)
            .ok_or_else(|| Status::from_raw(cfe_sys::OS_ERR_BAD_ADDRESS))?;
        let host = core::str::from_utf8(ip_slice)
            .map_err(|_| Status::from_raw(cfe_sys::OS_ERR_BAD_ADDRESS))?;
        self.send_to(buf, host, port)
    }

    /// Closes the socket explicitly.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if OSAL close fails.
    pub fn close(mut self) -> Result<(), Status> {
        let id = self.id;
        self.id = cfe_sys::OS_OBJECT_ID_UNDEFINED;
        // SAFETY: close valid socket id.
        let status = unsafe { cfe_sys::OS_close(id) };
        Status::from_raw(status).result()
    }
}

impl Drop for UdpSocket {
    fn drop(&mut self) {
        if self.id != cfe_sys::OS_OBJECT_ID_UNDEFINED {
            // SAFETY: Clean up open socket resource if not already closed.
            unsafe {
                cfe_sys::OS_close(self.id);
            }
        }
    }
}

/// Returns the OSAL local clock in microseconds.
///
/// # Errors
///
/// Returns `Err(Status)` if `OS_GetLocalTime` fails or returns a negative timestamp.
pub fn local_time_us() -> Result<u64, Status> {
    let mut time = cfe_sys::OS_time_t { ticks: 0 };
    // SAFETY: time is a valid out-pointer for OS_GetLocalTime.
    let status = unsafe { cfe_sys::OS_GetLocalTime(&raw mut time) };
    if status != cfe_sys::OS_SUCCESS.cast_signed() {
        return Err(Status::from_raw(status));
    }
    let ticks = u64::try_from(time.ticks).map_err(|_| Status::from_raw(cfe_sys::OS_ERROR))?;
    let ticks_per_usec = u64::from(cfe_sys::OS_TIME_TICKS_PER_USEC);
    ticks
        .checked_div(ticks_per_usec)
        .ok_or_else(|| Status::from_raw(cfe_sys::OS_ERROR))
}

/// Delays the calling task for the specified duration in milliseconds.
///
/// # Errors
///
/// Returns `Err(Status)` if OSAL task delay fails.
pub fn task_delay(millisecond: u32) -> Result<(), Status> {
    // SAFETY: OS_TaskDelay is thread-safe and suspends the calling thread for the duration.
    let status = unsafe { cfe_sys::OS_TaskDelay(millisecond) };
    let status = Status::from_raw(status);
    if status.is_success() {
        Ok(())
    } else {
        Err(status)
    }
}
