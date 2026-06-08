//! Derive macros for servant-rs.
//!
//! - [`NamedApi`] — turn a struct of endpoint fields into an `Alt` tree plus a
//!   name-keyed handler record, so routes and handlers are bound by field name
//!   (order-independent) rather than by manual `alt(...)`/tuple nesting. This is
//!   the record-routes analogue of Servant's `NamedRoutes`.
//! - [`ToSchema`] — derive an OpenAPI object schema for a struct from its
//!   fields (used by `servant-openapi`).

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, GenericArgument, PathArguments, Type, parse_macro_input};

/// Derive `into_api()` (a right-nested `Alt` of the fields, in declaration
/// order) and a companion `<Name>Handlers` record with `into_handlers()` (the
/// matching right-nested handler tuple).
///
/// ```ignore
/// #[derive(NamedApi)]
/// struct Api {
///     get_user: Path<Capture<u64, Strict, Verb<Get, 200, (Json,), User>>>,
///     list:     Path<Verb<Get, 200, (Json,), Vec<User>>>,
/// }
/// // serve(Api { get_user, list }.into_api(),
/// //       ApiHandlers { get_user: h1, list: h2 }.into_handlers())
/// ```
#[proc_macro_derive(NamedApi)]
pub fn derive_named_api(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let vis = &input.vis;

    let fields = match named_fields(&input.data) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error().into(),
    };
    if fields.is_empty() {
        return syn::Error::new_spanned(name, "NamedApi requires at least one field")
            .to_compile_error()
            .into();
    }

    let idents: Vec<_> = fields.iter().map(|f| f.ident.clone().unwrap()).collect();
    let types: Vec<_> = fields.iter().map(|f| f.ty.clone()).collect();

    // Right-nested Alt type + value over the endpoint fields.
    let api_ty = nest_type(
        &types,
        |t| quote!(#t),
        |l, r| quote!(::servant::api::Alt<#l, #r>),
    );
    let api_val = nest_expr(
        &idents,
        |id| quote!(self.#id),
        |l, r| quote!(::servant::api::alt(#l, #r)),
    );

    // Companion handlers record: one generic handler type per field.
    let handlers_name = format_ident!("{}Handlers", name);
    let generics: Vec<_> = idents
        .iter()
        .map(|id| format_ident!("H{}", to_pascal(&id.to_string())))
        .collect();
    let handler_ty = nest_type(&generics, |g| quote!(#g), |l, r| quote!((#l, #r)));
    let handler_val = nest_expr(&idents, |id| quote!(self.#id), |l, r| quote!((#l, #r)));

    let expanded = quote! {
        impl #name {
            /// Convert the route record into a left-biased `Alt` tree
            /// (declaration order = precedence).
            #vis fn into_api(self) -> #api_ty {
                #api_val
            }
        }

        #[doc = concat!("Handlers for [`", stringify!(#name), "`], keyed by route name.")]
        #vis struct #handlers_name<#(#generics),*> {
            #( #vis #idents: #generics, )*
        }

        impl<#(#generics),*> #handlers_name<#(#generics),*> {
            /// Convert the handler record into the tuple `into_api`'s router expects.
            #vis fn into_handlers(self) -> #handler_ty {
                #handler_val
            }
        }
    };
    expanded.into()
}

/// Derive an OpenAPI object schema for a struct (for `servant-openapi`).
/// Each field becomes a property (its schema from its Rust type name); non-`Option`
/// fields are `required`.
#[proc_macro_derive(ToSchema)]
pub fn derive_to_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_g, ty_g, where_g) = input.generics.split_for_impl();

    let fields = match named_fields(&input.data) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error().into(),
    };

    let mut props = Vec::new();
    let mut required = Vec::new();
    for f in &fields {
        let ident = f.ident.as_ref().unwrap();
        let field_name = ident.to_string();
        match option_inner(&f.ty) {
            Some(inner) => {
                props.push(quote! {
                    map.insert(
                        #field_name.to_string(),
                        ::servant_openapi::schema_for(::std::any::type_name::<#inner>()),
                    );
                });
            }
            None => {
                let ty = &f.ty;
                props.push(quote! {
                    map.insert(
                        #field_name.to_string(),
                        ::servant_openapi::schema_for(::std::any::type_name::<#ty>()),
                    );
                });
                required.push(quote!(#field_name));
            }
        }
    }

    let expanded = quote! {
        impl #impl_g ::servant_openapi::ToSchema for #name #ty_g #where_g {
            fn schema() -> ::serde_json::Value {
                let mut map = ::serde_json::Map::new();
                #( #props )*
                ::serde_json::json!({
                    "type": "object",
                    "properties": ::serde_json::Value::Object(map),
                    "required": [ #( #required ),* ],
                })
            }
        }
    };
    expanded.into()
}

// --- helpers ---

fn named_fields(data: &Data) -> Result<Vec<syn::Field>, syn::Error> {
    match data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => Ok(named.named.iter().cloned().collect()),
            _ => Err(syn::Error::new_spanned(
                &s.fields,
                "expected a struct with named fields",
            )),
        },
        _ => Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "expected a struct",
        )),
    }
}

/// `get_user` -> `GetUser` (for generating camel-case generic parameter names).
fn to_pascal(s: &str) -> String {
    s.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// `Option<T>` -> `Some(T)`, else `None`.
fn option_inner(ty: &Type) -> Option<&Type> {
    if let Type::Path(tp) = ty {
        let seg = tp.path.segments.last()?;
        if seg.ident == "Option" {
            if let PathArguments::AngleBracketed(args) = &seg.arguments {
                if let Some(GenericArgument::Type(inner)) = args.args.first() {
                    return Some(inner);
                }
            }
        }
    }
    None
}

/// Right-nest a list of tokens via `combine`, mapping each leaf via `leaf`.
fn nest_type<T>(
    items: &[T],
    leaf: impl Fn(&T) -> proc_macro2::TokenStream + Copy,
    combine: impl Fn(proc_macro2::TokenStream, proc_macro2::TokenStream) -> proc_macro2::TokenStream
    + Copy,
) -> proc_macro2::TokenStream {
    match items {
        [] => quote!(()),
        [one] => leaf(one),
        [head, rest @ ..] => {
            let h = leaf(head);
            let t = nest_type(rest, leaf, combine);
            combine(h, t)
        }
    }
}

fn nest_expr<T>(
    items: &[T],
    leaf: impl Fn(&T) -> proc_macro2::TokenStream + Copy,
    combine: impl Fn(proc_macro2::TokenStream, proc_macro2::TokenStream) -> proc_macro2::TokenStream
    + Copy,
) -> proc_macro2::TokenStream {
    match items {
        [] => quote!(()),
        [one] => leaf(one),
        [head, rest @ ..] => {
            let h = leaf(head);
            let t = nest_expr(rest, leaf, combine);
            combine(h, t)
        }
    }
}
