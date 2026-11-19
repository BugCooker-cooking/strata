import AppKit

struct PathAnchor: Decodable, Equatable {
    var position: CGPoint
    var incoming = CGPoint.zero
    var outgoing = CGPoint.zero

    var parameters: [String: [Double]] {
        ["position": [position.x, position.y], "incoming": [incoming.x, incoming.y],
         "outgoing": [outgoing.x, outgoing.y]]
    }
}
struct WorkPath: Decodable, Identifiable, Equatable {
    let id: String
    let name: String
    let anchors: [PathAnchor]
}

@MainActor
final class CanvasPathInput: ObservableObject {
    @Published private(set) var selectedID: String?
    @Published private(set) var anchors: [PathAnchor] = []
    @Published private(set) var index: Int?
    @Published private(set) var selectedAnchors: Set<Int> = []
    @Published var name = ""
    @Published private(set) var paths: [WorkPath] = []
    private(set) var enabled = false
    private var documentID: String?
    private var revision: UInt64 = 0
    private var control = ""
    private var pendingCreation = false
    private enum Drag { case position, incoming, outgoing, symmetric, translate }
    private var drag: Drag?
    private var lastPoint = CGPoint.zero
    var onChanged: (() -> Void)?
    var onCommand: ((String, [String: Any]) -> Void)?

    func bind(_ snapshot: EditorSnapshot?, enabled: Bool) {
        let newPaths = snapshot?.paths ?? []
        let newControl = snapshot?.control?.kind ?? ""
        let switched = documentID != snapshot?.documentId
        let changed = switched || revision != snapshot?.revision || control != newControl || self.enabled != enabled
        self.enabled = enabled
        guard changed || paths != newPaths else { return }
        let created = pendingCreation && !switched ? newPaths.first(where: { candidate in
            !paths.contains(where: { $0.id == candidate.id }) && candidate.name == name && candidate.anchors == anchors
        })?.id : nil
        paths = newPaths
        documentID = snapshot?.documentId
        revision = snapshot?.revision ?? 0
        control = newControl
        select(switched ? nil : created ?? selectedID)
    }
    func select(_ id: String?) {
        let selected = paths.first { $0.id == id }
        selectedID = selected?.id
        anchors = selected?.anchors ?? []
        name = selected?.name ?? L10n.format("路径 {0}", String(paths.count + 1))
        index = nil
        selectedAnchors.removeAll()
        pendingCreation = false
        drag = nil
        onChanged?()
    }
    var hasSavedGeometry: Bool {
        guard let p = paths.first(where: { $0.id == selectedID }) else { return false }
        return p.anchors == anchors && p.name == name
    }
    var canApply: Bool {
        guard enabled, anchors.count >= 3, !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              name.utf8.count <= 256, !name.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains)
        else { return false }
        guard let old = paths.first(where: { $0.id == selectedID }) else { return true }
        return anchors != old.anchors || name != old.name
    }
    func down(_ point: CGPoint, tolerance: CGFloat, modifiers: NSEvent.ModifierFlags, clicks: Int) {
        guard enabled else { return }
        func near(_ p: CGPoint) -> Bool { hypot(p.x - point.x, p.y - point.y) <= tolerance }
        lastPoint = point
        if selectedID != nil, modifiers.contains(.command) {
            selectedAnchors = Set(anchors.indices)
            index = nil
            drag = .translate
            onChanged?()
            return
        }
        if selectedID == nil, !modifiers.contains(.shift), anchors.count >= 3, clicks > 1 || near(anchors[0].position) {
            apply()
            return
        }
        if let index, !modifiers.contains(.shift) {
            let a = anchors[index]
            if a.incoming != .zero, near(a.position.offset(a.incoming)) { drag = .incoming; return }
            if a.outgoing != .zero, near(a.position.offset(a.outgoing)) { drag = .outgoing; return }
        }
        if let hit = anchors.indices.min(by: {
            hypot(anchors[$0].position.x-point.x, anchors[$0].position.y-point.y) <
                hypot(anchors[$1].position.x-point.x, anchors[$1].position.y-point.y)
        }), near(anchors[hit].position) {
            if modifiers.contains(.shift) {
                if selectedAnchors.contains(hit) { selectedAnchors.remove(hit) } else { selectedAnchors.insert(hit) }
                index = selectedAnchors.contains(hit) ? hit : selectedAnchors.min()
                drag = nil
            } else {
                if !selectedAnchors.contains(hit) || modifiers.contains(.option) { selectedAnchors = [hit] }
                index = hit
                drag = modifiers.contains(.option) ? .symmetric : .position
            }
        } else if anchors.count < 128 {
            let insertion = (index ?? (anchors.count - 1)) + 1
            anchors.insert(PathAnchor(position: point), at: insertion)
            index = insertion
            selectedAnchors = [insertion]
            drag = .symmetric
        }
        onChanged?()
    }
    func move(_ point: CGPoint) {
        guard enabled, let drag else { return }
        if drag == .translate || drag == .position {
            let delta = CGPoint(x: point.x-lastPoint.x, y: point.y-lastPoint.y)
            for i in selectedAnchors { anchors[i].position = anchors[i].position.offset(delta) }
            lastPoint = point
            onChanged?()
            return
        }
        guard let index else { return }
        let offset = CGPoint(x: point.x - anchors[index].position.x, y: point.y - anchors[index].position.y)
        switch drag {
        case .position, .translate: return
        case .incoming: anchors[index].incoming = offset
        case .outgoing: anchors[index].outgoing = offset
        case .symmetric:
            anchors[index].outgoing = offset
            anchors[index].incoming = CGPoint(x: -offset.x, y: -offset.y)
        }
        onChanged?()
    }
    func endDrag() { drag = nil }
    func corner() {
        guard enabled else { return }
        for i in selectedAnchors { anchors[i].incoming = .zero; anchors[i].outgoing = .zero }
        onChanged?()
    }
    var canRemoveAnchors: Bool {
        !selectedAnchors.isEmpty && (selectedID == nil || anchors.count - selectedAnchors.count >= 3)
    }
    func splitNext() {
        guard enabled, hasSavedGeometry, let selectedID, let index, anchors.count < 128 else { return }
        onCommand?("path.split", ["path": ["id": selectedID], "segment": index, "t": 0.5, "expected_revision": revision])
    }
    func removeAnchor() {
        guard enabled, canRemoveAnchors else { return }
        for i in selectedAnchors.sorted(by: >) { anchors.remove(at: i) }
        selectedAnchors.removeAll()
        self.index = nil
        drag = nil
        onChanged?()
    }
    func apply() {
        guard canApply else { return }
        var params: [String: Any] = ["name": name, "anchors": anchors.map(\.parameters), "expected_revision": revision]
        if let selectedID {
            params["path"] = ["id": selectedID]
            onCommand?("path.update", params)
        } else {
            pendingCreation = true
            onCommand?("path.create", params)
        }
    }
    func deletePath() {
        guard enabled, let selectedID else { return }
        onCommand?("path.delete", ["path": ["id": selectedID], "expected_revision": revision])
    }
    func loadSelection() {
        guard enabled, let selectedID, hasSavedGeometry else { return }
        onCommand?("select.from_path", ["path": ["id": selectedID], "mode": "replace"])
    }
    func outlines(radius: CGFloat) -> (CGPath, CGPath, CGPath) {
        let curve = CGMutablePath(), nodes = CGMutablePath(), handles = CGMutablePath()
        guard enabled, let first = anchors.first else { return (curve, nodes, handles) }
        curve.move(to: first.position)
        for i in anchors.indices {
            let a = anchors[i]
            let next = (i + 1) % anchors.count
            if next != 0 || anchors.count >= 3 {
                let b = anchors[next]
                curve.addCurve(to: b.position, control1: a.position.offset(a.outgoing), control2: b.position.offset(b.incoming))
            }
            let r = selectedAnchors.contains(i) ? radius * 1.35 : radius
            nodes.addRect(CGRect(x: a.position.x-r, y: a.position.y-r, width: 2*r, height: 2*r))
        }
        if let index {
            let a = anchors[index]
            for offset in [a.incoming, a.outgoing] where offset != .zero {
                let end = a.position.offset(offset)
                handles.move(to: a.position)
                handles.addLine(to: end)
                handles.addEllipse(in: CGRect(x: end.x-radius, y: end.y-radius, width: 2*radius, height: 2*radius))
            }
        }
        return (curve, nodes, handles)
    }
}
private extension CGPoint {
    func offset(_ v: CGPoint) -> CGPoint { CGPoint(x: x + v.x, y: y + v.y) }
}
