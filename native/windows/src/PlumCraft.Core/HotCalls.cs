// The typed hot calls' results read in words (contract section 9; plumcraft.h's "hot calls"): what a
// canvas asks during a gesture, synchronously, with no JSON. The structs are Native.cs's, laid out as
// the header's; these are their readings (the Swift kit's HotHit and HotDrag).

namespace PlumCraft.Core;

/// <summary>plum_hit's kind, by plumcraft.h's numbers.</summary>
public enum HitKind : uint { None = 0, Node = 1, Pin = 2, Wire = 3, Bend = 4, Tag = 5, Frame = 6, Group = 7 }

/// <summary>A face's part (kind node), by plumcraft.h's numbers.</summary>
public enum NodePart : uint { Body = 1, Header, Title, Chevron, Icon, Menu, Enter, Row, Preview, Footer }

/// <summary>plum_drag's kind: what the drag's begin decided it is.</summary>
public enum DragKind : uint { Move = 1, Wire = 2, Marquee = 3, Bend = 4, Frame = 5, Resize = 6, Group = 7, Scrub = 8 }

public partial struct PlumHit
{
    public readonly bool Ok => Status == PlumStatus.Ok;

    /// <summary>The kind, or null for a number this kit doesn't know.</summary>
    public readonly HitKind? KindOf => Kind <= 7 ? (HitKind)Kind : null;

    /// <summary>A face's part, when a face was hit.</summary>
    public readonly NodePart? PartOf => Kind == 1 && Part is >= 1 and <= 10 ? (NodePart)Part : null;

    /// <summary>A pin's side: true for an output.</summary>
    public readonly bool IsOutput => Kind == 2 && Part == 2;

    /// <summary>The hit's kind as the JSON forms name it (`none`, `node`, `pin`, ...; `?` for one unknown).</summary>
    public readonly string KindName => Kind switch
    {
        0 => "none", 1 => "node", 2 => "pin", 3 => "wire", 4 => "bend", 5 => "tag", 6 => "frame", 7 => "group", _ => "?",
    };
}

public partial struct PlumDrag
{
    public readonly bool Ok => Status == PlumStatus.Ok;

    public readonly DragKind? KindOf => Kind is >= 1 and <= 8 ? (DragKind)Kind : null;

    /// <summary>A scrub's value now: a double whose low and high 32 bits are R0 and R1, bit-cast (9.6).</summary>
    public readonly double? ScrubValue => Kind == 8
        ? BitConverter.UInt64BitsToDouble(BitConverter.SingleToUInt32Bits(R0) | ((ulong)BitConverter.SingleToUInt32Bits(R1) << 32))
        : null;
}
