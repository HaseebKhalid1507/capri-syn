// A stand-in synaps.exe for the shots' brain/crash (tools/shots/scenes.ps1 builds it with the .NET
// Framework's csc): one process, as Synaps is (tools/fakesynaps.cmd is cmd.exe running python, so
// the fake isn't the process she starts). `login --provider github-copilot`: Copilot's device login
// as Synaps 0.12 says it on stderr (its code and GitHub's page, coloured), its "browser" started
// (a process of its own, as Synaps' `cmd /c start` is), then it polls on for its token whatever its
// stdin does, as GitHub's device flow does: only her job object ends it. FAKE_SYNAPS_PIDS: a file
// its pid, then its browser's, are appended to. She dies: it must go with her, its browser stay.
using System;
using System.Diagnostics;
using System.IO;
using System.Threading;

static class FakeSynaps
{
    static int Main(string[] args)
    {
        var browser = Process.Start(new ProcessStartInfo("ping.exe", "-n 900 127.0.0.1") { UseShellExecute = false, CreateNoWindow = true });
        var pids = Environment.GetEnvironmentVariable("FAKE_SYNAPS_PIDS");
        if (!string.IsNullOrEmpty(pids))
        {
            File.AppendAllText(pids, Process.GetCurrentProcess().Id + "\n" + browser.Id + "\n");
        }
        const string E = "\u001b";
        var err = Console.Error;
        err.Write("\n" + E + "[1mGitHub Copilot device login" + E + "[0m\n");
        err.Write("  Enter code: " + E + "[36mFAKE-P0LL" + E + "[0m\n");
        err.Write("  At:         " + E + "[36mhttps://github.com/login/device" + E + "[0m\n\n");
        err.Flush();
        while (true)
        {
            Thread.Sleep(1000);
        }
    }
}
