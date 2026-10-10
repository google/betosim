// Copyright 2026 The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident, Type, parse_macro_input};

#[proc_macro_derive(ParsableEnum)]
pub fn parsable_enum_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let variants = match &input.data {
        Data::Enum(data) => &data.variants,
        _ => panic!("#[derive(ParsableEnum)] is only supported for enums"),
    };

    let mut match_arms = Vec::new();
    let mut next_discriminant = 0u8;

    for variant in variants {
        let v_name = &variant.ident;
        let val =
            if let Some((_, syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(lit_int), .. }))) =
                &variant.discriminant
            {
                let parsed_val: u8 = lit_int.base10_parse().expect("Discriminant must be u8");
                next_discriminant = parsed_val.checked_add(1).unwrap_or(0);
                parsed_val
            } else {
                let current = next_discriminant;
                next_discriminant = next_discriminant.checked_add(1).unwrap_or(0);
                current
            };

        match_arms.push(quote! {
            #val => Ok((input, Self::#v_name)),
        });
    }

    let expanded = quote! {
        impl<'a> crate::types::Parsable<'a> for #name {
            fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
                let (input, val) = <u8 as crate::types::Parsable>::parse(input)?;
                match val {
                    #(#match_arms)*
                    _ => Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::MapRes))),
                }
            }
        }
    };

    TokenStream::from(expanded)
}

#[proc_macro_derive(CommandParser, attributes(command, parser))]
pub fn command_parser_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let has_lifetime = !input.generics.lifetimes().collect::<Vec<_>>().is_empty();

    let variants = match &input.data {
        Data::Enum(data) => &data.variants,
        _ => panic!("#[derive(CommandParser)] is only supported for enums"),
    };

    let mut parser_fns = Vec::new();
    for variant in variants {
        let variant_name = &variant.ident;
        let parser_fn_name = Ident::new(
            &format!("parse_{}", variant_name.to_string().to_lowercase()),
            variant_name.span(),
        );

        let tag_attr = variant
            .attrs
            .iter()
            .find(|a| a.path().is_ident("command"))
            .expect("All variants must have a #[command(tag = ...)] attribute");
        let tag = if let syn::Meta::List(list) = &tag_attr.meta {
            let nested =
                list.parse_args::<syn::MetaNameValue>().expect("Failed to parse nested meta");
            if let syn::Expr::Lit(expr_lit) = nested.value {
                if let syn::Lit::Str(lit_str) = expr_lit.lit {
                    let b = syn::LitByteStr::new(lit_str.value().as_bytes(), lit_str.span());
                    quote! { #b }
                } else {
                    panic!("command attribute value must be a string literal");
                }
            } else {
                panic!("command attribute value must be a literal expression");
            }
        } else {
            panic!("command attribute must be a list");
        };

        let (field_names, field_parsers) = match &variant.fields {
            Fields::Unnamed(fields) => {
                let mut names = Vec::new();
                let mut parsers = Vec::new();
                for (i, field) in fields.unnamed.iter().enumerate() {
                    let field_name = Ident::new(&format!("field_{i}"), variant_name.span());
                    names.push(quote! { #field_name });
                    parsers.push(generate_field_parser(&field_name, field));
                }
                (quote! { ( #(#names),* ) }, parsers)
            }
            Fields::Named(fields) => {
                let mut names = Vec::new();
                let mut parsers = Vec::new();
                for field in &fields.named {
                    let field_name = field.ident.as_ref().unwrap();
                    names.push(quote! { #field_name });
                    parsers.push(generate_field_parser(field_name, field));
                }
                (quote! { { #(#names),* } }, parsers)
            }
            Fields::Unit => (quote! {}, vec![]),
        };

        let parser_fn = if has_lifetime {
            quote! {
                fn #parser_fn_name(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
                    let (input, _) = nom::bytes::complete::tag(#tag)(input)?;
                    #(#field_parsers)*
                    Ok((input, #name::#variant_name #field_names))
                }
            }
        } else {
            quote! {
                fn #parser_fn_name(input: &[u8]) -> nom::IResult<&[u8], Self> {
                    let (input, _) = nom::bytes::complete::tag(#tag)(input)?;
                    #(#field_parsers)*
                    Ok((input, #name::#variant_name #field_names))
                }
            }
        };
        parser_fns.push(parser_fn);
    }

    let parser_fn_names = variants.iter().map(|v| {
        let variant_name = &v.ident;
        Ident::new(
            &format!("parse_{}", variant_name.to_string().to_lowercase()),
            variant_name.span(),
        )
    });

    let impl_block = if has_lifetime {
        quote! {
            impl<'a> #name<'a> {
                #(#parser_fns)*

                pub fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
                    #(
                        if let Ok(result) = Self::#parser_fn_names(input) {
                            return Ok(result);
                        }
                    )*
                    Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Alt)))
                }
            }
        }
    } else {
        quote! {
            impl #name {
                #(#parser_fns)*

                pub fn parse(input: &[u8]) -> nom::IResult<&[u8], Self> {
                    #(
                        if let Ok(result) = Self::#parser_fn_names(input) {
                            return Ok(result);
                        }
                    )*
                    Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Alt)))
                }
            }
        }
    };

    TokenStream::from(impl_block)
}

fn generate_field_parser(field_name: &Ident, field: &syn::Field) -> proc_macro2::TokenStream {
    let field_type = &field.ty;

    let parse_field =
        if let Some(parser_attr) = field.attrs.iter().find(|a| a.path().is_ident("parser")) {
            let parser: syn::Expr = parser_attr.parse_args().unwrap();
            quote! { let (input, #field_name) = #parser(input)?; }
        } else if let Some(inner_ty) = get_option_inner_type(field_type) {
            // Slot-aligned parsing: an optional field is None only when its comma- or
            // semicolon-delimited slot is empty. Non-empty tokens that fail to
            // parse trigger a syntax error.
            quote! {
                let (input, #field_name) = if input.is_empty()
                    || input.starts_with(b",")
                    || input.starts_with(b"\r")
                    || input.starts_with(b"\n")
                    || input.starts_with(b";")
                {
                    (input, None)
                } else {
                    let (input, v) = <#inner_ty as Parsable>::parse(input)?;
                    (input, Some(v))
                };
            }
        } else {
            quote! {
                let (input, #field_name) = <#field_type as Parsable>::parse(input)?;
            }
        };

    quote! {
        #parse_field
        let (input, _) = nom::combinator::opt(nom::bytes::complete::tag(b","))(input)?;
    }
}

fn get_option_inner_type(ty: &Type) -> Option<&Type> {
    let Type::Path(type_path) = ty else { return None };
    if type_path.path.segments.len() != 1 {
        return None;
    }
    let seg = &type_path.path.segments[0];
    if seg.ident != "Option" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else { return None };
    if args.args.len() != 1 {
        return None;
    }
    match &args.args[0] {
        syn::GenericArgument::Type(inner_ty) => Some(inner_ty),
        _ => None,
    }
}
