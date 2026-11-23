// PlumCraft for Windows: the system's file dialogs for a path row's Browse (B133; contract 11.2, purpose
// `path` and `files`). The common item dialog through COM, because the WinRT pickers can only start in a known
// library: a path's dialog starts where its current value is, as the Mac's panel does. Real paths come back
// (no copy, unlike iPadOS). Built-in COM interop: no unsafe code.

using System.Runtime.InteropServices;
using PlumCraft.Models;

namespace PlumCraft.App;

public static class PathDialogs
{
    /// <summary>What a Browse asks for: one file to read, several, one to write, or a folder.</summary>
    public enum Pick { Open, Many, Save, Folder }

    /// <summary>Show the dialog over `owner`; the chosen paths, or null when cancelled.</summary>
    public static List<string>? Show(nint owner, Pick pick, string title, IReadOnlyList<PathBrowse.Kind> kinds, string? startFolder, string? name)
    {
        IFileDialog d = pick == Pick.Save ? (IFileDialog)new FileSaveDialogCo() : (IFileDialog)new FileOpenDialogCo();
        try
        {
            d.GetOptions(out var o);
            o |= Fos.ForceFileSystem | Fos.NoChangeDir;
            o |= pick switch
            {
                Pick.Folder => Fos.PickFolders | Fos.PathMustExist,
                Pick.Many => Fos.AllowMultiSelect | Fos.FileMustExist,
                Pick.Open => Fos.FileMustExist,
                _ => Fos.OverwritePrompt,
            };
            d.SetOptions(o);
            d.SetTitle(title);
            if (pick != Pick.Folder)
            {
                var specs = Specs(kinds);
                d.SetFileTypes((uint)specs.Length, specs);
                d.SetFileTypeIndex(1);
                if (pick == Pick.Save && kinds.SelectMany(k => k.Extensions).FirstOrDefault(e => e != "*") is { } ext) d.SetDefaultExtension(ext.TrimStart('.'));
            }
            if (!string.IsNullOrEmpty(name) && pick is Pick.Save or Pick.Open) d.SetFileName(name);
            if (startFolder is { Length: > 0 } && Directory.Exists(startFolder) && SHCreateItemFromParsingName(startFolder, 0, typeof(IShellItem).GUID, out var folder) == 0)
            {
                d.SetFolder(folder);
                Marshal.ReleaseComObject(folder);
            }
            var hr = d.Show(owner);
            if (hr == Cancelled) return null;
            Marshal.ThrowExceptionForHR(hr);
            var paths = new List<string>();
            if (pick == Pick.Many && d is IFileOpenDialog od)
            {
                od.GetResults(out var items);
                items.GetCount(out var n);
                for (uint i = 0; i < n; i++)
                {
                    items.GetItemAt(i, out var it);
                    if (PathOf(it) is { } p) paths.Add(p);
                    Marshal.ReleaseComObject(it);
                }
                Marshal.ReleaseComObject(items);
            }
            else
            {
                d.GetResult(out var it);
                if (PathOf(it) is { } p) paths.Add(p);
                Marshal.ReleaseComObject(it);
            }
            return paths.Count == 0 ? null : paths;
        }
        finally { Marshal.ReleaseComObject(d); }
    }

    static ComdlgFilterspec[] Specs(IReadOnlyList<PathBrowse.Kind> kinds) =>
        [.. PathBrowse.Filters(kinds).Select(f => new ComdlgFilterspec { Name = f.Name, Spec = f.Spec })];

    static string? PathOf(IShellItem it) => it.GetDisplayName(Sigdn.FileSysPath, out var p) == 0 ? p : null;

    const int Cancelled = unchecked((int)0x800704C7);

    [Flags]
    enum Fos : uint
    {
        OverwritePrompt = 0x2, NoChangeDir = 0x8, PickFolders = 0x20, ForceFileSystem = 0x40,
        AllowMultiSelect = 0x200, PathMustExist = 0x800, FileMustExist = 0x1000,
    }

    enum Sigdn : uint { FileSysPath = 0x80058000 }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct ComdlgFilterspec
    {
        [MarshalAs(UnmanagedType.LPWStr)] public string Name;
        [MarshalAs(UnmanagedType.LPWStr)] public string Spec;
    }

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, PreserveSig = true)]
    static extern int SHCreateItemFromParsingName(string path, nint bindCtx, [MarshalAs(UnmanagedType.LPStruct)] Guid riid, out IShellItem item);

    [ComImport, Guid("DC1C5A9C-E88A-4dde-A5A1-60F82A20AEF7")]
    class FileOpenDialogCo { }

    [ComImport, Guid("C0B4E2F3-BA21-4773-8DBA-335EC946EB8B")]
    class FileSaveDialogCo { }

    [ComImport, Guid("43826D1E-E718-42EE-BC55-A1E261C37BFE"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IShellItem
    {
        void BindToHandler(nint pbc, ref Guid bhid, ref Guid riid, out nint ppv);
        void GetParent(out IShellItem ppsi);
        [PreserveSig] int GetDisplayName(Sigdn sigdnName, [MarshalAs(UnmanagedType.LPWStr)] out string ppszName);
        void GetAttributes(uint sfgaoMask, out uint psfgaoAttribs);
        void Compare(IShellItem psi, uint hint, out int piOrder);
    }

    [ComImport, Guid("B63EA76D-1F85-456F-A19C-48159EFA858B"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IShellItemArray
    {
        void BindToHandler(nint pbc, ref Guid bhid, ref Guid riid, out nint ppvOut);
        void GetPropertyStore(int flags, ref Guid riid, out nint ppv);
        void GetPropertyDescriptionList(nint keyType, ref Guid riid, out nint ppv);
        void GetAttributes(int attribFlags, uint sfgaoMask, out uint psfgaoAttribs);
        void GetCount(out uint pdwNumItems);
        void GetItemAt(uint dwIndex, out IShellItem ppsi);
        void EnumItems(out nint ppenumShellItems);
    }

    [ComImport, Guid("42f85136-db7e-439c-85f1-e4075d135fc8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IFileDialog
    {
        [PreserveSig] int Show(nint parent);
        void SetFileTypes(uint cFileTypes, [MarshalAs(UnmanagedType.LPArray)] ComdlgFilterspec[] rgFilterSpec);
        void SetFileTypeIndex(uint iFileType);
        void GetFileTypeIndex(out uint piFileType);
        void Advise(nint pfde, out uint pdwCookie);
        void Unadvise(uint dwCookie);
        void SetOptions(Fos fos);
        void GetOptions(out Fos pfos);
        void SetDefaultFolder(IShellItem psi);
        void SetFolder(IShellItem psi);
        void GetFolder(out IShellItem ppsi);
        void GetCurrentSelection(out IShellItem ppsi);
        void SetFileName([MarshalAs(UnmanagedType.LPWStr)] string pszName);
        void GetFileName([MarshalAs(UnmanagedType.LPWStr)] out string pszName);
        void SetTitle([MarshalAs(UnmanagedType.LPWStr)] string pszTitle);
        void SetOkButtonLabel([MarshalAs(UnmanagedType.LPWStr)] string pszText);
        void SetFileNameLabel([MarshalAs(UnmanagedType.LPWStr)] string pszLabel);
        void GetResult(out IShellItem ppsi);
        void AddPlace(IShellItem psi, int fdap);
        void SetDefaultExtension([MarshalAs(UnmanagedType.LPWStr)] string pszDefaultExtension);
        void Close(int hr);
        void SetClientGuid(ref Guid guid);
        void ClearClientData();
        void SetFilter(nint pFilter);
    }

    [ComImport, Guid("d57c7288-d4ad-4768-be02-9d969532d960"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface IFileOpenDialog : IFileDialog
    {
        [PreserveSig] new int Show(nint parent);
        new void SetFileTypes(uint cFileTypes, [MarshalAs(UnmanagedType.LPArray)] ComdlgFilterspec[] rgFilterSpec);
        new void SetFileTypeIndex(uint iFileType);
        new void GetFileTypeIndex(out uint piFileType);
        new void Advise(nint pfde, out uint pdwCookie);
        new void Unadvise(uint dwCookie);
        new void SetOptions(Fos fos);
        new void GetOptions(out Fos pfos);
        new void SetDefaultFolder(IShellItem psi);
        new void SetFolder(IShellItem psi);
        new void GetFolder(out IShellItem ppsi);
        new void GetCurrentSelection(out IShellItem ppsi);
        new void SetFileName([MarshalAs(UnmanagedType.LPWStr)] string pszName);
        new void GetFileName([MarshalAs(UnmanagedType.LPWStr)] out string pszName);
        new void SetTitle([MarshalAs(UnmanagedType.LPWStr)] string pszTitle);
        new void SetOkButtonLabel([MarshalAs(UnmanagedType.LPWStr)] string pszText);
        new void SetFileNameLabel([MarshalAs(UnmanagedType.LPWStr)] string pszLabel);
        new void GetResult(out IShellItem ppsi);
        new void AddPlace(IShellItem psi, int fdap);
        new void SetDefaultExtension([MarshalAs(UnmanagedType.LPWStr)] string pszDefaultExtension);
        new void Close(int hr);
        new void SetClientGuid(ref Guid guid);
        new void ClearClientData();
        new void SetFilter(nint pFilter);
        void GetResults(out IShellItemArray ppenum);
        void GetSelectedItems(out IShellItemArray ppsai);
    }
}
