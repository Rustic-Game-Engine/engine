using System.Text.Json;

internal static class WindowTests
{
    private static void Check(bool condition, string message)
    {
        if (!condition) throw new Exception(message);
    }
    [STAThread]
    private static void Main()
    {
        ApplicationConfiguration.Initialize();
        using var form = new Form();
        var options = JsonSerializer.Deserialize<InstallerWindowOptions>(
            """{"titleBar":false,"width":800,"height":500,"minWidth":600,"minHeight":400,"maxWidth":1200,"maxHeight":800,"minimizeButton":false}""",
            new JsonSerializerOptions { PropertyNameCaseInsensitive = true })!;
        options.Apply(form);
        Check(form.FormBorderStyle == FormBorderStyle.None, "Borderless window");
        Check(form.Size == new Size(800, 500), "Initial dimensions");
        Check(form.MinimumSize == new Size(600, 400), "Minimum dimensions");
        Check(form.MaximumSize == new Size(1200, 800), "Maximum dimensions");
        Check(!form.MinimizeBox && form.MaximizeBox, "Window buttons");
        form.Size = new Size(1600, 1000);
        Check(form.Width <= 1200 && form.Height <= 800, "Maximum size enforced");
        form.Size = new Size(100, 100);
        Check(form.Width >= 600 && form.Height >= 400, "Minimum size enforced");
        using var fixedForm = new Form { WindowState = FormWindowState.Maximized };
        new InstallerWindowOptions { Resizable = false, Width = 700, Height = 450 }.Apply(fixedForm);
        Check(fixedForm.FormBorderStyle == FormBorderStyle.FixedSingle, "Fixed title bar");
        Check(!fixedForm.MaximizeBox, "Fixed window cannot maximize");
        Check(fixedForm.WindowState == FormWindowState.Normal, "Manifest overrides constructor state");
        InstallerWindowCommands.ToggleMaximize(fixedForm);
        Check(fixedForm.WindowState == FormWindowState.Normal, "Bridge respects maximize policy");
        using var maximized = new Form();
        new InstallerWindowOptions { StartMaximized = true }.Apply(maximized);
        maximized.Show();
        Check(maximized.WindowState == FormWindowState.Maximized, "Initial maximize");
        maximized.Close();
        using var borderless = new Form();
        new InstallerWindowOptions { TitleBar = false, StartMaximized = true,
            MaxWidth = 1000, MaxHeight = 700 }.Apply(borderless);
        borderless.Show();
        Application.DoEvents();
        var workArea = Screen.FromControl(borderless).WorkingArea;
        Check(borderless.Width <= Math.Min(1000, workArea.Width)
            && borderless.Height <= Math.Min(700, workArea.Height), "Borderless maximize respects size limits");
        Check(workArea.Contains(borderless.Bounds), "Borderless maximize stays within working area");
        borderless.Close();
        foreach (var invalid in new[] {
            new InstallerWindowOptions { MaxWidth = 500 },
            new InstallerWindowOptions { MinHeight = 700 },
            new InstallerWindowOptions { MaxHeight = -1 },
            new InstallerWindowOptions { Resizable = false, StartMaximized = true } })
        {
            try { invalid.Apply(form); throw new Exception("Invalid settings were accepted"); }
            catch (ArgumentException) { }
        }
        Console.WriteLine("Installer native window settings passed.");
    }
}
