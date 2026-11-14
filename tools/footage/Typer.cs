// Footage branch only: types a scene script through SendInput with virtual keys, the way a
// keyboard would, so the real oh-key hook (started with OHKEY_ACCEPT_INJECTED_KEYS=1) sees it.
using System;
using System.Runtime.InteropServices;
using System.Threading;

public static class FootageTyper
{
    [StructLayout(LayoutKind.Sequential)]
    struct KeyboardInput
    {
        public ushort VirtualKey;
        public ushort ScanCode;
        public uint Flags;
        public uint Time;
        public IntPtr ExtraInfo;
    }

    [StructLayout(LayoutKind.Explicit)]
    struct InputUnion
    {
        [FieldOffset(0)] public KeyboardInput Keyboard;
        [FieldOffset(0)] public long Padding0;
        [FieldOffset(8)] public long Padding1;
        [FieldOffset(16)] public long Padding2;
        [FieldOffset(24)] public long Padding3;
    }

    [StructLayout(LayoutKind.Sequential)]
    struct Input
    {
        public uint Type;
        public InputUnion Data;
    }

    [DllImport("user32.dll", SetLastError = true)]
    static extern uint SendInput(uint count, Input[] inputs, int size);

    [DllImport("user32.dll")]
    static extern short VkKeyScan(char character);

    const uint InputKeyboard = 1;
    const uint KeyUp = 0x0002;
    const ushort VirtualShift = 0x10;
    const ushort VirtualControl = 0x11;
    const ushort VirtualBack = 0x08;
    const ushort VirtualReturn = 0x0D;

    static readonly Random Jitter = new Random(7);

    static void Send(ushort virtualKey, bool release)
    {
        var input = new Input { Type = InputKeyboard };
        input.Data.Keyboard.VirtualKey = virtualKey;
        input.Data.Keyboard.Flags = release ? KeyUp : 0;
        uint sent = SendInput(1, new[] { input }, Marshal.SizeOf(typeof(Input)));
        if (sent != 1)
        {
            throw new InvalidOperationException(
                "SendInput failed, error " + Marshal.GetLastWin32Error() + ", input size " + Marshal.SizeOf(typeof(Input)));
        }
        if (!release)
        {
            Events.Add(Clock.ElapsedMilliseconds + " down " + virtualKey);
        }
    }

    static readonly System.Diagnostics.Stopwatch Clock = new System.Diagnostics.Stopwatch();
    static readonly System.Collections.Generic.List<string> Events = new System.Collections.Generic.List<string>();

    /// Key-down times since `Run` began, one "milliseconds down virtual-key" line each.
    public static string[] TakeEvents()
    {
        string[] lines = Events.ToArray();
        Events.Clear();
        return lines;
    }

    static void Tap(ushort virtualKey, int holdMilliseconds)
    {
        Send(virtualKey, false);
        Thread.Sleep(holdMilliseconds);
        Send(virtualKey, true);
    }

    static void TypeCharacter(char character)
    {
        short scan = VkKeyScan(character);
        ushort virtualKey = (ushort)(scan & 0xFF);
        bool needsShift = (scan & 0x100) != 0;
        if (needsShift)
        {
            Send(VirtualShift, false);
        }
        Tap(virtualKey, 28);
        if (needsShift)
        {
            Send(VirtualShift, true);
        }
    }

    static void Toggle()
    {
        Send(VirtualControl, false);
        Thread.Sleep(40);
        Send(VirtualShift, false);
        Thread.Sleep(60);
        Send(VirtualShift, true);
        Thread.Sleep(30);
        Send(VirtualControl, true);
    }

    /// Plain characters are typed; {BS}, {ENTER}, {TOGGLE} and {WAIT n} are commands.
    /// `pace` is the average gap between keys in milliseconds.
    public static void Run(string script, int pace)
    {
        Clock.Restart();
        Events.Clear();
        int index = 0;
        while (index < script.Length)
        {
            char character = script[index];
            if (character == '{')
            {
                int end = script.IndexOf('}', index);
                string command = script.Substring(index + 1, end - index - 1);
                RunCommand(command);
                index = end + 1;
                continue;
            }
            TypeCharacter(character);
            int pause = character == ' ' ? pace + 70 : character == ',' ? pace + 140 : pace;
            Thread.Sleep(Math.Max(20, pause + Jitter.Next(-pace / 3, pace / 3)));
            index++;
        }
    }

    static void RunCommand(string command)
    {
        if (command == "BS")
        {
            Tap(VirtualBack, 30);
            Thread.Sleep(160);
        }
        else if (command == "ENTER")
        {
            Tap(VirtualReturn, 30);
            Thread.Sleep(200);
        }
        else if (command == "TOGGLE")
        {
            Toggle();
            Thread.Sleep(300);
        }
        else if (command.StartsWith("WAIT "))
        {
            Thread.Sleep(int.Parse(command.Substring(5)));
        }
        else
        {
            throw new ArgumentException("unknown command " + command);
        }
    }
}
