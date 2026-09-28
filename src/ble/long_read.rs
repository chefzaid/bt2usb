//! Bounded ATT Read/Read Blob assembly, shared by live BLE and host tests.

use heapless::Vec;

/// ATT attributes have a maximum length of 512 bytes.
pub const MAX_ATTRIBUTE_LEN: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadFailure {
    InvalidFragment,
    TooLarge,
    UnexpectedEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndOfValue {
    InvalidOffset,
    AttributeNotLong,
}

pub struct LongRead {
    bytes: Vec<u8, MAX_ATTRIBUTE_LEN>,
    payload: usize,
    complete: bool,
}

impl LongRead {
    pub fn new(mtu: u16) -> Result<Self, ReadFailure> {
        if !(23..=517).contains(&mtu) {
            return Err(ReadFailure::InvalidFragment);
        }
        Ok(Self {
            bytes: Vec::new(),
            payload: usize::from(mtu) - 1,
            complete: false,
        })
    }

    pub fn offset(&self) -> u16 {
        self.bytes.len() as u16
    }

    /// Return true only when the peer supplied a short (including empty) final
    /// fragment. Filling the buffer exactly is not proof of completeness.
    pub fn append(&mut self, fragment: &[u8]) -> Result<bool, ReadFailure> {
        if self.complete || fragment.len() > self.payload {
            return Err(ReadFailure::InvalidFragment);
        }
        self.bytes
            .extend_from_slice(fragment)
            .map_err(|_| ReadFailure::TooLarge)?;
        self.complete = fragment.len() < self.payload;
        Ok(self.complete)
    }

    /// ATT may terminate an exact-MTU value with Invalid Offset on the next
    /// request. Attribute Not Long is valid only after the first full fragment;
    /// accepting it later could silently classify a truncated long descriptor.
    pub fn finish_at_end(&mut self, reason: EndOfValue) -> Result<(), ReadFailure> {
        let valid = !self.complete
            && !self.bytes.is_empty()
            && self.bytes.len().is_multiple_of(self.payload)
            && (reason == EndOfValue::InvalidOffset || self.bytes.len() == self.payload);
        if !valid {
            return Err(ReadFailure::UnexpectedEnd);
        }
        self.complete = true;
        Ok(())
    }

    pub fn value(&self) -> Option<&[u8]> {
        self.complete.then_some(self.bytes.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_map_is_unavailable_until_final_fragment() {
        let mut read = LongRead::new(23).unwrap();
        for n in 0..12 {
            assert_eq!(read.offset(), n * 22);
            assert!(!read.append(&[1; 22]).unwrap());
            assert!(read.value().is_none());
        }
        assert!(read.append(&[2; 7]).unwrap());
        assert_eq!(read.value().unwrap().len(), 271);
    }

    #[test]
    fn exact_mtu_needs_an_end_response() {
        for end in [EndOfValue::InvalidOffset, EndOfValue::AttributeNotLong] {
            let mut read = LongRead::new(64).unwrap();
            read.append(&[1; 63]).unwrap();
            read.finish_at_end(end).unwrap();
            assert_eq!(read.value().unwrap().len(), 63);
        }
        let mut read = LongRead::new(23).unwrap();
        read.append(&[1; 22]).unwrap();
        assert!(read.append(&[]).unwrap());
    }

    #[test]
    fn maximum_value_completes_and_oversized_map_fails() {
        let mut read = LongRead::new(65).unwrap();
        for _ in 0..8 {
            assert!(!read.append(&[1; 64]).unwrap());
        }
        assert_eq!(read.offset(), 512);
        assert!(read.value().is_none());
        read.finish_at_end(EndOfValue::InvalidOffset).unwrap();
        assert_eq!(read.value().unwrap().len(), 512);

        let mut read = LongRead::new(65).unwrap();
        for _ in 0..8 {
            read.append(&[1; 64]).unwrap();
        }
        assert_eq!(read.append(&[1]), Err(ReadFailure::TooLarge));
        assert!(read.value().is_none());
    }

    #[test]
    fn malformed_termination_never_exposes_partial_value() {
        let mut read = LongRead::new(23).unwrap();
        assert_eq!(
            read.finish_at_end(EndOfValue::InvalidOffset),
            Err(ReadFailure::UnexpectedEnd)
        );
        read.append(&[1; 22]).unwrap();
        read.append(&[1; 22]).unwrap();
        assert_eq!(
            read.finish_at_end(EndOfValue::AttributeNotLong),
            Err(ReadFailure::UnexpectedEnd)
        );
        assert!(read.value().is_none());
        assert_eq!(read.append(&[1; 23]), Err(ReadFailure::InvalidFragment));
        assert!(LongRead::new(22).is_err());
    }
}
