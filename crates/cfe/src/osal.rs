//! OSAL wrappers, including `TcpStream` and queue property helpers.

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
        if let (Some(dest), Some(src)) = (host_buf.get_mut(..len), host.as_bytes().get(..len)) {
            dest.copy_from_slice(src);
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

/// Properties of an OSAL queue returned by [`queue_get_info`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueProp {
    /// Object ID of the queue creator task.
    pub creator: cfe_sys::osal_id_t,
}

/// Queries properties of an OSAL queue by its ID.
///
/// # Errors
///
/// Returns `Err(Status)` if `queue_id` is invalid or OSAL returns an error.
pub fn queue_get_info(queue_id: cfe_sys::osal_id_t) -> Result<QueueProp, Status> {
    let mut prop = core::mem::MaybeUninit::<cfe_sys::OS_queue_prop_t>::uninit();
    // SAFETY: prop points to a valid local uninitialized struct.
    let status = unsafe { cfe_sys::OS_QueueGetInfo(queue_id, prop.as_mut_ptr()) };
    let status = Status::from_raw(status);
    if status.is_success() {
        // SAFETY: OS_QueueGetInfo initialized prop on success.
        let prop = unsafe { prop.assume_init() };
        Ok(QueueProp {
            creator: prop.creator,
        })
    } else {
        Err(status)
    }
}

/// Finds an OSAL queue ID by its name.
///
/// # Errors
///
/// Returns `Err(Status)` if the queue name is not found or is too long.
pub fn queue_get_id_by_name(name: &str) -> Result<cfe_sys::osal_id_t, Status> {
    let max_len = cfe_sys::OS_MAX_API_NAME as usize;
    if name.len() >= max_len {
        return Err(Status::from_raw(cfe_sys::OS_ERR_NAME_TOO_LONG));
    }
    let mut name_buf = [0u8; cfe_sys::OS_MAX_API_NAME as usize];
    if let (Some(dest), Some(src)) = (
        name_buf.get_mut(..name.len()),
        name.as_bytes().get(..name.len()),
    ) {
        dest.copy_from_slice(src);
    }
    let mut queue_id = cfe_sys::OS_OBJECT_ID_UNDEFINED;
    // SAFETY: queue_id is a valid out-pointer; name_buf is null-terminated and bounded by OS_MAX_API_NAME.
    let status =
        unsafe { cfe_sys::OS_QueueGetIdByName(&raw mut queue_id, name_buf.as_ptr().cast()) };
    let status = Status::from_raw(status);
    if status.is_success() {
        Ok(queue_id)
    } else {
        Err(status)
    }
}
