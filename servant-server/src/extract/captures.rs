use servant::api::{Capture, CaptureAll, Endpoint};
use servant::error::ServerError;
use servant::hlist::HCons;
use servant::http_data::FromHttpApiData;
use servant::modifiers::CaptureShape;

use super::chain::{Rendered, ServerChain, bad_request, cons_tail};
use super::state::ExtractState;
use crate::result::RouteResult;

// --- Capture ---

impl<A, S, Next> ServerChain for Capture<A, S, Next>
where
    A: FromHttpApiData,
    S: CaptureShape<A>,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<<S as CaptureShape<A>>::Out, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        caps: &[String],
        idx: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        let i = *idx;
        *idx += 1;
        let Some(seg) = caps.get(i) else {
            return RouteResult::Fail(not_found_capture());
        };
        match <S as CaptureShape<A>>::build(A::from_url_piece(seg)) {
            Ok(_) => self.next.validate_captures(caps, idx, ca),
            // Strict parse failure: recoverable Fail (a sibling may parse it).
            Err(e) => RouteResult::Fail(bad_request(format!(
                "could not parse capture `{}`: {}",
                self.name, e
            ))),
        }
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let Some(seg) = st.next_capture() else {
            return RouteResult::Fail(not_found_capture());
        };
        match <S as CaptureShape<A>>::build(A::from_url_piece(&seg)) {
            Ok(out) => cons_tail(out, &self.next, st),
            Err(e) => RouteResult::Fail(bad_request(format!(
                "could not parse capture `{}`: {}",
                self.name, e
            ))),
        }
    }
}

fn not_found_capture() -> ServerError {
    ServerError::err404()
}

// --- CaptureAll ---

impl<A, Next> ServerChain for CaptureAll<A, Next>
where
    A: FromHttpApiData,
    Next: ServerChain,
    Self: Endpoint<Args = HCons<Vec<A>, Next::Args>, Output = Next::Output>,
{
    fn validate_captures(
        &self,
        caps: &[String],
        idx: &mut usize,
        ca: &Option<Vec<String>>,
    ) -> RouteResult<()> {
        if let Some(segs) = ca {
            for s in segs {
                if let Err(e) = A::from_url_piece(s) {
                    return RouteResult::Fail(bad_request(format!(
                        "could not parse capture-all `{}`: {}",
                        self.name, e
                    )));
                }
            }
        }
        self.next.validate_captures(caps, idx, ca)
    }
    forward_response_checks!();
    fn extract(&self, st: &mut ExtractState<'_>) -> RouteResult<Self::Args> {
        let segs = st.take_capture_all();
        let mut out = Vec::with_capacity(segs.len());
        for s in &segs {
            match A::from_url_piece(s) {
                Ok(a) => out.push(a),
                Err(e) => {
                    return RouteResult::Fail(bad_request(format!(
                        "could not parse capture-all `{}`: {}",
                        self.name, e
                    )));
                }
            }
        }
        cons_tail(out, &self.next, st)
    }
}
