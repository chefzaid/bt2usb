//
// nRF52840 TWIM model: the I2C master with EasyDMA (TWIM0 at 0x40003000).
//
// Loaded at runtime by Renode (`include @renode/nrf52840_twim.cs`) and used by
// the renode/nrf52840-twim-oled.repl overlay in place of the stock `twi0`.
//
// Why: the stock `I2C.NRF52840_I2C` model implements only the legacy TWI
// (one byte at a time through TXD/RXD at 0x51C/0x518). embassy-nrf's `Twim`
// driver, which the firmware's display task uses, moves whole buffers with
// EasyDMA (TXD.PTR/MAXCNT, RXD.PTR/MAXCNT) and waits for EVENTS_STOPPED,
// EVENTS_ERROR, or EVENTS_SUSPENDED through the TWISPI0 interrupt, so on the
// stock model it never sees a transfer finish.
//
// This model follows the nRF52840 Product Specification (TWIM chapter):
//
//   * STARTTX raises TXSTARTED, latches TXD.PTR and TXD.MAXCNT (they are
//     double-buffered, so firmware may load the next buffer after
//     TXSTARTED), sends ADDRESS and then the bytes, and finally raises LASTTX
//     and sets TXD.AMOUNT. STARTRX does the same for a read into RXD.PTR,
//     raising RXSTARTED and LASTRX.
//   * SHORTS chain the next step after the last byte: LASTTX_STARTRX,
//     LASTTX_SUSPEND, LASTTX_STOP, LASTRX_STARTTX, LASTRX_SUSPEND, and
//     LASTRX_STOP. Changing direction sends a repeated START and the address
//     byte again, which the target can NACK.
//   * STOP takes effect after the byte on the wire, as on the chip: the bytes
//     already started are moved and counted in AMOUNT, an address byte in
//     flight is still NACKed, and then the STOP condition goes out and
//     STOPPED follows one bit time later. STOP while suspended is ignored
//     (the Product Specification requires RESUME first) and logs a warning.
//   * SUSPEND raises SUSPENDED and keeps the transaction open until RESUME.
//     A STARTTX or STARTRX written while suspended runs on RESUME, as
//     embassy-nrf writes them in that order.
//   * A transfer takes the time it would on the wire at FREQUENCY (9 bit
//     times per byte, plus the START bit and the address byte when a
//     transaction starts), so the driver really waits for the interrupt.
//   * An address no target answers is NACKed: ERRORSRC.ANACK and ERROR, and
//     the transaction stays open until STOP.
//   * EasyDMA reaches Data RAM only. A TXD.PTR or RXD.PTR outside it moves no
//     bytes (AMOUNT stays 0) and logs an error, so firmware that would fail
//     on the chip fails here too instead of passing on a forgiving model.
//   * The IRQ is any EVENTS_x && INTEN.x, for STOPPED, ERROR, SUSPENDED,
//     RXSTARTED, TXSTARTED, LASTRX, and LASTTX.
//
// Not modelled: SUSPEND in the middle of a buffer (the model suspends after
// the current buffer), clock stretching, a bus held low, bus errors other
// than an address NACK, the TXD/RXD ArrayList mode, telling the target about
// a repeated START (Renode's II2CPeripheral has no call for it), and the
// other peripherals that share this address (SPIM0, SPIS0, TWIS0); ENABLE
// values other than TWIM log a warning.
//
// Monitor and test hooks:
//
//   twi0 SetDevicePresent 0x3C false   # the target stops answering (unplugged)
//   twi0 SetDevicePresent 0x3C true    # it answers again
//

using System;
using System.Collections.Generic;

using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.Timers;
using Antmicro.Renode.Time;

namespace Antmicro.Renode.Peripherals.I2C
{
    public class NRF52840_TWIM : SimpleContainer<II2CPeripheral>, IDoubleWordPeripheral, IKnownSize
    {
        public NRF52840_TWIM(IMachine machine) : base(machine)
        {
            IRQ = new GPIO();
            sysbus = machine.GetSystemBus(this);
            absent = new HashSet<int>();
            // One tick per microsecond; the limit is set per transfer.
            wire = new LimitTimer(machine.ClockSource, 1000000, this, "wire",
                limit: MaxTransferMicroseconds, direction: Direction.Ascending,
                enabled: false, workMode: WorkMode.OneShot, eventEnabled: true);
            wire.LimitReached += OnTransferDone;
            Reset();
        }

        public override void Reset()
        {
            lock(sync)
            {
                wire.Enabled = false;
                stopped = error = suspended = rxStarted = txStarted = lastRx = lastTx = false;
                shorts = 0;
                inten = 0;
                errorSource = 0;
                enable = 0;
                pselScl = pselSda = 0xFFFFFFFF;
                frequency = Frequency250K;
                rxPointer = rxMaxCount = rxAmount = rxList = 0;
                txPointer = txMaxCount = txAmount = txList = 0;
                address = 0;
                state = State.Idle;
                target = null;
                transactionOpen = false;
                pending = Transfer.None;
                inFlight = Transfer.None;
                inFlightPointer = inFlightCount = addressBytes = 0;
                suspendAfterBuffer = false;
                stopPending = false;
                cutCount = 0;
                UpdateInterrupt();
            }
        }

        public GPIO IRQ { get; }

        public long Size => 0x1000;

        /// Make the target at `address` answer (true) or NACK its address
        /// (false), as if it were plugged in or unplugged.
        public void SetDevicePresent(int address, bool present)
        {
            lock(sync)
            {
                if(present)
                {
                    absent.Remove(address);
                }
                else
                {
                    absent.Add(address);
                }
                this.Log(LogLevel.Info, "target 0x{0:X2} {1}", address, present ? "present" : "absent");
            }
        }

        public uint ReadDoubleWord(long offset)
        {
            lock(sync)
            {
                return ReadRegister(offset);
            }
        }

        public void WriteDoubleWord(long offset, uint value)
        {
            lock(sync)
            {
                WriteRegister(offset, value);
                UpdateInterrupt();
            }
        }

        private uint ReadRegister(long offset)
        {
            switch((Registers)offset)
            {
            case Registers.EventsStopped: return Flag(stopped);
            case Registers.EventsError: return Flag(error);
            case Registers.EventsSuspended: return Flag(suspended);
            case Registers.EventsRxStarted: return Flag(rxStarted);
            case Registers.EventsTxStarted: return Flag(txStarted);
            case Registers.EventsLastRx: return Flag(lastRx);
            case Registers.EventsLastTx: return Flag(lastTx);
            case Registers.Shorts: return shorts;
            case Registers.InterruptEnable:
            case Registers.InterruptEnableSet:
            case Registers.InterruptEnableClear:
                return inten;
            case Registers.ErrorSource: return errorSource;
            case Registers.Enable: return enable;
            case Registers.PinSelectScl: return pselScl;
            case Registers.PinSelectSda: return pselSda;
            case Registers.Frequency: return frequency;
            case Registers.RxdPointer: return rxPointer;
            case Registers.RxdMaxCount: return rxMaxCount;
            case Registers.RxdAmount: return rxAmount;
            case Registers.RxdList: return rxList;
            case Registers.TxdPointer: return txPointer;
            case Registers.TxdMaxCount: return txMaxCount;
            case Registers.TxdAmount: return txAmount;
            case Registers.TxdList: return txList;
            case Registers.Address: return address;
            case Registers.TasksStartRx:
            case Registers.TasksStartTx:
            case Registers.TasksStop:
            case Registers.TasksSuspend:
            case Registers.TasksResume:
                return 0;
            }
            this.Log(LogLevel.Warning, "Unhandled read from offset 0x{0:X}", offset);
            return 0;
        }

        private void WriteRegister(long offset, uint value)
        {
            var set = (value & 1) != 0;
            switch((Registers)offset)
            {
            case Registers.TasksStartRx:
                if(set)
                {
                    StartTask(Transfer.Rx);
                }
                return;
            case Registers.TasksStartTx:
                if(set)
                {
                    StartTask(Transfer.Tx);
                }
                return;
            case Registers.TasksStop:
                if(set)
                {
                    Stop();
                }
                return;
            case Registers.TasksSuspend:
                if(set)
                {
                    Suspend();
                }
                return;
            case Registers.TasksResume:
                if(set)
                {
                    Resume();
                }
                return;
            case Registers.EventsStopped: stopped = set; return;
            case Registers.EventsError: error = set; return;
            case Registers.EventsSuspended: suspended = set; return;
            case Registers.EventsRxStarted: rxStarted = set; return;
            case Registers.EventsTxStarted: txStarted = set; return;
            case Registers.EventsLastRx: lastRx = set; return;
            case Registers.EventsLastTx: lastTx = set; return;
            case Registers.Shorts: shorts = value & ShortsMask; return;
            case Registers.InterruptEnable: inten = value & InterruptMask; return;
            case Registers.InterruptEnableSet: inten |= value & InterruptMask; return;
            case Registers.InterruptEnableClear: inten &= ~value; return;
            case Registers.ErrorSource: errorSource &= ~value; return; // write 1 to clear
            case Registers.Enable:
                enable = value & 0xF;
                if(enable != EnableTwim && enable != 0)
                {
                    this.Log(LogLevel.Warning, "ENABLE={0} selects a peripheral this model does not implement (only TWIM, 6)", enable);
                }
                return;
            case Registers.PinSelectScl: pselScl = value; return;
            case Registers.PinSelectSda: pselSda = value; return;
            case Registers.Frequency: frequency = value; return;
            case Registers.RxdPointer: rxPointer = value; return;
            case Registers.RxdMaxCount: rxMaxCount = value & 0xFFFF; return;
            case Registers.RxdList: rxList = value; WarnList(value, "RXD"); return;
            case Registers.TxdPointer: txPointer = value; return;
            case Registers.TxdMaxCount: txMaxCount = value & 0xFFFF; return;
            case Registers.TxdList: txList = value; WarnList(value, "TXD"); return;
            case Registers.Address: address = value & 0x7F; return;
            case Registers.RxdAmount:
            case Registers.TxdAmount:
                return; // read-only
            }
            this.Log(LogLevel.Warning, "Unhandled write to offset 0x{0:X}, value 0x{1:X}", offset, value);
        }

        private void StartTask(Transfer transfer)
        {
            if(enable != EnableTwim)
            {
                this.Log(LogLevel.Warning, "START{0} ignored: TWIM is not enabled", transfer == Transfer.Tx ? "TX" : "RX");
                return;
            }
            switch(state)
            {
            case State.Suspended:
                pending = transfer; // runs on RESUME
                return;
            case State.Idle:
                Begin(transfer);
                return;
            default:
                this.Log(LogLevel.Warning, "START{0} ignored while a transfer is in progress", transfer == Transfer.Tx ? "TX" : "RX");
                return;
            }
        }

        private void Begin(Transfer transfer, bool repeatedStart = false)
        {
            var tx = transfer == Transfer.Tx;
            inFlight = transfer;
            inFlightPointer = tx ? txPointer : rxPointer;
            inFlightCount = tx ? txMaxCount : rxMaxCount;
            if(tx)
            {
                txStarted = true;
                txAmount = 0;
            }
            else
            {
                rxStarted = true;
                rxAmount = 0;
            }

            var addressed = !transactionOpen || repeatedStart;
            transactionOpen = true;
            addressBytes = addressed ? 1u : 0u;
            if(addressed && !Answers())
            {
                // The address byte goes out and nobody pulls SDA low.
                state = State.AddressNack;
                StartWire(1, true);
                return;
            }
            state = State.Busy;
            StartWire(addressBytes + inFlightCount, addressed);
        }

        /// Whether a target answers ADDRESS, resolving it when a transaction
        /// starts. A repeated START keeps the target unless it went away.
        private bool Answers()
        {
            var present = !absent.Contains((int)address);
            if(target == null)
            {
                return present && TryGetByAddress((int)address, out target);
            }
            if(!present)
            {
                target.FinishTransmission();
                target = null;
            }
            return present;
        }

        private void StartWire(uint bytes, bool startBit)
        {
            // 9 bit times per byte (8 data bits and the ACK), plus the START bit.
            StartWireBits(9.0 * bytes + (startBit ? 1 : 0));
        }

        private void StartWireBits(double bits)
        {
            var micros = (ulong)Math.Ceiling(bits * 1000000.0 / BitRate());
            wire.Enabled = false;
            wire.Value = 0;
            wire.Limit = Math.Max(1UL, Math.Min(micros, MaxTransferMicroseconds));
            wire.Enabled = true;
        }

        private void OnTransferDone()
        {
            lock(sync)
            {
                var transfer = inFlight;
                inFlight = Transfer.None;
                switch(state)
                {
                case State.AddressNack:
                    errorSource |= ErrorAddressNack;
                    error = true;
                    this.Log(LogLevel.Debug, "address 0x{0:X2} NACKed", address);
                    if(stopPending)
                    {
                        BeginStopCondition();
                    }
                    else
                    {
                        state = State.WaitingForStop;
                    }
                    break;
                case State.Busy:
                    state = State.Idle;
                    Finish(transfer);
                    break;
                case State.Stopping:
                    EndTransaction();
                    break;
                }
                UpdateInterrupt();
            }
        }

        private void Finish(Transfer transfer)
        {
            var tx = transfer == Transfer.Tx;
            var pointer = inFlightPointer;
            var count = (int)(stopPending ? Math.Min(cutCount, inFlightCount) : inFlightCount);
            if(count > 0 && !InDataRam(pointer, count))
            {
                this.Log(LogLevel.Error, "{0}.PTR 0x{1:X8} (+{2}) is not in Data RAM; EasyDMA moves nothing",
                    tx ? "TXD" : "RXD", pointer, count);
            }
            else if(count > 0 && tx)
            {
                target.Write(sysbus.ReadBytes(pointer, count));
                txAmount = (uint)count;
            }
            else if(count > 0)
            {
                var data = target.Read(count) ?? new byte[0];
                for(var i = 0; i < count; i++)
                {
                    // A target with nothing more to send leaves SDA high.
                    sysbus.WriteByte(pointer + (ulong)i, i < data.Length ? data[i] : (byte)0xFF);
                }
                rxAmount = (uint)count;
            }

            // A buffer with no bytes has no last byte, so no LASTTX/LASTRX and
            // no shortcut; embassy-nrf ends it with STOP or SUSPEND itself.
            var complete = count > 0 && count == (int)inFlightCount;
            if(complete && tx)
            {
                lastTx = true;
            }
            else if(complete)
            {
                lastRx = true;
            }
            var startOther = tx ? ShortLastTxStartRx : ShortLastRxStartTx;
            var suspend = tx ? ShortLastTxSuspend : ShortLastRxSuspend;
            var stop = tx ? ShortLastTxStop : ShortLastRxStop;
            if(stopPending)
            {
                BeginStopCondition();
            }
            else if(suspendAfterBuffer || (complete && (shorts & suspend) != 0))
            {
                suspendAfterBuffer = false;
                EnterSuspend();
            }
            else if(complete && (shorts & startOther) != 0)
            {
                Begin(tx ? Transfer.Rx : Transfer.Tx, repeatedStart: true);
            }
            else if(complete && (shorts & stop) != 0)
            {
                BeginStopCondition();
            }
        }

        private void Stop()
        {
            switch(state)
            {
            case State.Suspended:
                this.Log(LogLevel.Warning, "STOP ignored while suspended: trigger RESUME first");
                return;
            case State.Stopping:
                return;
            case State.AddressNack:
                // The address byte finishes and is NACKed, then the STOP goes out.
                stopPending = true;
                return;
            case State.Busy:
                if(!stopPending)
                {
                    CutAfterCurrentByte();
                }
                return;
            }
            pending = Transfer.None;
            suspendAfterBuffer = false;
            if(transactionOpen)
            {
                BeginStopCondition();
            }
            else
            {
                stopped = true; // nothing on the bus to stop
            }
        }

        /// STOP during a buffer: finish the byte on the wire, then stop. The
        /// bytes already started are moved; the rest are not.
        private void CutAfterCurrentByte()
        {
            var bitsPerMicro = BitRate() / 1000000.0;
            var elapsedBits = wire.Value * bitsPerMicro;
            var startBits = addressBytes; // a transaction's first byte follows a START bit
            var total = addressBytes + inFlightCount;
            var onWire = (uint)Math.Max(0, Math.Floor((elapsedBits - startBits) / 9));
            var done = Math.Min(total, onWire + 1);
            stopPending = true;
            cutCount = done > addressBytes ? done - addressBytes : 0;
            var remaining = startBits + 9.0 * done - elapsedBits;
            StartWireBits(Math.Max(remaining, bitsPerMicro));
        }

        /// The STOP condition takes one bit time; STOPPED follows it.
        private void BeginStopCondition()
        {
            stopPending = false;
            pending = Transfer.None;
            suspendAfterBuffer = false;
            state = State.Stopping;
            StartWireBits(1);
        }

        private void EndTransaction()
        {
            if(transactionOpen && target != null)
            {
                target.FinishTransmission();
            }
            transactionOpen = false;
            target = null;
            state = State.Idle;
            stopped = true;
        }

        private void Suspend()
        {
            if(state == State.Busy)
            {
                if(inFlightCount > 0)
                {
                    this.Log(LogLevel.Warning, "SUSPEND during a buffer: this model suspends after it");
                }
                suspendAfterBuffer = true;
                return;
            }
            if(state == State.Idle && transactionOpen)
            {
                EnterSuspend();
            }
        }

        private void EnterSuspend()
        {
            state = State.Suspended;
            suspended = true;
        }

        private void Resume()
        {
            if(state != State.Suspended)
            {
                return;
            }
            state = State.Idle;
            var next = pending;
            pending = Transfer.None;
            if(next != Transfer.None)
            {
                Begin(next);
            }
        }

        private void UpdateInterrupt()
        {
            var active = (stopped && (inten & IntStopped) != 0)
                || (error && (inten & IntError) != 0)
                || (suspended && (inten & IntSuspended) != 0)
                || (rxStarted && (inten & IntRxStarted) != 0)
                || (txStarted && (inten & IntTxStarted) != 0)
                || (lastRx && (inten & IntLastRx) != 0)
                || (lastTx && (inten & IntLastTx) != 0);
            IRQ.Set(active);
        }

        private double BitRate()
        {
            switch(frequency)
            {
            case Frequency100K: return 100000;
            case Frequency250K: return 250000;
            case Frequency400K: return 400000;
            default:
                // The register scales linearly: 0x06400000 is 400 kbps.
                return Math.Max(1000.0, frequency * 400000.0 / Frequency400K);
            }
        }

        private void WarnList(uint value, string name)
        {
            if(value != 0)
            {
                this.Log(LogLevel.Warning, "{0}.LIST={1}: the ArrayList mode is not modelled", name, value);
            }
        }

        private static bool InDataRam(uint pointer, int count)
        {
            return pointer >= DataRamStart && (ulong)pointer + (ulong)count <= DataRamEnd;
        }

        private static uint Flag(bool value)
        {
            return value ? 1u : 0u;
        }

        private readonly object sync = new object();
        private readonly IBusController sysbus;
        private readonly HashSet<int> absent;
        private readonly LimitTimer wire;

        private bool stopped, error, suspended, rxStarted, txStarted, lastRx, lastTx;
        private uint shorts, inten, errorSource, enable, pselScl, pselSda, frequency;
        private uint rxPointer, rxMaxCount, rxAmount, rxList;
        private uint txPointer, txMaxCount, txAmount, txList;
        private uint address;
        private State state;
        private II2CPeripheral target;
        private bool transactionOpen;
        private Transfer pending;
        private Transfer inFlight;
        private uint inFlightPointer, inFlightCount, addressBytes;
        private bool suspendAfterBuffer;
        private bool stopPending;
        private uint cutCount;

        private const uint EnableTwim = 6;
        private const uint Frequency100K = 0x01980000;
        private const uint Frequency250K = 0x04000000;
        private const uint Frequency400K = 0x06400000;
        private const ulong MaxTransferMicroseconds = 10000000;
        private const uint DataRamStart = 0x20000000;
        private const ulong DataRamEnd = 0x20040000;

        private const uint ShortLastTxStartRx = 1u << 7;
        private const uint ShortLastTxSuspend = 1u << 8;
        private const uint ShortLastTxStop = 1u << 9;
        private const uint ShortLastRxStartTx = 1u << 10;
        private const uint ShortLastRxSuspend = 1u << 11;
        private const uint ShortLastRxStop = 1u << 12;
        private const uint ShortsMask = 0x1F80;

        private const uint IntStopped = 1u << 1;
        private const uint IntError = 1u << 9;
        private const uint IntSuspended = 1u << 18;
        private const uint IntRxStarted = 1u << 19;
        private const uint IntTxStarted = 1u << 20;
        private const uint IntLastRx = 1u << 23;
        private const uint IntLastTx = 1u << 24;
        private const uint InterruptMask = IntStopped | IntError | IntSuspended | IntRxStarted | IntTxStarted | IntLastRx | IntLastTx;

        private const uint ErrorAddressNack = 1u << 1;

        private enum State
        {
            Idle,          // no buffer on the wire (a transaction may be open)
            Busy,          // a buffer is on the wire
            AddressNack,   // the address byte is on the wire and will be NACKed
            WaitingForStop, // NACKed; STOP ends the transaction
            Suspended,     // SUSPENDED; RESUME continues
            Stopping,      // the STOP condition is on the wire; STOPPED follows
        }

        private enum Transfer
        {
            None,
            Tx,
            Rx,
        }

        private enum Registers : long
        {
            TasksStartRx = 0x000,
            TasksStartTx = 0x008,
            TasksStop = 0x014,
            TasksSuspend = 0x01C,
            TasksResume = 0x020,
            EventsStopped = 0x104,
            EventsError = 0x124,
            EventsSuspended = 0x148,
            EventsRxStarted = 0x14C,
            EventsTxStarted = 0x150,
            EventsLastRx = 0x15C,
            EventsLastTx = 0x160,
            Shorts = 0x200,
            InterruptEnable = 0x300,
            InterruptEnableSet = 0x304,
            InterruptEnableClear = 0x308,
            ErrorSource = 0x4C4,
            Enable = 0x500,
            PinSelectScl = 0x508,
            PinSelectSda = 0x50C,
            Frequency = 0x524,
            RxdPointer = 0x534,
            RxdMaxCount = 0x538,
            RxdAmount = 0x53C,
            RxdList = 0x540,
            TxdPointer = 0x544,
            TxdMaxCount = 0x548,
            TxdAmount = 0x54C,
            TxdList = 0x550,
            Address = 0x588,
        }
    }
}
