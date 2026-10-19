//! Operator definitions: `fn Vec.+(right)`, `fn Vec.-`,
//! `fn Vec.[](key)`, `fn Vec.len`, `fn Vec.coerce(left)`, `fn Vec.iter`, …
//! installed as
//! metamethods through the runtime.
//!
//! Each definition becomes a method (the operand, or the left operand, is
//! `self`) passed with the name of its event to `rt.opdef` (structs) or
//! `rt.primop` (`Number`, `String`, `Bool`); `runtime/nl_42_opdef.lua`
//! installs it and propagates it to child structs. The parameter count
//! picks the event where the symbol is ambiguous (`-`) and is otherwise
//! checked here.

use nl_sema::util::*;
use nl_sema::{DeclKind, MethodOwner};
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::Lowerer;
use super::func::FnSpec;
use crate::ir::*;

use SyntaxKind::*;

/// How many parameters a definition takes.
#[derive(Clone, Copy)]
enum Arity {
    Exactly(usize),
    /// `()`: an ordinary parameter list.
    Any,
}

/// One operator that can be defined.
struct OpKind {
    /// How it is written, for messages.
    symbol: &'static str,
    /// The runtime's event name.
    event: &'static str,
    arity: Arity,
    /// Can be defined on `Number`, `String` and `Bool`.
    on_primitive: bool,
}

const fn op(symbol: &'static str, event: &'static str, arity: Arity, on_primitive: bool) -> OpKind {
    OpKind { symbol, event, arity, on_primitive }
}

/// The operator a definition names: the tokens of its `OP_NAME`, or the
/// word `len`, `coerce` or `iter`. `params` decides between binary and
/// unary `-`.
fn op_kind(tokens: &[SyntaxKind], word: Option<&str>, params: usize) -> Option<OpKind> {
    use Arity::*;
    if let Some(w) = word {
        // Primitive `len`, `coerce` and `iter`: `x.len` of a primitive is
        // Lua's length, `coerce` is asked only of a struct, and strings
        // iterate by character, so none has a meaning there.
        return match w {
            "len" => Some(op("len", "__len", Exactly(0), false)),
            "coerce" => Some(op("coerce", "__coerce", Exactly(1), false)),
            // `iter`: what `for v in x` iterates over.
            "iter" => Some(op("iter", "__iter", Exactly(0), false)),
            _ => None,
        };
    }
    Some(match tokens {
        [PLUS] => op("+", "__add", Exactly(1), true),
        [MINUS] if params == 0 => op("-", "__unm", Exactly(0), true),
        [MINUS] => op("-", "__sub", Exactly(1), true),
        [STAR] => op("*", "__mul", Exactly(1), true),
        [SLASH] => op("/", "__div", Exactly(1), true),
        [PERCENT] => op("%", "__mod", Exactly(1), true),
        [STAR2] => op("**", "__pow", Exactly(1), true),
        [SLASH2] => op("//", "__idiv", Exactly(1), true),
        [CONCAT] => op("<>", "__concat", Exactly(1), true),
        [PIPE] => op("|", "__bor", Exactly(1), true),
        [AMP] => op("&", "__band", Exactly(1), true),
        [CARET] => op("^", "__bxor", Exactly(1), true),
        [SHL] => op("<<", "__shl", Exactly(1), true),
        [SHR] => op(">>", "__shr", Exactly(1), true),
        [USHR] => op(">>>", "__ushr", Exactly(1), true),
        // `==` is consulted only between two tables or two
        // userdata, so it can never run for a primitive.
        [EQ2] => op("==", "__eq", Exactly(1), false),
        [LT] => op("<", "__lt", Exactly(1), true),
        [LT_EQ] => op("<=", "__le", Exactly(1), true),
        [GT] => op(">", "__gt", Exactly(1), true),
        [GT_EQ] => op(">=", "__ge", Exactly(1), true),
        [TILDE] => op("~", "__bnot", Exactly(0), true),
        // Indexing and calling a primitive value keep their meaning.
        [L_BRACK, R_BRACK] => op("[]", "__index", Exactly(1), false),
        [L_BRACK, R_BRACK, EQ] => op("[]=", "__newindex", Exactly(2), false),
        [L_PAREN, R_PAREN] => op("()", "__call", Any, false),
        _ => return None,
    })
}

impl Lowerer<'_> {
    /// `fn Owner.<op> … end`.
    pub(crate) fn lower_operator_def(&mut self, node: &SyntaxNode) {
        let tokens: Vec<SyntaxKind> = child(node, OP_NAME).map(|o| tokens(&o).map(|t| t.kind()).collect()).unwrap_or_default();
        let word = child(node, NAME).and_then(|n| name_text(&n));
        let list = child(node, PARAM_LIST);
        let plain = list.as_ref().map_or(0, |l| children(l, PARAM).count());
        let rest = list.as_ref().is_some_and(|l| child(l, REST_PARAM).is_some() || child(l, KEYED_PARAM).is_some());
        let Some(kind) = op_kind(&tokens, word.as_deref(), plain + usize::from(rest)) else {
            self.internal(node, "operator name");
            return;
        };
        if let Arity::Exactly(n) = kind.arity {
            if rest || plain != n {
                let wanted = match n {
                    0 => "no parameters".to_string(),
                    1 => "one parameter".to_string(),
                    n => format!("{n} parameters"),
                };
                let also = if kind.symbol == "-" { " (`-` takes one for subtraction, none for negation)" } else { "" };
                self.error(node, "operator-arity", format!("the operator `{}` takes {wanted}{also}", kind.symbol));
                return;
            }
        }
        let Some(owner) = self.res.method_owner(node.text_range()).cloned() else {
            self.internal(node, "operator owner");
            return;
        };
        let owner_text = child(node, QUAL_NAME).map(|q| qual_text(&q)).unwrap_or_default();
        if let MethodOwner::Binding { id, path } = &owner {
            if path.is_empty() && self.res.binding(*id).is_some_and(|b| b.decl == DeclKind::Module) {
                self.error(node, "operator-owner", format!("`{owner_text}` is a module; operators are defined on structs, `Number`, `String` and `Bool`"));
                return;
            }
        }
        if matches!(owner, MethodOwner::Primitive(_)) && !kind.on_primitive {
            self.error(node, "operator-owner", format!("the operator `{}` cannot be defined on `{owner_text}`; only structs define it", kind.symbol));
            return;
        }
        let self_binding = self.res.method_self(node.text_range());
        let spec = FnSpec { name: format!("{owner_text}.{}", kind.symbol), self_binding, discard_tail: false };
        let f = self.function_value(node, spec);
        let call = match owner {
            MethodOwner::Binding { id, path } => {
                let mut o = Expr::Local(self.local_of(id));
                for p in &path {
                    o = Expr::index(o, Expr::str(p));
                }
                Expr::intrinsic_call("opdef", vec![o, Expr::str(kind.event), f])
            }
            MethodOwner::Primitive(p) => {
                let owner = self.owner_value();
                Expr::intrinsic_call("primop", vec![Expr::str(p.name), Expr::str(kind.event), f, owner])
            }
        };
        self.emit(StmtKind::Expr(call));
    }
}
