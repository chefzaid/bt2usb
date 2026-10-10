//
// SSD1306 128x64 OLED controller model, on I2C, with a screen text reader.
//
// Loaded at runtime by Renode (`include @renode/ssd1306.cs`) and attached to
// the TWIM model at address 0x3C by renode/nrf52840-twim-oled.repl. Renode has
// no SSD1306 model.
//
// The I2C protocol follows the SSD1306 datasheet (section 8.1.5): each write
// starts with a control byte. Co=0 makes the rest of the write commands
// (D/C#=0, 0x00) or display data (D/C#=1, 0x40); Co=1 (0x80, 0xC0) covers one
// byte and another control byte follows. Commands with arguments take them
// from the next command bytes, across control bytes and writes. A read
// returns the status byte, whose bit 6 is set while the display is off.
//
// Modelled: display on/off (0xAE/0xAF), the charge pump (0x8D; the panel
// stays dark without it), the three addressing modes (0x20) with the column
// and page windows (0x21, 0x22) and the page-mode pointers (0x00-0x1F,
// 0xB0-0xB7), segment remap (0xA0/0xA1), COM scan direction (0xC0/0xC8),
// entire display on (0xA4/0xA5), and inverse (0xA6/0xA7). As the datasheet
// says (10.1.12), segment remap applies to data written after it, so it
// never mirrors what RAM already holds; the COM scan direction flips the
// picture at once. Contrast, multiplex ratio, display offset, start line,
// COM pins configuration, and scrolling are recorded; clock, pre-charge, and
// VCOMH settings only change timing and brightness and are ignored. The
// panel's geometry is modelled only for the 128x64 defaults (multiplex 64,
// offset 0, start line 0, COM pins 0x12, no scrolling): any other value logs
// a warning, and `Text` then reports the settings instead of reading text the
// panel would not show that way.
//
// The panel is mounted the way common 128x64 modules are: with segment remap
// and the remapped COM scan (0xA1, 0xC8), RAM column 0 and row 0 are the top
// left; without them the picture is mirrored.
//
// A power-on reset (Reset, or `oled Reset` from the monitor, as when the
// module is plugged back in) turns the display off, restores the defaults,
// and leaves RAM holding power-on noise. `NoisyBytes` counts the bytes the
// panel received while it was lit (display and charge pump on) and still
// showed some of that noise: firmware that lights the panel before clearing
// or drawing its RAM shows noise until it does, and the next byte it sends,
// whatever it is, counts.
//
// Text reader: `oled LoadFont @renode/oled-font-6x10.txt` loads the firmware's
// font (generated from it by tests/oled_font.rs), and `oled Text` returns the
// lines of text on the panel, top to bottom, one per line: it finds every row
// where 21 consecutive 6-pixel cells all match glyphs, keeps leading spaces,
// and drops trailing ones. Lit pixels that no line explains are reported as
// "(unreadable pixels in rows A-B)", so nothing on the panel goes unchecked.
// A line made only of '-' and spaces reads as '_' (those glyphs are the same
// bar at different heights). `oled Dump` draws the panel as '#' and '.'.
//

using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;

using Antmicro.Renode.Exceptions;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.I2C;
using Antmicro.Renode.Utilities;

namespace Antmicro.Renode.Peripherals.Video
{
    public class SSD1306 : II2CPeripheral
    {
        public SSD1306()
        {
            ram = new byte[Pages * Columns];
            written = new bool[Pages * Columns];
            arguments = new List<byte>();
            glyphs = new Dictionary<ulong, char>();
            Reset();
        }

        public void Reset()
        {
            for(var i = 0; i < ram.Length; i++)
            {
                // Power-on RAM content is undefined; a fixed pattern keeps runs
                // reproducible and is easy to recognise in a dump.
                ram[i] = (byte)((i % 2 == 0) ? 0xA5 : 0x5A);
                written[i] = false;
            }
            unwritten = ram.Length;
            NoisyBytes = 0;
            displayOn = false;
            chargePump = false;
            mode = AddressingMode.Page;
            columnStart = 0;
            columnEnd = Columns - 1;
            pageStart = 0;
            pageEnd = Pages - 1;
            column = 0;
            page = 0;
            pageModeColumn = 0;
            segmentRemap = false;
            comRemap = false;
            entireOn = false;
            inverse = false;
            multiplex = Rows - 1;
            displayOffset = 0;
            startLine = 0;
            comPins = DefaultComPins;
            scrolling = false;
            contrast = 0x7F;
            pendingCommand = null;
            arguments.Clear();
            control = ControlState.ExpectControl;
        }

        public bool DisplayOn => displayOn;

        public bool ChargePumpOn => chargePump;

        public bool Scrolling => scrolling;

        public byte Contrast => contrast;

        /// Bytes received while the panel was lit and some of its RAM still
        /// held power-on noise (reset by Reset).
        public int NoisyBytes { get; private set; }

        public void Write(byte[] data)
        {
            foreach(var b in data)
            {
                if(Lit && unwritten > 0)
                {
                    NoisyBytes++;
                }
                switch(control)
                {
                case ControlState.ExpectControl:
                    var single = (b & 0x80) != 0;
                    var isData = (b & 0x40) != 0;
                    control = single
                        ? (isData ? ControlState.SingleData : ControlState.SingleCommand)
                        : (isData ? ControlState.DataStream : ControlState.CommandStream);
                    break;
                case ControlState.SingleCommand:
                    Command(b);
                    control = ControlState.ExpectControl;
                    break;
                case ControlState.SingleData:
                    Data(b);
                    control = ControlState.ExpectControl;
                    break;
                case ControlState.CommandStream:
                    Command(b);
                    break;
                case ControlState.DataStream:
                    Data(b);
                    break;
                }
            }
        }

        public byte[] Read(int count = 1)
        {
            var status = (byte)(displayOn ? 0x00 : 0x40);
            return Enumerable.Repeat(status, count).ToArray();
        }

        public void FinishTransmission()
        {
            control = ControlState.ExpectControl;
        }

        /// Load the glyph table written by tests/oled_font.rs.
        public void LoadFont(ReadFilePath path)
        {
            glyphs.Clear();
            foreach(var raw in File.ReadAllLines(path))
            {
                var line = raw.Trim();
                if(line.Length == 0 || line.StartsWith("#"))
                {
                    continue;
                }
                var fields = line.Split((char[])null, StringSplitOptions.RemoveEmptyEntries);
                if(fields.Length != GlyphHeight + 1)
                {
                    throw new RecoverableException($"{path}: expected a code and {GlyphHeight} rows in \"{line}\"");
                }
                ulong key = 0;
                for(var row = 0; row < GlyphHeight; row++)
                {
                    key |= (ulong)(Convert.ToByte(fields[row + 1], 16) & 0x3F) << (GlyphWidth * row);
                }
                glyphs[key] = (char)Convert.ToByte(fields[0], 16);
            }
            this.Log(LogLevel.Info, "loaded {0} glyphs from {1}", glyphs.Count, path);
        }

        /// The text on the panel, one line per text row, or a note in
        /// parentheses when the panel shows no readable picture.
        public string Text()
        {
            if(!displayOn)
            {
                return "(display off)";
            }
            if(!chargePump)
            {
                return "(display dark: charge pump off)";
            }
            if(entireOn)
            {
                return "(entire display on)";
            }
            var unmodelled = UnmodelledGeometry();
            if(unmodelled.Count > 0)
            {
                return $"(picture not modelled: {string.Join(", ", unmodelled)})";
            }
            if(glyphs.Count == 0)
            {
                return "(no font: oled LoadFont @renode/oled-font-6x10.txt)";
            }
            var picture = Picture(false);
            var explained = new bool[Rows, Columns];
            var lines = new List<string>();
            if(inverse)
            {
                lines.Add("(inverse)");
            }
            for(var top = 0; top <= Rows - MinimumVisibleRows;)
            {
                var text = ReadLine(picture, top);
                if(text != null && text.Trim().Length > 0)
                {
                    lines.Add(text.TrimEnd());
                    for(var row = top; row < Math.Min(Rows, top + GlyphHeight); row++)
                    {
                        for(var x = 0; x < TextColumns * GlyphWidth; x++)
                        {
                            explained[row, x] = true;
                        }
                    }
                    top += GlyphHeight;
                }
                else
                {
                    top++;
                }
            }
            int first = -1, last = -1;
            for(var row = 0; row < Rows; row++)
            {
                for(var x = 0; x < Columns; x++)
                {
                    if(picture[row, x] && !explained[row, x])
                    {
                        first = first < 0 ? row : first;
                        last = row;
                    }
                }
            }
            if(first >= 0)
            {
                lines.Add($"(unreadable pixels in rows {first}-{last})");
            }
            return lines.Count == 0 ? "(blank)" : string.Join("\n", lines);
        }

        /// The panel as 64 rows of 128 characters: '#' lit, '.' dark. With
        /// settings `Text` reports as not modelled, this is the RAM as the
        /// default geometry would show it.
        public string Dump()
        {
            var picture = Picture(true);
            var text = new StringBuilder();
            for(var row = 0; row < Rows; row++)
            {
                for(var x = 0; x < Columns; x++)
                {
                    text.Append(picture[row, x] ? '#' : '.');
                }
                if(row < Rows - 1)
                {
                    text.Append('\n');
                }
            }
            return text.ToString();
        }

        private void Command(byte b)
        {
            if(pendingCommand.HasValue)
            {
                arguments.Add(b);
                if(arguments.Count == ArgumentCount(pendingCommand.Value))
                {
                    var command = pendingCommand.Value;
                    pendingCommand = null;
                    Execute(command, arguments.ToArray());
                    arguments.Clear();
                }
                return;
            }
            if(ArgumentCount(b) > 0)
            {
                pendingCommand = b;
                arguments.Clear();
                return;
            }
            Execute(b, new byte[0]);
        }

        private static int ArgumentCount(byte command)
        {
            switch(command)
            {
            case 0x20: case 0x81: case 0x8D: case 0xA8: case 0xD3: case 0xD5: case 0xD9: case 0xDA: case 0xDB:
                return 1;
            case 0x21: case 0x22: case 0xA3:
                return 2;
            case 0x29: case 0x2A:
                return 5;
            case 0x26: case 0x27:
                return 6;
            default:
                return 0;
            }
        }

        private void Execute(byte command, byte[] args)
        {
            if(command <= 0x0F)
            {
                pageModeColumn = (pageModeColumn & 0xF0) | (command & 0x0F);
                column = mode == AddressingMode.Page ? pageModeColumn : column;
                return;
            }
            if(command <= 0x1F)
            {
                pageModeColumn = (pageModeColumn & 0x0F) | ((command & 0x07) << 4);
                column = mode == AddressingMode.Page ? pageModeColumn : column;
                return;
            }
            if(command >= 0x40 && command <= 0x7F)
            {
                startLine = command & 0x3F;
                WarnIf(startLine != 0, "display start line {0} is not modelled", startLine);
                return;
            }
            if(command >= 0xB0 && command <= 0xB7)
            {
                page = command & 0x07;
                return;
            }
            switch(command)
            {
            case 0x20:
                if((args[0] & 0x03) == 0x03)
                {
                    this.Log(LogLevel.Warning, "invalid addressing mode 0x{0:X2} ignored", args[0]);
                    return;
                }
                mode = (AddressingMode)(args[0] & 0x03);
                return;
            case 0x21:
                columnStart = args[0] & 0x7F;
                columnEnd = args[1] & 0x7F;
                column = columnStart;
                return;
            case 0x22:
                pageStart = args[0] & 0x07;
                pageEnd = args[1] & 0x07;
                page = pageStart;
                return;
            case 0x26: case 0x27: case 0x29: case 0x2A: case 0xA3:
                return; // scroll setup only; 0x2F starts scrolling
            case 0x2E:
                scrolling = false;
                return;
            case 0x2F:
                scrolling = true;
                this.Log(LogLevel.Warning, "scrolling is not modelled");
                return;
            case 0x81:
                contrast = args[0];
                return;
            case 0x8D:
                chargePump = (args[0] & 0x04) != 0;
                return;
            case 0xA0: case 0xA1:
                segmentRemap = command == 0xA1;
                return;
            case 0xA4: case 0xA5:
                entireOn = command == 0xA5;
                return;
            case 0xA6: case 0xA7:
                inverse = command == 0xA7;
                return;
            case 0xA8:
                multiplex = args[0] & 0x3F;
                WarnIf(multiplex != Rows - 1, "multiplex ratio {0} is not modelled", multiplex + 1);
                return;
            case 0xAE: case 0xAF:
                displayOn = command == 0xAF;
                return;
            case 0xC0: case 0xC8:
                comRemap = command == 0xC8;
                return;
            case 0xD3:
                displayOffset = args[0] & 0x3F;
                WarnIf(displayOffset != 0, "display offset {0} is not modelled", displayOffset);
                return;
            case 0xDA:
                comPins = args[0] & 0x32;
                WarnIf(comPins != DefaultComPins, "COM pins configuration 0x{0:X2} is not modelled", comPins);
                return;
            case 0xD5: case 0xD9: case 0xDB: case 0xE3:
                return; // clock, pre-charge, VCOMH, NOP: timing and brightness only
            default:
                this.Log(LogLevel.Warning, "unknown command 0x{0:X2} ignored", command);
                return;
            }
        }

        private void Data(byte b)
        {
            // RAM is kept by segment: segment remap decides where a column's
            // byte lands as it is written.
            var segment = segmentRemap ? Columns - 1 - column : column;
            var index = page * Columns + segment;
            ram[index] = b;
            if(!written[index])
            {
                written[index] = true;
                unwritten--;
            }
            switch(mode)
            {
            case AddressingMode.Horizontal:
                if(++column > columnEnd)
                {
                    column = columnStart;
                    page = page >= pageEnd ? pageStart : page + 1;
                }
                break;
            case AddressingMode.Vertical:
                if(++page > pageEnd)
                {
                    page = pageStart;
                    column = column >= columnEnd ? columnStart : column + 1;
                }
                break;
            case AddressingMode.Page:
                // The page stays; the column returns to the page-mode start.
                column = column >= Columns - 1 ? pageModeColumn : column + 1;
                break;
            }
        }

        /// The display is on and the charge pump powers it.
        private bool Lit => displayOn && chargePump;

        /// The settings that differ from the 128x64 geometry this model draws.
        private List<string> UnmodelledGeometry()
        {
            var settings = new List<string>();
            if(multiplex != Rows - 1)
            {
                settings.Add($"multiplex ratio {multiplex + 1}");
            }
            if(displayOffset != 0)
            {
                settings.Add($"display offset {displayOffset}");
            }
            if(startLine != 0)
            {
                settings.Add($"start line {startLine}");
            }
            if(comPins != DefaultComPins)
            {
                settings.Add($"COM pins 0x{comPins:X2}");
            }
            if(scrolling)
            {
                settings.Add("scrolling");
            }
            return settings;
        }

        /// The picture as the panel shows it. `asShown` applies display off,
        /// a missing charge pump, entire display on, and inverse; without it
        /// the result is the RAM content as mounted, for the text reader.
        private bool[,] Picture(bool asShown)
        {
            var picture = new bool[Rows, Columns];
            var dark = asShown && !Lit;
            for(var row = 0; row < Rows; row++)
            {
                var ramRow = comRemap ? row : Rows - 1 - row;
                for(var x = 0; x < Columns; x++)
                {
                    // Column 0 lands on the last segment under remap (0xA1),
                    // which this module wires to the left edge.
                    var segment = Columns - 1 - x;
                    var lit = (ram[(ramRow / 8) * Columns + segment] >> (ramRow % 8) & 1) != 0;
                    if(asShown)
                    {
                        lit = !dark && (entireOn || (lit != inverse));
                    }
                    picture[row, x] = lit;
                }
            }
            return picture;
        }

        /// The 21 cells whose top row is `top`, or null if any cell matches no
        /// glyph. Rows below the panel are unknown; a cell they cut off
        /// matches the glyph whose hidden rows are blank, if one is unique.
        private string ReadLine(bool[,] picture, int top)
        {
            var visible = Math.Min(GlyphHeight, Rows - top);
            var mask = visible == GlyphHeight ? ulong.MaxValue : (1UL << (GlyphWidth * visible)) - 1;
            var text = new StringBuilder();
            for(var cell = 0; cell < TextColumns; cell++)
            {
                ulong key = 0;
                for(var row = 0; row < visible; row++)
                {
                    for(var x = 0; x < GlyphWidth; x++)
                    {
                        if(picture[top + row, cell * GlyphWidth + x])
                        {
                            key |= 1UL << (GlyphWidth * row + GlyphWidth - 1 - x);
                        }
                    }
                }
                char glyph;
                if(visible == GlyphHeight)
                {
                    if(!glyphs.TryGetValue(key, out glyph))
                    {
                        return null;
                    }
                }
                else
                {
                    var matches = glyphs.Where(g => (g.Key & mask) == key).ToList();
                    var blankBelow = matches.Where(g => (g.Key & ~mask) == 0).ToList();
                    if(blankBelow.Count == 1)
                    {
                        glyph = blankBelow[0].Value;
                    }
                    else if(matches.Count == 1)
                    {
                        glyph = matches[0].Value;
                    }
                    else
                    {
                        return null;
                    }
                }
                text.Append(glyph);
            }
            return text.ToString();
        }

        private void WarnIf(bool condition, string format, int value)
        {
            if(condition)
            {
                this.Log(LogLevel.Warning, format, value);
            }
        }

        private readonly byte[] ram;
        private readonly bool[] written;
        private readonly List<byte> arguments;
        private readonly Dictionary<ulong, char> glyphs;

        private int unwritten;
        private bool displayOn, chargePump, segmentRemap, comRemap, entireOn, inverse, scrolling;
        private AddressingMode mode;
        private int columnStart, columnEnd, pageStart, pageEnd, column, page, pageModeColumn;
        private int multiplex, displayOffset, startLine, comPins;
        private byte contrast;
        private byte? pendingCommand;
        private ControlState control;

        private const int Columns = 128;
        private const int Rows = 64;
        private const int Pages = Rows / 8;
        private const int GlyphWidth = 6;
        private const int GlyphHeight = 10;
        private const int TextColumns = Columns / GlyphWidth;
        private const int MinimumVisibleRows = 8;
        private const int DefaultComPins = 0x12;

        private enum AddressingMode
        {
            Horizontal = 0,
            Vertical = 1,
            Page = 2,
        }

        private enum ControlState
        {
            ExpectControl,
            SingleCommand,
            SingleData,
            CommandStream,
            DataStream,
        }
    }
}
