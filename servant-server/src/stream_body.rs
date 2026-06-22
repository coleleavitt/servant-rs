use futures_util::StreamExt;
use servant::content::{MediaType, MimeUnrender};
use servant::stream::{DEFAULT_MAX_DECODED_FRAME_BYTES, Framing, SourceStream, StreamBodyError};

use crate::request::RequestBodyStream;

pub(crate) fn decode_stream<Fr, CType, T>(
    body: RequestBodyStream,
) -> SourceStream<Result<T, StreamBodyError>>
where
    Fr: Framing + Send + 'static,
    CType: MediaType + Send + 'static,
    T: MimeUnrender<CType> + Send + 'static,
{
    struct State {
        body: RequestBodyStream,
        buf: Vec<u8>,
        eof: bool,
        done: bool,
    }

    let stream = futures_util::stream::unfold(
        State {
            body,
            buf: Vec::new(),
            eof: false,
            done: false,
        },
        |mut state| async move {
            loop {
                if state.done {
                    return None;
                }
                match Fr::deframe_limited(
                    &mut state.buf,
                    state.eof,
                    DEFAULT_MAX_DECODED_FRAME_BYTES,
                ) {
                    Ok(Some(frame)) => {
                        let item = match <T as MimeUnrender<CType>>::mime_unrender(&frame) {
                            Ok(item) => Ok(item),
                            Err(message) => {
                                return Some((
                                    Err(StreamBodyError::Decode { message }),
                                    State {
                                        done: true,
                                        ..state
                                    },
                                ));
                            }
                        };
                        return Some((item, state));
                    }
                    Ok(None) if state.eof => {
                        return None;
                    }
                    Ok(None) => {}
                    Err(error) => {
                        state.done = true;
                        return Some((Err(error), state));
                    }
                }
                match state.body.next().await {
                    Some(Ok(chunk)) => state.buf.extend_from_slice(&chunk),
                    Some(Err(error)) => {
                        state.done = true;
                        return Some((Err(error), state));
                    }
                    None => state.eof = true,
                }
            }
        },
    );

    SourceStream::new(stream)
}
