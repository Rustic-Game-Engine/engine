using System.Diagnostics;
using System.Text.Json;

public sealed class InstallerContext
{
    public string GameName { get; } = JsonSerializer.Deserialize<string>(File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "game-name.json")))!;
    public string DefaultDirectory => Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", GameName);
    public string AssetsDirectory => Path.Combine(AppContext.BaseDirectory, "ui");
    public bool Installing { get; private set; }
    public bool Installed { get; private set; }

    // Returns the Inno Setup exit code. Zero means success; failures remain retryable.
    public async Task<int> InstallAsync(string directory, bool desktopShortcut = false)
    {
        if (Installing || Installed) throw new InvalidOperationException("Installation already started or completed.");
        if (string.IsNullOrWhiteSpace(directory) || !Path.IsPathFullyQualified(directory) || directory.Any(char.IsControl) || directory.Contains('"'))
            throw new ArgumentException("Choose an absolute installation folder without quotes or control characters.");
        Installing = true;
        try
        {
            var start = new ProcessStartInfo(Path.Combine(AppContext.BaseDirectory, "payload", "setup.exe")) { UseShellExecute = false };
            foreach (var argument in new[] { "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-", "/DIR=" + directory, "/TASKS=" + (desktopShortcut ? "desktopicon" : "") })
                start.ArgumentList.Add(argument);
            using var process = Process.Start(start) ?? throw new IOException("Could not start installation.");
            await process.WaitForExitAsync();
            Installed = process.ExitCode == 0;
            return process.ExitCode;
        }
        finally { Installing = false; }
    }
}

internal static class Program
{
    [STAThread]
    private static void Main()
    {
        ApplicationConfiguration.Initialize();
        try
        {
            var context = new InstallerContext();
            using var form = new InstallerUi(context);
            form.FormClosing += (_, e) => { if (context.Installing) e.Cancel = true; };
            Application.Run(form);
        }
        catch (Exception error) { MessageBox.Show(error.Message, "Installer could not start"); }
    }
}
