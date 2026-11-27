using System.Runtime.InteropServices;
using System.Text.Json.Serialization;

[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]
public sealed class InstallerWindowOptions
{
    public int Width { get; init; } = 900;
    public int Height { get; init; } = 650;
    public int MinWidth { get; init; } = 320;
    public int MinHeight { get; init; } = 240;
    public int MaxWidth { get; init; }
    public int MaxHeight { get; init; }
    public bool TitleBar { get; init; } = true;
    public bool Resizable { get; init; } = true;
    public bool MinimizeButton { get; init; } = true;
    public bool MaximizeButton { get; init; } = true;
    public bool StartMaximized { get; init; }

    public void Apply(Form form)
    {
        ValidateAxis(Width, MinWidth, MaxWidth);
        ValidateAxis(Height, MinHeight, MaxHeight);
        if (StartMaximized && (!Resizable || !MaximizeButton))
            throw new ArgumentException("startMaximized requires resizable and maximizeButton.");
        form.FormBorderStyle = TitleBar
            ? (Resizable ? FormBorderStyle.Sizable : FormBorderStyle.FixedSingle)
            : FormBorderStyle.None;
        form.MinimizeBox = MinimizeButton;
        form.MaximizeBox = Resizable && MaximizeButton;
        form.MinimumSize = new Size(MinWidth, MinHeight);
        form.MaximumSize = new Size(MaxWidth, MaxHeight);
        form.WindowState = FormWindowState.Normal;
        form.Size = new Size(Width, Height);
        form.StartPosition = FormStartPosition.CenterScreen;
        // Borderless maximization otherwise covers the taskbar. Keep finite limits.
        if (!TitleBar) _ = new InstallerWindowBounds(form, this);
        // Maximize after the native sizing hook is attached to the created handle.
        if (StartMaximized) form.Load += (_, _) => form.WindowState = FormWindowState.Maximized;
    }

    private static void ValidateAxis(int initial, int minimum, int maximum)
    {
        if (minimum < 1 || minimum > 32767 || initial < minimum || initial > 32767
            || maximum < 0 || maximum > 32767 || (maximum != 0 && (maximum < minimum || initial > maximum)))
            throw new ArgumentException("Invalid installer window dimensions: minimum <= initial <= maximum; maximum 0 means unlimited.");
    }
}

internal sealed class InstallerWindowBounds : NativeWindow
{
    private readonly InstallerWindowOptions options;
    public InstallerWindowBounds(Form form, InstallerWindowOptions options)
    {
        this.options = options;
        form.HandleCreated += (_, _) => AssignHandle(form.Handle);
        form.HandleDestroyed += (_, _) => ReleaseHandle();
        if (form.IsHandleCreated) AssignHandle(form.Handle);
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct MinMaxInfo
    {
        public Point Reserved, MaxSize, MaxPosition, MinTrackSize, MaxTrackSize;
    }

    protected override void WndProc(ref Message message)
    {
        base.WndProc(ref message);
        if (message.Msg != 0x24) return; // WM_GETMINMAXINFO
        var screen = Screen.FromHandle(message.HWnd);
        var area = screen.WorkingArea;
        var info = Marshal.PtrToStructure<MinMaxInfo>(message.LParam);
        info.MaxPosition = new Point(area.X - screen.Bounds.X, area.Y - screen.Bounds.Y);
        info.MaxSize = new Point(
            options.MaxWidth == 0 ? area.Width : Math.Min(area.Width, options.MaxWidth),
            options.MaxHeight == 0 ? area.Height : Math.Min(area.Height, options.MaxHeight));
        Marshal.StructureToPtr(info, message.LParam, false);
    }
}

// Used only by the generated web host; C# authors can use ordinary Forms APIs.
public static class InstallerWindowCommands
{
    [DllImport("user32.dll")]
    private static extern bool ReleaseCapture();
    [DllImport("user32.dll")]
    private static extern IntPtr SendMessage(IntPtr window, int message, IntPtr wParam, IntPtr lParam);

    public static void Drag(Form form)
    {
        if (form.WindowState != FormWindowState.Normal) return;
        ReleaseCapture();
        SendMessage(form.Handle, 0xA1, new IntPtr(2), IntPtr.Zero); // WM_NCLBUTTONDOWN, HTCAPTION
    }

    public static void ToggleMaximize(Form form)
    {
        if (form.MaximizeBox)
            form.WindowState = form.WindowState == FormWindowState.Maximized
                ? FormWindowState.Normal : FormWindowState.Maximized;
    }

    public static void Resize(Form form, InstallerWindowOptions? options, string edge)
    {
        if (options?.TitleBar != false || !options.Resizable || form.WindowState != FormWindowState.Normal) return;
        var hit = edge switch { "left" => 10, "right" => 11, "top" => 12,
            "topLeft" => 13, "topRight" => 14, "bottom" => 15, "bottomLeft" => 16, "bottomRight" => 17, _ => 0 };
        if (hit == 0) throw new ArgumentException("Unknown window resize edge.");
        ReleaseCapture();
        SendMessage(form.Handle, 0xA1, new IntPtr(hit), IntPtr.Zero);
    }
}
