//
// nRF52840 GPIO + GPIOTE models with pin SENSE / LATCH / DETECT support.
//
// Loaded at runtime by Renode (`include @renode/nrf52840_sense_gpio.cs`) and
// used by the renode/nrf52840-sense-gpio.repl overlay (loaded from
// renode/bt2usb-sim.resc) in place of the stock `NRF52840_GPIO` /
// `NRF52840_GPIOTasksEvents` models.
//
// Why: embassy-nrf waits for input edges (`Input::wait_for_low/high/*_edge`)
// through the SENSE -> DETECT -> LATCH -> GPIOTE PORT chain, not through the
// GPIOTE IN channels:
//
//   * init():   GPIO.DETECTMODE = LDETECT, GPIO.LATCH = 0xFFFFFFFF,
//               GPIOTE.INTENSET = PORT, GPIOTE IRQ enabled
//   * wait:     PIN_CNF[n].SENSE = HIGH/LOW, then pend until SENSE reads back
//               DISABLED
//   * GPIOTE IRQ: if EVENTS_PORT { clear it; bits = LATCH; for each bit set
//               SENSE = DISABLED and wake the pin's waker; LATCH = bits }
//
// The stock Renode models tag LATCH and DETECTMODE as unimplemented (LATCH
// always reads 0), so the IRQ handler never finds the triggering pin and the
// future never completes. The stock GPIO also ignores `OnGPIO <pin> false` on a
// pin that is only pulled up (its "no change" check compares against the
// never-driven state), so a first button press is lost.
//
// This model follows the nRF52840 Product Specification (GPIO "Pin sense
// mechanism", GPIOTE "Port event"):
//
//   * a pin's sense condition is met when SENSE=HIGH and the pin is high, or
//     SENSE=LOW and the pin is low;
//   * LATCH[n] is set while pin n's sense condition is met and stays set until
//     the CPU writes 1 to it (write-1-to-clear); a clear does not take effect
//     while the condition still holds;
//   * DETECT is the OR of all sense conditions (DETECTMODE=Default) or
//     LATCH != 0 (DETECTMODE=LDETECT). A rising edge of DETECT raises
//     GPIOTE.EVENTS_PORT. In LDETECT mode, a LATCH clear that leaves bits set
//     generates a new rising edge;
//   * the GPIOTE IRQ is (EVENTS_PORT && INTEN.PORT) || any(EVENTS_IN[i] &&
//     INTEN.IN[i]).
//
// An input pin that has never been driven externally reads its pull level
// (pull-up -> high), so buttons idle high without any setup. Drive a pin from
// the monitor or a test with `gpio0 OnGPIO <pin> <true|false>`.
//
// Also kept from the stock models: OUT/OUTSET/OUTCLR/IN/DIR/DIRSET/DIRCLR,
// GPIO output connections, GPIOTE IN-channel events (edge polarity), GPIOTE
// task mode (TASKS_OUT/SET/CLR driving the pin), and the PPI event hook.
//

using System;
using System.Collections.Generic;

using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.Miscellaneous;

namespace Antmicro.Renode.Peripherals.GPIOPort
{
    public class NRF52840_SenseGPIO : BaseGPIOPort, IDoubleWordPeripheral, IKnownSize
    {
        public NRF52840_SenseGPIO(IMachine machine) : base(machine, NumberOfPins)
        {
            pinCnf = new uint[NumberOfPins];
            driven = new bool[NumberOfPins];
            drivenValue = new bool[NumberOfPins];
            taskOverride = new bool?[NumberOfPins];
            lastLevel = new bool[NumberOfPins];
            Reset();
        }

        public override void Reset()
        {
            base.Reset();
            if(pinCnf == null)
            {
                return; // called from the base constructor, before our fields exist
            }
            outValue = 0;
            direction = 0;
            latch = 0;
            latchedDetectMode = false;
            detect = false;
            for(var i = 0; i < NumberOfPins; i++)
            {
                pinCnf[i] = PinCnfResetValue;
                driven[i] = false;
                drivenValue[i] = false;
                taskOverride[i] = null;
                lastLevel[i] = Level(i);
            }
        }

        public long Size => 0x300;

        /// Raised on a rising edge of this port's DETECT signal (-> GPIOTE PORT).
        public event Action Detect;

        /// Raised when a pin's level changes: (pin, newLevel).
        public event Action<int, bool> PinLevelChanged;

        /// External stimulus: drive pin `number` to `value` (e.g. a button).
        public override void OnGPIO(int number, bool value)
        {
            if(!CheckPinNumber(number))
            {
                return;
            }
            lock(sync)
            {
                base.OnGPIO(number, value);
                driven[number] = true;
                drivenValue[number] = value;
                this.DebugLog("pin {0} driven {1}", number, value ? "high" : "low");
                Update();
            }
        }

        /// Current level of a pin as seen by the input buffer / sense logic.
        public bool Level(int pin)
        {
            if(taskOverride[pin].HasValue)
            {
                return taskOverride[pin].Value;
            }
            if(IsOutput(pin))
            {
                return BitSet(outValue, pin);
            }
            if(driven[pin])
            {
                return drivenValue[pin];
            }
            // Undriven input: the pull resistor decides (floating reads low).
            return Pull(pin) == PullUp;
        }

        /// GPIOTE task mode: take over the pin (value) or release it (null).
        public void SetTaskOverride(int pin, bool? value)
        {
            if(!CheckPinNumber(pin))
            {
                return;
            }
            lock(sync)
            {
                taskOverride[pin] = value;
                Update();
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
            }
        }

        private uint ReadRegister(long offset)
        {
            switch((Registers)offset)
            {
            case Registers.Out:
            case Registers.OutSet:
            case Registers.OutClear:
                return outValue;
            case Registers.In:
                return InputRegister();
            case Registers.Direction:
            case Registers.DirectionSet:
            case Registers.DirectionClear:
                return direction;
            case Registers.Latch:
                return latch;
            case Registers.DetectMode:
                return latchedDetectMode ? 1u : 0u;
            }

            if(offset >= (long)Registers.PinConfigure && offset < (long)Registers.PinConfigure + NumberOfPins * 4 && offset % 4 == 0)
            {
                var pin = (int)((offset - (long)Registers.PinConfigure) / 4);
                return (pinCnf[pin] & ~1u) | (IsOutput(pin) ? 1u : 0u);
            }

            this.Log(LogLevel.Warning, "Unhandled read from offset 0x{0:X}", offset);
            return 0;
        }

        private void WriteRegister(long offset, uint value)
        {
            switch((Registers)offset)
            {
            case Registers.Out:
                outValue = value;
                break;
            case Registers.OutSet:
                outValue |= value;
                break;
            case Registers.OutClear:
                outValue &= ~value;
                break;
            case Registers.In:
                return; // read-only
            case Registers.Direction:
                direction = value;
                break;
            case Registers.DirectionSet:
                direction |= value;
                break;
            case Registers.DirectionClear:
                direction &= ~value;
                break;
            case Registers.Latch:
                WriteLatch(value);
                return;
            case Registers.DetectMode:
                latchedDetectMode = (value & 1) != 0;
                this.DebugLog("DETECTMODE = {0}", latchedDetectMode ? "LDETECT" : "DEFAULT");
                break;
            default:
                if(offset >= (long)Registers.PinConfigure && offset < (long)Registers.PinConfigure + NumberOfPins * 4 && offset % 4 == 0)
                {
                    var pin = (int)((offset - (long)Registers.PinConfigure) / 4);
                    pinCnf[pin] = value & PinCnfMask;
                    if((value & 1) != 0)
                    {
                        direction |= 1u << pin;
                    }
                    else
                    {
                        direction &= ~(1u << pin);
                    }
                    break;
                }
                this.Log(LogLevel.Warning, "Unhandled write to offset 0x{0:X}, value 0x{1:X}", offset, value);
                return;
            }
            Update();
        }

        private uint InputRegister()
        {
            uint result = 0;
            for(var i = 0; i < NumberOfPins; i++)
            {
                // INPUT=Disconnect (PIN_CNF bit 1) detaches the input buffer.
                if((pinCnf[i] & 2) == 0 && Level(i))
                {
                    result |= 1u << i;
                }
            }
            return result;
        }

        private void WriteLatch(uint value)
        {
            // Write-1-to-clear; bits whose sense condition still holds stay set.
            latch &= ~value;
            latch |= SenseConditions();
            this.DebugLog("LATCH cleared 0x{0:X} -> 0x{1:X}", value, latch);

            if(latchedDetectMode)
            {
                if(latch != 0)
                {
                    // PS: bits still set after a LATCH clear generate a new
                    // rising edge on LDETECT.
                    detect = true;
                    RaiseDetect();
                }
                else
                {
                    detect = false;
                }
            }
        }

        // Re-evaluate outputs, pin-level notifications, LATCH and DETECT after
        // any state change.
        private void Update()
        {
            for(var i = 0; i < NumberOfPins; i++)
            {
                var level = Level(i);
                if(IsOutput(i) || taskOverride[i].HasValue)
                {
                    Connections[i].Set(level);
                }
                if(level != lastLevel[i])
                {
                    lastLevel[i] = level;
                    PinLevelChanged?.Invoke(i, level);
                }
            }

            var conditions = SenseConditions();
            latch |= conditions;

            var nextDetect = latchedDetectMode ? latch != 0 : conditions != 0;
            if(nextDetect && !detect)
            {
                detect = true;
                RaiseDetect();
            }
            else
            {
                detect = nextDetect;
            }
        }

        private void RaiseDetect()
        {
            this.DebugLog("DETECT rising edge (LATCH=0x{0:X})", latch);
            Detect?.Invoke();
        }

        private uint SenseConditions()
        {
            uint result = 0;
            for(var i = 0; i < NumberOfPins; i++)
            {
                var sense = (pinCnf[i] >> 16) & 3;
                var level = Level(i);
                if((sense == SenseHigh && level) || (sense == SenseLow && !level))
                {
                    result |= 1u << i;
                }
            }
            return result;
        }

        private bool IsOutput(int pin) => BitSet(direction, pin);

        private uint Pull(int pin) => (pinCnf[pin] >> 2) & 3;

        private static bool BitSet(uint value, int bit) => (value & (1u << bit)) != 0;

        private readonly object sync = new object();
        private uint outValue;
        private uint direction;
        private uint latch;
        private bool latchedDetectMode;
        private bool detect;
        private readonly uint[] pinCnf;
        private readonly bool[] driven;
        private readonly bool[] drivenValue;
        private readonly bool?[] taskOverride;
        private readonly bool[] lastLevel;

        private const int NumberOfPins = 32;
        private const uint PinCnfResetValue = 0x2; // input, buffer disconnected
        // DIR | INPUT | PULL | DRIVE | SENSE
        private const uint PinCnfMask = 0x1u | 0x2u | (0x3u << 2) | (0x7u << 8) | (0x3u << 16);
        private const uint PullUp = 3;
        private const uint SenseHigh = 2;
        private const uint SenseLow = 3;

        private enum Registers
        {
            Out = 0x4,
            OutSet = 0x8,
            OutClear = 0xC,
            In = 0x10,
            Direction = 0x14,
            DirectionSet = 0x18,
            DirectionClear = 0x1C,
            Latch = 0x20,
            DetectMode = 0x24,
            PinConfigure = 0x200,
        }
    }

    public class NRF52840_SenseGPIOTE : IDoubleWordPeripheral, IKnownSize, INRFEventProvider
    {
        public NRF52840_SenseGPIOTE(IMachine machine, NRF52840_SenseGPIO port0 = null, NRF52840_SenseGPIO port1 = null)
        {
            IRQ = new GPIO();
            ports = new[] { port0, port1 };
            channels = new Channel[NumberOfChannels];
            for(var i = 0; i < NumberOfChannels; i++)
            {
                channels[i] = new Channel();
            }

            for(var p = 0; p < ports.Length; p++)
            {
                if(ports[p] == null)
                {
                    continue;
                }
                var portIndex = p;
                ports[p].Detect += OnDetect;
                ports[p].PinLevelChanged += (pin, level) => OnPinLevelChanged(portIndex, pin, level);
            }
            Reset();
        }

        public void Reset()
        {
            eventsPort = false;
            interruptEnable = 0;
            foreach(var ch in channels)
            {
                ch.Reset();
            }
            UpdateInterrupt();
        }

        public GPIO IRQ { get; }

        public long Size => 0x1000;

        public event Action<uint> EventTriggered;

        public uint ReadDoubleWord(long offset)
        {
            if(TryChannel(offset, (long)Registers.EventsIn, out var ch))
            {
                return channels[ch].EventPending ? 1u : 0u;
            }
            if(TryChannel(offset, (long)Registers.Configuration, out ch))
            {
                return channels[ch].Config;
            }
            switch((Registers)offset)
            {
            case Registers.EventsPort:
                return eventsPort ? 1u : 0u;
            case Registers.InterruptEnable:
            case Registers.EnableInterrupt:
            case Registers.DisableInterrupt:
                return interruptEnable;
            }
            if(TryChannel(offset, (long)Registers.TasksOut, out ch)
               || TryChannel(offset, (long)Registers.TasksSet, out ch)
               || TryChannel(offset, (long)Registers.TasksClear, out ch))
            {
                return 0; // write-only
            }
            this.Log(LogLevel.Warning, "Unhandled read from offset 0x{0:X}", offset);
            return 0;
        }

        public void WriteDoubleWord(long offset, uint value)
        {
            var trigger = (value & 1) != 0;
            if(TryChannel(offset, (long)Registers.TasksOut, out var ch))
            {
                if(trigger)
                {
                    RunTask(ch, TaskKind.Out);
                }
                return;
            }
            if(TryChannel(offset, (long)Registers.TasksSet, out ch))
            {
                if(trigger)
                {
                    RunTask(ch, TaskKind.Set);
                }
                return;
            }
            if(TryChannel(offset, (long)Registers.TasksClear, out ch))
            {
                if(trigger)
                {
                    RunTask(ch, TaskKind.Clear);
                }
                return;
            }
            if(TryChannel(offset, (long)Registers.EventsIn, out ch))
            {
                channels[ch].EventPending = trigger;
                UpdateInterrupt();
                return;
            }
            if(TryChannel(offset, (long)Registers.Configuration, out ch))
            {
                WriteConfig(ch, value);
                return;
            }
            switch((Registers)offset)
            {
            case Registers.EventsPort:
                eventsPort = trigger;
                break;
            case Registers.InterruptEnable:
                interruptEnable = value & InterruptMask;
                break;
            case Registers.EnableInterrupt:
                interruptEnable |= value & InterruptMask;
                break;
            case Registers.DisableInterrupt:
                interruptEnable &= ~(value & InterruptMask);
                break;
            default:
                this.Log(LogLevel.Warning, "Unhandled write to offset 0x{0:X}, value 0x{1:X}", offset, value);
                return;
            }
            UpdateInterrupt();
        }

        private void OnDetect()
        {
            this.DebugLog("EVENTS_PORT");
            eventsPort = true;
            EventTriggered?.Invoke((uint)Registers.EventsPort);
            UpdateInterrupt();
        }

        private void OnPinLevelChanged(int port, int pin, bool level)
        {
            for(var i = 0; i < NumberOfChannels; i++)
            {
                var channel = channels[i];
                if(channel.Mode != ModeEvent || channel.Port != port || channel.Pin != pin)
                {
                    continue;
                }
                var previous = channel.LastLevel;
                channel.LastLevel = level;
                var fired = (channel.Polarity == PolarityLoToHi && !previous && level)
                    || (channel.Polarity == PolarityHiToLo && previous && !level)
                    || (channel.Polarity == PolarityToggle && previous != level);
                if(fired)
                {
                    channel.EventPending = true;
                    EventTriggered?.Invoke((uint)Registers.EventsIn + (uint)i * 4);
                }
            }
            UpdateInterrupt();
        }

        private void WriteConfig(int ch, uint value)
        {
            var channel = channels[ch];
            var wasTask = channel.Mode == ModeTask;
            var oldPort = channel.Port;
            var oldPin = channel.Pin;

            channel.Config = value & ConfigMask;

            if(wasTask && (channel.Mode != ModeTask || oldPort != channel.Port || oldPin != channel.Pin))
            {
                ports[oldPort]?.SetTaskOverride(oldPin, null);
            }

            var port = ports[channel.Port];
            if(port == null)
            {
                if(channel.Mode != ModeDisabled)
                {
                    this.Log(LogLevel.Warning, "Channel {0} uses unconnected port {1}", ch, channel.Port);
                }
                return;
            }

            if(channel.Mode == ModeEvent)
            {
                channel.LastLevel = port.Level(channel.Pin);
            }
            else if(channel.Mode == ModeTask)
            {
                channel.TaskLevel = (value & OutInit) != 0;
                port.SetTaskOverride(channel.Pin, channel.TaskLevel);
            }
        }

        private void RunTask(int ch, TaskKind kind)
        {
            var channel = channels[ch];
            var port = ports[channel.Port];
            if(channel.Mode != ModeTask || port == null)
            {
                this.Log(LogLevel.Warning, "Task on channel {0} not configured as TASK", ch);
                return;
            }
            switch(kind)
            {
            case TaskKind.Set:
                channel.TaskLevel = true;
                break;
            case TaskKind.Clear:
                channel.TaskLevel = false;
                break;
            default:
                if(channel.Polarity == PolarityLoToHi)
                {
                    channel.TaskLevel = true;
                }
                else if(channel.Polarity == PolarityHiToLo)
                {
                    channel.TaskLevel = false;
                }
                else if(channel.Polarity == PolarityToggle)
                {
                    channel.TaskLevel = !channel.TaskLevel;
                }
                break;
            }
            port.SetTaskOverride(channel.Pin, channel.TaskLevel);
        }

        private void UpdateInterrupt()
        {
            var flag = eventsPort && (interruptEnable & PortInterrupt) != 0;
            for(var i = 0; i < NumberOfChannels; i++)
            {
                flag |= channels[i].EventPending && (interruptEnable & (1u << i)) != 0;
            }
            IRQ.Set(flag);
        }

        private static bool TryChannel(long offset, long baseOffset, out int channel)
        {
            var delta = offset - baseOffset;
            if(delta >= 0 && delta < NumberOfChannels * 4 && delta % 4 == 0)
            {
                channel = (int)(delta / 4);
                return true;
            }
            channel = -1;
            return false;
        }

        private bool eventsPort;
        private uint interruptEnable;
        private readonly NRF52840_SenseGPIO[] ports;
        private readonly Channel[] channels;

        private const int NumberOfChannels = 8;
        private const uint PortInterrupt = 1u << 31;
        private const uint InterruptMask = 0xFFu | PortInterrupt;
        // MODE | PSEL | PORT | POLARITY | OUTINIT
        private const uint ConfigMask = 0x3u | (0x1Fu << 8) | (0x1u << 13) | (0x3u << 16) | (0x1u << 20);
        private const uint OutInit = 1u << 20;
        private const uint ModeDisabled = 0;
        private const uint ModeEvent = 1;
        private const uint ModeTask = 3;
        private const uint PolarityLoToHi = 1;
        private const uint PolarityHiToLo = 2;
        private const uint PolarityToggle = 3;

        private enum TaskKind
        {
            Out,
            Set,
            Clear,
        }

        private class Channel
        {
            public void Reset()
            {
                Config = 0;
                EventPending = false;
                LastLevel = false;
                TaskLevel = false;
            }

            public uint Config { get; set; }

            public uint Mode => Config & 0x3;

            public int Pin => (int)((Config >> 8) & 0x1F);

            public int Port => (int)((Config >> 13) & 0x1);

            public uint Polarity => (Config >> 16) & 0x3;

            public bool EventPending { get; set; }

            public bool LastLevel { get; set; }

            public bool TaskLevel { get; set; }
        }

        private enum Registers
        {
            TasksOut = 0x0,
            TasksSet = 0x30,
            TasksClear = 0x60,
            EventsIn = 0x100,
            EventsPort = 0x17C,
            InterruptEnable = 0x300,
            EnableInterrupt = 0x304,
            DisableInterrupt = 0x308,
            Configuration = 0x510,
        }
    }
}
