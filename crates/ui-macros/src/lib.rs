use proc_macro::TokenStream;
use proc_macro2::{TokenStream as Tokens,TokenTree};
use quote::quote;
fn translate(input:Tokens)->Tokens{
    input.into_iter().map(|token|match token{
        TokenTree::Group(g)=>{let mut n=proc_macro2::Group::new(g.delimiter(),translate(g.stream()));n.set_span(g.span());TokenTree::Group(n)}
        TokenTree::Literal(l)=>{
            if let Ok(s)=syn::parse_str::<syn::LitStr>(&l.to_string()){
                let pairs:Vec<(String,String)>=serde_json::from_str(include_str!("../../../src/locales/messages.json")).expect("messages");
                let original=s.value();
                let mut value=String::new();
                let mut rest=original.as_str();
                while !rest.is_empty(){
                    let (plain,tail)=rest.split_once('{').unwrap_or((rest,""));
                    let mut translated=plain.to_owned();
                    for (en,zh) in &pairs{translated=translated.replace(en,zh);}
                    value.push_str(&translated);
                    if tail.is_empty(){if rest.ends_with('{'){value.push('{');}break;}
                    value.push('{');
                    if let Some((field,after))=tail.split_once('}') {value.push_str(field);value.push('}');rest=after;}
                    else {value.push_str(tail);break;}
                }
                let v=syn::LitStr::new(&value,l.span());
                quote!(#v).into_iter().next().unwrap()
            }else{TokenTree::Literal(l)}
        }
        t=>t
    }).collect()
}
fn formatted(input:Tokens)->Tokens{
    let zh=translate(input.clone());
    quote!{if ::frp_sh::i18n::chinese(){format!(#zh)}else{format!(#input)}}
}
#[proc_macro]pub fn ui_format(input:TokenStream)->TokenStream{formatted(input.into()).into()}
#[proc_macro]pub fn ui_println(input:TokenStream)->TokenStream{
    let input:Tokens=input.into();let value=if input.is_empty(){quote!(String::new())}else{formatted(input)};
    quote!(::frp_sh::terminal::output(#value,true)).into()
}
#[proc_macro]pub fn ui_print(input:TokenStream)->TokenStream{
    let value=formatted(input.into());quote!(::frp_sh::terminal::output(#value,false)).into()
}
