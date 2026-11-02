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

extern crate proc_macro;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{Attribute, FnArg, ItemMod, LitStr, Pat, Type, parse_macro_input};

#[proc_macro_attribute]
pub fn step_module(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemMod);
    let mod_name = &input.ident;
    let mod_vis = &input.vis;

    let Some((_, items)) = &input.content else {
        panic!(
            "step_module expected a module declaration with a body (e.g., `pub mod steps {{ ... }}`)"
        );
    };

    let mut wrappers = Vec::new();
    let mut registrations = Vec::new();
    let mut world_type = None;

    // Iterate over items in the module
    for item in items {
        if let syn::Item::Fn(item_fn) = item {
            if let Some(pattern) = extract_step_pattern(&item_fn.attrs) {
                let fn_name = &item_fn.sig.ident;
                let wrapper_name = format_ident!("{}_wrapper", fn_name);

                // Infer world type from first argument if not yet found
                if world_type.is_none() {
                    if let Some(FnArg::Typed(pat_type)) = item_fn.sig.inputs.first() {
                        if let Type::Reference(type_ref) = &*pat_type.ty {
                            if type_ref.mutability.is_some() {
                                world_type = Some((*type_ref.elem).clone());
                            }
                        }
                    }
                }

                // Generate argument parsing logic
                let mut args_parsing = Vec::new();
                let mut method_call_args = Vec::new();
                let mut arg_index = 0;

                for (i, arg) in item_fn.sig.inputs.iter().enumerate() {
                    if i == 0 {
                        method_call_args.push(quote! { w });
                        continue;
                    }

                    if let FnArg::Typed(pat_type) = arg {
                        if let Pat::Ident(_) = &*pat_type.pat {
                            let ty = &pat_type.ty;
                            let arg_ident = format_ident!("arg{}", arg_index);
                            let arg_idx_lit = syn::Index::from(arg_index);

                            let last_segment = if let syn::Type::Path(type_path) = &**ty {
                                type_path.path.segments.last().map(|s| s.ident.to_string())
                            } else {
                                None
                            };

                            if last_segment.as_deref() == Some("DataTable") {
                                args_parsing.push(quote! {
                                    let #arg_ident = ctx.table.clone().expect("Step expects a Data Table but none was provided");
                                });
                            } else if last_segment.as_deref() == Some("String") {
                                args_parsing.push(quote! {
                                    let #arg_ident = args[#arg_idx_lit].clone();
                                });
                                arg_index += 1;
                            } else {
                                let err_msg = format!("Failed to parse argument {}", arg_index);
                                args_parsing.push(quote! {
                                    let #arg_ident = args[#arg_idx_lit].parse::<#ty>().expect(#err_msg);
                                });
                                arg_index += 1;
                            }
                            method_call_args.push(quote! { #arg_ident });
                        }
                    }
                }

                let world_t = world_type.clone().expect("Could not infer World type");

                let return_type = &item_fn.sig.output;

                let wrapper_body = match return_type {
                    syn::ReturnType::Default => quote! {
                        Box::pin(async move {
                            #(#args_parsing)*
                            #fn_name(#(#method_call_args),*).await;
                            Ok(())
                        })
                    },
                    // NOTE: We assume that if a return type is specified, it is `Result<()>`.
                    // If it is another type, the generated code will fail to compile when
                    // assigned to the `AsyncStep` trait object.
                    syn::ReturnType::Type(_, _) => quote! {
                        Box::pin(async move {
                            #(#args_parsing)*
                            #fn_name(#(#method_call_args),*).await
                        })
                    },
                };

                wrappers.push(quote! {
                    fn #wrapper_name(w: &mut #world_t, #[allow(unused_variables)] args: Vec<String>, #[allow(unused_variables)] ctx: ::features::StepContext) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + '_>> {
                        #wrapper_body
                    }
                });

                let pattern_lit = LitStr::new(&pattern, proc_macro2::Span::call_site());

                registrations.push(quote! {
                    c.register(#pattern_lit, #wrapper_name);
                });
            }
        }
    }

    let world_type = world_type.expect("Could not infer World type");

    let expanded = quote! {
        #mod_vis mod #mod_name {
            #(#items)*

            #(#wrappers)*

            pub fn register_steps(c: &mut ::features::Features<#world_type>) {
                #(#registrations)*
            }
        }
    };

    TokenStream::from(expanded)
}

fn extract_step_pattern(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("step") {
            if let Ok(lit) = attr.parse_args::<LitStr>() {
                return Some(lit.value());
            }
        }
    }
    None
}

/// Marker attribute for individual step functions.
#[proc_macro_attribute]
pub fn step(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
