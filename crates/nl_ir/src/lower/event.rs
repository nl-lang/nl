//! `on Name [as 'handler'] do … end`: sugar for
//! `Event.on_run(name, handler_name, callback)`. The handler's tail value
//! is discarded; the expression's value is the handler name.
//!
//! ```lua
//! __rt.on("Game.Tick", "dotted", function(…) … end[, true])
//! ```
//!
//! The event name is the written name as a string. The handler name is the
//! string after `as`, or the canonical name of the file (the file
//! label, as messages give it). The trailing `true` marks an `async`
//! handler, which `rt.on` wraps (runtime/nl_62_event.lua).

use nl_sema::util::*;
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::Lowerer;
use super::func::FnSpec;
use crate::ir::Expr;

use SyntaxKind::*;

impl Lowerer<'_> {
    pub(crate) fn lower_on(&mut self, node: &SyntaxNode) -> Expr {
        let Some(q) = child(node, QUAL_NAME) else { return self.internal(node, "`on` without an event name") };
        let event = qual_text(&q);
        let handler = if has_token(node, AS_KW) {
            match token(node, STRING) {
                Some(t) => nl_syntax::decode_string(&[t.text()]).into_iter().next().unwrap_or_default(),
                None => return self.internal(node, "`on … as` without a handler name"),
            }
        } else {
            self.opts.file_label.as_bytes().to_vec()
        };
        let spec = FnSpec { name: format!("handler of {event}"), self_binding: None, discard_tail: true };
        let f = Expr::Function(Box::new(self.function(node, spec)));
        let mut args = vec![Expr::str(&event), Expr::Str(handler), f];
        if has_token(node, ASYNC_KW) {
            args.push(Expr::Bool(true));
        }
        Expr::intrinsic_call("on", args)
    }
}
