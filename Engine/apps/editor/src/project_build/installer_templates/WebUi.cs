using Microsoft.Web.WebView2.Core;
using Microsoft.Web.WebView2.WinForms;
using System.Text.Json;

public sealed class InstallerUi : Form
{
    private readonly InstallerContext context;
    private readonly WebView2 browser = new() { Dock = DockStyle.Fill };
    public InstallerUi(InstallerContext context)
    {
        this.context = context;
        Text = context.GameName + " Setup";
        Width = 900; Height = 650;
        Controls.Add(browser);
        Shown += async (_, _) =>
        {
            try
            {
                // Keep browser cache writable even when setup is on read-only media.
                var environment = await CoreWebView2Environment.CreateAsync(null,
                    Path.Combine(Path.GetTempPath(), "RusticInstaller", Guid.NewGuid().ToString("N")));
                await browser.EnsureCoreWebView2Async(environment);
                var core = browser.CoreWebView2;
                core.SetVirtualHostNameToFolderMapping("installer.rustic", context.AssetsDirectory, CoreWebView2HostResourceAccessKind.DenyCors);
                core.NavigationStarting += (_, e) => { if (!Local(e.Uri)) e.Cancel = true; };
                core.NewWindowRequested += (_, e) => e.Handled = true;
                core.PermissionRequested += (_, e) => e.State = CoreWebView2PermissionState.Deny;
                core.Settings.AreHostObjectsAllowed = false;
                core.WebMessageReceived += Receive;
                var entry = JsonSerializer.Deserialize<string>(File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "ui-entry.json")))!;
                browser.Source = new Uri("https://installer.rustic/" + string.Join("/", entry.Replace('\\', '/').Split('/').Select(Uri.EscapeDataString)));
            }
            catch (Exception error) { MessageBox.Show("Install Microsoft Edge WebView2 Runtime, then retry. " + error.Message, Text); Close(); }
        };
    }
    private static bool Local(string uri) => Uri.TryCreate(uri, UriKind.Absolute, out var parsed)
        && parsed.Scheme == "https" && parsed.Host == "installer.rustic" && parsed.IsDefaultPort;
    private void Send(object message) => browser.CoreWebView2.PostWebMessageAsJson(JsonSerializer.Serialize(message));
    private async void Receive(object? sender, CoreWebView2WebMessageReceivedEventArgs e)
    {
        if (!Local(e.Source)) return;
        try
        {
            using var message = JsonDocument.Parse(e.WebMessageAsJson);
            var root = message.RootElement;
            switch (root.GetProperty("type").GetString())
            {
                case "ready": Send(new { type = "ready", name = context.GameName, directory = context.DefaultDirectory }); break;
                case "install":
                    if (context.Installing || context.Installed) return;
                    var directory = root.TryGetProperty("directory", out var path) ? path.GetString()! : context.DefaultDirectory;
                    var shortcut = root.TryGetProperty("desktopShortcut", out var option) && option.GetBoolean();
                    Send(new { type = "installing" });
                    var code = await context.InstallAsync(directory, shortcut);
                    Send(new { type = code == 0 ? "complete" : "error", exitCode = code });
                    break;
                case "close": if (!context.Installing) Close(); break;
            }
        }
        catch (Exception error) { Send(new { type = "error", message = error.Message }); }
    }
}
