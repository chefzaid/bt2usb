//! The PnP ID a peripheral publishes in its Device Information Service
//! (service `0x180A`, characteristic `0x2A50`): who assigned its vendor ID,
//! the vendor and product IDs, and the product version.
//!
//! The firmware reads it once per link after HID input flows and logs it
//! (`ble::device_info`), so a defect report names the peripheral's model and
//! firmware release from the log instead of from memory. Parsing is pure so
//! the host tests cover it; the value identifies a product, never a person or
//! a unit.

/// Who assigned [`PnpId::vendor`], the PnP ID's first byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VendorIdSource {
    /// A company identifier assigned by the Bluetooth SIG (value 1).
    BluetoothSig,
    /// A vendor ID assigned by the USB Implementers Forum (value 2), as in a
    /// USB device descriptor.
    UsbIf,
    /// Any other value, which the specification reserves.
    Reserved(u8),
}

impl VendorIdSource {
    const fn from_byte(byte: u8) -> Self {
        match byte {
            1 => VendorIdSource::BluetoothSig,
            2 => VendorIdSource::UsbIf,
            other => VendorIdSource::Reserved(other),
        }
    }
}

/// A parsed PnP ID characteristic value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PnpId {
    pub source: VendorIdSource,
    /// The vendor ID, from the registry [`PnpId::source`] names.
    pub vendor: u16,
    /// The vendor's own product ID.
    pub product: u16,
    /// The vendor's product version; by the specification's convention
    /// `0xJJMN` for release JJ.M.N.
    pub version: u16,
}

impl PnpId {
    /// The characteristic's length in bytes: one source byte, then the three
    /// 16-bit fields, little-endian.
    pub const LEN: usize = 7;

    /// The PnP ID in a characteristic value, or `None` when the value is not
    /// exactly [`PnpId::LEN`] bytes.
    pub fn parse(value: &[u8]) -> Option<Self> {
        let [source, vendor_lo, vendor_hi, product_lo, product_hi, version_lo, version_hi] =
            <[u8; Self::LEN]>::try_from(value).ok()?;
        Some(Self {
            source: VendorIdSource::from_byte(source),
            vendor: u16::from_le_bytes([vendor_lo, vendor_hi]),
            product: u16::from_le_bytes([product_lo, product_hi]),
            version: u16::from_le_bytes([version_lo, version_hi]),
        })
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for PnpId {
    fn format(&self, f: defmt::Formatter) {
        match self.source {
            VendorIdSource::BluetoothSig => defmt::write!(f, "Bluetooth SIG"),
            VendorIdSource::UsbIf => defmt::write!(f, "USB-IF"),
            VendorIdSource::Reserved(byte) => defmt::write!(f, "reserved source {=u8}", byte),
        }
        defmt::write!(
            f,
            " vendor {=u16:#06x}, product {=u16:#06x}, version {=u16:#06x}",
            self.vendor,
            self.product,
            self.version
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seven_byte_value_parses_little_endian() {
        // A USB-IF vendor 0x1234, product 0x5678, release 1.2.3.
        let value = [0x02, 0x34, 0x12, 0x78, 0x56, 0x23, 0x01];
        assert_eq!(
            PnpId::parse(&value),
            Some(PnpId {
                source: VendorIdSource::UsbIf,
                vendor: 0x1234,
                product: 0x5678,
                version: 0x0123,
            })
        );
    }

    #[test]
    fn each_source_byte_maps_to_its_registry() {
        let parse = |source| PnpId::parse(&[source, 0, 0, 0, 0, 0, 0]).map(|id| id.source);
        assert_eq!(parse(1), Some(VendorIdSource::BluetoothSig));
        assert_eq!(parse(2), Some(VendorIdSource::UsbIf));
        assert_eq!(parse(0), Some(VendorIdSource::Reserved(0)));
        assert_eq!(parse(0xFF), Some(VendorIdSource::Reserved(0xFF)));
    }

    #[test]
    fn a_value_of_any_other_length_is_rejected() {
        let value = [0x01, 0x59, 0x00, 0x01, 0x00, 0x00, 0x01, 0x00];
        for len in 0..value.len() {
            if len != PnpId::LEN {
                assert_eq!(PnpId::parse(&value[..len]), None, "{len} bytes");
            }
        }
        assert_eq!(PnpId::parse(&value), None, "8 bytes");
        assert!(PnpId::parse(&value[..PnpId::LEN]).is_some());
    }
}
