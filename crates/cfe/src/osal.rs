//! OSAL wrappers, including `TcpStream`.

use crate::status::Status;

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
        let mut sock_id: cfe_sys::osal_id_t = 0;
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

        let mut addr = cfe_sys::OS_SockAddr_t {
            ActualLength: 0,
            AddrData: cfe_sys::OS_SockAddrData_t {
                Buffer: [0; cfe_sys::OS_SOCKADDR_MAX_LEN as usize],
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
            // SAFETY: close socket on error.
            unsafe { cfe_sys::OS_close(sock_id) };
            return Err(Status::from_raw(init_status));
        }

        // Copy host to null-terminated stack buffer.
        let mut host_buf = [0u8; 64];
        let len = host.len().min(63);
        let mut i = 0usize;
        for b in host.as_bytes().iter().take(len) {
            if let Some(slot) = host_buf.get_mut(i) {
                *slot = *b;
            }
            i = i.saturating_add(1);
        }
        if let Some(slot) = host_buf.get_mut(i) {
            *slot = 0;
        }

        // SAFETY: addr and host_buf are valid.
        let addr_status =
            unsafe { cfe_sys::OS_SocketAddrFromString(&raw mut addr, host_buf.as_ptr().cast()) };
        if addr_status != cfe_sys::OS_SUCCESS.cast_signed() {
            // SAFETY: close socket on error.
            unsafe { cfe_sys::OS_close(sock_id) };
            return Err(Status::from_raw(addr_status));
        }

        // SAFETY: addr is valid.
        let port_status = unsafe { cfe_sys::OS_SocketAddrSetPort(&raw mut addr, port) };
        if port_status != cfe_sys::OS_SUCCESS.cast_signed() {
            // SAFETY: close socket on error.
            unsafe { cfe_sys::OS_close(sock_id) };
            return Err(Status::from_raw(port_status));
        }

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
    /// Returns `Err(Status)` if OSAL recv fails.
    pub fn recv(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, Status> {
        // SAFETY: buf is a valid buffer of length buf.len().
        let nbytes = unsafe {
            cfe_sys::OS_SocketRecvFrom(
                self.id,
                buf.as_mut_ptr().cast(),
                buf.len(),
                core::ptr::null_mut(),
                timeout_ms,
            )
        };
        usize::try_from(nbytes).map_err(|_| Status::from_raw(nbytes))
    }

    /// Sends bytes to the socket.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if OSAL send fails.
    pub fn send(&mut self, buf: &[u8]) -> Result<usize, Status> {
        // SAFETY: buf is a valid buffer of length buf.len().
        let nbytes = unsafe {
            cfe_sys::OS_SocketSendTo(self.id, buf.as_ptr().cast(), buf.len(), core::ptr::null())
        };
        usize::try_from(nbytes).map_err(|_| Status::from_raw(nbytes))
    }

    /// Closes the socket explicitly.
    ///
    /// # Errors
    ///
    /// Returns `Err(Status)` if OSAL close fails.
    pub fn close(mut self) -> Result<(), Status> {
        let id = self.id;
        self.id = 0;
        // SAFETY: close valid socket id.
        let status = unsafe { cfe_sys::OS_close(id) };
        Status::from_raw(status).result()
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        if self.id != 0 {
            // SAFETY: Clean up open socket resource if not already closed.
            unsafe {
                cfe_sys::OS_close(self.id);
            }
        }
    }
}
