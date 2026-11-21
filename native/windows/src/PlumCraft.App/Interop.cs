// What the app asks Windows itself (the app's only `unsafe` code: the DXGI vtable calls): the
// display's refresh rate (EnumDisplaySettings, for the core's frame_ms), DWM's frame counters, the DXGI
// adapters (WARP, a software rasteriser, or a real GPU: the budgets' caveat), and the double-click time.
// From the N5 spike.

using System.Runtime.InteropServices;

namespace PlumCraft.App;

[StructLayout(LayoutKind.Sequential, Pack = 1)]
public struct DwmTimingInfo
{
    public uint cbSize;
    public uint rateRefreshNum, rateRefreshDen;
    public ulong qpcRefreshPeriod;
    public uint rateComposeNum, rateComposeDen;
    public ulong qpcVBlank, cRefresh;
    public uint cDXRefresh;
    public ulong qpcCompose, cFrame;
    public uint cDXPresent;
    public ulong cRefreshFrame, cFrameSubmitted;
    public uint cDXPresentSubmitted;
    public ulong cFrameConfirmed;
    public uint cDXPresentConfirmed;
    public ulong cRefreshConfirmed;
    public uint cDXRefreshConfirmed;
    public ulong cFramesLate;
    public uint cFramesOutstanding;
    public ulong cFrameDisplayed, qpcFrameDisplayed, cRefreshFrameDisplayed, cFrameComplete, qpcFrameComplete, cFramePending, qpcFramePending;
    public ulong cFramesDisplayed, cFramesComplete, cFramesPending, cFramesAvailable, cFramesDropped, cFramesMissed;
    public ulong cRefreshNextDisplayed, cRefreshNextPresented, cRefreshesDisplayed, cRefreshesPresented, cRefreshStarted;
    public ulong cPixelsReceived, cPixelsDrawn, cBuffersEmpty;
}

[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct DevMode
{
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmDeviceName;
    public ushort dmSpecVersion, dmDriverVersion, dmSize, dmDriverExtra;
    public uint dmFields;
    public int dmPositionX, dmPositionY;
    public uint dmDisplayOrientation, dmDisplayFixedOutput;
    public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string dmFormName;
    public ushort dmLogPixels;
    public uint dmBitsPerPel, dmPelsWidth, dmPelsHeight, dmDisplayFlags, dmDisplayFrequency;
    public uint dmICMMethod, dmICMIntent, dmMediaType, dmDitherType, dmReserved1, dmReserved2, dmPanningWidth, dmPanningHeight;
}

public static unsafe class Interop
{
    [DllImport("dwmapi.dll")]
    static extern int DwmGetCompositionTimingInfo(IntPtr hwnd, ref DwmTimingInfo info);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern bool EnumDisplaySettingsW(string? device, int mode, ref DevMode dm);

    [DllImport("dxgi.dll")]
    static extern int CreateDXGIFactory1(in Guid riid, out IntPtr factory);

    [DllImport("user32.dll")]
    static extern uint GetDpiForWindow(IntPtr hwnd);

    public static uint Dpi(IntPtr hwnd) => GetDpiForWindow(hwnd);

    [DllImport("user32.dll")]
    static extern uint GetDoubleClickTime();

    public static uint DoubleClickMs => GetDoubleClickTime();

    public static (int hr, DwmTimingInfo info) DwmTiming()
    {
        var t = new DwmTimingInfo { cbSize = (uint)Marshal.SizeOf<DwmTimingInfo>() };
        var hr = DwmGetCompositionTimingInfo(IntPtr.Zero, ref t);
        return (hr, t);
    }

    public static (uint hz, uint w, uint h) Display()
    {
        var dm = new DevMode { dmDeviceName = "", dmFormName = "", dmSize = (ushort)Marshal.SizeOf<DevMode>() };
        return EnumDisplaySettingsW(null, -1, ref dm) ? (dm.dmDisplayFrequency, dm.dmPelsWidth, dm.dmPelsHeight) : (0, 0, 0);
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct AdapterDesc1
    {
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string Description;
        public uint VendorId, DeviceId, SubSysId, Revision;
        public nuint DedicatedVideoMemory, DedicatedSystemMemory, SharedSystemMemory;
        public uint LuidLow;
        public int LuidHigh;
        public uint Flags;
    }

    /// <summary>Every DXGI adapter: name, vendor and device ids, video memory, and the software flag.</summary>
    public static List<string> Adapters()
    {
        var outp = new List<string>();
        var iid = new Guid("770aae78-f26f-4dba-a829-253c83d1b387"); // IDXGIFactory1
        if (CreateDXGIFactory1(iid, out var factory) < 0 || factory == IntPtr.Zero) return ["CreateDXGIFactory1 failed"];
        try
        {
            var fvt = *(IntPtr**)factory;
            var enumAdapters1 = (delegate* unmanaged[Stdcall]<IntPtr, uint, IntPtr*, int>)fvt[12];
            for (uint i = 0; i < 16; i++)
            {
                IntPtr adapter;
                if (enumAdapters1(factory, i, &adapter) < 0) break;
                var avt = *(IntPtr**)adapter;
                var getDesc1 = (delegate* unmanaged[Stdcall]<IntPtr, IntPtr, int>)avt[10];
                var buf = Marshal.AllocHGlobal(Marshal.SizeOf<AdapterDesc1>());
                try
                {
                    if (getDesc1(adapter, buf) >= 0)
                    {
                        var d = Marshal.PtrToStructure<AdapterDesc1>(buf);
                        outp.Add($"{d.Description} | vendor 0x{d.VendorId:x4} device 0x{d.DeviceId:x4} | video {d.DedicatedVideoMemory / (1024 * 1024)} MB, shared {d.SharedSystemMemory / (1024 * 1024)} MB | {((d.Flags & 2) != 0 ? "software" : "hardware")}");
                    }
                }
                finally
                {
                    Marshal.FreeHGlobal(buf);
                    var release = (delegate* unmanaged[Stdcall]<IntPtr, uint>)avt[2];
                    release(adapter);
                }
            }
        }
        finally
        {
            var release = (delegate* unmanaged[Stdcall]<IntPtr, uint>)(*(IntPtr**)factory)[2];
            release(factory);
        }
        return outp;
    }
}
