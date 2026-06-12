//! Internal proc-macros for `uc-cli`.
//!
//! Currently provides the `#[autostop]` attribute, which wires up automatic
//! daemon shutdown for one-shot CLI commands.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::visit_mut::{self, VisitMut};
use syn::{parse2, parse_quote, ExprCall, Ident, ItemFn};

/// Apply `#[autostop]` to a CLI command function to automatically stop the
/// daemon on exit if (and only if) that command spawned it.
///
/// ## How it works
///
/// The macro performs two transformations on the function body:
///
/// 1. Prepends a binding `let mut __autostop_guard: Option<AutostopGuard> = None;`.
///    This slot is dropped when the function returns, running the stop logic.
///
/// 2. Rewrites every `ensure_local_daemon_running(...)` call in the body to
///    `ensure_local_daemon_running_capture(&mut __autostop_guard, ...)`. The
///    capture helper arms the guard whenever the daemon was spawned.
///
/// If the function body contains no `ensure_local_daemon_running` call the
/// macro emits a compile error — `#[autostop]` on a command that doesn't spawn
/// a daemon is almost certainly a bug.
///
/// ## Example
///
/// ```ignore
/// #[autostop]
/// pub async fn run_reset(json: bool, verbose: bool) -> i32 {
///     if let Err(e) = ensure_local_daemon_running().await {
///         return print_local_daemon_error(e);
///     }
///     // ... rest; daemon auto-stops here if we spawned it
/// }
/// ```
#[proc_macro_attribute]
pub fn autostop(_attr: TokenStream, item: TokenStream) -> TokenStream {
    match autostop_impl(item.into()) {
        Ok(ts) => ts.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Core logic of [`autostop`] exposed for unit testing.
fn autostop_impl(item: TokenStream2) -> Result<TokenStream2, syn::Error> {
    let mut func: ItemFn = parse2(item)?;

    // Pass 1: rewrite `ensure_local_daemon_running(...)` calls inside the body.
    let mut rewriter = EnsureCallRewriter {
        count: 0,
        slot_ident: Ident::new("__autostop_guard", Span::call_site()),
    };
    rewriter.visit_block_mut(&mut func.block);

    if rewriter.count == 0 {
        return Err(syn::Error::new_spanned(
            &func.sig.ident,
            "#[autostop] requires at least one `ensure_local_daemon_running` call in the function body; \
             use it only on commands that may spawn the daemon",
        ));
    }

    // Pass 2: prepend the guard slot binding to the body.
    let slot_decl: syn::Stmt = parse_quote! {
        let mut __autostop_guard: ::std::option::Option<crate::autostop::AutostopGuard> = ::std::option::Option::None;
    };
    func.block.stmts.insert(0, slot_decl);

    Ok(quote!(#func))
}

/// AST visitor that rewrites `ensure_local_daemon_running(ARGS)` into
/// `ensure_local_daemon_running_capture(&mut __autostop_guard, ARGS)`.
///
/// We match on the *last* segment of the call path so both the bare ident
/// (`ensure_local_daemon_running()`) and the qualified form
/// (`crate::local_daemon::ensure_local_daemon_running()`) are handled.
struct EnsureCallRewriter {
    count: usize,
    slot_ident: Ident,
}

impl VisitMut for EnsureCallRewriter {
    fn visit_expr_call_mut(&mut self, call: &mut ExprCall) {
        // Recurse first so nested calls are also rewritten.
        visit_mut::visit_expr_call_mut(self, call);

        let path = match call.func.as_mut() {
            syn::Expr::Path(p) => p,
            _ => return,
        };

        let last_segment = match path.path.segments.last_mut() {
            Some(s) => s,
            None => return,
        };

        if last_segment.ident != "ensure_local_daemon_running" {
            return;
        }

        // Rename to the capture helper.
        last_segment.ident = Ident::new(
            "ensure_local_daemon_running_capture",
            last_segment.ident.span(),
        );

        // Prepend `&mut __autostop_guard` as the first argument.
        let slot = &self.slot_ident;
        let new_arg: syn::Expr = parse_quote!(&mut #slot);
        let mut new_args = syn::punctuated::Punctuated::new();
        new_args.push(new_arg);
        for existing in std::mem::take(&mut call.args) {
            new_args.push(existing);
        }
        call.args = new_args;

        self.count += 1;
    }
}
