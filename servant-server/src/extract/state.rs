use crate::request::RequestData;

/// Mutable cursor over the request while extracting an endpoint's arguments.
pub struct ExtractState<'a> {
    pub(super) captures: std::vec::IntoIter<String>,
    pub(super) capture_all: Option<Vec<String>>,
    pub(super) req: &'a RequestData,
    /// Stack of contexts: the base context at the bottom, named sub-contexts
    /// (from `WithNamedContext`) pushed on top; lookups search top-down.
    pub(super) contexts: Vec<&'a crate::context::Context>,
    prechecked: Vec<Box<dyn std::any::Any + Send + Sync>>,
}

impl<'a> ExtractState<'a> {
    /// Build extraction state from the collected captures, the request, and the
    /// server context.
    pub fn new(
        captures: Vec<String>,
        capture_all: Option<Vec<String>>,
        req: &'a RequestData,
        ctx: &'a crate::context::Context,
    ) -> Self {
        ExtractState {
            captures: captures.into_iter(),
            capture_all,
            req,
            contexts: vec![ctx],
            prechecked: Vec::new(),
        }
    }

    pub(super) fn next_capture(&mut self) -> Option<String> {
        self.captures.next()
    }

    pub(super) fn take_capture_all(&mut self) -> Vec<String> {
        self.capture_all.take().unwrap_or_default()
    }

    /// Look up a context entry, searching pushed named sub-contexts first.
    pub(super) fn lookup_ctx<T: std::any::Any + Send + Sync>(&self) -> Option<&'a T> {
        self.contexts
            .iter()
            .rev()
            .copied()
            .find_map(|c| c.get::<T>())
    }

    pub(super) fn push_ctx(&mut self, ctx: &'a crate::context::Context) {
        self.contexts.push(ctx);
    }

    pub(super) fn pop_ctx(&mut self) {
        self.contexts.pop();
    }

    pub(super) fn push_prechecked<T: std::any::Any + Send + Sync>(&mut self, value: T) {
        self.prechecked.push(Box::new(value));
    }

    pub(super) fn take_prechecked<T: std::any::Any + Send + Sync>(&mut self) -> Option<T> {
        let index = self.prechecked.iter().position(|value| value.is::<T>())?;
        self.prechecked
            .remove(index)
            .downcast::<T>()
            .ok()
            .map(|value| *value)
    }
}
