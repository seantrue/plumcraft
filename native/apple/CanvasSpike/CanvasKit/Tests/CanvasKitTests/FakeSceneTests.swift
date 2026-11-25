import CanvasKit
import Testing

@Test func sceneIsDeterministicAndFull() {
    let a = FakeScene.make(nodes: 500, wires: 800)
    let b = FakeScene.make(nodes: 500, wires: 800)
    #expect(a.nodes.count == 500)
    #expect(a.wires.count == 800)
    #expect(a.nodes.map(\.title) == b.nodes.map(\.title))
    #expect(a.wires.map(\.to) == b.wires.map(\.to))
    for n in a.nodes { #expect((2...8).contains(n.rows.count)) }
}

@Test func faceRendersAtScale() {
    let s = FakeScene.make(nodes: 10, wires: 10)
    let img = FaceRenderer.render(s.nodes[0], thumbnail: nil, scale: 2)
    #expect(img?.width == Int(s.nodes[0].size.width * 2))
}
