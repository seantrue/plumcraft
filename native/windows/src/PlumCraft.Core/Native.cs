// The C ABI of plumcraft-ffi (crates/ffi/include/plumcraft.h, PLUM_ABI_VERSION 1), as P/Invoke.
//
// Every layout here mirrors the header field for field; PlumCraft.Probe checks the sizes at start
// (Marshal.SizeOf) against what the header's natural alignment gives on a 64-bit target. This file
// is the only one in the Windows shell that uses pointers (`unsafe`): everything above it sees
// spans, strings and records.

using System.Runtime.InteropServices;

namespace PlumCraft.Core;

[StructLayout(LayoutKind.Sequential)]
public struct PlumBuf
{
    public IntPtr Ptr;
    public nuint Len;
    public nuint Cap;
}

[StructLayout(LayoutKind.Sequential)]
public struct PlumBytes
{
    public IntPtr Ptr;
    public nuint Len;
    public ulong Token;
    public int Status;
}

/// <summary>plum_hit: what is at a point, or where a dragged wire may land.</summary>
[StructLayout(LayoutKind.Sequential)]
public struct PlumHit
{
    public int Status;
    public uint Kind;
    public uint Part;
    public int Index;
    public ulong Item;
    public ulong Node;
    public ulong Wire;
    public uint Pin;
    public uint Fits;
    public float X, Y;
}

/// <summary>plum_face: one face's measurements, graph units at zoom 1.</summary>
[StructLayout(LayoutKind.Sequential)]
public struct PlumFace
{
    public ulong Face;
    public ulong Rev;
    public float W, H;
    public float Header;
    public IntPtr Inputs;
    public nuint NInputs;
    public IntPtr Outputs;
    public nuint NOutputs;
    public IntPtr Rows;
    public nuint NRows;
}

/// <summary>plum_drag: where a drag under way is.</summary>
[StructLayout(LayoutKind.Sequential)]
public struct PlumDrag
{
    public int Status;
    public uint Kind;
    public ulong Serial;
    public float Dx, Dy;
    public float R0, R1, R2, R3;
    public PlumHit Target;
}

public static class PlumStatus
{
    public const int Ok = 0, ErrCore = -1, ErrArg = -2, ErrMissing = -3, ErrPanic = -4, ErrUnreadable = -5, Pending = 1;

    public static string Word(int s) => s switch
    {
        Ok => "ok",
        ErrCore => "no such core",
        ErrArg => "a bad argument",
        ErrMissing => "missing",
        ErrPanic => "a panic caught at the boundary",
        ErrUnreadable => "unreadable",
        Pending => "pending",
        _ => $"status {s}",
    };
}

public static unsafe partial class Native
{
    public const string Lib = "plumcraft_ffi";
    public const uint AbiVersion = 1;

    [LibraryImport(Lib, EntryPoint = "plum_abi_version")]
    public static partial uint plum_abi_version();

    [LibraryImport(Lib, EntryPoint = "plum_open")]
    public static partial ulong plum_open(byte* config, nuint len, delegate* unmanaged[Cdecl]<IntPtr, void> wake, IntPtr ctx);

    [LibraryImport(Lib, EntryPoint = "plum_last_error")]
    public static partial PlumBuf plum_last_error();

    [LibraryImport(Lib, EntryPoint = "plum_close")]
    public static partial int plum_close(ulong core);

    [LibraryImport(Lib, EntryPoint = "plum_request")]
    public static partial ulong plum_request(ulong core, byte* json, nuint len);

    [LibraryImport(Lib, EntryPoint = "plum_poll")]
    public static partial PlumBuf plum_poll(ulong core);

    [LibraryImport(Lib, EntryPoint = "plum_free")]
    public static partial void plum_free(PlumBuf buf);

    [LibraryImport(Lib, EntryPoint = "plum_blob")]
    public static partial PlumBytes plum_blob(ulong core, byte* handle, nuint len);

    [LibraryImport(Lib, EntryPoint = "plum_release")]
    public static partial int plum_release(ulong token);

    [LibraryImport(Lib, EntryPoint = "plum_leases")]
    public static partial ulong plum_leases();

    [LibraryImport(Lib, EntryPoint = "plum_hit_test")]
    public static partial PlumHit plum_hit_test(ulong core, ulong doc, float x, float y, float slop);

    [LibraryImport(Lib, EntryPoint = "plum_wire_target")]
    public static partial PlumHit plum_wire_target(ulong core, ulong doc, ulong face, uint side, uint pin, float x, float y, float slop);

    [LibraryImport(Lib, EntryPoint = "plum_wire_paths")]
    public static partial nuint plum_wire_paths(ulong core, ulong doc, ulong* faces, nuint n, float dx, float dy, float* outp, nuint cap);

    [LibraryImport(Lib, EntryPoint = "plum_geometry_set")]
    public static partial int plum_geometry_set(ulong core, ulong doc, PlumFace* faces, nuint n);

    [LibraryImport(Lib, EntryPoint = "plum_gesture_move")]
    public static partial PlumDrag plum_gesture_move(ulong core, ulong doc, float x, float y, float slop);

    /// <summary>A library buffer as bytes (copied once into managed memory), freed at once.</summary>
    public static byte[] Take(PlumBuf b)
    {
        if (b.Ptr == IntPtr.Zero || b.Len == 0) return [];
        var bytes = new ReadOnlySpan<byte>((void*)b.Ptr, checked((int)b.Len)).ToArray();
        plum_free(b);
        return bytes;
    }

    /// <summary>Lent bytes as a span, valid until the lease is released.</summary>
    public static ReadOnlySpan<byte> Span(IntPtr p, nuint len) => p == IntPtr.Zero ? default : new ReadOnlySpan<byte>((void*)p, checked((int)len));
}
