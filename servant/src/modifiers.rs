//! Presence (`Required`/`Optional`) and parse-strictness (`Strict`/`Lenient`)
//! modifiers, and the shared rules that turn a raw lookup into a handler
//! argument.
//!
//! These mirror `Servant.API.Modifiers`. The crucial property — and the reason
//! this lives in one place — is that the *same* [`ArgShape`] table is consumed
//! by the server (extraction), the client (call argument), the link builder, and
//! the docs walker, so the four interpretations can never disagree about whether
//! an argument is `T`, `Option<T>`, `Result<T, _>`, or `Option<Result<T, _>>`.

/// A failure to parse a request component (capture, query value, header).
///
/// Carries only a human-readable message; it never embeds secret material.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ParseError(pub String);

impl ParseError {
    /// Build a [`ParseError`] from anything displayable.
    pub fn new(msg: impl std::fmt::Display) -> Self {
        ParseError(msg.to_string())
    }
}

/// Argument must be present; absence rejects the request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Required;

/// Argument may be absent; absence yields `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Optional;

/// A parse failure rejects the request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Strict;

/// A parse failure is surfaced to the handler as `Err`, never rejecting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lenient;

/// Why building an argument failed in a way that must reject the request.
///
/// (Only ever produced for `Strict` parsing or a missing `Required` argument;
/// `Lenient`/`Optional` shapes never reject.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgError {
    /// A required argument was absent.
    Missing,
    /// A strictly-parsed argument failed to parse.
    Parse(ParseError),
}

/// The shape of, and the rule for building, a `QueryParam`/`Header` argument
/// from a raw optional parse result.
///
/// Implemented for the four `(presence, strictness)` marker pairs. `build`'s
/// input encodes presence and parse outcome:
/// - `None` — the component was absent,
/// - `Some(Ok(a))` — present and parsed,
/// - `Some(Err(e))` — present but failed to parse.
pub trait ArgShape<A> {
    /// The wrapped type handed to the handler.
    type Out;
    /// Apply the presence/strictness rule (the `unfoldRequestArgument` fold).
    fn build(value: Option<Result<A, ParseError>>) -> Result<Self::Out, ArgError>;
    /// The reverse direction (for clients/links): recover the underlying value
    /// to render into a request. `None` means "omit" (absent, or a lenient
    /// parse error that has no renderable value).
    fn into_value(out: Self::Out) -> Option<A>;
}

impl<A> ArgShape<A> for (Required, Strict) {
    type Out = A;
    fn build(value: Option<Result<A, ParseError>>) -> Result<Self::Out, ArgError> {
        match value {
            None => Err(ArgError::Missing),
            Some(Ok(a)) => Ok(a),
            Some(Err(e)) => Err(ArgError::Parse(e)),
        }
    }
    fn into_value(out: Self::Out) -> Option<A> {
        Some(out)
    }
}

impl<A> ArgShape<A> for (Required, Lenient) {
    type Out = Result<A, ParseError>;
    fn build(value: Option<Result<A, ParseError>>) -> Result<Self::Out, ArgError> {
        match value {
            None => Err(ArgError::Missing),
            Some(res) => Ok(res),
        }
    }
    fn into_value(out: Self::Out) -> Option<A> {
        out.ok()
    }
}

impl<A> ArgShape<A> for (Optional, Strict) {
    type Out = Option<A>;
    fn build(value: Option<Result<A, ParseError>>) -> Result<Self::Out, ArgError> {
        match value {
            None => Ok(None),
            Some(Ok(a)) => Ok(Some(a)),
            Some(Err(e)) => Err(ArgError::Parse(e)),
        }
    }
    fn into_value(out: Self::Out) -> Option<A> {
        out
    }
}

impl<A> ArgShape<A> for (Optional, Lenient) {
    type Out = Option<Result<A, ParseError>>;
    fn build(value: Option<Result<A, ParseError>>) -> Result<Self::Out, ArgError> {
        Ok(value)
    }
    fn into_value(out: Self::Out) -> Option<A> {
        out.and_then(|r| r.ok())
    }
}

/// The shape of, and rule for, a `Capture` argument. Captures are always present
/// (the router only descends when a segment exists) so there is no `Optional`
/// form; only strictness varies.
pub trait CaptureShape<A> {
    /// The wrapped type handed to the handler.
    type Out;
    /// `Strict` propagates a parse failure (the route does not match);
    /// `Lenient` surfaces it as `Err`.
    fn build(parse: Result<A, ParseError>) -> Result<Self::Out, ParseError>;
    /// The reverse direction (for clients/links): recover the underlying value.
    fn into_value(out: Self::Out) -> Option<A>;
}

impl<A> CaptureShape<A> for Strict {
    type Out = A;
    fn build(parse: Result<A, ParseError>) -> Result<Self::Out, ParseError> {
        parse
    }
    fn into_value(out: Self::Out) -> Option<A> {
        Some(out)
    }
}

impl<A> CaptureShape<A> for Lenient {
    type Out = Result<A, ParseError>;
    fn build(parse: Result<A, ParseError>) -> Result<Self::Out, ParseError> {
        Ok(parse)
    }
    fn into_value(out: Self::Out) -> Option<A> {
        out.ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p() -> ParseError {
        ParseError::new("bad")
    }

    #[test]
    fn required_strict() {
        type S = (Required, Strict);
        assert_eq!(<S as ArgShape<u32>>::build(Some(Ok(5))), Ok(5));
        assert_eq!(<S as ArgShape<u32>>::build(None), Err(ArgError::Missing));
        assert_eq!(
            <S as ArgShape<u32>>::build(Some(Err(p()))),
            Err(ArgError::Parse(p()))
        );
    }

    #[test]
    fn required_lenient_surfaces_parse_error() {
        type S = (Required, Lenient);
        assert_eq!(<S as ArgShape<u32>>::build(Some(Ok(5))), Ok(Ok(5)));
        assert_eq!(<S as ArgShape<u32>>::build(Some(Err(p()))), Ok(Err(p())));
        assert_eq!(<S as ArgShape<u32>>::build(None), Err(ArgError::Missing));
    }

    #[test]
    fn optional_strict() {
        type S = (Optional, Strict);
        assert_eq!(<S as ArgShape<u32>>::build(None), Ok(None));
        assert_eq!(<S as ArgShape<u32>>::build(Some(Ok(5))), Ok(Some(5)));
        assert_eq!(
            <S as ArgShape<u32>>::build(Some(Err(p()))),
            Err(ArgError::Parse(p()))
        );
    }

    #[test]
    fn optional_lenient_never_rejects() {
        type S = (Optional, Lenient);
        assert_eq!(<S as ArgShape<u32>>::build(None), Ok(None));
        assert_eq!(<S as ArgShape<u32>>::build(Some(Ok(5))), Ok(Some(Ok(5))));
        assert_eq!(
            <S as ArgShape<u32>>::build(Some(Err(p()))),
            Ok(Some(Err(p())))
        );
    }

    #[test]
    fn capture_shapes() {
        assert_eq!(<Strict as CaptureShape<u32>>::build(Ok(5)), Ok(5));
        assert_eq!(<Strict as CaptureShape<u32>>::build(Err(p())), Err(p()));
        assert_eq!(<Lenient as CaptureShape<u32>>::build(Ok(5)), Ok(Ok(5)));
        assert_eq!(
            <Lenient as CaptureShape<u32>>::build(Err(p())),
            Ok(Err(p()))
        );
    }
}
