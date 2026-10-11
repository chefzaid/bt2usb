//! The bridge's one BLE security handler: bond storage for the SoftDevice,
//! and the answer to a peripheral's connection parameter request.
//!
//! [`Bonder`] keeps the bonds loaded from flash and those made since boot,
//! finds a bonded peer's keys when a link is encrypted again, and replaces only
//! the bonding peer's own record on a new pairing. Its
//! `conn_param_update_request` bounds a peripheral's request through the pure
//! [`conn_params`] policy (docs/adr/0016-bounded-peer-connection-parameters.md).
//! Every connection slot shares the one instance returned by [`bonder`].

use core::cell::RefCell;

use crate::ble::conn_params::{self, ConnParamLimits, ConnParams};
use crate::config;
use crate::config::{BLE_MAX_CONNECTIONS, MAX_PAIRED_DEVICES};
use crate::storage::{irk_present, is_identity_address, BondInfo};
use defmt::{info, warn};
use heapless::Vec;
use nrf_softdevice::ble::security::{IoCapabilities, SecurityHandler};
use nrf_softdevice::ble::{
    Address, Connection, EncryptionInfo, IdentityKey, MasterId, SecurityMode,
};
use nrf_softdevice::raw;
use static_cell::StaticCell;

/// What the bridge grants when a peripheral asks to change the connection
/// parameters: the configured interval range (or, for a peripheral that asks
/// only for slower intervals, its fastest one up to 30 ms), a bounded latency,
/// and a supervision timeout no longer than the one each link is opened with.
const PEER_CONN_PARAM_LIMITS: ConnParamLimits = ConnParamLimits {
    min_interval: config::BLE_CONN_INTERVAL_MIN,
    max_interval: config::BLE_CONN_INTERVAL_MAX,
    slow_request_max_interval: config::BLE_PEER_MAX_CONN_INTERVAL,
    max_latency: config::BLE_MAX_PERIPHERAL_LATENCY,
    min_supervision_timeout: config::BLE_MIN_SUP_TIMEOUT,
    max_supervision_timeout: config::BLE_SUP_TIMEOUT,
};

/// Whether `key` belongs to the device at `address`, as
/// `IdentityKey::is_match` decides, except that an all-zero IRK, which the
/// vendored crate stores for a peer that distributed no identity key, resolves
/// no private address ([`irk_present`]). Every match of a bond against an
/// address goes through it.
pub(crate) fn key_matches(key: &IdentityKey, address: Address) -> bool {
    key.is_match(address) && (irk_present(&key.as_raw().id_info.irk) || key.addr == address)
}

pub(crate) struct Bonder {
    peers: RefCell<Vec<BondInfo, MAX_PAIRED_DEVICES>>,
    /// The addresses of links whose pairing named an identity the store
    /// refuses, until [`Self::take_refused`] reports them.
    refused: RefCell<Vec<Address, BLE_MAX_CONNECTIONS>>,
}

impl Bonder {
    fn new() -> Self {
        Self {
            peers: RefCell::new(Vec::new()),
            refused: RefCell::new(Vec::new()),
        }
    }

    /// Whether two addresses belong to one peer: they are equal, or one
    /// bond's identity key matches both, whatever private addresses the peer
    /// used ([`key_matches`]).
    pub(crate) fn same_peer(&self, a: Address, b: Address) -> bool {
        a == b
            || self
                .peers
                .borrow()
                .iter()
                .any(|bond| key_matches(&bond.peer_id, a) && key_matches(&bond.peer_id, b))
    }

    /// Whether the pairing on the link to `address` was refused, forgetting
    /// the refusal.
    pub(crate) fn take_refused(&self, address: Address) -> bool {
        let mut refused = self.refused.borrow_mut();
        match refused.iter().position(|link| *link == address) {
            Some(index) => {
                refused.remove(index);
                true
            }
            None => false,
        }
    }

    pub(crate) fn load_bonds(&self, bonds: &Vec<BondInfo, MAX_PAIRED_DEVICES>) {
        let mut peers = self.peers.borrow_mut();
        peers.clear();
        for bond in bonds {
            if let Some(existing) = peers
                .iter_mut()
                .find(|p| p.peer_id.addr == bond.peer_id.addr)
            {
                *existing = *bond;
            } else {
                let _ = peers.push(*bond);
            }
        }
        info!("Loaded {} BLE bonds into security handler", peers.len());
    }

    pub(crate) fn bond_for_address(&self, address: Address) -> Option<BondInfo> {
        self.peers
            .borrow()
            .iter()
            .find(|p| key_matches(&p.peer_id, address))
            .copied()
    }

    pub(crate) fn forget(&self, address: Address) {
        self.peers
            .borrow_mut()
            .retain(|bond| !key_matches(&bond.peer_id, address));
    }

    /// Drop exactly `bond`. [`Self::forget`] drops every bond whose key matches
    /// an address, and peers that distributed no IRK all hold the all-zero
    /// key, which resolves any private address built from it.
    pub(crate) fn forget_bond(&self, bond: &BondInfo) {
        self.peers.borrow_mut().retain(|kept| kept != bond);
    }

    pub(crate) fn clear(&self) {
        self.peers.borrow_mut().clear();
        self.refused.borrow_mut().clear();
    }
}

impl SecurityHandler for Bonder {
    fn io_capabilities(&self) -> IoCapabilities {
        IoCapabilities::None
    }

    fn can_bond(&self, _conn: &Connection) -> bool {
        true
    }

    fn on_bonded(
        &self,
        conn: &Connection,
        master_id: MasterId,
        key: EncryptionInfo,
        peer_id: IdentityKey,
    ) {
        let link = conn.peer_address();
        let mut refused = self.refused.borrow_mut();
        refused.retain(|refused_link| *refused_link != link);
        // The peer chose its identity address. The store refuses one that is
        // not public or random static (a reload would refuse the whole store),
        // so keep no such bond: it would only displace a valid one here.
        // `execute_action` reports the refusal when the link is persisted.
        if !is_identity_address(peer_id.addr) {
            warn!("Bond refused: identity address is not public or random static");
            if refused.is_full() {
                refused.remove(0);
            }
            let _ = refused.push(link);
            return;
        }
        drop(refused);
        let mut peers = self.peers.borrow_mut();
        // MasterId is not a peer identity (LE Secure Connections can use the
        // same all-zero EDIV/RAND for multiple peers). Re-pairing replaces only
        // this peer's keys and must not overwrite another keyboard's bond.
        if let Some(existing) = peers
            .iter_mut()
            .find(|p| p.peer_id.addr == peer_id.addr || key_matches(&p.peer_id, link))
        {
            existing.master_id = master_id;
            existing.key = key;
            existing.peer_id = peer_id;
            return;
        }

        if peers.is_full() {
            peers.remove(0);
        }

        let _ = peers.push(BondInfo {
            master_id,
            key,
            peer_id,
        });
    }

    fn get_key(&self, conn: &Connection, master_id: MasterId) -> Option<EncryptionInfo> {
        self.peers.borrow().iter().find_map(|p| {
            (p.master_id == master_id && key_matches(&p.peer_id, conn.peer_address()))
                .then_some(p.key)
        })
    }

    fn get_peripheral_key(&self, conn: &Connection) -> Option<(MasterId, EncryptionInfo)> {
        self.peers.borrow().iter().find_map(|p| {
            key_matches(&p.peer_id, conn.peer_address()).then_some((p.master_id, p.key))
        })
    }

    fn on_security_update(&self, _conn: &Connection, mode: SecurityMode) {
        info!("BLE security mode updated: {}", mode);
    }

    fn conn_param_update_request(
        &self,
        _conn: &Connection,
        requested: raw::ble_gap_conn_params_t,
    ) -> raw::ble_gap_conn_params_t {
        let asked = ConnParams {
            min_interval: requested.min_conn_interval,
            max_interval: requested.max_conn_interval,
            latency: requested.slave_latency,
            supervision_timeout: requested.conn_sup_timeout,
        };
        let granted = conn_params::bound_request(asked, &PEER_CONN_PARAM_LIMITS);
        if granted == asked {
            info!("peer connection parameters granted: {}", granted);
        } else if conn_params::interval_within_request(asked, granted) {
            info!(
                "peer asked for connection parameters {}; granting {}",
                asked, granted
            );
        } else {
            // Some peripherals disconnect when the interval is outside the
            // range they asked for; the compatibility baseline needs to see it.
            warn!(
                "peer asked for connection parameters {}; granting {}, outside its interval range",
                asked, granted
            );
        }
        raw::ble_gap_conn_params_t {
            min_conn_interval: granted.min_interval,
            max_conn_interval: granted.max_interval,
            slave_latency: granted.latency,
            conn_sup_timeout: granted.supervision_timeout,
        }
    }
}

/// The single BLE bonder/security handler, shared by every connection slot.
///
/// `Bonder` holds a `RefCell` so it is `!Sync` and can't live in a `static`
/// directly (nor in `LazyLock`, which requires `Sync`). `StaticCell` only
/// requires `Send`, so it backs the storage; the first caller initialises it and
/// caches the `&'static` in an `AtomicPtr` so later calls don't re-`init` (which
/// would panic). On the single-threaded cooperative executor the init can't
/// race, so the spin fallback is just defensive.
pub(crate) fn bonder() -> &'static Bonder {
    use core::sync::atomic::{AtomicPtr, Ordering};

    static BONDER: StaticCell<Bonder> = StaticCell::new();
    static BONDER_REF: AtomicPtr<Bonder> = AtomicPtr::new(core::ptr::null_mut());

    let ptr = BONDER_REF.load(Ordering::Acquire);
    if !ptr.is_null() {
        // SAFETY: pointer came from StaticCell::try_init; the Bonder is 'static.
        unsafe { &*ptr }
    } else if let Some(b) = BONDER.try_init(Bonder::new()) {
        BONDER_REF.store(b as *mut Bonder, Ordering::Release);
        b
    } else {
        loop {
            let ptr = BONDER_REF.load(Ordering::Acquire);
            if !ptr.is_null() {
                // SAFETY: as above.
                break unsafe { &*ptr };
            }
        }
    }
}
