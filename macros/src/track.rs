use super::*;

/// Make a type trackable.
pub fn expand(block: &syn::ItemImpl) -> Result<proc_macro2::TokenStream> {
    let ty = &block.self_ty;

    // Extract and validate the methods.
    let mut methods = vec![];
    for item in &block.items {
        methods.push(method(&item)?);
    }

    let tracked_fields = methods.iter().map(|method| {
        let name = &method.sig.ident;
        let ty = match &method.sig.output {
            syn::ReturnType::Default => unreachable!(),
            syn::ReturnType::Type(_, ty) => ty.as_ref(),
        };
        quote! { #name: ::comemo::internal::AccessTracker<#ty>, }
    });

    let tracked_methods = methods.iter().map(|method| {
        let name = &method.sig.ident;
        let mut method = (*method).clone();
        if matches!(method.vis, syn::Visibility::Inherited) {
            method.vis = parse_quote! { pub(super) };
        }
        method.block = parse_quote! { {
            let (inner, tracker) = ::comemo::internal::to_parts(self.0);
            let output = inner.#name();
            if let Some(tracker) = &tracker {
                tracker.#name.track(&output);
            }
            output
        } };
        method
    });

    let tracked_valids = methods.iter().map(|method| {
        let name = &method.sig.ident;
        quote! {
            tracker.#name.valid(&self.#name())
        }
    });

    let track_impl = quote! {
        use super::*;

        impl<'a> ::comemo::Track<'a> for #ty {}
        impl<'a> ::comemo::internal::Trackable<'a> for #ty {
            type Tracker = Tracker;
            type Surface = Surface<'a>;

            fn valid(&self, tracker: &Self::Tracker) -> bool {
                #(#tracked_valids)&&*
            }

            fn surface<'s>(tracked: &'s Tracked<'a, #ty>) -> &'s Self::Surface
            where
                Self: Track<'a>,
            {
                // Safety: Surface is repr(transparent).
                unsafe { &*(tracked as *const _ as *const Self::Surface) }
            }
        }

        #[repr(transparent)]
        pub struct Surface<'a>(Tracked<'a, #ty>);

        impl Surface<'_> {
            #(#tracked_methods)*
        }

        #[derive(Default)]
        pub struct Tracker {
            #(#tracked_fields)*
        }
    };

    Ok(quote! {
        #block
        const _: () = { mod private { #track_impl } };
    })
}

/// Extract and validate a method.
fn method(item: &syn::ImplItem) -> Result<&syn::ImplItemMethod> {
    let method = match item {
        syn::ImplItem::Method(method) => method,
        _ => bail!(item, "only methods are supported"),
    };

    match method.vis {
        syn::Visibility::Inherited => {}
        syn::Visibility::Public(_) => {}
        _ => bail!(method.vis, "only private and public methods are supported"),
    }

    let mut inputs = method.sig.inputs.iter();
    let receiver = match inputs.next() {
        Some(syn::FnArg::Receiver(recv)) => recv,
        _ => bail!(method, "method must take self"),
    };

    if receiver.reference.is_none() || receiver.mutability.is_some() {
        bail!(receiver, "must take self by shared reference");
    }

    if inputs.next().is_some() {
        bail!(
            method.sig,
            "currently, only methods without extra arguments are supported"
        );
    }

    let output = &method.sig.output;
    match output {
        syn::ReturnType::Default => {
            bail!(method.sig, "method must have a return type")
        }
        syn::ReturnType::Type(..) => {}
    }

    Ok(method)
}
