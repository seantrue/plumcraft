// The canvas's UI Automation peer (N5 gate): the canvas as a pane whose children are its faces, each a
// list item named by its title with its pins as children, placed on the screen where they are drawn.
// The parts are read when UI Automation asks (CompositionCanvas.AccessibleParts), and a part keeps its
// peer while it stays on the canvas, so a client holding one finds it again.

using Microsoft.UI.Xaml.Automation.Peers;
using WRect = Windows.Foundation.Rect;

namespace PlumCraft.App.Canvas;

public sealed class CanvasPeer(CanvasView owner) : FrameworkElementAutomationPeer(owner)
{
    readonly Dictionary<string, PartPeer> peers = new(StringComparer.Ordinal);

    protected override string GetClassNameCore() => "PlumCraftCanvas";
    protected override string GetNameCore() => "Canvas";
    protected override AutomationControlType GetAutomationControlTypeCore() => AutomationControlType.Pane;

    protected override IList<AutomationPeer> GetChildrenCore() => [.. Parts(null)];

    internal List<PartPeer> Parts(PartPeer? parent)
    {
        List<CanvasPart> parts;
        try { parts = owner.Canvas.AccessibleParts(); }
        catch (InvalidOperationException) { return []; }
        var seen = new HashSet<string>(StringComparer.Ordinal);
        var top = new List<PartPeer>();
        foreach (var p in parts)
        {
            var peer = Peer(p, null);
            seen.Add(p.Id);
            peer.Kids.Clear();
            foreach (var c in p.Children) { peer.Kids.Add(Peer(c, peer)); seen.Add(c.Id); }
            top.Add(peer);
        }
        foreach (var gone in peers.Keys.Where(k => !seen.Contains(k)).ToList()) peers.Remove(gone);
        return parent is null ? top : parent.Kids;
    }

    PartPeer Peer(CanvasPart p, PartPeer? parent)
    {
        if (!peers.TryGetValue(p.Id, out var peer)) peers[p.Id] = peer = new PartPeer(this);
        peer.Part = p;
        peer.Up = parent;
        return peer;
    }

    /// <summary>A view rectangle on the screen, in the canvas peer's own coordinates (physical pixels).</summary>
    internal WRect Screen(WRect view)
    {
        var o = GetBoundingRectangle();
        var s = owner.XamlRoot?.RasterizationScale ?? 1;
        return new WRect(o.X + view.X * s, o.Y + view.Y * s, view.Width * s, view.Height * s);
    }

    internal AutomationPeer Self => this;
}

public sealed class PartPeer(CanvasPeer canvas) : AutomationPeer
{
    internal CanvasPart Part = new("", "", "", default, "", []);
    internal PartPeer? Up;
    internal readonly List<PartPeer> Kids = [];

    protected override string GetNameCore() => Part.Name;
    protected override string GetAutomationIdCore() => Part.Id;
    protected override string GetClassNameCore() => Part.Role == "node" ? "PlumCraftFace" : "PlumCraftPin";
    protected override string GetItemStatusCore() => Part.Status;
    protected override string GetItemTypeCore() => Part.Role;
    protected override string GetLocalizedControlTypeCore() => Part.Role == "node" ? "node" : Part.Role + " pin";
    protected override AutomationControlType GetAutomationControlTypeCore() => Part.Role == "node" ? AutomationControlType.ListItem : AutomationControlType.Custom;
    protected override bool IsContentElementCore() => true;
    protected override bool IsControlElementCore() => true;
    protected override bool IsEnabledCore() => true;
    protected override bool IsOffscreenCore() => false;
    protected override WRect GetBoundingRectangleCore() => canvas.Screen(Part.View);
    protected override IList<AutomationPeer> GetChildrenCore() => [.. Kids];

    protected override object? NavigateCore(AutomationNavigationDirection direction)
    {
        var siblings = Up is { } u ? u.Kids : canvas.Parts(null);
        var i = siblings.IndexOf(this);
        return direction switch
        {
            AutomationNavigationDirection.Parent => Up is { } p ? p : canvas.Self,
            AutomationNavigationDirection.FirstChild => Kids.FirstOrDefault(),
            AutomationNavigationDirection.LastChild => Kids.LastOrDefault(),
            AutomationNavigationDirection.NextSibling => i >= 0 && i + 1 < siblings.Count ? siblings[i + 1] : null,
            AutomationNavigationDirection.PreviousSibling => i > 0 ? siblings[i - 1] : null,
            _ => null,
        };
    }
}
