//! Pure, hardware-free framing for the paired-device flash record.
//!
//! The store is persisted as one flash blob holding several variable-length
//! records (one per paired device). This module owns the *framing* — the magic
//! byte, version, record count and per-record length prefixes — and the offset
//! bookkeeping that goes with it, independent of what each record contains
//! (that's [`super::PairedDevice`]/`codec`). Keeping it separate makes the
//! error-prone length/offset handling unit-testable on the host (the embedded
//! `storage` shell that calls it isn't).
//!
//! Wire layout (versioned):
//! ```text
//! [0]   magic (0xB2)
//! [1]   version (0x01)
//! [2]   record count
//! [3..] repeated: [len:u8][record bytes; len]
//! ```

const MAGIC: u8 = 0xB2;
const VERSION: u8 = 0x01;

/// `true` if `data` carries the versioned framing (vs. a legacy/blank blob).
pub fn is_versioned(data: &[u8]) -> bool {
    data.len() >= 3 && data[0] == MAGIC && data[1] == VERSION
}

/// Builds a versioned blob into a caller-provided buffer.
pub struct Writer<'a> {
    buf: &'a mut [u8],
    offset: usize,
    count: u8,
}

impl<'a> Writer<'a> {
    /// Start a blob (writes magic + version; reserves the count byte). Returns
    /// `None` if the buffer can't even hold the 3-byte header.
    pub fn new(buf: &'a mut [u8]) -> Option<Self> {
        if buf.len() < 3 {
            return None;
        }
        buf[0] = MAGIC;
        buf[1] = VERSION;
        Some(Self {
            buf,
            offset: 3,
            count: 0,
        })
    }

    /// Append one record, serialized in place by `serialize` (which writes into
    /// the slice after the length prefix and returns the byte count).
    ///
    /// Returns `false` without advancing if the record doesn't fit, is empty, or
    /// exceeds the 255-byte length prefix — so a full buffer truncates cleanly
    /// rather than corrupting the blob.
    pub fn push(&mut self, serialize: impl FnOnce(&mut [u8]) -> usize) -> bool {
        // Need room for at least the length prefix plus one body byte.
        if self.count == u8::MAX || self.offset + 1 >= self.buf.len() {
            return false;
        }
        let body = self.offset + 1;
        let written = serialize(&mut self.buf[body..]);
        if written == 0 || written > u8::MAX as usize || body + written > self.buf.len() {
            return false; // offset unchanged → record rolled back
        }
        self.buf[self.offset] = written as u8;
        self.offset = body + written;
        self.count += 1;
        true
    }

    /// Finalize: write the record count and return the total blob length.
    pub fn finish(self) -> usize {
        self.buf[2] = self.count;
        self.offset
    }
}

/// Iterate the record byte-slices of a versioned blob.
///
/// Stops cleanly at the declared count or at the first malformed/truncated
/// length prefix, so a corrupt blob yields a prefix of valid records rather
/// than reading out of bounds.
pub fn records(data: &[u8]) -> Records<'_> {
    let remaining = if is_versioned(data) { data[2] } else { 0 };
    Records {
        data,
        offset: 3,
        remaining,
    }
}

/// Iterator returned by [`records`].
pub struct Records<'a> {
    data: &'a [u8],
    offset: usize,
    remaining: u8,
}

impl<'a> Iterator for Records<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if self.remaining == 0 || self.offset >= self.data.len() {
            return None;
        }
        let len = self.data[self.offset] as usize;
        let body = self.offset + 1;
        if len == 0 || body + len > self.data.len() {
            self.remaining = 0; // truncated/corrupt → stop
            return None;
        }
        let record = &self.data[body..body + len];
        self.offset = body + len;
        self.remaining -= 1;
        Some(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frame a set of records and read them back.
    fn round_trip(records_in: &[&[u8]]) -> heapless::Vec<heapless::Vec<u8, 64>, 8> {
        let mut buf = [0u8; 256];
        let mut w = Writer::new(&mut buf).unwrap();
        for r in records_in {
            assert!(w.push(|b| {
                b[..r.len()].copy_from_slice(r);
                r.len()
            }));
        }
        let len = w.finish();

        let mut out = heapless::Vec::new();
        for rec in records(&buf[..len]) {
            let mut v: heapless::Vec<u8, 64> = heapless::Vec::new();
            v.extend_from_slice(rec).unwrap();
            out.push(v).unwrap();
        }
        out
    }

    #[test]
    fn empty_blob_round_trips() {
        let out = round_trip(&[]);
        assert!(out.is_empty());
        let mut buf = [0u8; 8];
        let len = Writer::new(&mut buf).unwrap().finish();
        assert!(is_versioned(&buf[..len]));
        assert_eq!(records(&buf[..len]).count(), 0);
    }

    #[test]
    fn multiple_records_round_trip_in_order() {
        let out = round_trip(&[&[1, 2, 3], &[9], &[7, 7, 7, 7]]);
        assert_eq!(out.len(), 3);
        assert_eq!(&out[0][..], &[1, 2, 3]);
        assert_eq!(&out[1][..], &[9]);
        assert_eq!(&out[2][..], &[7, 7, 7, 7]);
    }

    #[test]
    fn non_versioned_data_yields_no_records() {
        assert!(!is_versioned(&[]));
        assert!(!is_versioned(&[0x00, 0x01, 0x02]));
        assert_eq!(records(&[0x00, 0x01, 0x02]).count(), 0);
    }

    #[test]
    fn writer_truncates_cleanly_when_full() {
        // Tiny buffer: header (3) + one 4-byte record (1 len + 4) = 8 fits;
        // the second record must be rejected, leaving a valid 1-record blob.
        let mut buf = [0u8; 8];
        let mut w = Writer::new(&mut buf).unwrap();
        assert!(w.push(|b| {
            b[..4].copy_from_slice(&[1, 2, 3, 4]);
            4
        }));
        assert!(!w.push(|b| {
            b[0] = 9;
            1
        }));
        let len = w.finish();
        let recs: heapless::Vec<&[u8], 4> = records(&buf[..len]).collect();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0], &[1, 2, 3, 4]);
    }

    #[test]
    fn reader_stops_on_truncated_record() {
        // Claims 2 records but the second's length runs past the buffer end.
        let data = [
            MAGIC, VERSION, 2, /*len*/ 2, 0xAA, 0xBB, /*len*/ 5, 0xCC,
        ];
        let recs: heapless::Vec<&[u8], 4> = records(&data).collect();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0], &[0xAA, 0xBB]);
    }

    #[test]
    fn reader_stops_on_zero_length_record() {
        let data = [MAGIC, VERSION, 2, 0x00, 0x01];
        assert_eq!(records(&data).count(), 0);
    }
}
