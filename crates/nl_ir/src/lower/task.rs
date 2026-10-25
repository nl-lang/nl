//! `async fn` and `async do`: a function that, when called,
//! runs its body as a new task and returns the `Task`.
//!
//! The function is lowered as usual and wrapped: `rt.async(function … end)`
//! returns the function that starts the task (runtime/nl_60_task.lua).
//! `.await` is an ordinary method of tasks, so it needs nothing here: it
//! works at any call depth because the task's body runs in a coroutine.
//! Wrapping happens before a method is marked or a name registered, so
//! those apply to the function that starts the task.

use super::Lowerer;
use crate::ir::Expr;

impl Lowerer<'_> {
    /// Makes the lowered function `f` async.
    pub(crate) fn wrap_async(&mut self, f: Expr) -> Expr {
        Expr::intrinsic_call("async", vec![f])
    }
}
