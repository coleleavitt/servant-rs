use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use servant::content::{Json, PlainText};
use servant::uverb::{
    ArmBody,
    Union2,
    UnionDecode,
    UnionResponse,
    WithFixedStatus,
    WithStatus,
    WithStatusHeaders,
    WithStatusNoBody,
};

fn full_body(body: ArmBody) -> Bytes {
    match body {
        ArmBody::Full(bytes) => bytes,
        ArmBody::Stream(_) => panic!("expected buffered union body"),
    }
}

#[test]
fn renders_active_arm_status_and_body() {
    type Resp = Union2<WithStatus<200, u32>, WithStatus<404, String>>;
    let ok: Resp = Union2::V0(WithStatus::new(7u32));
    let (status, mt, body, _h) =
        UnionResponse::<(Json,)>::render_union(ok, Some("application/json")).unwrap();
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mt.unwrap(), mime::APPLICATION_JSON);
    let body = full_body(body);
    assert_eq!(&body[..], b"7");

    let nf: Resp = Union2::V1(WithStatus::new("nope".to_string()));
    let (status, _, body, _h) =
        UnionResponse::<(Json,)>::render_union(nf, Some("application/json")).unwrap();
    assert_eq!(status, StatusCode::NOT_FOUND);
    let body = full_body(body);
    assert_eq!(&body[..], br#""nope""#);
}

#[test]
fn decodes_by_status() {
    type Resp = Union2<WithStatus<200, u32>, WithStatus<404, String>>;
    let empty = HeaderMap::new();
    let r: Resp = <Resp as UnionDecode<(Json,)>>::decode_union(
        StatusCode::NOT_FOUND,
        &empty,
        Some(&mime::APPLICATION_JSON),
        br#""nope""#,
    )
    .unwrap()
    .unwrap();
    assert_eq!(r, Union2::V1(WithStatus::new("nope".to_string())));
    assert!(
        <Resp as UnionDecode<(Json,)>>::decode_union(
            StatusCode::INTERNAL_SERVER_ERROR,
            &empty,
            Some(&mime::APPLICATION_JSON),
            b"",
        )
        .is_none()
    );
}

#[test]
fn arm_with_headers_round_trips_status_headers_and_body() {
    type Resp = Union2<WithStatusHeaders<201, u32>, WithStatus<404, String>>;
    let created: Resp = Union2::V0(
        WithStatusHeaders::new(9u32)
            .header("location", "/things/9")
            .unwrap(),
    );
    let (status, _, body, headers) =
        UnionResponse::<(Json,)>::render_union(created, Some("application/json")).unwrap();
    assert_eq!(status, StatusCode::CREATED);
    let body = full_body(body);
    assert_eq!(&body[..], b"9");
    assert_eq!(headers[0].0.as_str(), "location");
}

#[test]
fn fixed_and_empty_arms_render_and_decode() {
    type Resp = Union2<WithFixedStatus<202, PlainText, String>, WithStatusNoBody<204>>;

    let accepted: Resp = Union2::V0(WithFixedStatus::new("queued".to_string()));
    let (status, mt, body, _headers) =
        UnionResponse::<(Json, PlainText)>::render_union(accepted, None).unwrap();
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(mt.unwrap().essence_str(), "text/plain");
    let body = full_body(body);
    assert_eq!(&body[..], b"queued");

    let decoded: Resp = <Resp as UnionDecode<(Json, PlainText)>>::decode_union(
        StatusCode::ACCEPTED,
        &HeaderMap::new(),
        Some(&mime::TEXT_PLAIN_UTF_8),
        b"queued",
    )
    .unwrap()
    .unwrap();
    assert!(matches!(decoded, Union2::V0(_)));

    let empty: Resp = Union2::V1(WithStatusNoBody::new());
    let (status, mt, body, _headers) =
        UnionResponse::<(Json, PlainText)>::render_union(empty, None).unwrap();
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(mt.is_none());
    assert!(full_body(body).is_empty());
}

#[test]
fn fixed_arm_rejects_unacceptable_content_type() {
    type Resp = Union2<WithFixedStatus<202, PlainText, String>, WithStatusNoBody<204>>;
    let accepted: Resp = Union2::V0(WithFixedStatus::new("queued".to_string()));

    let err = match UnionResponse::<(Json, PlainText)>::render_union(
        accepted,
        Some("application/json"),
    ) {
        Ok(_) => panic!("fixed arm should reject unacceptable content type"),
        Err(err) => err,
    };

    assert_eq!(err, "not acceptable");
}
