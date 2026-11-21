import AppKit

struct ShapeSource: Decodable, Equatable {
    struct Geometry: Decodable, Equatable {
        struct Primitive: Decodable, Equatable {
            let kind: String
            let radius: Double?
            let sides: Int?
            let weight: Double?
        }
        let start: CGPoint
        let end: CGPoint
        let primitive: Primitive
    }
    let geometry: Geometry
    let placement: [Double]
}

@MainActor
final class CanvasShapeInput: ObservableObject {
    enum Tool: String, CaseIterable {
        case rectangle, rounded, ellipse, line, polygon, triangle
        var title: String {
            switch self {
            case .rectangle: L10n.text("矩形")
            case .rounded: L10n.text("圆角矩形")
            case .ellipse: L10n.text("椭圆")
            case .line: L10n.text("直线")
            case .polygon: L10n.text("多边形")
            case .triangle: L10n.text("三角形")
            }
        }
    }
    @Published var tool = Tool.rectangle
    @Published var radius = 8.0
    @Published var sides = 5
    @Published var weight = 2.0
    @Published var start = CGPoint.zero
    @Published var end = CGPoint(x: 64, y: 64)
    @Published private(set) var target: String?
    @Published private(set) var error = ""
    private var snapshot: EditorSnapshot?
    private(set) var enabled = false
    private var downPoint: CGPoint?
    private var beforeDrag: (CGPoint, CGPoint)?
    private(set) var preview: CGPath?
    var onChange: (() -> Void)?
    var onCommand: ((String, [String: Any]) -> Void)?

    func bind(_ value: EditorSnapshot?, enabled: Bool) {
        let changeDocument = value?.documentId != snapshot?.documentId || value?.sessionId != snapshot?.sessionId
        let targetChanged = changeDocument || value?.activeLayer != snapshot?.activeLayer
        let changed = targetChanged || self.enabled != enabled || value?.revision != snapshot?.revision ||
            value?.control?.kind != snapshot?.control?.kind
        snapshot = value
        self.enabled = enabled
        guard changed else { return }
        cancel()
        let layer = value?.layers.first { $0.id == value?.activeLayer }
        let source = value?.paths?.first { $0.id == layer?.vectorMask?.path }?.shape
        target = layer?.shapePaint != nil && source != nil ? layer?.id : nil
        if let source, target != nil {
            let geometry = source.geometry
            start = geometry.start
            end = geometry.end
            tool = geometry.primitive.kind == "rectangle" && (geometry.primitive.radius ?? 0) > 0
                ? .rounded : (Tool(rawValue: geometry.primitive.kind) ?? .rectangle)
            if let r = geometry.primitive.radius, r > 0 { radius = r }
            if let n = geometry.primitive.sides { sides = n }
            if let w = geometry.primitive.weight { weight = w }
        } else if changeDocument, let frame = value?.frame {
            start = CGPoint(x: Double(frame.x), y: Double(frame.y))
            end = CGPoint(x: start.x + Double(frame.width)/2, y: start.y + Double(frame.height)/2)
        }
    }
    private var geometry: [String: Any] {
        var primitive: [String: Any] = ["kind": tool == .rounded ? "rectangle" : tool.rawValue]
        if tool == .rounded { primitive["radius"] = radius }
        if tool == .polygon { primitive["sides"] = sides }
        if tool == .line { primitive["weight"] = weight }
        return ["start": [start.x, start.y], "end": [end.x, end.y], "primitive": primitive]
    }
    private func outline() throws -> [PathAnchor] {
        let json = try shapeOutlineJson(geometryJson: commandJSON(geometry))
        return try JSONDecoder().decode([PathAnchor].self, from: Data(json.utf8))
    }
    func down(_ p: CGPoint) {
        guard enabled else { return }
        beforeDrag = (start, end)
        downPoint = p
        start = p; end = p; error = ""
    }
    func move(_ p: CGPoint, constrain: Bool) {
        guard let origin = downPoint else { return }
        var dx = p.x-origin.x, dy = p.y-origin.y
        if constrain, tool == .line {
            let angle = (atan2(dy, dx) / (.pi/4)).rounded() * (.pi/4)
            let length = hypot(dx, dy)
            dx = cos(angle)*length; dy = sin(angle)*length
        } else if constrain {
            let side = max(abs(dx),abs(dy))
            dx = dx < 0 ? -side : side; dy = dy < 0 ? -side : side
        }
        end = CGPoint(x: origin.x+dx, y: origin.y+dy)
        preview = nil
        if let anchors = try? outline(), let first = anchors.first {
            let p = CGMutablePath()
            p.move(to: first.position)
            for i in anchors.indices {
                let a = anchors[i], b = anchors[(i+1)%anchors.count]
                p.addCurve(to: b.position,
                    control1: CGPoint(x: a.position.x+a.outgoing.x, y: a.position.y+a.outgoing.y),
                    control2: CGPoint(x: b.position.x+b.incoming.x, y: b.position.y+b.incoming.y))
            }
            p.closeSubpath()
            preview = p
        }
        onChange?()
    }
    func up(_ p: CGPoint, constrain: Bool) {
        guard downPoint != nil else { return }
        move(p, constrain: constrain)
        guard preview != nil else { cancel(); return }
        downPoint = nil; beforeDrag = nil; preview = nil
        onChange?()
        apply(create: true)
    }
    func cancel() {
        if let (a,b) = beforeDrag { start = a; end = b }
        downPoint = nil; beforeDrag = nil; preview = nil; error = ""
        onChange?()
    }
    func useForeground() {
        guard enabled, let target, let snapshot, let rgba = snapshot.paintColors?.foreground else { return }
        let maxCode = snapshot.bitDepth == 16 ? 65535.0 : 255.0
        onCommand?("shape.update", ["layer": ["id":target], "color": rgba.map { Int(($0*maxCode).rounded()) },
                                    "expected_revision": snapshot.revision])
    }
    func apply(create: Bool) {
        guard enabled, let snapshot, let document = snapshot.documentId else { return }
        do {
            _ = try outline()
            var p: [String: Any] = ["geometry": geometry, "expected_revision": snapshot.revision]
            if create {
                guard let rgba = snapshot.paintColors?.foreground else { return }
                let maxCode = snapshot.bitDepth == 16 ? 65535.0 : 255.0
                p["name"] = L10n.format("{0} {1}", tool.title, String((snapshot.paths?.count ?? 0)+1))
                p["color"] = rgba.map { Int(($0*maxCode).rounded()) }
                p["expected_document"] = document
                if let active = snapshot.activeLayer { p["expected_active_layer"] = active }
            } else {
                guard let target else { return }
                p["layer"] = ["id":target]
            }
            error = ""
            onCommand?(create ? "shape.create" : "shape.update", p)
        } catch {
            self.error = L10n.text("请检查形状的两个端点和尺寸参数。")
        }
    }
}
