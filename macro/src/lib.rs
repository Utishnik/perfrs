use std::fmt::{Debug, write};

use proc_macro::TokenStream;
use quote::*;
use syn::{
    parse::Parse, parse_macro_input, punctuated::Punctuated, Ident, ItemFn, Meta, ReturnType,
    Token, Type,
};

//TODO: 
/*
    #[measure(
        Config_Group(
            exclude_kernel => true,
            exclude_hv => true,
            pinned => true
        ),
        PerfGroup(
             "cycles" => Hardware::CPU_CYCLES,
             "branches" => Hardware::BRANCH_INSTRUCTIONS,
             "branch_misses" => Hardware::BRANCH_MISSES,
             "l1d_access" => L1D_ACCESS,
             "l1d_miss" => L1D_MISS,
        ),
    )]

 */
//tune версия
/*
    #[tune]
    fn tune_fn1(builder: &mut Builder){
        
    }

    #[tune_ret]
    fn tune_fn2(builder: &mut Builder) -> F
    where
         F: for<'b> Fn(&mut Builder<'b>) + 'static,
    {
    
    }
 */

#[derive(Default)]
struct PerfGroupAttribute {
    pub counter: Vec<Ident>,
}

#[derive(Default)]
struct PerfArgs {
    pub hardware: PerfGroupAttribute,
}

impl Parse for PerfArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut args = PerfArgs::default();

        while !input.is_empty() {
            let key: Ident = input.parse()?;

            match key.to_string().as_str() {
                "HardWare" | "Hardware" => {

                }
                _ => {}
            };
        }

        Ok(args)
    }
}

#[proc_macro_attribute]
pub fn measure(attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let attr2: proc_macro2::TokenStream = attr.into();

    //func.block.stmts.insert(index, element);
    let expd = quote::quote! {
        #func
    };
    TokenStream::from(expd)
}
