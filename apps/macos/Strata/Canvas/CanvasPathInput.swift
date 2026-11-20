import AppKit

struct PathAnchor: Decodable, Equatable {
    var position: CGPoint
    var linked: Bool?
    var incoming = CGPoint.zero
    var outgoing = CGPoint.zero

    var parameters: [String: Any] {
        var result: [String: Any] = ["position": [position.x, position.y], "incoming": [incoming.x, incoming.y],
                                     "outgoing": [outgoing.x, outgoing.y]]
        if linked == true { result["linked"] = true }
        return result
    }
}
struct WorkPath: Decodable, Identifiable, Equatable {
    let id: String
    let name: String
    let anchors: [PathAnchor]
    let openContours: Set<Int>?
    let subpaths: [[PathAnchor]]?
    var contours: [[PathAnchor]] { [anchors] + (subpaths ?? []) }
}

@MainActor
final class CanvasPathInput: ObservableObject {
    @Published private(set) var selectedID: String?
    @Published private(set) var contours: [[PathAnchor]] = [[]]
    @Published private(set) var openContours: Set<Int> = []
    @Published private(set) var contour = 0
    private var anchorCount: Int { contours.reduce(0) { $0 + $1.count } }
    var anchors: [PathAnchor] {
        get { contours[contour] }
        set { contours[contour] = newValue }
    }
    @Published private(set) var index: Int?
    @Published private(set) var selectedAnchors: Set<Int> = []
    @Published var name = ""
    @Published private(set) var paths: [WorkPath] = []
    private(set) var enabled = false
    private var documentID: String?
    private var revision: UInt64 = 0
    private var control = ""
    private var pendingCreation = false
    private var pendingTraceName: String?
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
        let created = !switched ? newPaths.first(where: { candidate in
            !paths.contains(where: { $0.id == candidate.id }) &&
                ((pendingCreation && candidate.name == name && candidate.contours == contours && (candidate.openContours ?? []) == openContours) ||
                 (pendingTraceName == candidate.name))
        })?.id : nil
        paths = newPaths
        documentID = snapshot?.documentId
        revision = snapshot?.revision ?? 0
        control = newControl
        select(switched ? nil : created ?? selectedID)
    }
    func select(_ id: String?) {
        let selected = paths.first { $0.id == id }
        let nextContour = selectedID == id ? contour : 0
        selectedID = selected?.id
        contours = selected?.contours ?? [[]]
        openContours = selected?.openContours ?? []
        contour = min(nextContour, contours.count - 1)
        name = selected?.name ?? L10n.format("路径 {0}", String(paths.count + 1))
        index = nil
        selectedAnchors.removeAll()
        pendingCreation = false
        pendingTraceName = nil
        drag = nil
        onChanged?()
    }
    func chooseContour(_ i: Int) {
        guard contours.indices.contains(i) else { return }
        contour = i
        index = nil
        selectedAnchors.removeAll()
        drag = nil
        onChanged?()
    }
    func setOpen(_ value: Bool) {
        guard enabled else { return }
        if value { openContours.insert(contour) } else { openContours.remove(contour) }
        onChanged?()
    }
    func addContour() {
        guard canAddContour else { return }
        contours.append([])
        chooseContour(contours.count - 1)
    }
    func removeContour() {
        guard enabled, contours.count > 1 else { return }
        let next = min(contour, contours.count - 2)
        openContours = Set(openContours.compactMap { i in i == contour ? nil : (i > contour ? i-1 : i) })
        contours.remove(at: contour)
        chooseContour(next)
    }
    func trace(threshold: Int, tolerance: Double) {
        guard enabled, !hasDraftChanges, (1...65535).contains(threshold), (0...10).contains(tolerance) else { return }
        let newName = L10n.format("选区路径 {0}", String(paths.count + 1))
        pendingTraceName = newName
        onCommand?("path.from_selection", ["name": newName, "threshold": threshold, "tolerance": tolerance, "expected_revision": revision])
    }
    var hasDraftChanges: Bool {
        if selectedID != nil { return !hasSavedGeometry }
        return contours.contains { !$0.isEmpty }
    }
    var canAddContour: Bool {
        enabled && contours.count < 64 && anchorCount <= 2045 && contours.allSatisfy { $0.count >= 2 }
    }
    var hasSavedGeometry: Bool {
        guard let p = paths.first(where: { $0.id == selectedID }) else { return false }
        return p.contours == contours && (p.openContours ?? []) == openContours && p.name == name
    }
    var canApply: Bool {
        guard enabled, contours.allSatisfy({ (2...1024).contains($0.count) }), anchorCount <= 2048, !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              name.utf8.count <= 256, !name.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains)
        else { return false }
        guard let old = paths.first(where: { $0.id == selectedID }) else { return true }
        return contours != old.contours || (old.openContours ?? []) != openContours || name != old.name
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
        if selectedID == nil, !modifiers.contains(.shift), anchors.count >= 2, clicks > 1 || (!openContours.contains(contour) && anchors.count >= 3 && near(anchors[0].position)) {
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
        } else if anchors.count < 1024 && anchorCount < 2048 {
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
            if drag == .translate {
                for c in contours.indices {
                    for i in contours[c].indices { contours[c][i].position = contours[c][i].position.offset(delta) }
                }
            } else {
                for i in selectedAnchors { anchors[i].position = anchors[i].position.offset(delta) }
            }
            lastPoint = point
            onChanged?()
            return
        }
        guard let index else { return }
        let offset = CGPoint(x: point.x - anchors[index].position.x, y: point.y - anchors[index].position.y)
        switch drag {
        case .position, .translate: return
        case .incoming, .outgoing:
            let incoming = drag == .incoming
            let oldOpposite = incoming ? anchors[index].outgoing : anchors[index].incoming
            if incoming { anchors[index].incoming = offset } else { anchors[index].outgoing = offset }
            let length = hypot(offset.x, offset.y)
            if anchors[index].linked == true, length > 0 {
                let scale = -hypot(oldOpposite.x, oldOpposite.y) / length
                let opposite = CGPoint(x: offset.x * scale, y: offset.y * scale)
                if incoming { anchors[index].outgoing = opposite } else { anchors[index].incoming = opposite }
            }
        case .symmetric:
            anchors[index].linked = offset == .zero ? nil : true
            anchors[index].outgoing = offset
            anchors[index].incoming = CGPoint(x: -offset.x, y: -offset.y)
        }
        onChanged?()
    }
    func endDrag() { drag = nil }
    func corner() {
        guard enabled else { return }
        for i in selectedAnchors { anchors[i].incoming = .zero; anchors[i].outgoing = .zero; anchors[i].linked = nil }
        onChanged?()
    }
    func unlinkHandles() {
        guard enabled else { return }
        for i in selectedAnchors { anchors[i].linked = nil }
        onChanged?()
    }
    var canRemoveAnchors: Bool {
        !selectedAnchors.isEmpty && (selectedID == nil || anchors.count - selectedAnchors.count >= 2)
    }
    func splitNext() {
        guard enabled, hasSavedGeometry, let selectedID, let index, anchors.count < 1024, anchorCount < 2048, !openContours.contains(contour) || index < anchors.count-1 else { return }
        onCommand?("path.split", ["path": ["id": selectedID], "contour": contour, "segment": index, "t": 0.5, "expected_revision": revision])
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
        var params: [String: Any] = ["name": name, "anchors": contours[0].map(\.parameters),
            "open_contours": openContours.sorted(), "subpaths": contours.dropFirst().map { $0.map(\.parameters) }, "expected_revision": revision]
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
        guard enabled else { return (curve, nodes, handles) }
        for (c, part) in contours.enumerated() {
            guard let first = part.first else { continue }
            curve.move(to: first.position)
            for i in part.indices {
                let a = part[i]
                let next = (i + 1) % part.count
                if next != 0 || (!openContours.contains(c) && part.count >= 2) {
                    let b = part[next]
                    curve.addCurve(to: b.position, control1: a.position.offset(a.outgoing), control2: b.position.offset(b.incoming))
                }
                if c == contour {
                    let r = selectedAnchors.contains(i) ? radius * 1.35 : radius
                    nodes.addRect(CGRect(x: a.position.x-r, y: a.position.y-r, width: 2*r, height: 2*r))
                }
            }
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
