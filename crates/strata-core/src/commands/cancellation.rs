use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

/// Trusted host handle independent of the editor/render mutex. Cancel marks
/// every existing token permanently, then publishes a fresh generation so a
/// subsequent request never revives work that was already cancelled.
#[derive(Clone, Debug)]
pub struct WorkCancellation {
    current: Arc<Mutex<Arc<AtomicBool>>>,
}
impl Default for WorkCancellation {
    fn default() -> Self {
        Self {
            current: Arc::new(Mutex::new(Arc::new(AtomicBool::new(false)))),
        }
    }
}
impl WorkCancellation {
    pub fn cancel(&self) {
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        current.store(true, Ordering::Release);
        *current = Arc::new(AtomicBool::new(false));
    }
    pub(super) fn token(&self) -> Arc<AtomicBool> {
        self.current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generations_never_reactivate_cancelled_tokens() {
        let handle = WorkCancellation::default();
        let first = handle.token();
        let second = handle.token();
        handle.clone().cancel();
        assert!(first.load(Ordering::Acquire));
        assert!(second.load(Ordering::Acquire));
        let next = handle.token();
        assert!(!next.load(Ordering::Acquire));
        handle.cancel();
        assert!(next.load(Ordering::Acquire));
        assert!(first.load(Ordering::Acquire));
        assert!(!handle.token().load(Ordering::Acquire));
    }
}
