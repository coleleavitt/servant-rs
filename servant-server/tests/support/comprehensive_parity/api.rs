macro_rules! create_api {
    () => {
        host(
            "api.example.com",
            operation_id(
                "createParity",
                summary(
                    "Create parity item",
                    description(
                        "Exercises metadata, query, header, body, and link combinators.",
                        path(
                            "parity",
                            capture::<u64, _>(
                                "id",
                                capture_all::<String, _>(
                                    "tail",
                                    query_string(deep_query::<fixture::Filter, _>(
                                        "filter",
                                        query_params::<String, _>(
                                            "tag",
                                            query_flag(
                                                "active",
                                                query_param::<u32, _>(
                                                    "limit",
                                                    header::<String, _>(
                                                        "x-note",
                                                        fragment::<String, _>(
                                                            "section anchor",
                                                            req_body::<
                                                                (Json,),
                                                                fixture::NewParity,
                                                                _,
                                                            >(
                                                                verb::<
                                                                    Post,
                                                                    201,
                                                                    (Json,),
                                                                    fixture::ParityItem,
                                                                >(
                                                                ),
                                                            ),
                                                        ),
                                                    ),
                                                ),
                                            ),
                                        ),
                                    )),
                                ),
                            ),
                        ),
                    ),
                ),
            ),
        )
    };
}

macro_rules! client_api {
    () => {
        servant::alt_all![
            create_api!(),
            path(
                "stream-sum",
                stream_body::<NetstringFraming, Json, u64, _>(verb::<
                    Post,
                    200,
                    (PlainText,),
                    String,
                >()),
            ),
            path("headers", get_with_headers::<(Json,), u32>()),
            path("raw", raw()),
            path("rawm", raw_m()),
        ]
    };
}

macro_rules! full_api {
    () => {
        alt(
            client_api!(),
            servant::alt_all![
                path("vault", vault(get::<(PlainText,), String>())),
                path("resource", with_resource::<u32, _>(get::<(Json,), u32>())),
                path(
                    "info",
                    is_secure(remote_host(http_version(get::<(PlainText,), String>())))
                ),
                path(
                    "auth",
                    auth_protect::<fixture::User, _>(get::<(PlainText,), String>())
                ),
                path(
                    "basic",
                    basic_auth::<fixture::User, _>("parity", get::<(PlainText,), String>())
                ),
                path("gone", no_content::<Delete>()),
            ],
        )
    };
}

macro_rules! full_handlers {
    () => {
        (
            (
                fixture::create_handler,
                (
                    fixture::stream_handler,
                    (
                        fixture::headers_handler,
                        (fixture::raw_handler, fixture::raw_m_handler),
                    ),
                ),
            ),
            (
                fixture::vault_handler,
                (
                    fixture::resource_handler,
                    (
                        fixture::info_handler,
                        (
                            fixture::auth_handler,
                            (fixture::basic_handler, fixture::gone_handler),
                        ),
                    ),
                ),
            ),
        )
    };
}

macro_rules! create_args {
    () => {
        servant::hlist![
            42u64,
            vec!["alpha".to_string(), "beta".to_string()],
            servant::query::Query::from_raw("seed=yes", vec![("seed".into(), Some("yes".into()))],),
            fixture::Filter {
                author: "Ada Lovelace".into(),
                year: 1843,
            },
            vec!["rust".to_string(), "servant".to_string()],
            true,
            Some(2u32),
            Some("noted".to_string()),
            fixture::NewParity {
                label: "new".into()
            },
        ]
    };
}

macro_rules! link_args {
    () => {
        servant::hlist![
            42u64,
            vec!["alpha".to_string(), "beta".to_string()],
            servant::query::Query::from_raw("seed=yes", vec![("seed".into(), Some("yes".into()))],),
            fixture::Filter {
                author: "Ada Lovelace".into(),
                year: 1843,
            },
            vec!["rust".to_string(), "servant".to_string()],
            true,
            Some(2u32),
            "details".to_string(),
        ]
    };
}
