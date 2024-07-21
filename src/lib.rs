extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, FnArg, ImplItem, ItemImpl, ItemMod, ItemStruct, Pat};

#[proc_macro_attribute]
pub fn generate_userdata(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemMod);

    // Extract the module name and content
    let mod_name = &input.ident;
    let content = match &input.content {
        Some((_, items)) => items,
        None => {
            return syn::Error::new_spanned(&input, "Expected module content")
                .to_compile_error()
                .into();
        }
    };

    // Find the struct and its impl block
    let mut struct_item: Option<ItemStruct> = None;
    let mut impl_item: Option<ItemImpl> = None;

    for item in content {
        if let syn::Item::Struct(s) = item {
            struct_item = Some(s.clone());
        } else if let syn::Item::Impl(i) = item {
            impl_item = Some(i.clone());
        }
    }

    let struct_item = match struct_item {
        Some(s) => s,
        None => {
            return syn::Error::new_spanned(&input, "Expected a struct definition")
                .to_compile_error()
                .into();
        }
    };

    let impl_item = match impl_item {
        Some(i) => i,
        None => {
            return syn::Error::new_spanned(&input, "Expected an impl block")
                .to_compile_error()
                .into();
        }
    };

    let struct_name = &struct_item.ident;
    let visibility = &struct_item.vis;

    // Generate field accessors for pub fields
    let pub_fields = struct_item.fields.iter().filter_map(|f| {
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
    });

    // Generate UserData implementation
    let mut methods = Vec::new();
    let mut accessors = Vec::new();
    let mut functions = Vec::new();
    let mut free_funcs = Vec::new();

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
                            this.#method_name(#( #arg_names ),*);
                            Ok(())
                        });
                    }
                } else {
                    quote! {
                        methods.add_method(#method_name_str, |_, this, #args_param| {
                            this.#method_name(#( #arg_names ),*);
                            Ok(())
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

    // // Generate FromLua implementation
    // let from_lua_impl = quote! {
    //     impl<'lua> mlua::FromLua<'lua> for #struct_name {
    //         fn from_lua(lua_value: mlua::Value<'lua>, lua: &'lua mlua::Lua) -> mlua::Result<Self> {
    //             use mlua::LuaSerdeExt;
    //             let value: Self = lua.from_value(lua_value)?;
    //             Ok(value)
    //         }
    //     }
    // };

    let expanded = quote! {
        #visibility mod #mod_name {
            use super::*;
            use mlua::{UserData, UserDataMethods, UserDataFields, MetaMethod, FromLua};

            // #[derive(Debug, Clone, FromLua)]
            #[derive(FromLua)]
            #struct_item

            #impl_item

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

            impl #struct_name {
                pub fn free_functions_table(lua: &mlua::Lua) -> mlua::Result<mlua::Table> {
                    let exports = lua.create_table()?;
                    #( #free_funcs )*
                    Ok(exports)
                }
            }

            // #from_lua_impl
        }
    };

    TokenStream::from(expanded)
}
