import Foundation

/// Immutable Sendable callback; UI delivery is always on the main actor.
final class EngineEvents: EngineListener, Sendable {
    private let handler: @MainActor @Sendable (EngineEventDto) -> Void

    init(onEvent: @escaping @MainActor @Sendable (EngineEventDto) -> Void) {
        handler = onEvent
    }

    func onEvent(event: EngineEventDto) {
        let handler = handler
        DispatchQueue.main.async { handler(event) }
    }
}
