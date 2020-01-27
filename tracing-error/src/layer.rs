use std::any::TypeId;
use std::marker::PhantomData;
use tracing::{span, Dispatch, Metadata, Subscriber};
use tracing_subscriber::fmt::format::{DefaultFields, FormatFields};
use tracing_subscriber::{
    fmt::FormattedFields,
    layer::{self, Layer},
    registry::LookupSpan,
};

pub struct ErrorLayer<S, F = DefaultFields> {
    format: F,

    get_context: WithContext,
    _subscriber: PhantomData<fn(S)>,
}

// this function "remembers" the types of the subscriber and the formatter,
// so that we can downcast to something aware of them without knowing those
// types at the callsite.
pub(crate) struct WithContext(
    fn(&Dispatch, &span::Id, f: &mut dyn FnMut(&'static Metadata<'static>, &str) -> bool),
);

impl<S, F> Layer<S> for ErrorLayer<S, F>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
    F: for<'writer> FormatFields<'writer> + 'static,
{
    /// Notifies this layer that a new span was constructed with the given
    /// `Attributes` and `Id`.
    fn new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: layer::Context<'_, S>) {
        let span = ctx.span(id).expect("span must already exist!");
        if span.extensions().get::<FormattedFields<F>>().is_some() {
            return;
        }
        let mut fields = String::new();
        self.format.format_fields(&mut fields, attrs);
        span.extensions_mut()
            .insert(FormattedFields::<F>::new(fields));
    }

    unsafe fn downcast_raw(&self, id: TypeId) -> Option<*const ()> {
        match id {
            id if id == TypeId::of::<Self>() => Some(self as *const _ as *const ()),
            id if id == TypeId::of::<WithContext>() => {
                Some(&self.get_context as *const _ as *const ())
            }
            _ => None,
        }
    }
}

impl<S, F> ErrorLayer<S, F>
where
    F: for<'writer> FormatFields<'writer> + 'static,
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    pub fn new(format: F) -> Self {
        Self {
            format,
            get_context: WithContext(Self::get_context),
            _subscriber: PhantomData,
        }
    }

    fn get_context(
        dispatch: &Dispatch,
        id: &span::Id,
        f: &mut dyn FnMut(&'static Metadata<'static>, &str) -> bool,
    ) {
        let subscriber = dispatch
            .downcast_ref::<S>()
            .expect("subscriber should downcast to expected type; this is a bug!");
        let span = subscriber
            .span(id)
            .expect("registry should have a span for the current ID");
        let parents = span.parents();
        for span in std::iter::once(span).chain(parents) {
            let cont = if let Some(fields) = span.extensions().get::<FormattedFields<F>>() {
                f(span.metadata(), fields.fields.as_str())
            } else {
                f(span.metadata(), "")
            };
            if !cont {
                break;
            }
        }
    }
}

impl WithContext {
    pub(crate) fn with_context<'a>(
        &self,
        dispatch: &'a Dispatch,
        id: &span::Id,
        mut f: impl FnMut(&'static Metadata<'static>, &str) -> bool,
    ) {
        (self.0)(dispatch, id, &mut f)
    }
}

impl<S> Default for ErrorLayer<S>
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn default() -> Self {
        Self::new(DefaultFields::default())
    }
}
