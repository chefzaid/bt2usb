//! One connection slot's worker loop: serve the coordinator's commands and
//! reconnect in the background, generic over the radio so host tests run it
//! against fakes.
//!
//! [`run`] takes [`SlotCommand`]s from the coordinator, reports
//! [`SlotEvent`]s back, and between commands keeps retrying a saved device
//! whose link dropped or that has not been seen since power-up, through the
//! shared reconnect scan. A silent retry never pairs, so it runs only while
//! the bridge holds the device's keys and the device accepts them; once
//! either fails, the slot is freed with an error that tells the user to pair
//! the device again.
//!
//! Every event carries the number of the attempt it reports on (see
//! [`ConnManager`](super::coordinator::ConnManager)). A `Connect` for the
//! device a background retry is after takes the retry over: a link or an
//! attempt already under way is kept and reported under the new number, and
//! if the user's connection fails while the device's keys remain, the worker
//! reports the error and goes back to the retry. What the worker does after
//! each attempt is [`after_attempt`]; the firmware's [`SlotIo`], in
//! `slot_worker.rs`, runs each attempt (connect, secure, discover, run) in
//! `slot_link.rs`.

use core::future::Future;

use embassy_futures::select::{select, select3, Either, Either3};

use super::coordinator::{DeviceInfo, ErrorTag};
use super::reconnect::SavedPeer;
use super::slot_policy::{after_attempt, AttemptEnd, NextStep};
use crate::diagnostics::{Counter, COUNTERS};

/// A coordinator command to one slot worker. `attempt` is the number the
/// coordinator gave the slot's reservation (see
/// [`ConnManager`](super::coordinator::ConnManager)); the worker carries it
/// on every event it reports for that command.
#[derive(Clone, Debug, PartialEq)]
pub enum SlotCommand<A> {
    /// Connect to a device the user just picked; a failure is reported. Sent
    /// to a free slot, or to the slot retrying the same device in the
    /// background, which it takes over.
    Connect {
        device: DeviceInfo<A>,
        attempt: u32,
    },
    /// Keep silently retrying a paired device until it connects (boot-time
    /// auto-reconnect). Ends on success, on an error other than a failed
    /// connection (reported), including the bridge losing the device's keys
    /// or the device refusing them, or on a new command.
    Reconnect {
        device: DeviceInfo<A>,
        attempt: u32,
    },
    Disconnect,
    /// Acknowledge only after the link and automatic retry target are gone.
    Quiesce(u32),
}

/// A slot worker's report. Every variant but `Quiesced` carries the number of
/// the attempt it reports on, so the coordinator can ignore one it replaced.
#[derive(Clone, Debug, PartialEq)]
pub enum SlotEvent<A> {
    Quiesced {
        slot: usize,
        token: u32,
    },
    Connected {
        slot: usize,
        attempt: u32,
        device: DeviceInfo<A>,
    },
    Disconnected {
        slot: usize,
        attempt: u32,
    },
    /// An established link dropped; the slot is now silently reconnecting.
    LinkLost {
        slot: usize,
        attempt: u32,
        device: DeviceInfo<A>,
    },
    /// The attempt failed. `retrying` when it was a user connection that
    /// took over a background reconnect, which the worker has resumed.
    Error {
        slot: usize,
        attempt: u32,
        tag: ErrorTag,
        retrying: bool,
    },
}

/// How a single attempt (connect, secure, discover, run) ended.
#[derive(Clone, Debug, PartialEq)]
pub enum SlotOutcome<A> {
    /// The peer closed the link (or it ended normally).
    Closed,
    /// The connection attempt failed before the link was usable. A link that
    /// dropped while being secured or discovered fails as `ConnectFailed`.
    Failed(ErrorTag),
    /// A `Connect` for this attempt's device took over the attempt, which then
    /// failed before the link was usable. The user's connection, already
    /// numbered with the new attempt, is returned to be made next.
    TakenOver(DeviceInfo<A>),
    /// A new command arrived and superseded the active link, which has already
    /// been explicitly disconnected. The command is returned to be processed next.
    Superseded(SlotCommand<A>),
}

/// A log line the firmware prints for the worker; the guides quote them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// `slot N has no keys to reconnect`
    NoKeysToReconnect,
    /// `slot N link lost; reconnecting`
    LinkLostReconnecting,
    /// `slot N link lost; no keys to reconnect`
    LinkLostWithoutKeys,
}

/// What the worker needs from the firmware: its command and event channels,
/// the bonding keys, the shared reconnect scan, and one connection attempt.
pub trait SlotIo {
    type Address: Copy + PartialEq;
    /// The identity key a bond holds, which resolves the device's private
    /// addresses.
    type Identity: Copy;

    /// The next command from the coordinator.
    fn command(&self) -> impl Future<Output = SlotCommand<Self::Address>>;
    /// Report to the coordinator; waits while its channel is full.
    fn report(&self, event: SlotEvent<Self::Address>) -> impl Future<Output = ()>;
    /// The pause between background retries.
    fn backoff(&self) -> impl Future<Output = ()>;
    /// The identity in the keys the bridge holds for the device at `address`.
    fn identity(&self, address: Self::Address) -> Option<Self::Identity>;
    /// Make this slot's device a target of the shared reconnect scan.
    fn register_reconnect(&self, peer: SavedPeer<Self::Address, Self::Identity>);
    /// Stop this slot's reconnect target.
    fn clear_reconnect(&self);
    /// A background attempt to the target failed; the other slot's scans
    /// ignore the device for a while.
    fn reconnect_attempt_failed(&self);
    /// Resolves when the other slot's scan sees this slot's target.
    fn reconnect_sighted(&self) -> impl Future<Output = ()>;
    /// Scan for the target's live address; `None` when the scan ends without
    /// it.
    fn find_saved_peer(&self) -> impl Future<Output = Option<Self::Address>>;
    /// One attempt to `device`, racing the slot's commands, which may update
    /// `attempt` when the user's connection takes it over.
    fn attempt(
        &mut self,
        device: &DeviceInfo<Self::Address>,
        allow_pairing: bool,
        attempt: &mut u32,
    ) -> impl Future<Output = SlotOutcome<Self::Address>>;
    fn note(&self, note: Note);
}

/// Run slot `slot`'s worker forever.
pub async fn run<Io: SlotIo>(io: &mut Io, slot: usize) -> ! {
    let mut pending_cmd: Option<SlotCommand<Io::Address>> = None;
    // A paired device this slot keeps trying to reach: set after its link
    // drops (keyboards/mice disconnect when they sleep and advertise again on
    // the next key press) or when a boot-time reconnect hasn't found it yet.
    let mut retry: Option<DeviceInfo<Io::Address>> = None;
    // The retry target the command handled next replaces, resumed should that
    // command be a user connection that fails while the device's keys remain.
    // Taken on every pass, so it never outlives that command.
    let mut replaced: Option<DeviceInfo<Io::Address>> = None;
    // The number of the attempt this slot serves, carried on every event.
    let mut attempt = 0;

    loop {
        let cmd = match (pending_cmd.take(), retry.take()) {
            (Some(cmd), _) => cmd,
            (None, None) => io.command().await,
            // Between attempts, stay responsive: any command from the
            // coordinator replaces the retry.
            // The other slot's scan may see this slot's device first; it
            // wakes the slot so it connects without waiting out the pause.
            (None, Some(device)) => {
                match select3(io.command(), io.backoff(), io.reconnect_sighted()).await {
                    Either3::First(cmd) => {
                        replaced = Some(device);
                        cmd
                    }
                    Either3::Second(()) | Either3::Third(()) => {
                        SlotCommand::Reconnect { device, attempt }
                    }
                }
            }
        };
        let replaced_retry = replaced.take();
        // Only a background retry keeps this slot's reconnect target.
        if !matches!(cmd, SlotCommand::Reconnect { .. }) {
            io.clear_reconnect();
        }

        let (mut device, silent) = match cmd {
            SlotCommand::Connect {
                device,
                attempt: next,
            } => {
                attempt = next;
                (device, false)
            }
            SlotCommand::Reconnect {
                device,
                attempt: next,
            } => {
                attempt = next;
                (device, true)
            }
            SlotCommand::Disconnect => {
                // Also ends a retry between attempts: the coordinator still
                // counts this slot as reserved, so tell it the slot is free.
                io.report(SlotEvent::Disconnected { slot, attempt }).await;
                continue;
            }
            SlotCommand::Quiesce(token) => {
                io.report(SlotEvent::Quiesced { slot, token }).await;
                continue;
            }
        };

        if silent {
            // A background reconnect never pairs, so without the device's
            // keys (a newer pairing evicted them, or it was saved without
            // any) it could never secure the link: free the slot instead of
            // holding it for good, and tell the user to pair it again.
            let Some(identity) = io.identity(device.address) else {
                io.note(Note::NoKeysToReconnect);
                io.clear_reconnect();
                io.report(keys_missing(slot, attempt)).await;
                continue;
            };
            // RPA rotation can happen at any time, not just at boot. A
            // whitelist retry against the previous address never recovers, so
            // find the device's live address first.
            io.register_reconnect(SavedPeer {
                address: device.address,
                identity: Some(identity),
            });
            match select(io.command(), io.find_saved_peer()).await {
                Either::First(next) => {
                    io.clear_reconnect();
                    // Only the `Connect` handled next can resume the retry;
                    // anything left here would reach a later, unrelated one.
                    if matches!(next, SlotCommand::Connect { .. }) {
                        replaced = Some(device);
                    }
                    pending_cmd = superseded(io, slot, attempt, next).await;
                    continue;
                }
                Either::Second(Some(address)) => {
                    COUNTERS.bump(Counter::ReconnectAttempts);
                    device.address = address;
                }
                Either::Second(None) => {
                    retry = Some(device);
                    continue;
                }
            }
        }

        let outcome = io.attempt(&device, !silent, &mut attempt).await;
        let end = match outcome {
            SlotOutcome::Closed => {
                COUNTERS.bump(Counter::LinksLost);
                AttemptEnd::Closed
            }
            SlotOutcome::Failed(tag) => {
                if silent {
                    COUNTERS.bump(Counter::ReconnectFailures);
                }
                AttemptEnd::Failed(tag)
            }
            SlotOutcome::TakenOver(next) => AttemptEnd::TakenOver(next),
            SlotOutcome::Superseded(next_cmd) => {
                pending_cmd = superseded(io, slot, attempt, next_cmd).await;
                io.clear_reconnect();
                continue;
            }
        };
        let closed = matches!(end, AttemptEnd::Closed);
        let has_keys = |target: &DeviceInfo<Io::Address>| io.identity(target.address).is_some();
        match after_attempt(end, device, silent, replaced_retry, has_keys) {
            // The peer dropped a working link: keep the slot for it and
            // reconnect as soon as it advertises again.
            NextStep::LinkLost(device) => {
                io.note(Note::LinkLostReconnecting);
                io.report(SlotEvent::LinkLost {
                    slot,
                    attempt,
                    device: device.clone(),
                })
                .await;
                retry = Some(device);
            }
            // Not found in this attempt's window (or the link couldn't be
            // secured): nothing new to report, try again. Meanwhile the other
            // slot's scans ignore this device for a while, in case it
            // advertises but will not connect.
            NextStep::RetryQuietly(device) => {
                io.reconnect_attempt_failed();
                retry = Some(device);
            }
            // The user's connection took over this background attempt, which
            // then failed: make that connection now, with pairing allowed,
            // and come back to this retry if it fails too.
            NextStep::Connect { device, resume } => {
                replaced = Some(resume);
                pending_cmd = Some(SlotCommand::Connect { device, attempt });
            }
            NextStep::Report { tag, resume } => {
                if closed {
                    io.note(Note::LinkLostWithoutKeys);
                }
                io.report(SlotEvent::Error {
                    slot,
                    attempt,
                    tag,
                    retrying: resume.is_some(),
                })
                .await;
                retry = resume;
            }
        }
        if retry.is_none() {
            io.clear_reconnect();
        }
    }
}

/// The slot gave up on a device it holds no keys for: it is free, with the
/// error that tells the user to pair the device again.
fn keys_missing<A>(slot: usize, attempt: u32) -> SlotEvent<A> {
    SlotEvent::Error {
        slot,
        attempt,
        tag: ErrorTag::KeysMissing,
        retrying: false,
    }
}

/// A command ended this slot's attempt or background retry; returns the
/// command still to handle. The attempt is reported over. When the command is
/// a `Connect` that took over the retry, the coordinator has already given it
/// a new attempt number and ignores the report. A `Disconnect` needs nothing
/// more, since the attempt is already torn down.
async fn superseded<Io: SlotIo>(
    io: &Io,
    slot: usize,
    attempt: u32,
    next: SlotCommand<Io::Address>,
) -> Option<SlotCommand<Io::Address>> {
    io.report(SlotEvent::Disconnected { slot, attempt }).await;
    (!matches!(next, SlotCommand::Disconnect)).then_some(next)
}

#[cfg(test)]
#[path = "slot_loop_bench.rs"]
mod bench;

#[cfg(test)]
#[path = "slot_loop_tests.rs"]
mod tests;
