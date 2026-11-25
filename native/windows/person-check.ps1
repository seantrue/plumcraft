# The launch as a person would (N5 gate; the maintainer: "Add it to every shell's gate").
#
# Starts the built Plummer with nothing but --root (a scratch data folder): no file, no engine flag, no
# control port, no status file, no script steps. Then it uses the app the way a person does: real mouse and
# keyboard input (SendInput) on the signed-in desktop, UI Automation to find and read what is on the
# screen, and a screenshot after every step for someone to look at. It prints one line per step, PASS or
# FAIL, with what it saw.
#
# What it cannot see: anything UI Automation does not expose and a screenshot does not show to a script
# (how the canvas looks: the screenshots are for a person to look at), sound, and timing a person would
# notice. The canvas's faces and pins are read through the canvas's automation peer.
#
# The environment is a person's, with two folders redirected so nothing outside scratch is touched:
# PLUMCRAFT_ENGINE_FOLDER (the engines folder, holding a copy of the conformance suite's probe manifest)
# and TEMP/TMP. Everything else the app writes is under --root.
#
# It must run on the signed-in console desktop (input from another session goes nowhere). From an SSH
# session it starts itself there as a one-shot Interactive scheduled task, waits (with a limit) for it to
# finish, prints its report, removes the task and stops the app it started (by the process id it captured).
#
#   person-check.ps1 [-Exe PATH] [-Platform ARM64|x64] [-Work DIR] [-Timeout 600] [-SlowStartWaitS N]
#
# -SlowStartWaitS (off by default): wait N seconds, not 60, for the app's first window, for a machine known to
# start apps slowly (antivirus holding new files for a verdict it can't get offline). The report says when it
# was used; a run with it does not stand for the 60-second start a person expects.
param([string]$Exe = '', [string]$Platform = '', [string]$Work = '', [int]$Timeout = 600,
      [int]$SlowStartWaitS = 0, [string]$Only = '', [switch]$Inner)
$Discover = $false
# the machine's own platform unless one is named (ARM64 or x64)
if (-not $Platform) { $Platform = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'ARM64' } else { 'x64' } }

$here = Split-Path -Parent $PSCommandPath
$checkout = Split-Path -Parent (Split-Path -Parent $here)
if (-not $Exe) {
    $rid = if ($Platform -eq 'x64') { 'win-x64' } else { 'win-arm64' }
    $Exe = Join-Path $here "src\PlumCraft.App\bin\$Platform\Release\net10.0-windows10.0.26100.0\$rid\Plummer.exe"
}
if (-not $Work) { $Work = Join-Path $env:TEMP 'plummer-person-check' }
$report = Join-Path $Work 'report.txt'
$done = Join-Path $Work 'done.txt'
$pidFile = Join-Path $Work 'app.pid'

if (-not $Inner) {
    if (-not (Test-Path $Exe)) { "FAIL build: no app at $Exe"; exit 2 }
    New-Item -ItemType Directory -Force $Work | Out-Null
    Remove-Item $report, $done, $pidFile -ErrorAction SilentlyContinue
    $name = 'plummer-person-check'
    Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue
    $flags = "-Inner -Exe `"$Exe`" -Work `"$Work`" -SlowStartWaitS $SlowStartWaitS$(if ($Only) { " -Only $Only" })"
    $action = New-ScheduledTaskAction -Execute 'conhost.exe' -Argument "--headless powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$PSCommandPath`" $flags"
    $principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive
    $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds ($Timeout + 60)) -AllowStartIfOnBatteries
    Register-ScheduledTask -TaskName $name -Action $action -Principal $principal -Settings $settings | Out-Null
    Start-ScheduledTask -TaskName $name
    $t0 = Get-Date
    while (-not (Test-Path $done) -and ((Get-Date) - $t0).TotalSeconds -lt $Timeout) { Start-Sleep 1 }
    if (Test-Path $report) { Get-Content $report }
    if (-not (Test-Path $done)) { "FAIL timeout: no result after $Timeout s" }
    Stop-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
    Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue
    if (Test-Path $pidFile) {
        $appPid = [int](Get-Content $pidFile)
        $p = Get-Process -Id $appPid -ErrorAction SilentlyContinue
        if ($p -and $p.Path -eq $Exe) { Stop-Process -Id $appPid -Force -ErrorAction SilentlyContinue; "note: stopped the app ($appPid), still running at the end" }
    }
    if (Test-Path $done) { exit [int](Get-Content $done) } else { exit 3 }
}

# ---- inside the signed-in session ----
$src = @"
using System; using System.Collections.Generic; using System.Linq; using System.Text; using System.Threading;
using System.Diagnostics; using System.Runtime.InteropServices; using System.Windows.Automation;
public static class P {
  [StructLayout(LayoutKind.Sequential)] struct MOUSEINPUT { public int dx, dy; public uint data, flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] struct KEYBDINPUT { public ushort vk, scan; public uint flags, time; public IntPtr extra; }
  [StructLayout(LayoutKind.Explicit)] struct INPUT { [FieldOffset(0)] public uint type; [FieldOffset(8)] public MOUSEINPUT mi; [FieldOffset(8)] public KEYBDINPUT ki; }
  [DllImport("user32.dll", SetLastError = true)] static extern uint SendInput(uint n, INPUT[] i, int size);
  [DllImport("user32.dll")] static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] static extern uint MapVirtualKey(uint c, uint t);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [StructLayout(LayoutKind.Sequential)] public struct PT { public int X, Y; }
  [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(PT p);
  [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h, uint f);
  public static long TopAt(double x, double y) { var p = new PT(); p.X = (int)x; p.Y = (int)y; return GetAncestor(WindowFromPoint(p), 2).ToInt64(); }
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] static extern short VkKeyScan(char c);
  static void Send(params INPUT[] ins) { SendInput((uint)ins.Length, ins, Marshal.SizeOf(typeof(INPUT))); }
  static INPUT M(uint f, int x = 0, int y = 0) { var i = new INPUT(); i.type = 0; i.mi.flags = f; i.mi.dx = x; i.mi.dy = y; return i; }
  static INPUT K(ushort vk, bool up) { var i = new INPUT(); i.type = 1; i.ki.vk = vk; i.ki.scan = (ushort)MapVirtualKey(vk, 0); i.ki.flags = up ? 2u : 0u; return i; }
  static INPUT U(char c, bool up) { var i = new INPUT(); i.type = 1; i.ki.scan = c; i.ki.flags = 4u | (up ? 2u : 0u); return i; }
  public static void MoveTo(double x, double y) {
    int vx = GetSystemMetrics(76), vy = GetSystemMetrics(77), vw = GetSystemMetrics(78), vh = GetSystemMetrics(79);
    Send(M(0x0001 | 0x8000 | 0x4000, (int)Math.Round((x - vx) * 65535.0 / (vw - 1)), (int)Math.Round((y - vy) * 65535.0 / (vh - 1))));
  }
  public static void Click(double x, double y, bool right = false) {
    MoveTo(x, y); Thread.Sleep(80); Send(M(right ? 0x0008u : 0x0002u)); Thread.Sleep(60); Send(M(right ? 0x0010u : 0x0004u)); Thread.Sleep(120);
  }
  public static void DoubleClick(double x, double y) { MoveTo(x, y); Thread.Sleep(80); Send(M(2)); Send(M(4)); Thread.Sleep(60); Send(M(2)); Send(M(4)); Thread.Sleep(150); }
  public static void Drag(double x0, double y0, double x1, double y1) {
    MoveTo(x0, y0); Thread.Sleep(80); Send(M(2)); Thread.Sleep(100);
    for (int k = 1; k <= 24; k++) { MoveTo(x0 + (x1 - x0) * k / 24, y0 + (y1 - y0) * k / 24); Thread.Sleep(20); }
    Thread.Sleep(150); Send(M(4)); Thread.Sleep(200);
  }
  public static void Key(ushort vk) { Send(K(vk, false)); Thread.Sleep(30); Send(K(vk, true)); Thread.Sleep(80); }
  public static void Chord(ushort mod, ushort vk) { Send(K(mod, false), K(vk, false)); Thread.Sleep(40); Send(K(vk, true), K(mod, true)); Thread.Sleep(100); }
  public static void Type(string s) { foreach (var c in s) { Send(U(c, false)); Send(U(c, true)); Thread.Sleep(15); } Thread.Sleep(80); }

  // ---- UI Automation ----
  public static List<AutomationElement> Windows(int pid) {
    var c = new PropertyCondition(AutomationElement.ProcessIdProperty, pid);
    return AutomationElement.RootElement.FindAll(TreeScope.Children, c).Cast<AutomationElement>().ToList();
  }
  public static string Name(AutomationElement e) { try { return e.Current.Name ?? ""; } catch { return ""; } }
  public static string Id(AutomationElement e) { try { return e.Current.AutomationId ?? ""; } catch { return ""; } }
  public static string Kind(AutomationElement e) { try { return e.Current.ControlType.ProgrammaticName.Replace("ControlType.", ""); } catch { return ""; } }
  public static bool Enabled(AutomationElement e) { try { return e.Current.IsEnabled; } catch { return false; } }
  public static string Status(AutomationElement e) { try { return e.Current.ItemStatus ?? ""; } catch { return ""; } }
  public static string Help(AutomationElement e) { try { return e.Current.HelpText ?? ""; } catch { return ""; } }
  public static System.Windows.Rect Box(AutomationElement e) { try { return e.Current.BoundingRectangle; } catch { return System.Windows.Rect.Empty; } }
  public static List<AutomationElement> All(AutomationElement root) {
    var o = new List<AutomationElement>(); var w = TreeWalker.ControlViewWalker; var stack = new Stack<AutomationElement>();
    stack.Push(root);
    while (stack.Count > 0 && o.Count < 4000) {
      var e = stack.Pop(); o.Add(e);
      AutomationElement c = null; try { c = w.GetFirstChild(e); } catch { }
      var kids = new List<AutomationElement>();
      while (c != null) { kids.Add(c); try { c = w.GetNextSibling(c); } catch { c = null; } }
      for (int k = kids.Count - 1; k >= 0; k--) stack.Push(kids[k]);
    }
    return o;
  }
  public static List<AutomationElement> Everything(int pid) { var o = new List<AutomationElement>(); foreach (var w in Windows(pid)) o.AddRange(All(w)); return o; }
  public static AutomationElement Find(int pid, Func<AutomationElement, bool> f, int ms) {
    var t = Stopwatch.StartNew();
    do { foreach (var e in Everything(pid)) if (f(e)) return e; Thread.Sleep(250); } while (t.ElapsedMilliseconds < ms);
    return null;
  }
  public static AutomationElement ByName(int pid, string name, string kind, int ms) {
    return Find(pid, e => Name(e) == name && (kind == null || Kind(e) == kind), ms);
  }
  public static AutomationElement FindIn(AutomationElement root, Func<AutomationElement, bool> f, int ms) {
    var t = Stopwatch.StartNew();
    do { foreach (var e in All(root)) if (f(e)) return e; Thread.Sleep(250); } while (t.ElapsedMilliseconds < ms);
    return null;
  }
  public static string Value(AutomationElement e) {
    try { object p; if (e.TryGetCurrentPattern(ValuePattern.Pattern, out p)) return ((ValuePattern)p).Current.Value ?? ""; } catch { }
    return "";
  }
  public static void Center(AutomationElement e, out double x, out double y) { var b = Box(e); x = b.X + b.Width / 2; y = b.Y + b.Height / 2; }
  public static void Hover(AutomationElement e) { double x, y; Center(e, out x, out y); MoveTo(x, y); }
  public static void ClickOn(AutomationElement e, bool right = false) { var b = Box(e); Click(b.X + b.Width / 2, b.Y + b.Height / 2, right); }
  public static string Dump(int pid) {
    var sb = new StringBuilder();
    foreach (var w in Windows(pid)) foreach (var e in All(w)) {
      var b = Box(e);
      sb.AppendLine(Kind(e) + " '" + Name(e) + "' id=" + Id(e) + (Enabled(e) ? "" : " DISABLED") + " [" + (int)b.X + "," + (int)b.Y + " " + (int)b.Width + "x" + (int)b.Height + "]" +
        (Status(e) != "" ? " status=" + Status(e) : "") + (Help(e) != "" ? " help=" + Help(e) : ""));
    }
    return sb.ToString();
  }
  public static void Shot(string path) {
    var vx = GetSystemMetrics(76); var vy = GetSystemMetrics(77); var vw = GetSystemMetrics(78); var vh = GetSystemMetrics(79);
    using (var bmp = new System.Drawing.Bitmap(vw, vh)) { using (var g = System.Drawing.Graphics.FromImage(bmp)) g.CopyFromScreen(vx, vy, 0, 0, bmp.Size); bmp.Save(path, System.Drawing.Imaging.ImageFormat.Png); }
  }
}
"@
Add-Type -TypeDefinition $src -ReferencedAssemblies UIAutomationClient, UIAutomationTypes, WindowsBase, System.Drawing
[P]::SetProcessDPIAware() | Out-Null

$lines = New-Object System.Collections.Generic.List[string]
$failed = 0
function Say([string]$s) {
    $lines.Add($s)
    # the report is read while it is written (g-tail.sh copies it every few seconds): a write that meets a read
    # waits a moment and tries again, at most ten times
    for ($k = 0; $k -lt 10; $k++) {
        try { Set-Content -Path $report -Value $lines -ErrorAction Stop; return } catch { Start-Sleep -Milliseconds 100 }
    }
}
function Step([string]$name, [bool]$ok, [string]$saw) {
    if (-not $ok) { $script:failed++ }
    Say ("{0} {1} {2}: {3}" -f (Get-Date).ToString("HH:mm:ss.fff"), $(if ($ok) { "PASS" } else { "FAIL" }), $name, $saw)
    $n = $lines.Count
    try { [P]::Shot((Join-Path $Work ("{0:D2}-{1}.png" -f $n, $name))) } catch { }
}
# each step's start, with the time, so a run that never ends still names the step it was in
function Starting([string]$what) { Say ("begin {0} {1}" -f (Get-Date).ToString('HH:mm:ss.fff'), $what) }
function Finish([int]$code) { Set-Content -Path $done -Value $code; exit $code }

# a scratch data folder and a scratch engines folder with the conformance suite's probe manifest
$root = Join-Path $Work 'root'
$engines = Join-Path $Work 'engines'
Remove-Item -Recurse -Force $root, $engines -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $root, $engines, (Join-Path $Work 'tmp') | Out-Null
Copy-Item (Join-Path $checkout 'crates\editor-core\fixtures\conformance\engines\probe.toml') $engines
# the test engine (crates/ports), when it was built with the core: installed in the scratch engines folder as a
# person's engine would be, for Save Result's step (its result node saves in its own format, with an option)
$portsExe = Join-Path $checkout 'target\aarch64-pc-windows-msvc\release\plumcraft-ports-engine.exe'
if ($Exe -like '*\x64\*') { $portsExe = Join-Path $checkout 'target\x86_64-pc-windows-msvc\release\plumcraft-ports-engine.exe' }
$hasPorts = Test-Path $portsExe
if ($hasPorts) {
    $pbin = Join-Path $Work 'bin'
    New-Item -ItemType Directory -Force $pbin | Out-Null
    Copy-Item $portsExe $pbin -Force
    Set-Content -Encoding ascii (Join-Path $engines 'ports.toml') "id = `"ports`"`nname = `"Ports (test engine)`"`nmode = `"process`"`ncommand = ['$pbin\plumcraft-ports-engine.exe']`n"
}
$env:PLUMCRAFT_ENGINE_FOLDER = $engines
$env:TEMP = Join-Path $Work 'tmp'; $env:TMP = $env:TEMP
Get-ChildItem $Work -Filter '*.png' | Remove-Item -ErrorAction SilentlyContinue

Say "app: $Exe"
Say "launch: Plummer.exe --root $root"
$startWait = if ($SlowStartWaitS -gt 0) { Say "note: -SlowStartWaitS $SlowStartWaitS, the first window is given $SlowStartWaitS s, not 60"; $SlowStartWaitS * 1000 } else { 60000 }
$proc = Start-Process -FilePath $Exe -ArgumentList @('--root', "`"$root`"") -PassThru
$appPid = $proc.Id
Set-Content $pidFile $appPid

function Wait-Until([scriptblock]$f, [int]$ms) {
    $t = [Diagnostics.Stopwatch]::StartNew()
    do { $v = & $f; if ($v) { return $v }; Start-Sleep -Milliseconds 250 } while ($t.ElapsedMilliseconds -lt $ms)
    return $null
}
function Pred([scriptblock]$b) { [Func[System.Windows.Automation.AutomationElement, bool]]$b }
function In($root, [scriptblock]$b, [int]$ms = 4000) { if (-not $root) { return $null }; [P]::FindIn($root, (Pred $b), $ms) }
function Doc-Windows { @([P]::Windows($appPid) | Where-Object { [P]::Kind($_) -eq 'Window' -and [P]::Name($_) -like '* - Plummer' }) }
function Titles { (Doc-Windows | ForEach-Object { [P]::Name($_) }) -join ', ' }
function Doc-Window([string]$title, [int]$ms = 10000) { Wait-Until { Doc-Windows | Where-Object { [P]::Name($_) -like $title } | Select-Object -First 1 } $ms }
function Front($w) {
    # forward the way a person does: a click on its title bar, where nothing else covers it
    try {
        $h = [long]$w.Current.NativeWindowHandle
        if ([P]::GetForegroundWindow().ToInt64() -eq $h) { return }
        $b = [P]::Box($w)
        for ($x = $b.X + 8; $x -lt $b.X + $b.Width - 150; $x += 12) {
            $y = $b.Y + 12
            if ([P]::TopAt($x, $y) -eq $h) { [P]::Click($x, $y); break }
        }
        Start-Sleep -Milliseconds 300
        if ([P]::GetForegroundWindow().ToInt64() -ne $h) { $w.SetFocus(); [P]::SetForegroundWindow([IntPtr]$h) | Out-Null }
    } catch { }
    Start-Sleep -Milliseconds 300
}
function Menu-Item($w, [string]$top, [string]$like) {
    # a person's way: click the menu's title, then find the item to click
    Front $w
    $m = In $w { param($e) [P]::Name($e) -eq $top -and [P]::Kind($e) -eq 'MenuItem' }
    if (-not $m) { return $null }
    [P]::ClickOn($m); Start-Sleep -Milliseconds 600
    return In $w { param($e) [P]::Kind($e) -eq 'MenuItem' -and [P]::Name($e) -like $like }
}
function Menu-Names($w) {
    $top = [P]::Box($w).Y + 60
    @([P]::All($w) | Where-Object { [P]::Kind($_) -eq 'MenuItem' -and [P]::Box($_).Y -gt $top } | ForEach-Object { $n = [P]::Name($_); if (-not [P]::Enabled($_)) { "$n (disabled)" } else { $n } })
}
function Esc { [P]::Key(0x1B); Start-Sleep -Milliseconds 400 }
function Close-Button($w) { In $w { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'Close' -and [P]::Box($e).Width -gt 0 -and [P]::Box($e).Width -lt 200 } 3000 }
function Canvas-Of($w) { In $w { param($e) [P]::Name($e) -eq 'Canvas' -and [P]::Kind($e) -eq 'Pane' } }
function Nodes($w) { $c = Canvas-Of $w; if (-not $c) { return @() }; @([P]::All($c) | Where-Object { [P]::Id($_) -like 'node.*' }) }
function Node($w, [string]$name) { Nodes $w | Where-Object { [P]::Name($_) -eq $name } | Select-Object -First 1 }
function Describe-Nodes($w) { (Nodes $w | ForEach-Object { [P]::Name($_) + ' [' + [P]::Status($_) + ']' }) -join ', ' }
function Pin($node, [string]$side) { if (-not $node) { return $null }; @([P]::All($node) | Where-Object { [P]::Id($_) -like "$side.*" }) | Select-Object -First 1 }
function Status-Text($w) { $t = In $w { param($e) [P]::Kind($e) -eq 'Text' -and [P]::Name($e) -like 'engine:*' } 2000; if ($t) { [P]::Name($t) } else { '' } }
function Other-Windows { @([P]::Windows($appPid) | Where-Object { [P]::Kind($_) -eq 'Window' -and [P]::Name($_) -notlike '* - Plummer' -and [P]::Name($_) -ne '' -and [P]::Name($_) -ne 'Popup' }) }
function Add-From-Palette($w, [double]$fx, [double]$fy, [string]$query, [string]$name) {
    # the palette at the pointer: a person clicks empty canvas, presses Tab, types and presses Enter
    Front $w
    $b = [P]::Box((Canvas-Of $w))
    [P]::Click($b.X + $b.Width * $fx, $b.Y + $b.Height * $fy)
    Start-Sleep -Milliseconds 300
    [P]::Key(0x09); Start-Sleep -Milliseconds 800
    $search = In $w { param($e) [P]::Kind($e) -eq 'Edit' } 3000
    [P]::Type($query); Start-Sleep -Milliseconds 700
    # a person reads the list and clicks the one wanted
    $items = @([P]::All($w) | Where-Object { [P]::Kind($_) -eq 'ListItem' } | ForEach-Object { [P]::Name($_) })
    # the kind, not a section heading of the same name (a family's heading is a caption row before its kinds):
    # of the rows named so, the last
    Start-Sleep -Milliseconds 300
    # (a kind of an engine not started yet reads "Edge, starts the ports engine")
    $pick = @([P]::All($w) | Where-Object { [P]::Kind($_) -eq 'ListItem' -and ([P]::Name($_) -eq $name -or [P]::Name($_) -like "$name, *") }) | Select-Object -Last 1
    if (-not $pick) { $pick = In $w { param($e) [P]::Kind($e) -eq 'ListItem' -and [P]::Name($e) -like "$name*" } 3000 }
    if ($pick) { [P]::ClickOn($pick) } else { [P]::Key(0x0D) }
    $n = Wait-Until { Node $w $name } 15000
    return @{ node = $n; search = $(if ($search) { [P]::Name($search) } else { 'none' }); list = ($items | Select-Object -First 6) -join ' / '; picked = $(if ($pick) { [P]::Name($pick) } else { 'none (Enter)' }) }
}

# 22, as a function: the normal run calls it after step 21; -Only view-modes runs it alone after step 1
function Step-ViewModes($opened) {
    if (-not (Get-Command Close-Pop -ErrorAction SilentlyContinue)) {
        function Close-Pop($p) { if ($p) { $h = [long]$p.Current.NativeWindowHandle; Front $p; [P]::Chord(0x12, 0x73); $null = Wait-Until { if (-not (Other-Windows | Where-Object { [long]$_.Current.NativeWindowHandle -eq $h })) { 1 } } 5000 } }
    }
    # 22. an array's view modes and histograms (B154 slice 1): a Mandelbrot in a new graph, popped out; its
    # channel's histogram drawn; View > heat, and the picture changes (another value's handle); Log scale,
    # and the histogram says so. All through the core: run.view, viewer.array {log}.
    Starting '22. an array''s View picker and its histogram'
    $vSaw = @(); $ok22 = $false
    # File > New Graph, a second time if the first click didn't take (after step 21 the menu can open late)
    $nw = $null; $fileItems = ''
    for ($try = 0; $try -lt 2 -and -not $nw; $try++) {
        $it = Menu-Item $opened 'File' 'New*'
        if ($it) { $fileItems = [P]::Name($it); [P]::ClickOn($it) } else { $fileItems = (Menu-Names $opened) -join ' / '; Esc }
        $nw = Doc-Window 'Untitled*' 15000
    }
    if (-not $nw) { $vSaw += "File > New made no window (the menu item: $fileItems)" }
    $mb = if ($nw) { Add-From-Palette $nw 0.3 0.3 'Mandelbrot' 'Mandelbrot' }
    if ($mb.node) {
        $run = In $nw { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'Run' }
        if ($run) { [P]::ClickOn($run) }
        $null = Wait-Until { $n = Node $nw 'Mandelbrot'; if ($n -and [P]::Status($n) -like '*done*') { 1 } } 60000
        $o = Pin (Node $nw 'Mandelbrot') 'out'
        $pm = $null
        if ($o) {
            [P]::ClickOn($o, $true); Start-Sleep -Milliseconds 900
            $watch = In $nw { param($e) [P]::Kind($e) -eq 'MenuItem' -and ([P]::Name($e) -like 'Watch*' -or [P]::Name($e) -like 'Pop Out*') } 3000
            if ($watch) { [P]::ClickOn($watch) } else { Esc }
            $pm = Wait-Until { Other-Windows | Where-Object { [P]::Name($_) -like 'Mandelbrot *' } | Select-Object -First 1 } 20000
        }
        function Pic($w) { $p = In $w { param($e) [P]::Id($e) -eq 'view-picture' } 15000; if ($p) { [P]::Status($p) } else { '' } }
        function Hist($w) { $h = In $w { param($e) [P]::Id($e) -eq 'histogram' } 8000; if ($h) { [P]::Name($h) } else { '' } }
        if ($pm) {
            Front $pm
            $pic1 = Pic $pm; $hist1 = Hist $pm
            [P]::Shot((Join-Path $Work 'view-auto.png'))
            $combo = In $pm { param($e) [P]::Id($e) -eq 'view-mode' } 5000
            $modes = ''; $pic2 = ''; $hist2 = ''; $hist3 = ''
            if ($combo) {
                [P]::ClickOn($combo); Start-Sleep -Milliseconds 900
                $items = @([P]::Everything($appPid) | Where-Object { [P]::Kind($_) -eq 'ListItem' -and [P]::Name($_) -in 'auto', 'color', 'stretch', 'clip', 'heat', 'labels', 'angle' })
                $modes = ($items | ForEach-Object { [P]::Name($_) }) -join ' / '
                $heat = $items | Where-Object { [P]::Name($_) -eq 'heat' } | Select-Object -First 1
                if ($heat) { [P]::ClickOn($heat) } else { Esc }
                $pic2 = Wait-Until { $x = Pic $pm; if ($x -and $x -ne $pic1) { $x } } 60000
                Start-Sleep 1
                $hist2 = Hist $pm
                [P]::Shot((Join-Path $Work 'view-heat.png'))
                # the log toggle: the core's viewer.array {log}; the bars change, and the histogram says so
                $lg = In $pm { param($e) [P]::Id($e) -eq 'histogram-log' } 5000
                if ($lg) { [P]::ClickOn($lg) }
                $hist3 = Wait-Until { $x = Hist $pm; if ($x -like '*log scale*') { $x } } 15000
                Start-Sleep 1
                [P]::Shot((Join-Path $Work 'view-heat-log.png'))
            }
            $short = { param($h) if ($h.Length -gt 16) { $h.Substring(0, 16) + '...' } else { $h } }
            $vSaw += "pop-out: " + [P]::Name($pm) + "; its picture " + (& $short $pic1) + ", histogram: $hist1; View offers: $modes; after heat: picture " + (& $short "$pic2") + ", histogram: $hist2; after Log scale: $hist3"
            $ok22 = $pic1 -and $hist1 -like '*bins*' -and $modes -like '*heat*' -and $pic2 -and $pic2 -ne $pic1 -and $hist1 -notlike '*log scale*' -and $hist3 -like '*log scale*'
            Close-Pop $pm
        } else { $vSaw += "no pop-out of Mandelbrot's escape" }
    } else { $vSaw += "no new graph, or no Mandelbrot from the palette: [" + $mb.list + "]" }
    Step 'view-modes' $ok22 ($vSaw -join '; ')
}

function Run-Steps {
    # 1. a window with an untitled graph
    Starting '1. a window with an untitled graph'
    $w = Doc-Window 'Untitled*' $startWait
    Step 'window' ($null -ne $w -and (Doc-Windows).Count -eq 1) ("windows: " + (Titles))
    if (-not $w) { return }
    # -Only view-modes: step 22 alone, from this first window (it opens a graph of its own)
    if ($Only -eq 'view-modes') { Step-ViewModes $w; return }
    Start-Sleep 2

    # 2. File > New makes another; it is closed again by its close button
    Starting '2. File > New makes another; it is closed again by its close bu'
    $it = Menu-Item $w 'File' 'New*'
    if ($it) { [P]::ClickOn($it) }
    $two = Wait-Until { $d = Doc-Windows; if ($d.Count -ge 2) { $d } } 15000
    Step 'file-new' ($null -ne $two) ("item: " + $(if ($it) { [P]::Name($it) } else { 'none' }) + "; windows: " + (Titles))
    if ($two) {
        $other = $two | Where-Object { $_.Current.NativeWindowHandle -ne $w.Current.NativeWindowHandle } | Select-Object -First 1
        Front $other
        $close = Close-Button $other
        if ($close) { [P]::ClickOn($close) }
        $one = Wait-Until { if ((Doc-Windows).Count -eq 1) { 1 } } 10000
        Step 'close-new' ($null -ne $one) ("after its close button: " + (Titles))
    }

    # 3. the Engine menu lists the engines, the probe from the scratch manifest among them
    Starting '3. the Engine menu lists the engines, the probe from the scratc'
    $null = Menu-Item $w 'Engine' 'Reconnect*'
    Start-Sleep -Milliseconds 500
    $names = Menu-Names $w
    $probe = $names | Where-Object { $_ -like '*Conformance Probe*' }
    $textEngine = $names | Where-Object { $_ -like 'Text*' }
    Step 'engine-menu' ($probe -and $textEngine) ("items: " + ($names -join '; '))

    Esc; Esc
    # 4. Text as a separate process. Engines are picked automatically at first, and the menu offers each
    Starting '4. Text as a separate process. Engines are picked automatically'
    # engine, not its hostings: a person turns Pick Engines Automatically off in Settings, then picks one.
    $it = Menu-Item $w 'File' 'Settings*'
    if ($it) { [P]::ClickOn($it) }
    $sw = Wait-Until { Other-Windows | Where-Object { [P]::Name($_) -like '*Settings*' } | Select-Object -First 1 } 10000
    $toggled = ''
    if ($sw) {
        $nav = In $sw { param($e) [P]::Id($e) -eq 'settings.section.engines' } 5000
        if ($nav) { [P]::ClickOn($nav); Start-Sleep 1 }
        $tg = In $sw { param($e) [P]::Id($e) -eq 'setting.autoEngines' } 5000
        if ($tg) { [P]::ClickOn($tg); Start-Sleep 1; $toggled = [P]::Name($tg) }
        $c = Close-Button $sw; if ($c) { [P]::ClickOn($c) }
        Start-Sleep 1
    }
    Step 'settings-auto-off' ($toggled -ne '') ("settings window: " + $(if ($sw) { [P]::Name($sw) } else { 'none' }) + "; toggled: $toggled")
    $before = Status-Text $w
    $null = Menu-Item $w 'Engine' 'Reconnect*'
    Start-Sleep -Milliseconds 400
    $textItem = In $w { param($e) [P]::Kind($e) -eq 'MenuItem' -and [P]::Name($e) -like 'Text*' } 2000
    $chosen = ''; $subs = @()
    if ($textItem) {
        [P]::Hover($textItem); Start-Sleep -Milliseconds 300; [P]::ClickOn($textItem); Start-Sleep -Milliseconds 900
        $subs = Menu-Names $w
        $sep = In $w { param($e) [P]::Kind($e) -eq 'MenuItem' -and [P]::Enabled($e) -and [P]::Name($e) -like '*process*' -and [P]::Name($e) -notlike '*in-process*' } 3000
        if ($sep) { $chosen = [P]::Name($sep); [P]::ClickOn($sep) } else { Esc; Esc }
    } else { Esc }
    $after = Wait-Until { $s = Status-Text $w; if ($s -like '* connected*' -and $s -notlike '*not connected*' -and $s -notlike '*in-process*') { $s } } 45000
    Step 'engine-separate-process' ($null -ne $after) ("chose '$chosen' from [" + ($subs -join '; ') + "]; status before: $before; after: " + (Status-Text $w))

    # 5. Engine > Attach... : a form whose labels sit beside their own fields
    Starting '5. Engine > Attach... : a form whose labels sit beside their ow'
    $it = Menu-Item $w 'Engine' 'Attach*'
    if ($it) { [P]::ClickOn($it) }
    Start-Sleep 1
    $dialog = Wait-Until { [P]::All($w) | Where-Object { [P]::Kind($_) -eq 'Window' -and [P]::Name($_) -ne '' -and [P]::Name($_) -notlike '* - Plummer' -and [P]::Name($_) -ne 'Popup' } | Select-Object -First 1 } 6000
    $scope = if ($dialog) { $dialog } else { $w }
    Set-Content (Join-Path $Work 'attach-dump.txt') $(if ($dialog) { [P]::Dump($appPid) } else { '' })
    $edits = @([P]::All($scope) | Where-Object { [P]::Kind($_) -eq 'Edit' })
    $texts = @([P]::All($scope) | Where-Object { [P]::Kind($_) -eq 'Text' })
    $pairs = @(); $ok = $edits.Count -gt 0
    foreach ($e in $edits) {
        $eb = [P]::Box($e); $label = [P]::Name($e)
        # its label: a text of that name at the field's left edge, above the box a person types in and at
        # most a line above the field (a TextBox's header is inside its bounds, above its box)
        $lt = $texts | Where-Object { [P]::Name($_) -eq $label } | Where-Object { $b = [P]::Box($_); ($b.Y + $b.Height / 2) -lt ($eb.Y + $eb.Height / 2) -and ($b.Y + $b.Height) -gt ($eb.Y - 16) -and [math]::Abs($b.X - $eb.X) -lt 24 } | Select-Object -First 1
        if (-not $label -or -not $lt) { $ok = $false }
        $pairs += ("'{0}' field at {1},{2}, label {3}" -f $label, [int]$eb.X, [int]$eb.Y, $(if ($lt) { "at {0},{1}" -f [int][P]::Box($lt).X, [int][P]::Box($lt).Y } else { 'not beside it' }))
    }
    Step 'attach-form' $ok ("dialog: " + $(if ($dialog) { [P]::Name($dialog) } else { 'none' }) + "; " + ($pairs -join '; '))
    $cancel = In $scope { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'Cancel' } 2000
    if ($cancel) { [P]::ClickOn($cancel) } else { Esc }
    Start-Sleep 1

    # 6. Install Engine...: where the machine can't install (no uv, so no Python declared) it is disabled
    Starting '6. Install Engine...: where the machine can''t install (no uv, '
    # and says why; where it can, it opens the core's form, which is cancelled here
    $it = Menu-Item $w 'Engine' 'Install Engine*'
    $why = ''; $tip = ''; $form = ''
    $enabled = $it -and [P]::Enabled($it)
    if ($it -and -not $enabled) {
        $why = [P]::Help($it)
        [P]::Hover($it); Start-Sleep -Milliseconds 2000
        $tt = [P]::Everything($appPid) | Where-Object { [P]::Kind($_) -eq 'ToolTip' } | Select-Object -First 1
        if ($tt) { $tip = [P]::Name($tt) }
        Esc; Esc
        Step 'install-disabled' ([bool]($why -or $tip)) ("disabled; reason to automation: $why; tooltip shown: $tip")
    } elseif ($it) {
        [P]::ClickOn($it); Start-Sleep 1
        $dlg = Wait-Until { [P]::All($w) | Where-Object { [P]::Kind($_) -eq 'Window' -and [P]::Name($_) -like '*Install*' } | Select-Object -First 1 } 6000
        if ($dlg) { $form = ((@([P]::All($dlg) | Where-Object { [P]::Kind($_) -eq 'Edit' -or [P]::Kind($_) -eq 'ComboBox' -or [P]::Kind($_) -eq 'Button' }) | ForEach-Object { [P]::Kind($_) + " '" + [P]::Name($_) + "'" }) -join ', ') }
        Step 'install-form' ($null -ne $dlg) ("enabled (uv found, Python declared); form: " + $(if ($dlg) { [P]::Name($dlg) + ': ' + $form } else { 'none' }))
        $cancel = if ($dlg) { In $dlg { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'Cancel' } 2000 }
        if ($cancel) { [P]::ClickOn($cancel) } else { Esc }
        Start-Sleep 1
    } else { Esc; Step 'install' $false 'no Install Engine item' }

    # 7. a node from the palette, given a value in its field
    Starting '7. a node from the palette, given a value in its field'
    $a = Add-From-Palette $w 0.15 0.3 'Text' 'Text'
    $typed = ''
    if ($a.node) {
        $in = Pin $a.node 'in'
        $nb = [P]::Box($a.node); $pb = [P]::Box($in)
        # the field is the row's right part
        [P]::Click($nb.X + $nb.Width * 0.7, $pb.Y + $pb.Height / 2); Start-Sleep -Milliseconds 700
        [P]::Type('hello person'); [P]::Key(0x0D); Start-Sleep -Milliseconds 1000
        $typed = [P]::Status((Pin (Node $w 'Text') 'in'))
    }
    Step 'palette-add' ($null -ne $a.node -and $typed -like '*hello person*') ("palette search field: " + $a.search + "; first in its list: " + $a.list + "; nodes: " + (Describe-Nodes $w) + "; the text field reads: $typed")

    # 8. a second node, wired from the first's output to its input by a drag
    Starting '8. a second node, wired from the first''s output to its input b'
    $b = Add-From-Palette $w 0.55 0.3 'Upper' 'Upper case'
    $toStatus = ''
    if ($a.node -and $b.node) {
        $fb = [P]::Box((Pin (Node $w 'Text') 'out')); $tb = [P]::Box((Pin (Node $w 'Upper case') 'in'))
        [P]::Drag($fb.X + $fb.Width / 2, $fb.Y + $fb.Height / 2, $tb.X + $tb.Width / 2, $tb.Y + $tb.Height / 2)
        Start-Sleep 1
        $toStatus = [P]::Status((Pin (Node $w 'Upper case') 'in'))
    }
    Step 'wire' ($null -ne $b.node -and $toStatus -ne '') ("nodes: " + (Describe-Nodes $w) + "; Upper case's input now reads: $toStatus")

    # 9. Run, and the node runs to a value on the separate-process engine
    Starting '9. Run, and the node runs to a value on the separate-process en'
    $run = In $w { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'Run' }
    if ($run) { [P]::ClickOn($run) }
    $val = Wait-Until { $n = Node $w 'Upper case'; if ($n -and [P]::Status($n) -like '*HELLO PERSON*') { [P]::Status($n) } } 45000
    Step 'run' ($null -ne $val) ("Upper case reads: " + [P]::Status((Node $w 'Upper case')) + "; " + (Status-Text $w))

    # 10. pop the value out from the output pin's menu
    Starting '10. pop the value out from the output pin''s menu'
    $popTitle = ''; $popText = ''; $pop = $null
    $out = Pin (Node $w 'Upper case') 'out'
    $items = @()
    if ($out) {
        [P]::ClickOn($out, $true); Start-Sleep -Milliseconds 900
        $items = Menu-Names $w
        $watch = In $w { param($e) [P]::Kind($e) -eq 'MenuItem' -and ([P]::Name($e) -like 'Watch*' -or [P]::Name($e) -like 'Pop Out*') } 3000
        if ($watch) { [P]::ClickOn($watch) } else { Esc }
        $pop = Wait-Until { Other-Windows | Select-Object -First 1 } 15000
        if ($pop) {
            $popTitle = [P]::Name($pop)
            $t = In $pop { param($e) (([P]::Name($e) + [P]::Value($e)) -like '*HELLO PERSON*') } 8000
            if ($t) { $popText = [P]::Kind($t) + ' ' + [P]::Name($t) + [P]::Value($t) }
        }
    }
    Step 'pop-out' ($popText -like '*HELLO PERSON*') ("menu: " + ($items -join '; ') + "; window: $popTitle; it shows: $popText")
    if ($pop) { $close = Close-Button $pop; if ($close) { [P]::ClickOn($close) }; Start-Sleep 1 }

    # 11. Save As to a scratch file
    Starting '11. Save As to a scratch file'
    $file = Join-Path $Work 'saved\person check.json'
    New-Item -ItemType Directory -Force (Split-Path $file) | Out-Null
    Remove-Item $file -ErrorAction SilentlyContinue
    $it = Menu-Item $w 'File' 'Save As*'
    if ($it) { [P]::ClickOn($it) }
    $dlg = Wait-Until { @([P]::All($w)) + @(Other-Windows) | Where-Object { [P]::Kind($_) -eq 'Window' -and [P]::Name($_) -like 'Save*' } | Select-Object -First 1 } 15000
    $field = if ($dlg) { In $dlg { param($e) [P]::Kind($e) -eq 'Edit' -and [P]::Name($e) -like 'File name*' } 5000 }
    if ($field) { [P]::ClickOn($field); [P]::Chord(0x11, 0x41); [P]::Type($file); Start-Sleep -Milliseconds 300; [P]::Key(0x0D) }
    $saved = Wait-Until { if ((Test-Path $file) -and (Titles) -like '*person check*') { 1 } } 20000
    Step 'save-as' ($null -ne $saved) ("dialog: " + $(if ($dlg) { [P]::Name($dlg) } else { 'none' }) + "; file written: " + (Test-Path $file) + "; windows: " + (Titles))

    # 12. close it (after a New Graph, so the app keeps a window to open it from), then reopen it
    Starting '12. close it (after a New Graph, so the app keeps a window to op'
    $it = Menu-Item $w 'File' 'New*'
    if ($it) { [P]::ClickOn($it) }
    $blank = Doc-Window 'Untitled*' 15000
    Front $w
    # read the button where it is now (bringing the window forward may have changed its placement)
    Start-Sleep -Milliseconds 700
    $close = Close-Button $w
    $closeAt = if ($close) { [P]::Box($close).ToString() } else { 'none' }
    # a person closes the window in front with Alt+F4, or clicks its close button where it shows
    $front = [P]::GetForegroundWindow().ToInt64() -eq [long]$w.Current.NativeWindowHandle
    if ($front) { [P]::Chord(0x12, 0x73); $closeAt += " (Alt+F4, the window in front)" }
    elseif ($close) { [P]::ClickOn($close) }
    Start-Sleep -Milliseconds 1200
    [P]::Shot((Join-Path $Work 'close-after-click.png'))
    $gone = Wait-Until { if ((Titles) -notlike '*person check*') { 1 } } 10000
    $asks = if (-not $gone) { ((@([P]::All($w)) | Where-Object { [P]::Kind($_) -eq 'Window' -and [P]::Name($_) -notlike '* - Plummer' }) | ForEach-Object { [P]::Name($_) }) -join ', ' } else { '' }
    Step 'close' ($null -ne $gone -and $null -ne $blank) ("close button at $closeAt; windows after closing it: " + (Titles) + $(if ($asks) { "; showing: $asks" } else { '' }))
    $reopened = $null
    if ($blank) {
        $it = Menu-Item $blank 'File' 'Open*'
        if ($it) { [P]::ClickOn($it) }
        $dlg = Wait-Until { @([P]::All($blank)) + @(Other-Windows) | Where-Object { [P]::Kind($_) -eq 'Window' -and [P]::Name($_) -like 'Open*' } | Select-Object -First 1 } 15000
        $field = if ($dlg) { In $dlg { param($e) [P]::Kind($e) -eq 'Edit' -and [P]::Name($e) -like 'File name*' } 5000 }
        if ($field) { [P]::ClickOn($field); [P]::Chord(0x11, 0x41); [P]::Type($file); Start-Sleep -Milliseconds 300; [P]::Key(0x0D) }
        $reopened = Doc-Window '*person check*' 20000
        # a failed Open leaves its dialog up (a "File not found" box over it), and the system's picker host
        # outlives the app: dismiss both as a person would, each with a limit, so nothing is left on the desktop
        if (-not $reopened -and $dlg) {
            $ok = In $dlg { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'OK' } 2000
            if ($ok) { [P]::ClickOn($ok); Start-Sleep -Milliseconds 500 }
            $cancel = In $dlg { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'Cancel' } 2000
            if ($cancel) { [P]::ClickOn($cancel) } else { Esc }
            $null = Wait-Until { if (-not (@([P]::All($blank)) | Where-Object { [P]::Kind($_) -eq 'Window' -and [P]::Name($_) -like 'Open*' })) { 1 } } 5000
        }
    }
    $what = if ($reopened) { Start-Sleep 2; Describe-Nodes $reopened } else { '' }
    Step 'reopen' ($null -ne $gone -and $null -ne $reopened -and $what -like '*Text*' -and $what -like '*Upper case*') ("windows: " + (Titles) + "; its nodes: $what")

    # 13. exit: File > Quit, and the app ends
    Starting '13. exit: File > Quit, and the app ends'
    $any = Doc-Windows | Select-Object -First 1
    $it = Menu-Item $any 'File' 'Quit*'
    # the menu opens with an animation: click the item where it settles, as a person's eye follows it
    $at = ''
    if ($it) { $null = Wait-Until { $b1 = [P]::Box($it); Start-Sleep -Milliseconds 150; $b2 = [P]::Box($it); if ($b1.Width -gt 0 -and $b1.ToString() -eq $b2.ToString()) { 1 } } 3000; $at = [P]::Box($it).ToString(); [P]::ClickOn($it) }
    $ended = Wait-Until { if (-not (Get-Process -Id $appPid -ErrorAction SilentlyContinue)) { 1 } } 5000
    $again = ''
    if (-not $ended) {
        # still open: a person sees the menu still showing and clicks Quit again
        $it2 = In $any { param($e) [P]::Kind($e) -eq 'MenuItem' -and [P]::Name($e) -like 'Quit*' } 1000
        if ($it2) { $again = '; the menu was still open, Quit clicked again at ' + [P]::Box($it2).ToString(); [P]::ClickOn($it2) }
        $ended = Wait-Until { if (-not (Get-Process -Id $appPid -ErrorAction SilentlyContinue)) { 1 } } 15000
    }
    if (-not $ended) { [P]::Shot((Join-Path $Work 'quit-failed.png')) }
    $left = if (-not $ended) { (([P]::Everything($appPid) | Where-Object { [P]::Kind($_) -eq 'Window' -or [P]::Kind($_) -eq 'Button' -or [P]::Kind($_) -eq 'MenuItem' } | ForEach-Object { [P]::Name($_) }) -join ', ') } else { '' }
    $how = "Quit " + $(if ($it) { "clicked at $at" } else { 'not found in the File menu' }) + $again
    Step 'quit' ($null -ne $ended) $(if ($ended) { "the app ended ($how)" } else { "still running ($how); it shows: $left" })
    if (-not $ended) { return }

    # 14. the saved graph opened from the command line, as opening the file from Explorer does: its window is
    # titled by its file (B93), with its nodes; then File > Quit again
    Starting '14. the saved file opened from the command line'
    $proc2 = Start-Process -FilePath $Exe -ArgumentList @('--root', "`"$root`"", "`"$file`"") -PassThru
    $script:appPid = $proc2.Id
    Set-Content $pidFile $appPid
    $opened = Doc-Window '*Plummer' $startWait
    Start-Sleep 2
    $title = if ($opened) { [P]::Name($opened) } else { '' }
    $what = if ($opened) { Describe-Nodes $opened } else { '' }
    Step 'open-from-command-line' ($title -like 'person check*' -and $what -like '*Upper case*') ("title: $title; its nodes: $what")

    # 15. Save Result from a result node's footer (the test engine's Result: its own format first, with an
    # option): the file picker with the output's formats, the format's options in the core's form, the file read back
    if ($opened -and $hasPorts) {
        Starting '15. Save Result from the footer'
        # step 4 turned Pick Engines Automatically off (kept in the root's preferences): a person turns it
        # back on, so the palette offers the test engine's kinds (it starts that engine for them)
        $it = Menu-Item $opened 'File' 'Settings*'
        if ($it) { [P]::ClickOn($it) }
        $sw = Wait-Until { Other-Windows | Where-Object { [P]::Name($_) -like '*Settings*' } | Select-Object -First 1 } 10000
        if ($sw) {
            $nav = In $sw { param($e) [P]::Id($e) -eq 'settings.section.engines' } 5000
            if ($nav) { [P]::ClickOn($nav); Start-Sleep 1 }
            $tg = In $sw { param($e) [P]::Id($e) -eq 'setting.autoEngines' } 5000
            if ($tg -and [P]::Name($tg) -like '*Off*') { [P]::ClickOn($tg); Start-Sleep 1 }
            $c = Close-Button $sw; if ($c) { [P]::ClickOn($c) }
            Start-Sleep 1
        }
        # the test engine is an engine of the engines folder: the palette offers its kinds once it is in use,
        # so a person picks it in the Engine menu first
        $pe = Menu-Item $opened 'Engine' 'Ports*'
        if ($pe) { [P]::ClickOn($pe); Start-Sleep -Milliseconds 600 }
        # (with engines not picked automatically the item is a submenu of hostings: the process)
        $host1 = In $opened { param($e) [P]::Kind($e) -eq 'MenuItem' -and [P]::Enabled($e) -and [P]::Name($e) -like '*Process*' } 1500
        if ($host1) { [P]::ClickOn($host1) }
        $null = Wait-Until { $t = Status-Text $opened; if ($t -like '*Ports*connected*' -or $t -like '*ports*connected*') { $t } } 30000
        $ed = Add-From-Palette $opened 0.12 0.72 'Edge' 'Edge'
        $rs = Add-From-Palette $opened 0.5 0.72 'Result' 'Result'
        $saw = ''
        if ($ed.node -and $rs.node) {
            $fb = [P]::Box((Pin (Node $opened 'Edge') 'out')); $tb = [P]::Box((Pin (Node $opened 'Result') 'in'))
            [P]::Drag($fb.X + $fb.Width / 2, $fb.Y + $fb.Height / 2, $tb.X + $tb.Width / 2, $tb.Y + $tb.Height / 2)
            Start-Sleep 1
            $run = In $opened { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'Run' }
            if ($run) { [P]::ClickOn($run) }
            $btn = Wait-Until { $n = Node $opened 'Result'; if ($n) { [P]::All($n) | Where-Object { [P]::Id($_) -like 'save.*' } | Select-Object -First 1 } } 45000
            $saved = Join-Path $Work 'saved\result.quoted'
            Remove-Item $saved -ErrorAction SilentlyContinue
            if ($btn) {
                [P]::ClickOn($btn)
                $dlg = Wait-Until { @([P]::All($opened)) + @(Other-Windows) | Where-Object { [P]::Kind($_) -eq 'Window' -and [P]::Name($_) -like 'Save*' -and [P]::Name($_) -notlike 'Save as *' } | Select-Object -First 1 } 15000
                $types = ''
                if ($dlg) {
                    Start-Sleep 1
                    $types = ((@([P]::All($dlg) | Where-Object { [P]::Kind($_) -eq 'ComboBox' -and [P]::Name($_) -like 'Save as type*' }) | ForEach-Object { [P]::Value($_) }) -join ' / ')
                    [P]::Shot((Join-Path $Work 'save-picker.png'))
                    $field = In $dlg { param($e) [P]::Kind($e) -eq 'Edit' -and [P]::Name($e) -like 'File name*' } 5000
                    if ($field) { [P]::ClickOn($field); [P]::Chord(0x11, 0x41); [P]::Type($saved); Start-Sleep -Milliseconds 300; [P]::Key(0x0D) }
                }
                # the format's options: the core's form, drawn by the app
                $form = Wait-Until { [P]::All($opened) | Where-Object { [P]::Kind($_) -eq 'Window' -and [P]::Name($_) -like 'Save as *' } | Select-Object -First 1 } 15000
                $fields = ''
                if ($form) {
                    Start-Sleep 1
                    $fields = ((@([P]::All($form) | Where-Object { [P]::Kind($_) -in 'ComboBox', 'Edit' }) | ForEach-Object { [P]::Kind($_) + " '" + [P]::Name($_) + "'" }) -join ', ')
                    [P]::Shot((Join-Path $Work 'save-options.png'))
                    $ok = In $form { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'Save' } 3000
                    if ($ok) { [P]::ClickOn($ok) }
                }
                $wrote = Wait-Until { if (Test-Path $saved) { Get-Content $saved -Raw } } 20000
                $saw = "picker: " + $(if ($dlg) { [P]::Name($dlg) + " (type: $types)" } else { 'none' }) + "; options form: " + $(if ($form) { [P]::Name($form) + " ($fields)" } else { 'none' }) + "; the file reads: $wrote"
            } else { $saw = "no Save Result on the Result face; it reads: " + [P]::Status((Node $opened 'Result')) }
        } else { $saw = "palette for Edge: [" + $ed.list + "] picked " + $ed.picked + "; for Result: [" + $rs.list + "] picked " + $rs.picked + "; nodes: " + (Describe-Nodes $opened) }
        Step 'save-result' ($saw -like '*the file reads: "*') $saw

        # 16. Recipe: a value with one (the test engine's Traced, which supplies it) has the pop-out bar's
        # Recipe, and it reads "traced"; the Result's value has none, so no Recipe
        Starting '16. Recipe in a pop-out, with a recipe and without'
        function Pop-Out([string]$name) {
            # the pop-out of that node's value, a window of its own named after the node ("Traced · text")
            Front $opened
            $o = Pin (Node $opened $name) 'out'
            if (-not $o) { return $null }
            [P]::ClickOn($o, $true); Start-Sleep -Milliseconds 900
            $watch = In $opened { param($e) [P]::Kind($e) -eq 'MenuItem' -and ([P]::Name($e) -like 'Watch*' -or [P]::Name($e) -like 'Pop Out*') } 3000
            if ($watch) { [P]::ClickOn($watch) } else { Esc; return $null }
            $p = Wait-Until { Other-Windows | Where-Object { [P]::Name($_) -like "$name *" } | Select-Object -First 1 } 15000
            if ($p) { Front $p }
            return $p
        }
        function Close-Pop($p) {
            if (-not $p) { return }
            $h = [long]$p.Current.NativeWindowHandle
            Front $p; [P]::Chord(0x12, 0x73)
            $null = Wait-Until { if (-not (Other-Windows | Where-Object { [long]$_.Current.NativeWindowHandle -eq $h })) { 1 } } 5000
        }
        $tr = Add-From-Palette $opened 0.12 0.1 'Traced' 'Traced'
        $recipeSaw = ''; $ok16 = $false
        if ($tr.node) {
            $run = In $opened { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'Run' }
            if ($run) { [P]::ClickOn($run) }
            $null = Wait-Until { $n = Node $opened 'Traced'; if ($n -and [P]::Status($n) -like '*traced*') { 1 } } 30000
            $pt = Pop-Out 'Traced'
            $recipeBtn = if ($pt) { In $pt { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'Recipe' } 5000 }
            $read = ''
            if ($recipeBtn) {
                [P]::ClickOn($recipeBtn); Start-Sleep 2
                [P]::Shot((Join-Path $Work 'recipe-traced.png'))
                $t = [P]::Find($appPid, (Pred { param($e) ([P]::Name($e) + [P]::Value($e)) -like '*traced*' -and [P]::Kind($e) -ne 'Window' -and [P]::Name($e) -notlike 'Traced*' }), 5000)
                if ($t) { $read = [P]::Name($t) + [P]::Value($t) }
                Esc
            }
            Close-Pop $pt
            $pr = Pop-Out 'Result'
            $noRecipe = $false
            if ($pr) {
                Start-Sleep 2
                [P]::Shot((Join-Path $Work 'recipe-none.png'))
                $noRecipe = -not (In $pr { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'Recipe' } 2000)
                Close-Pop $pr
            }
            $ok16 = $recipeBtn -and $read -like '*traced*' -and $noRecipe
            $recipeSaw = "Traced's pop-out: " + $(if ($pt) { [P]::Name($pt) } else { 'none' }) + ", Recipe button " + $(if ($recipeBtn) { 'there' } else { 'missing' }) + ", it reads: " + ($read -replace "`r?`n", ' ') + "; Result's pop-out: " + $(if ($pr) { [P]::Name($pr) + ", Recipe button " + $(if ($noRecipe) { 'not there' } else { 'there' }) } else { 'none' })
        } else { $recipeSaw = "no Traced from the palette: [" + $tr.list + "]" }
        Step 'recipe' $ok16 $recipeSaw

        # 17. a value not shaped like a picture saved as PNG: the core asks first, with the prompt's own words on
        # its buttons; NumPy saves an .npy, and a second time "PNG anyway" saves the PNG
        Starting '17. Strip saved as PNG: the question and its labelled buttons'
        Front $opened
        $st = Add-From-Palette $opened 0.55 0.1 'Strip' 'Strip'
        $shapeSaw = @(); $ok17 = $true
        if ($st.node) {
            $run = In $opened { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'Run' }
            if ($run) { [P]::ClickOn($run) }
            foreach ($pass in @(@{ file = 'strip.png'; press = 'Save as NumPy (.npy)'; want = 'strip.npy'; magic = 'NUMPY' }, @{ file = 'strip-anyway.png'; press = 'Save as PNG anyway'; want = 'strip-anyway.png'; magic = 'PNG' })) {
                $target = Join-Path $Work ('saved\' + $pass.file)
                $wantFile = Join-Path $Work ('saved\' + $pass.want)
                Remove-Item $target, $wantFile -ErrorAction SilentlyContinue
                $btn = Wait-Until { $n = Node $opened 'Strip'; if ($n) { [P]::All($n) | Where-Object { [P]::Id($_) -like 'save.*' } | Select-Object -First 1 } } 30000
                if (-not $btn) { $ok17 = $false; $shapeSaw += "no Save Result on Strip"; break }
                Front $opened
                [P]::ClickOn($btn)
                $dlg = Wait-Until { @([P]::All($opened)) + @(Other-Windows) | Where-Object { [P]::Kind($_) -eq 'Window' -and [P]::Name($_) -eq 'Save As' } | Select-Object -First 1 } 15000
                $type = ''
                if (-not $dlg) { [P]::Shot((Join-Path $Work ("save-shape-nodialog-" + $pass.want + ".png"))) }
                if ($dlg) {
                    Start-Sleep 1
                    $type = ((@([P]::All($dlg) | Where-Object { [P]::Kind($_) -eq 'ComboBox' -and [P]::Name($_) -like 'Save as type*' }) | ForEach-Object { [P]::Value($_) }) -join ' / ')
                    $field = In $dlg { param($e) [P]::Kind($e) -eq 'Edit' -and [P]::Name($e) -like 'File name*' } 5000
                    if ($field) { [P]::ClickOn($field); [P]::Chord(0x11, 0x41); [P]::Type($target); Start-Sleep -Milliseconds 300; [P]::Key(0x0D) }
                }
                # the core's question, drawn as a dialog with the prompt's labels on its buttons
                $q = Wait-Until { [P]::All($opened) | Where-Object { [P]::Kind($_) -eq 'Window' -and [P]::Name($_) -notlike '* - Plummer' -and [P]::Name($_) -ne 'Save As' -and (@([P]::All($_) | Where-Object { [P]::Kind($_) -eq 'Button' -and [P]::Name($_) -like 'Save as*' }).Count -gt 0) } | Select-Object -First 1 } 20000
                $buttons = ''
                if (-not $q) { [P]::Shot((Join-Path $Work ("save-shape-noquestion-" + $pass.want + ".png"))) }
                if ($q) {
                    Start-Sleep 1
                    $buttons = ((@([P]::All($q) | Where-Object { [P]::Kind($_) -eq 'Button' }) | ForEach-Object { [P]::Name($_) }) -join ' / ')
                    # the question's words begin with a capital (B127)
                    $qtext = @([P]::All($q) | Where-Object { [P]::Kind($_) -eq 'Text' -and [P]::Name($_) -like '*array*' } | ForEach-Object { [P]::Name($_) }) | Select-Object -First 1
                    [P]::Shot((Join-Path $Work ("save-shape-" + $pass.want + ".png")))
                    $press = In $q { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq $pass.press } 3000
                    if ($press) { [P]::ClickOn($press) }
                }
                $bytes = Wait-Until { if (Test-Path $wantFile) { $b = [IO.File]::ReadAllBytes($wantFile); if ($b.Length -gt 8) { $b } } } 20000
                $head = if ($bytes) { [Text.Encoding]::ASCII.GetString($bytes, 0, [Math]::Min(8, $bytes.Length)) -replace '[^ -~]', '.' } else { '' }
                $good = $q -and $buttons -like '*Save as PNG anyway*' -and $buttons -like '*Save as NumPy (.npy)*' -and $buttons -like '*Cancel*' -and $head -like "*$($pass.magic)*" -and "$qtext" -cmatch '^[A-Z]'
                if (-not $good) { $ok17 = $false }
                $shapeSaw += ("{0}: picker type {1}; question: {2} (buttons {3}), its text: {8}; pressed {4}; {5} starts {6} ({7} bytes)" -f $pass.file, $type, $(if ($q) { [P]::Name($q) } else { 'none' }), $buttons, $pass.press, $pass.want, $head, $(if ($bytes) { $bytes.Length } else { 0 }), $qtext)
            }
        } else { $ok17 = $false; $shapeSaw += "no Strip from the palette: [" + $st.list + "]" }
        Step 'save-shape' $ok17 ($shapeSaw -join '; ')

        # 18. a list's items by name (contract 8.7): Items' four words wired into Result's text make Result run
        # once a word, each result named by the word it took ("text a"); its pop-out's stepper says
        # "text a · 1 of 4" and offers every name in a pick list (Edge's seed, a STRING, takes TEXT only
        # through a Convert)
        Starting '18. a list''s items by name in the pop-out''s stepper'
        Front $opened
        $it18 = Add-From-Palette $opened 0.12 0.33 'Items' 'Items'
        $listSaw = ''; $ok18 = $false
        if ($it18.node -and (Node $opened 'Result')) {
            $fb = [P]::Box((Pin (Node $opened 'Items') 'out')); $tb = [P]::Box((Pin (Node $opened 'Result') 'in'))
            [P]::Drag($fb.X + $fb.Width / 2, $fb.Y + $fb.Height / 2, $tb.X + $tb.Width / 2, $tb.Y + $tb.Height / 2)
            Start-Sleep 1
            $run = In $opened { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'Run' }
            if ($run) { [P]::ClickOn($run) }
            $null = Wait-Until { $n = Node $opened 'Result'; if ($n -and [P]::Status($n) -like '*done*') { 1 } } 30000
            Start-Sleep 1
            $pe = Pop-Out 'Result'
            $stepper = if ($pe) { In $pe { param($e) [P]::Id($e) -eq 'viewer-items' } 8000 }
            $first = if ($stepper) { [P]::Name($stepper) } else { '' }
            if (-not $stepper) {
                [P]::Shot((Join-Path $Work 'list-no-stepper.png'))
                $bar = if ($pe) { ((@([P]::All($pe) | Where-Object { [P]::Kind($_) -in 'Button', 'Text', 'SplitButton', 'ComboBox' }) | ForEach-Object { [P]::Kind($_) + " '" + [P]::Name($_) + "'" }) | Select-Object -First 14) -join ', ' } else { '' }
                $listSaw = "Result's text input: " + [P]::Status((Pin (Node $opened 'Result') 'in')) + "; Result: " + [P]::Status((Node $opened 'Result')) + "; the pop-out shows: $bar; "
            }
            $names = ''; $after = ''
            if ($stepper) {
                [P]::ClickOn($stepper); Start-Sleep 1
                $items = @([P]::Everything($appPid) | Where-Object { [P]::Kind($_) -eq 'MenuItem' -and [P]::Name($_) -like 'text *' })
                $names = ($items | ForEach-Object { [P]::Name($_) }) -join ' / '
                [P]::Shot((Join-Path $Work 'list-names.png'))
                $c = $items | Where-Object { [P]::Name($_) -eq 'text c' } | Select-Object -First 1
                if ($c) { [P]::ClickOn($c) } else { Esc }
                $after = Wait-Until { $s = In $pe { param($e) [P]::Id($e) -eq 'viewer-items' } 2000; if ($s -and [P]::Name($s) -like 'text c*') { [P]::Name($s) } } 10000
                [P]::Shot((Join-Path $Work 'list-stepped.png'))
            }
            $dot = [char]0x00B7   # the script is read as ANSI: the middle dot by its code
            $ok18 = $first -eq "text a $dot 1 of 4" -and $names -eq 'text a / text b / text c / text d' -and $after -eq "text c $dot 3 of 4"
            $listSaw += "Result's pop-out: " + $(if ($pe){ [P]::Name($pe) } else { 'none' }) + "; its stepper read: $first; its pick list: $names; after choosing text c: $after"
            Close-Pop $pe
        } else { $listSaw = "no Items from the palette: [" + $it18.list + "], or no Result" }
        Step 'list-names' $ok18 $listSaw

        # 19. a pack's own type by its short name (x.ports.tune reads "tune"), the whole id in the pin's tooltip
        Starting '19. a pack''s type by its short name, the whole id on hover'
        Front $opened
        $tu = Add-From-Palette $opened 0.55 0.33 'Tuning' 'Tuning'
        $typeSaw = ''; $ok19 = $false
        if ($tu.node) {
            $op = Pin (Node $opened 'Tuning') 'out'
            $label = if ($op) { [P]::Name($op) } else { '' }
            $tip = ''
            if ($op) {
                $ob = [P]::Box($op)
                [P]::MoveTo($ob.X + $ob.Width / 2, $ob.Y - 40); Start-Sleep -Milliseconds 300
                [P]::MoveTo($ob.X + $ob.Width / 2, $ob.Y + $ob.Height / 2)
                $t = [P]::Find($appPid, (Pred { param($e) [P]::Name($e) -like '*x.ports.tune*' }), 5000)
                if ($t) { $tip = [P]::Kind($t) + ' ' + [P]::Name($t) }
                [P]::Shot((Join-Path $Work 'pack-type-tip.png'))
                [P]::MoveTo($ob.X + $ob.Width / 2, $ob.Y - 120)
            }
            $ok19 = $label -like '*tune*' -and $label -notlike '*x.ports*' -and $tip -like '*Type: x.ports.tune*'
            $typeSaw = "Tuning's output reads: $label; its tooltip: " + ($tip -replace "`r?`n", ' ')
        } else { $typeSaw = "no Tuning from the palette: [" + $tu.list + "]" }
        Step 'pack-type' $ok19 $typeSaw

        # 20. a path row's Browse (B133): the system's dialog, started where the value is, its kinds as the
        # filter; read through UI Automation and cancelled (never Open). Then Browse while the field is being
        # edited: the field closes and the dialog comes. The value stays as typed.
        Starting '20. a path row''s Browse: the system dialog, cancelled'
        $startDir = Join-Path $Work 'browse'
        New-Item -ItemType Directory -Force $startDir | Out-Null
        $startFile = Join-Path $startDir 'start.txt'
        Set-Content -Encoding ascii $startFile 'browse from here'
        Front $opened
        $rd = Add-From-Palette $opened 0.55 0.585 'Read Text' 'Read Text File'
        $browseSaw = @(); $ok20 = $false
        function Browse-Button { $n = Node $opened 'Read Text File'; if ($n) { [P]::All($n) | Where-Object { [P]::Id($_) -like 'browse.*' } | Select-Object -First 1 } }
        function Path-Dialog { Wait-Until { @([P]::All($opened)) + @(Other-Windows) | Where-Object { [P]::Kind($_) -eq 'Window' -and [P]::Name($_) -eq 'Choose a File' } | Select-Object -First 1 } 15000 }
        function Read-Dialog($d) {
            $addr = @([P]::All($d) | Where-Object { [P]::Kind($_) -eq 'ToolBar' -and [P]::Name($_) -like 'Address*' } | ForEach-Object { [P]::Name($_) }) | Select-Object -First 1
            # the file type box shows its choice as a child's name ("Text (*.txt)")
            $types = (@([P]::All($d) | Where-Object { ([P]::Name($_) + [P]::Value($_)) -like '*(*.txt)*' } | ForEach-Object { [P]::Name($_) + [P]::Value($_) }) | Select-Object -Unique) -join ' / '
            [pscustomobject]@{ Name = [P]::Name($d); Address = "$addr"; Types = $types }
        }
        function Cancel-Dialog($d) {
            $c = In $d { param($e) [P]::Kind($e) -eq 'Button' -and [P]::Name($e) -eq 'Cancel' } 3000
            if ($c) { [P]::ClickOn($c) } else { Esc }
            $null = Wait-Until { if (-not (@([P]::All($opened)) + @(Other-Windows) | Where-Object { [P]::Kind($_) -eq 'Window' -and [P]::Name($_) -eq 'Choose a File' })) { 1 } } 8000
        }
        $b = Browse-Button
        if ($rd.node -and $b) {
            # the value first, typed into the field to the left of Browse
            $bb = [P]::Box($b)
            [P]::Click($bb.X - 40, $bb.Y + $bb.Height / 2); Start-Sleep 1
            [P]::Chord(0x11, 0x41); [P]::Type($startFile); Start-Sleep -Milliseconds 300; [P]::Key(0x0D); Start-Sleep 1
            $typed = [P]::Status((Pin (Node $opened 'Read Text File') 'in'))
            # Browse: the dialog starts in the value's folder, with Text (*.txt) as its filter
            [P]::ClickOn((Browse-Button))
            $d1 = Path-Dialog
            $r1 = if ($d1) { Start-Sleep 1; Read-Dialog $d1 }
            if ($d1) { [P]::Shot((Join-Path $Work 'browse-dialog.png')); Cancel-Dialog $d1 }
            $browseSaw += "the field reads: $typed; Browse's dialog: " + $(if ($d1) { $r1.Name + "; address: " + $r1.Address + "; filter: " + $r1.Types } else { 'none' })
            # Browse while the field is open: the core closes the field, the dialog comes (the first click after
            # the dialog may only bring the window forward, so a second if the field didn't open)
            # The canvas's automation peer hides the field editor, so the check types into it: text typed but
            # not committed must not become the value when Browse closes the field (B133: "without its text")
            Front $opened; Start-Sleep -Milliseconds 500
            $bb = [P]::Box((Browse-Button))
            [P]::Click($bb.X - 40, $bb.Y + $bb.Height / 2); Start-Sleep 1
            [P]::Chord(0x11, 0x41); [P]::Type('typed-not-set.txt'); Start-Sleep -Milliseconds 500
            [P]::Shot((Join-Path $Work 'browse-typing.png'))
            $editing = $true
            [P]::ClickOn((Browse-Button))
            $d2 = Path-Dialog
            $d2Name = if ($d2) { [P]::Name($d2) } else { '' }
            # no field editor over the node now (the dialog's own file name box is elsewhere)
            $nb = [P]::Box((Node $opened 'Read Text File'))
            $closed = -not [bool](@([P]::All($opened)) | Where-Object { $eb = [P]::Box($_); [P]::Kind($_) -eq 'Edit' -and $eb.Width -gt 0 -and ($eb.X + $eb.Width / 2) -gt $nb.X -and ($eb.X + $eb.Width / 2) -lt ($nb.X + $nb.Width) -and ($eb.Y + $eb.Height / 2) -gt $nb.Y -and ($eb.Y + $eb.Height / 2) -lt ($nb.Y + $nb.Height) })
            if ($d2) { [P]::Shot((Join-Path $Work 'browse-editing.png')); Cancel-Dialog $d2 }
            Start-Sleep 1
            $after = [P]::Status((Pin (Node $opened 'Read Text File') 'in'))
            $browseSaw += "typed into the open field: typed-not-set.txt (not committed); Browse then: " + $(if ($d2) { $d2Name } else { 'no dialog' }) + ", the field closed: $closed; the value after both cancels: $after"
            $ok20 = $d1 -and $r1.Address -like '*browse*' -and $r1.Types -like '*.txt*' -and $editing -and $d2 -and $closed -and $after -like '*start.txt*' -and $after -notlike '*typed-not-set*'
        } else { $browseSaw += "no Read Text File from the palette: [" + $rd.list + "], or no Browse on it" }
        Step 'path-browse' $ok20 ($browseSaw -join '; ')

        # 21. a value with an editor (FR1): the test engine's Curve names one for its `curve` (no widget edits it),
        # so its row reads as a link; a tap there opens the value window. Then grouped, `curve` promoted and the
        # group collapsed by its band's chevron: the group's row opens the member's value window too.
        Starting '21. a row that opens its node''s value editor, on the node and on its group'
        Front $opened
        $cv = Add-From-Palette $opened 0.82 0.1 'Curve' 'Curve'
        $edSaw = @(); $ok21 = $false
        function Editor-Window { Wait-Until { Other-Windows | Where-Object { [P]::Name($_) -like 'Curve*curve' } | Select-Object -First 1 } 15000 }
        function Close-Editor($w) { if ($w) { Close-Pop $w } }
        $link = if ($cv.node) { $n = Node $opened 'Curve'; if ($n) { [P]::All($n) | Where-Object { [P]::Id($_) -like 'editor.*' } | Select-Object -First 1 } }
        if ($link) {
            $linkName = [P]::Name($link)
            [P]::ClickOn($link)
            $w1 = Editor-Window
            $w1Name = if ($w1) { [P]::Name($w1) } else { '' }
            # the whole value, not its first line: the JSON's later lines hold 0.6
            Start-Sleep 1
            $w1Text = if ($w1) { (@([P]::All($w1) | Where-Object { [P]::Kind($_) -eq 'Edit' -and [P]::Value($_) -like '*0.6*' } | ForEach-Object { ([P]::Value($_) -replace '\s+', ' ').Trim() }) | Select-Object -First 1) } else { '' }
            if ($w1) { [P]::Shot((Join-Path $Work 'editor-node.png')) }
            Close-Editor $w1
            $edSaw += "Curve's row: a button '$linkName'; its tap opened: " + $(if ($w1) { "$w1Name, the value shown ($w1Text)" } else { 'nothing' })
            # group it, promote curve, collapse it by the band's chevron
            Front $opened
            $nb = [P]::Box((Node $opened 'Curve'))
            [P]::Click($nb.X + $nb.Width / 2, $nb.Y + 12, $true); Start-Sleep -Milliseconds 900
            $gi = In $opened { param($e) [P]::Kind($e) -eq 'MenuItem' -and [P]::Name($e) -like 'Group this node*' } 3000
            if ($gi) { [P]::ClickOn($gi); Start-Sleep 1 } else { Esc }
            $nb = [P]::Box((Node $opened 'Curve'))
            [P]::Click($nb.X + $nb.Width / 2, $nb.Y + 12, $true); Start-Sleep -Milliseconds 900
            $pi = In $opened { param($e) [P]::Kind($e) -eq 'MenuItem' -and [P]::Name($e) -like 'Promote input to*' } 3000
            $promoted = $false
            if ($pi) {
                [P]::ClickOn($pi); Start-Sleep -Milliseconds 900
                $pc = [P]::Find($appPid, (Pred { param($e) [P]::Kind($e) -eq 'MenuItem' -and [P]::Name($e) -eq 'curve' }), 3000)
                if ($pc) { [P]::ClickOn($pc); $promoted = $true; Start-Sleep 1 } else { Esc; Esc }
            } else { Esc }
            $chev = Wait-Until { [P]::All((Canvas-Of $opened)) | Where-Object { [P]::Id($_) -like 'group.*' -and [P]::Name($_) -like 'Collapse *' } | Select-Object -First 1 } 5000
            $groupName = if ($chev) { [P]::Name($chev) -replace '^Collapse ', '' } else { '' }
            if ($chev) { [P]::ClickOn($chev); Start-Sleep 1 }
            # the collapsed group's face, and its promoted row that opens the member's editor
            # its face is titled "Group: <name>"
            $gface = if ($groupName) { Wait-Until { Nodes $opened | Where-Object { [P]::Name($_) -like "*$groupName" } | Select-Object -First 1 } 8000 }
            $glink = if ($gface) { [P]::All($gface) | Where-Object { [P]::Id($_) -like 'editor.*' } | Select-Object -First 1 }
            $w2Name = ''
            if ($glink) {
                [P]::ClickOn($glink)
                $w2 = Editor-Window
                if ($w2) { $w2Name = [P]::Name($w2); [P]::Shot((Join-Path $Work 'editor-group.png')) }
                Close-Editor $w2
            } else { [P]::Shot((Join-Path $Work 'editor-group-missing.png')) }
            $edSaw += "grouped: " + [bool]$gi + ", curve promoted: $promoted, collapsed by its chevron: " + $(if ($chev) { "'$groupName'" } else { 'no chevron' }) + "; the group's face: " + $(if ($gface) { [P]::Name($gface) + ", its row: " + $(if ($glink) { "'" + [P]::Name($glink) + "', its tap opened: " + $(if ($w2Name) { $w2Name } else { 'nothing' }) } else { 'no editor row' }) } else { 'none' })
            $ok21 = $w1Name -like 'Curve*curve' -and "$w1Text" -like '*0.6*' -and $w2Name -like 'Curve*curve' -and $promoted
        } else { $edSaw += "no Curve, or no editor row on it: [" + $cv.list + "]" }
        Step 'editor-row' $ok21 ($edSaw -join '; ')

        Step-ViewModes $opened
    }
    if ($opened) {
        $it = Menu-Item $opened 'File' 'Quit*'
        if ($it) { [P]::ClickOn($it) }
        $null = Wait-Until { if (-not (Get-Process -Id $appPid -ErrorAction SilentlyContinue)) { 1 } } 20000
    }
}

function Run-All { if ($Discover) { Discover } else { Run-Steps } }

try { Run-All } catch { Step 'script' $false ("stopped: " + $_.Exception.Message) }
$left = Get-Process -Id $appPid -ErrorAction SilentlyContinue
if ($left) { Stop-Process -Id $appPid -Force -ErrorAction SilentlyContinue; Say "note: the app was still running at the end and was stopped" }
Say ("{0}: {1} failed" -f $(if ($failed) { 'FAIL' } else { 'PASS' }), $failed)
Finish $(if ($failed) { 1 } else { 0 })
