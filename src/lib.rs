extern crate proc_macro;
use std::collections::HashMap;

use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{parse_macro_input, parse_quote, FnArg, ImplItem, ItemImpl, ItemMod, ItemStruct, Pat};

#[proc_macro_attribute]
pub fn generate_userdata(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemMod);

    // Extract the module name and content
    // let mod_name = &input.ident;
    let mut struct_data_map = HashMap::new();
    let content = match &mut input.content {
        Some((_, items)) => items,
        None => {
            return syn::Error::new_spanned(&input, "Expected module content")
                .to_compile_error()
                .into();
        }
    };

    for item in content.iter() {
        if let syn::Item::Struct(s) = item {
            let struct_data = struct_data_map.entry(s.ident.to_string()).or_default();
            generate_from_struct_def(struct_data, s)
        } else if let syn::Item::Impl(i) = item {
            let struct_data = struct_data_map
                .entry(i.self_ty.to_token_stream().to_string())
                .or_default();
            generate_from_impl(struct_data, i)
        }
    }

    content.insert(
        0,
        parse_quote! {
            use mlua::{UserData, UserDataMethods, UserDataFields, MetaMethod};
        },
    );

    for (struct_name, struct_data) in struct_data_map.drain() {
        let StructData {
            pub_fields,
            methods,
            accessors,
            functions,
            free_funcs,
        } = struct_data;
        let struct_name = format_ident!("{struct_name}");
        content.push(parse_quote! {
            impl UserData for #struct_name {
                fn add_fields<'lua, F: UserDataFields<'lua, Self>>(fields: &mut F) {
                    #( #accessors )*
                    #( #pub_fields )*
                }

                fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
                    #( #methods )*
                    #( #functions )*
                }
            }
        });
        content.push(parse_quote! {
            impl #struct_name {
                pub fn free_functions_table(lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
                    let exports = lua.create_table()?;
                    #( #free_funcs )*
                    Ok(exports)
                }
            }
        });
    }

    TokenStream::from(quote! { #input })
}

fn generate_from_struct_def(struct_data: &mut StructData, struct_item: &ItemStruct) {
    struct_data
        .pub_fields
        .extend(struct_item.fields.iter().filter_map(|f| {
            if matches!(f.vis, syn::Visibility::Public(_)) {
                let field_name = f.ident.as_ref().unwrap();
                let field_name_str = field_name.to_string();
                Some(quote! {
                    fields.add_field_method_get(#field_name_str, |_, this| {
                        Ok(this.#field_name.clone())
                    });

                    fields.add_field_method_set(#field_name_str, |_, this, value| {
                        this.#field_name = value;
                        Ok(())
                    });
                })
            } else {
                None
            }
        }));
}

#[derive(Default)]
struct StructData {
    pub pub_fields: Vec<proc_macro2::TokenStream>,
    pub methods: Vec<proc_macro2::TokenStream>,
    pub accessors: Vec<proc_macro2::TokenStream>,
    pub functions: Vec<proc_macro2::TokenStream>,
    pub free_funcs: Vec<proc_macro2::TokenStream>,
}

fn generate_from_impl(
    StructData {
        pub_fields: _,
        methods,
        accessors,
        functions,
        free_funcs,
    }: &mut StructData,
    impl_item: &ItemImpl,
) {
    let struct_name = &impl_item.self_ty;

    for item in &impl_item.items {
        if let ImplItem::Fn(method) = item {
            let method_name = &method.sig.ident;
            let method_name_str = method_name.to_string();
            let is_self_method = method
                .sig
                .inputs
                .iter()
                .any(|arg| matches!(arg, FnArg::Receiver(_)));
            let is_mut = method
                .sig
                .inputs
                .iter()
                .any(|arg| matches!(arg, FnArg::Receiver(rec) if rec.mutability.is_some()));

            let mut arg_types = Vec::new();
            let mut arg_names = Vec::new();
            for arg in method
                .sig
                .inputs
                .iter()
                .skip(if is_self_method { 1 } else { 0 })
            {
                if let FnArg::Typed(pat_type) = arg {
                    if let Pat::Ident(pat_ident) = &*pat_type.pat {
                        let arg_name = &pat_ident.ident;
                        let arg_type = &pat_type.ty;
                        arg_names.push(arg_name);
                        arg_types.push(arg_type);
                    }
                }
            }

            let args_param = if arg_names.is_empty() {
                quote!(_: mlua::MultiValue)
            } else {
                quote! { (#(#arg_names),*): (#(#arg_types),*) }
            };

            if method_name_str.starts_with("get_") {
                let field_name = method_name_str.trim_start_matches("get_");
                accessors.push(quote! {
                    fields.add_field_method_get(#field_name, |_, this| {
                        let result = this.#method_name();
                        Ok(result)
                    });
                });
            } else if method_name_str.starts_with("set_") {
                let field_name = method_name_str.trim_start_matches("set_");
                accessors.push(quote! {
                    fields.add_field_method_set(#field_name, |_, this, value| {
                        this.#method_name(value);
                        Ok(())
                    });
                });
            } else if is_self_method {
                let method_impl = if is_mut {
                    quote! {
                        methods.add_method_mut(#method_name_str, |_, this, #args_param| {
                            Ok(this.#method_name(#( #arg_names ),*))
                        });
                    }
                } else {
                    quote! {
                        methods.add_method(#method_name_str, |_, this, #args_param| {
                            Ok(this.#method_name(#( #arg_names ),*))
                        });
                    }
                };
                methods.push(method_impl);
            } else {
                functions.push(quote! {
                    methods.add_function(#method_name_str, |_, #args_param| {
                        Ok(#struct_name::#method_name(#( #arg_names ),*))
                    });
                });
                free_funcs.push(quote! {
                    exports.set(#method_name_str, lua.create_function(|_, #args_param| {
                        Ok(#struct_name::#method_name(#( #arg_names ),*))
                    })?)?;
                });
            }
        }
    }
}
