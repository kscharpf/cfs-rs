//! Messages and framing of the node protocol (docs/reference-application.md §8.2, APP-092).
//!
//! A message frame is a 4-octet big-endian body length, followed by a body that starts
//! with a type octet. All integers are big-endian.
//! Decoding is strict: short buffers, trailing octets, unknown types/kinds/roles, or
//! invalid `tx_slot` values are errors.

/// Protocol version of HELLO and `HELLO_ACK` (§8.2).
pub const PROTOCOL_VERSION: u16 = 1;

/// Largest body accepted (1 MiB).
pub const MAX_BODY: usize = 1 << 20;

pub(crate) const T_HELLO: u8 = 0x01;
pub(crate) const T_STEP: u8 = 0x02;
pub(crate) const T_END: u8 = 0x03;
pub(crate) const T_HELLO_ACK: u8 = 0x81;
pub(crate) const T_STEP_RESULT: u8 = 0x82;
pub(crate) const T_END_ACK: u8 = 0x83;

/// Errors that can occur during message encoding or decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecError {
    /// Buffer is too small to write or read data.
    BufferTooSmall,
    /// Unexpected end of input buffer.
    UnexpectedEof,
    /// Trailing unparsed octets remain in the buffer.
    TrailingOctets(usize),
    /// Message body length is 0 or exceeds `MAX_BODY`.
    InvalidBodyLength(usize),
    /// Unknown message type octet.
    UnknownMessageType(u8),
    /// Unknown role value.
    UnknownRole(u8),
    /// Case ID is not ASCII or exceeds 255 octets.
    InvalidCaseId,
    /// Invalid `tx_slot` flag (must be 0 or 1).
    InvalidTxSlot(u8),
    /// Unknown node output kind.
    UnknownOutputKind(u8),
    /// Count does not fit within allowed field bounds (e.g. `u16` or `u8`).
    CountOverflow,
}

/// Role a node plays in the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The flight node (`0`).
    Flight,
    /// The ground node (`1`).
    Ground,
}

/// Arguments of an application event output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventArgs<'a> {
    /// Borrowed slice of `u64` values (for encoding).
    Slice(&'a [u64]),
    /// Big-endian raw byte slice from wire (for decoding).
    RawBe(&'a [u8]),
}

impl<'a> EventArgs<'a> {
    /// Returns the number of `u64` arguments.
    #[must_use]
    pub const fn len(&self) -> usize {
        match self {
            Self::Slice(slice) => slice.len(),
            Self::RawBe(bytes) => bytes.len().wrapping_shr(3),
        }
    }

    /// Returns `true` if there are no arguments.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns an iterator yielding each `u64` argument value.
    #[must_use]
    pub const fn iter(&self) -> EventArgsIter<'a> {
        EventArgsIter {
            args: *self,
            index: 0,
        }
    }
}

impl<'a> IntoIterator for &EventArgs<'a> {
    type Item = u64;
    type IntoIter = EventArgsIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over event arguments.
#[derive(Debug, Clone)]
pub struct EventArgsIter<'a> {
    args: EventArgs<'a>,
    index: usize,
}

impl Iterator for EventArgsIter<'_> {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        match self.args {
            EventArgs::Slice(slice) => {
                let val = slice.get(self.index)?;
                self.index = self.index.checked_add(1)?;
                Some(*val)
            }
            EventArgs::RawBe(bytes) => {
                let byte_offset = self.index.checked_mul(8)?;
                let byte_end = byte_offset.checked_add(8)?;
                let chunk = bytes.get(byte_offset..byte_end)?;
                let arr: [u8; 8] = chunk.try_into().ok()?;
                self.index = self.index.checked_add(1)?;
                Some(u64::from_be_bytes(arr))
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.args.len().saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for EventArgsIter<'_> {}

/// One output of a step, in output order (`STEP_RESULT`, §8.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeOutput<'a> {
    /// A link unit (kind 1).
    Unit(&'a [u8]),
    /// A packet an application generated: APID and octets (kind 2).
    PacketOut(u16, &'a [u8]),
    /// A packet delivered to an application: APID and octets (kind 3).
    PacketIn(u16, &'a [u8]),
    /// An application event: code and arguments (kind 4).
    Event(u16, EventArgs<'a>),
}

/// Iterator over link units in a `STEP` message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitsIter<'a> {
    slice: &'a [u8],
    count: u16,
}

impl<'a> UnitsIter<'a> {
    /// Creates a `UnitsIter` from raw unit payload bytes and unit count.
    #[must_use]
    pub const fn new(slice: &'a [u8], count: u16) -> Self {
        Self { slice, count }
    }

    /// Returns the number of units.
    #[must_use]
    pub fn count(&self) -> usize {
        usize::from(self.count)
    }

    /// Returns `true` if there are no units.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Returns an iterator over unit byte slices.
    #[must_use]
    pub const fn iter(&self) -> UnitsIterator<'a> {
        UnitsIterator {
            slice: self.slice,
            remaining: self.count,
        }
    }
}

impl<'a> IntoIterator for UnitsIter<'a> {
    type Item = &'a [u8];
    type IntoIter = UnitsIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &UnitsIter<'a> {
    type Item = &'a [u8];
    type IntoIter = UnitsIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator yielding each unit slice.
#[derive(Debug, Clone)]
pub struct UnitsIterator<'a> {
    slice: &'a [u8],
    remaining: u16,
}

impl<'a> Iterator for UnitsIterator<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let len_bytes = self.slice.get(..2)?;
        let len = usize::from(u16::from_be_bytes([
            *len_bytes.first()?,
            *len_bytes.get(1)?,
        ]));
        let total_len = 2usize.checked_add(len)?;
        let unit_data = self.slice.get(2..total_len)?;
        self.slice = self.slice.get(total_len..)?;
        self.remaining = self.remaining.checked_sub(1)?;
        Some(unit_data)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = usize::from(self.remaining);
        (rem, Some(rem))
    }
}

impl ExactSizeIterator for UnitsIterator<'_> {}

/// Iterator over ops-script entries in a `STEP` message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpsIter<'a> {
    slice: &'a [u8],
    count: u16,
}

impl<'a> OpsIter<'a> {
    /// Creates an `OpsIter` from raw ops payload bytes and count.
    #[must_use]
    pub const fn new(slice: &'a [u8], count: u16) -> Self {
        Self { slice, count }
    }

    /// Returns the number of ops entries.
    #[must_use]
    pub fn count(&self) -> usize {
        usize::from(self.count)
    }

    /// Returns `true` if there are no ops.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Returns an iterator over ops `u16` values.
    #[must_use]
    pub const fn iter(&self) -> OpsIterator<'a> {
        OpsIterator {
            slice: self.slice,
            remaining: self.count,
        }
    }
}

impl<'a> IntoIterator for OpsIter<'a> {
    type Item = u16;
    type IntoIter = OpsIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &OpsIter<'a> {
    type Item = u16;
    type IntoIter = OpsIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator yielding each ops entry.
#[derive(Debug, Clone)]
pub struct OpsIterator<'a> {
    slice: &'a [u8],
    remaining: u16,
}

impl Iterator for OpsIterator<'_> {
    type Item = u16;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let bytes = self.slice.get(..2)?;
        let val = u16::from_be_bytes([*bytes.first()?, *bytes.get(1)?]);
        self.slice = self.slice.get(2..)?;
        self.remaining = self.remaining.checked_sub(1)?;
        Some(val)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = usize::from(self.remaining);
        (rem, Some(rem))
    }
}

impl ExactSizeIterator for OpsIterator<'_> {}

/// Outputs of a `STEP_RESULT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outputs<'a> {
    /// Slice of `NodeOutput`s (for encoding).
    Slice(&'a [NodeOutput<'a>]),
    /// Wire-decoded iterator over outputs.
    Iter(OutputsIter<'a>),
}

impl Outputs<'_> {
    /// Returns the number of outputs.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Slice(slice) => slice.len(),
            Self::Iter(iter) => iter.count(),
        }
    }

    /// Returns `true` if there are no outputs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Iterator over outputs decoded from wire bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputsIter<'a> {
    slice: &'a [u8],
    count: u16,
}

impl<'a> OutputsIter<'a> {
    /// Creates an `OutputsIter` from raw slice and count.
    #[must_use]
    pub const fn new(slice: &'a [u8], count: u16) -> Self {
        Self { slice, count }
    }

    /// Returns the count of outputs.
    #[must_use]
    pub fn count(&self) -> usize {
        usize::from(self.count)
    }

    /// Returns an iterator yielding each output.
    #[must_use]
    pub const fn iter(&self) -> OutputsIterator<'a> {
        OutputsIterator {
            slice: self.slice,
            remaining: self.count,
        }
    }
}

impl<'a> IntoIterator for OutputsIter<'a> {
    type Item = NodeOutput<'a>;
    type IntoIter = OutputsIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> IntoIterator for &OutputsIter<'a> {
    type Item = NodeOutput<'a>;
    type IntoIter = OutputsIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator yielding each `NodeOutput`.
#[derive(Debug, Clone)]
pub struct OutputsIterator<'a> {
    slice: &'a [u8],
    remaining: u16,
}

impl<'a> Iterator for OutputsIterator<'a> {
    type Item = NodeOutput<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let kind = *self.slice.first()?;
        let rest = self.slice.get(1..)?;
        match kind {
            1 => {
                let len_bytes = rest.get(..2)?;
                let len = usize::from(u16::from_be_bytes([
                    *len_bytes.first()?,
                    *len_bytes.get(1)?,
                ]));
                let total = 2usize.checked_add(len)?;
                let octets = rest.get(2..total)?;
                self.slice = rest.get(total..)?;
                self.remaining = self.remaining.checked_sub(1)?;
                Some(NodeOutput::Unit(octets))
            }
            2 => {
                let apid_bytes = rest.get(..2)?;
                let apid = u16::from_be_bytes([*apid_bytes.first()?, *apid_bytes.get(1)?]);
                let len_bytes = rest.get(2..4)?;
                let len = usize::from(u16::from_be_bytes([
                    *len_bytes.first()?,
                    *len_bytes.get(1)?,
                ]));
                let total = 4usize.checked_add(len)?;
                let octets = rest.get(4..total)?;
                self.slice = rest.get(total..)?;
                self.remaining = self.remaining.checked_sub(1)?;
                Some(NodeOutput::PacketOut(apid, octets))
            }
            3 => {
                let apid_bytes = rest.get(..2)?;
                let apid = u16::from_be_bytes([*apid_bytes.first()?, *apid_bytes.get(1)?]);
                let len_bytes = rest.get(2..4)?;
                let len = usize::from(u16::from_be_bytes([
                    *len_bytes.first()?,
                    *len_bytes.get(1)?,
                ]));
                let total = 4usize.checked_add(len)?;
                let octets = rest.get(4..total)?;
                self.slice = rest.get(total..)?;
                self.remaining = self.remaining.checked_sub(1)?;
                Some(NodeOutput::PacketIn(apid, octets))
            }
            4 => {
                let code_bytes = rest.get(..2)?;
                let code = u16::from_be_bytes([*code_bytes.first()?, *code_bytes.get(1)?]);
                let args_count = usize::from(*rest.get(2)?);
                let args_len = args_count
                    .checked_mul(8)
                    .ok_or(CodecError::CountOverflow)
                    .ok()?;
                let total = 3usize.checked_add(args_len)?;
                let raw_args = rest.get(3..total)?;
                self.slice = rest.get(total..)?;
                self.remaining = self.remaining.checked_sub(1)?;
                Some(NodeOutput::Event(code, EventArgs::RawBe(raw_args)))
            }
            _ => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let rem = usize::from(self.remaining);
        (rem, Some(rem))
    }
}

impl ExactSizeIterator for OutputsIterator<'_> {}

/// A message of the node protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message<'a> {
    /// `0x01`: HELLO (harness to node).
    Hello {
        /// Protocol version.
        version: u16,
        /// Scenario version.
        scenario_version: u16,
        /// Role of the node.
        role: Role,
        /// ASCII case ID.
        case_id: &'a str,
    },
    /// `0x02`: STEP (harness to node).
    Step {
        /// Simulated time in microseconds.
        now: u64,
        /// Whether the link takes a unit in this step.
        tx_slot: bool,
        /// Delivered units, in order.
        units: UnitsIter<'a>,
        /// Indices of due ops-script entries.
        ops: OpsIter<'a>,
    },
    /// `0x03`: END (harness to node).
    End,
    /// `0x81`: `HELLO_ACK` (node to harness).
    HelloAck {
        /// Protocol version.
        version: u16,
    },
    /// `0x82`: `STEP_RESULT` (node to harness).
    StepResult {
        /// Outputs, in order.
        outputs: Outputs<'a>,
        /// Earliest instant of the next step, in microseconds (`None` is all-ones on the wire).
        next_deadline: Option<u64>,
    },
    /// `0x83`: `END_ACK` (node to harness).
    EndAck,
}

impl Message<'_> {
    /// Returns the name of the message type.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Hello { .. } => "HELLO",
            Self::Step { .. } => "STEP",
            Self::End => "END",
            Self::HelloAck { .. } => "HELLO_ACK",
            Self::StepResult { .. } => "STEP_RESULT",
            Self::EndAck => "END_ACK",
        }
    }
}

struct Cursor<'a> {
    buf: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(buf: &'a [u8]) -> Self {
        Self { buf, offset: 0 }
    }

    const fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.offset)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], CodecError> {
        let end = self
            .offset
            .checked_add(n)
            .ok_or(CodecError::UnexpectedEof)?;
        let slice = self
            .buf
            .get(self.offset..end)
            .ok_or(CodecError::UnexpectedEof)?;
        self.offset = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, CodecError> {
        let b = self.take(1)?;
        b.first().copied().ok_or(CodecError::UnexpectedEof)
    }

    fn u16(&mut self) -> Result<u16, CodecError> {
        let b = self.take(2)?;
        let arr: [u8; 2] = b.try_into().map_err(|_| CodecError::UnexpectedEof)?;
        Ok(u16::from_be_bytes(arr))
    }

    fn u32(&mut self) -> Result<u32, CodecError> {
        let b = self.take(4)?;
        let arr: [u8; 4] = b.try_into().map_err(|_| CodecError::UnexpectedEof)?;
        Ok(u32::from_be_bytes(arr))
    }

    fn u64(&mut self) -> Result<u64, CodecError> {
        let b = self.take(8)?;
        let arr: [u8; 8] = b.try_into().map_err(|_| CodecError::UnexpectedEof)?;
        Ok(u64::from_be_bytes(arr))
    }
}

struct Writer<'a> {
    buf: &'a mut [u8],
    offset: usize,
}

impl<'a> Writer<'a> {
    const fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, offset: 0 }
    }

    fn put_bytes(&mut self, slice: &[u8]) -> Result<(), CodecError> {
        let end = self
            .offset
            .checked_add(slice.len())
            .ok_or(CodecError::BufferTooSmall)?;
        let dest = self
            .buf
            .get_mut(self.offset..end)
            .ok_or(CodecError::BufferTooSmall)?;
        dest.copy_from_slice(slice);
        self.offset = end;
        Ok(())
    }

    fn put_u8(&mut self, val: u8) -> Result<(), CodecError> {
        self.put_bytes(&[val])
    }

    fn put_u16(&mut self, val: u16) -> Result<(), CodecError> {
        self.put_bytes(&val.to_be_bytes())
    }

    fn put_u64(&mut self, val: u64) -> Result<(), CodecError> {
        self.put_bytes(&val.to_be_bytes())
    }

    fn put_count(&mut self, n: usize) -> Result<(), CodecError> {
        let count = u16::try_from(n).map_err(|_| CodecError::CountOverflow)?;
        self.put_u16(count)
    }

    fn put_octets(&mut self, octets: &[u8]) -> Result<(), CodecError> {
        self.put_count(octets.len())?;
        self.put_bytes(octets)
    }
}

/// Encodes a message body into `buf`, returning the number of octets written.
///
/// # Errors
///
/// Returns `CodecError::BufferTooSmall` if `buf` is insufficient,
/// `CodecError::InvalidCaseId` if `case_id` is not ASCII or longer than 255 octets,
/// or `CodecError::InvalidBodyLength` if the body exceeds `MAX_BODY`.
pub fn encode_body(msg: &Message<'_>, buf: &mut [u8]) -> Result<usize, CodecError> {
    let mut w = Writer::new(buf);
    match msg {
        Message::Hello {
            version,
            scenario_version,
            role,
            case_id,
        } => {
            w.put_u8(T_HELLO)?;
            w.put_u16(*version)?;
            w.put_u16(*scenario_version)?;
            w.put_u8(match role {
                Role::Flight => 0,
                Role::Ground => 1,
            })?;
            if !case_id.is_ascii() {
                return Err(CodecError::InvalidCaseId);
            }
            let len = u8::try_from(case_id.len()).map_err(|_| CodecError::InvalidCaseId)?;
            w.put_u8(len)?;
            w.put_bytes(case_id.as_bytes())?;
        }
        Message::Step {
            now,
            tx_slot,
            units,
            ops,
        } => {
            w.put_u8(T_STEP)?;
            w.put_u64(*now)?;
            w.put_u8(u8::from(*tx_slot))?;
            w.put_u16(units.count)?;
            for unit in units {
                w.put_octets(unit)?;
            }
            w.put_u16(ops.count)?;
            for op in ops {
                w.put_u16(op)?;
            }
        }
        Message::End => {
            w.put_u8(T_END)?;
        }
        Message::HelloAck { version } => {
            w.put_u8(T_HELLO_ACK)?;
            w.put_u16(*version)?;
        }
        Message::StepResult {
            outputs,
            next_deadline,
        } => {
            w.put_u8(T_STEP_RESULT)?;
            w.put_count(outputs.len())?;
            match outputs {
                Outputs::Slice(slice) => {
                    for o in *slice {
                        encode_node_output(&mut w, o)?;
                    }
                }
                Outputs::Iter(iter) => {
                    for o in iter {
                        encode_node_output(&mut w, &o)?;
                    }
                }
            }
            w.put_u64(next_deadline.unwrap_or(u64::MAX))?;
        }
        Message::EndAck => {
            w.put_u8(T_END_ACK)?;
        }
    }

    if w.offset > MAX_BODY {
        return Err(CodecError::InvalidBodyLength(w.offset));
    }
    Ok(w.offset)
}

fn encode_node_output(w: &mut Writer<'_>, o: &NodeOutput<'_>) -> Result<(), CodecError> {
    match o {
        NodeOutput::Unit(octets) => {
            w.put_u8(1)?;
            w.put_octets(octets)?;
        }
        NodeOutput::PacketOut(apid, octets) => {
            w.put_u8(2)?;
            w.put_u16(*apid)?;
            w.put_octets(octets)?;
        }
        NodeOutput::PacketIn(apid, octets) => {
            w.put_u8(3)?;
            w.put_u16(*apid)?;
            w.put_octets(octets)?;
        }
        NodeOutput::Event(code, args) => {
            w.put_u8(4)?;
            w.put_u16(*code)?;
            let count = u8::try_from(args.len()).map_err(|_| CodecError::CountOverflow)?;
            w.put_u8(count)?;
            for a in args {
                w.put_u64(a)?;
            }
        }
    }
    Ok(())
}

/// Encodes a framed message (4-byte length prefix + body) into `buf`.
///
/// # Errors
///
/// Returns `CodecError::BufferTooSmall` if `buf` is insufficient.
pub fn encode_frame(msg: &Message<'_>, buf: &mut [u8]) -> Result<usize, CodecError> {
    let body_buf = buf.get_mut(4..).ok_or(CodecError::BufferTooSmall)?;
    let body_len = encode_body(msg, body_buf)?;
    let prefix = u32::try_from(body_len).map_err(|_| CodecError::CountOverflow)?;
    let prefix_bytes = prefix.to_be_bytes();
    let dest_prefix = buf.get_mut(..4).ok_or(CodecError::BufferTooSmall)?;
    dest_prefix.copy_from_slice(&prefix_bytes);
    body_len.checked_add(4).ok_or(CodecError::CountOverflow)
}

fn decode_hello<'a>(c: &mut Cursor<'a>) -> Result<Message<'a>, CodecError> {
    let version = c.u16()?;
    let scenario_version = c.u16()?;
    let role_byte = c.u8()?;
    let role = match role_byte {
        0 => Role::Flight,
        1 => Role::Ground,
        r => return Err(CodecError::UnknownRole(r)),
    };
    let id_len = usize::from(c.u8()?);
    let id_bytes = c.take(id_len)?;
    let case_id = core::str::from_utf8(id_bytes).map_err(|_| CodecError::InvalidCaseId)?;
    if !case_id.is_ascii() {
        return Err(CodecError::InvalidCaseId);
    }
    Ok(Message::Hello {
        version,
        scenario_version,
        role,
        case_id,
    })
}

fn decode_step<'a>(c: &mut Cursor<'a>) -> Result<Message<'a>, CodecError> {
    let now = c.u64()?;
    let tx_slot = match c.u8()? {
        0 => false,
        1 => true,
        v => return Err(CodecError::InvalidTxSlot(v)),
    };
    let units_count = c.u16()?;
    let units_start = c.offset;
    for _ in 0..units_count {
        let u_len = usize::from(c.u16()?);
        let _ = c.take(u_len)?;
    }
    let units_slice = c
        .buf
        .get(units_start..c.offset)
        .ok_or(CodecError::UnexpectedEof)?;

    let ops_count = c.u16()?;
    let ops_len = usize::from(ops_count)
        .checked_mul(2)
        .ok_or(CodecError::CountOverflow)?;
    let ops_slice = c.take(ops_len)?;

    Ok(Message::Step {
        now,
        tx_slot,
        units: UnitsIter::new(units_slice, units_count),
        ops: OpsIter::new(ops_slice, ops_count),
    })
}

fn decode_step_result<'a>(c: &mut Cursor<'a>) -> Result<Message<'a>, CodecError> {
    let outputs_count = c.u16()?;
    let outputs_start = c.offset;
    for _ in 0..outputs_count {
        let kind = c.u8()?;
        match kind {
            1 => {
                let len = usize::from(c.u16()?);
                let _ = c.take(len)?;
            }
            2 | 3 => {
                let _apid = c.u16()?;
                let len = usize::from(c.u16()?);
                let _ = c.take(len)?;
            }
            4 => {
                let _code = c.u16()?;
                let args_count = usize::from(c.u8()?);
                let args_len = args_count.checked_mul(8).ok_or(CodecError::CountOverflow)?;
                let _ = c.take(args_len)?;
            }
            k => return Err(CodecError::UnknownOutputKind(k)),
        }
    }
    let outputs_slice = c
        .buf
        .get(outputs_start..c.offset)
        .ok_or(CodecError::UnexpectedEof)?;
    let deadline_raw = c.u64()?;
    let next_deadline = if deadline_raw == u64::MAX {
        None
    } else {
        Some(deadline_raw)
    };
    Ok(Message::StepResult {
        outputs: Outputs::Iter(OutputsIter::new(outputs_slice, outputs_count)),
        next_deadline,
    })
}

/// Decodes a message from a body slice (without 4-byte length prefix).
///
/// # Errors
///
/// Returns an error if the body is truncated, has trailing bytes, or has invalid fields.
pub fn decode_body(body: &[u8]) -> Result<Message<'_>, CodecError> {
    if body.is_empty() || body.len() > MAX_BODY {
        return Err(CodecError::InvalidBodyLength(body.len()));
    }
    let mut c = Cursor::new(body);
    let msg_type = c.u8()?;
    let msg = match msg_type {
        T_HELLO => decode_hello(&mut c)?,
        T_STEP => decode_step(&mut c)?,
        T_END => Message::End,
        T_HELLO_ACK => Message::HelloAck { version: c.u16()? },
        T_STEP_RESULT => decode_step_result(&mut c)?,
        T_END_ACK => Message::EndAck,
        t => return Err(CodecError::UnknownMessageType(t)),
    };

    if c.remaining() > 0 {
        return Err(CodecError::TrailingOctets(c.remaining()));
    }
    Ok(msg)
}

/// Decodes a framed message from `buf` (4-byte length prefix + body).
///
/// # Errors
///
/// Returns an error if the frame is incomplete or invalid.
pub fn decode_frame(buf: &[u8]) -> Result<Message<'_>, CodecError> {
    if buf.len() < 4 {
        return Err(CodecError::UnexpectedEof);
    }
    let mut c = Cursor::new(buf);
    let len = usize::try_from(c.u32()?).map_err(|_| CodecError::CountOverflow)?;
    if len == 0 || len > MAX_BODY {
        return Err(CodecError::InvalidBodyLength(len));
    }
    let body = c.take(len)?;
    if c.remaining() > 0 {
        return Err(CodecError::TrailingOctets(c.remaining()));
    }
    decode_body(body)
}
