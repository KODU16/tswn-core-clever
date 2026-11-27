//! 自动包装机制方法并生成同源重放入口，技能公式仍只维护在原函数里。
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{FnArg, ImplItem, ItemImpl, Pat, Type, parse_macro_input};

#[proc_macro_attribute]
pub fn capture_skills(attr: TokenStream, input: TokenStream) -> TokenStream {
    let module = parse_macro_input!(attr as syn::Ident);
    let mut implementation = parse_macro_input!(input as ItemImpl);
    let dispatcher = format_ident!("replay_skill_{}", module);
    let mut arms = Vec::new();
    for item in &mut implementation.items {
        let ImplItem::Fn(method) = item else { continue };
        if method.sig.ident.to_string().starts_with("replay_skill_") {
            continue;
        }
        let Some(FnArg::Receiver(receiver)) = method.sig.inputs.first() else {
            continue;
        };
        if receiver.mutability.is_none() {
            continue;
        }
        let mut names = Vec::new();
        let mut types = Vec::new();
        let mut update = None;
        let mut call_arguments = Vec::new();
        let mut valid = true;
        for arg in method.sig.inputs.iter().skip(1) {
            let FnArg::Typed(arg) = arg else {
                valid = false;
                break;
            };
            let Pat::Ident(name) = arg.pat.as_ref() else {
                valid = false;
                break;
            };
            let ty = quote!(#arg).to_string();
            if ty.contains("RunUpdates") {
                update = Some(name.ident.clone());
                call_arguments.push(quote!(updates));
                continue;
            }
            let argument_name = &name.ident;
            call_arguments.push(quote!(#argument_name));
            // 值参数按 serde 通用保存，不为技能或参数类型维护白名单。
            // 可变借用属于防御上下文等执行器内部数据，不作为独立技能入口。
            if !matches!(arg.ty.as_ref(), Type::Path(_) | Type::Array(_)) {
                valid = false;
                break;
            }
            names.push(name.ident.clone());
            types.push(arg.ty.clone());
        }
        let Some(update) = update else { continue };
        if !valid || names.is_empty() {
            continue;
        }
        let actor = &names[0];
        let first_type = &types[0];
        if quote!(#first_type).to_string() != "EntityIdx" {
            continue;
        }
        let name = &method.sig.ident;
        let entry = format!("{}::{}", module, name);
        let body = &method.block;
        method.block = syn::parse_quote!({
            let __capture = self.begin_skill_capture(#entry, #actor, || vec![#(serde_json::to_string(&#names).expect("技能参数必须可序列化")),*], #update);
            let __result = (|| #body)();
            self.end_skill_capture(__capture, #update);
            __result
        });
        let decoders = names.iter().zip(&types).enumerate().map(|(i, (n, t))| {
            let text = quote!(#t).to_string().replace(' ', "");
            if i > 0 && text == "EntityIdx" {
                quote! { let #n: #t = target; }
            } else if i > 0 && matches!(text.as_str(), "Vec<EntityIdx>" | "PreparedTargetList") {
                quote! { let #n: #t = std::iter::once(target).collect(); }
            } else if i > 0 && text == "Option<EntityIdx>" {
                quote! { let #n: #t = Some(target); }
            } else if i > 0 && matches!(text.as_str(), "PreparedBuiltinSkillAction" | "PreparedBossSkillAction") {
                quote! {
                    let mut #n: #t = serde_json::from_str(&call.arguments[#i]).expect("已捕获的技能参数必须可读");
                    #n.targets = std::iter::once(target).collect();
                }
            } else if i == 0 {
                quote! { let #n: #t = call.caster; }
            } else {
                quote! { let #n: #t = serde_json::from_str(&call.arguments[#i]).expect("已捕获的技能参数必须可读"); }
            }
        });
        let cfg: Vec<_> = method.attrs.iter().filter(|a| a.path().is_ident("cfg")).collect();
        arms.push(quote! { #(#cfg)* #entry => { #(#decoders)* self.#name(#(#call_arguments),*); true } });
    }
    let generated: ImplItem = syn::parse_quote! {
        /// 从统一执行凭据重放原函数，实体目标替换为本次食客。
        pub(crate) fn #dispatcher(&mut self, call: &crate::runtime::skill_call::SkillCall, target: EntityIdx, updates: &mut RunUpdates) -> bool {
            match call.entry.as_str() { #(#arms,)* _ => false }
        }
    };
    implementation.items.push(generated);
    quote!(#implementation).into()
}
