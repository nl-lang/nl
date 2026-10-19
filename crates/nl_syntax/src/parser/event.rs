//! Parser events and markers.
//!
//! The parser does not build the tree directly. It records a flat list of
//! events (start a node, add a token, finish a node), which [`super::build`]
//! turns into a `rowan` tree, inserting the trivia. A [`Marker`] is a started
//! node; completing it gives a [`CompletedMarker`], which can still be wrapped
//! in a new parent with [`CompletedMarker::precede`]: that is how a binary
//! expression becomes the parent of its already-parsed left operand.

use rowan::TextRange;

use super::Parser;
use crate::SyntaxKind;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Event {
    /// Starts a node. `kind` is `None` for an abandoned marker (a tombstone).
    /// `forward_parent` is the distance to a later `Start` event that must be
    /// opened before this one (it was created by `precede`).
    Start { kind: Option<SyntaxKind>, forward_parent: Option<u32> },
    /// Finishes the innermost open node.
    Finish,
    /// One token with the given kind (possibly remapped from the lexer's, as
    /// for `from` and `step`) covering `range` of the source (a part of a
    /// lexer token when a `>>` was split).
    Token { kind: SyntaxKind, range: TextRange },
}

/// A node that has been started but not finished. Must be completed or
/// abandoned; an unused marker leaves a tombstone, which the builder skips.
#[derive(Debug)]
pub(crate) struct Marker {
    pos: u32,
}

impl Marker {
    pub(crate) fn new(pos: u32) -> Marker {
        Marker { pos }
    }

    /// Finishes the node with `kind`.
    pub(crate) fn complete(self, p: &mut Parser<'_>, kind: SyntaxKind) -> CompletedMarker {
        if let Some(Event::Start { kind: slot, .. }) = p.events.get_mut(self.pos as usize) {
            *slot = Some(kind);
        }
        p.events.push(Event::Finish);
        CompletedMarker { pos: self.pos }
    }

    /// Drops the node; its children become children of the enclosing node.
    pub(crate) fn abandon(self, p: &mut Parser<'_>) {
        let idx = self.pos as usize;
        if idx + 1 == p.events.len() {
            if let Some(Event::Start { kind: None, forward_parent: None }) = p.events.last() {
                p.events.pop();
            }
        }
    }
}

/// A finished node, which can still be given a parent.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CompletedMarker {
    pos: u32,
}

impl CompletedMarker {
    /// Starts a new node that will contain this one as its first child.
    pub(crate) fn precede(self, p: &mut Parser<'_>) -> Marker {
        let new_pos = p.start_event();
        if let Some(Event::Start { forward_parent, .. }) = p.events.get_mut(self.pos as usize) {
            *forward_parent = Some(new_pos.saturating_sub(self.pos));
        }
        Marker::new(new_pos)
    }

    /// The kind this node was completed with.
    pub(crate) fn kind(self, p: &Parser<'_>) -> Option<SyntaxKind> {
        match p.events.get(self.pos as usize) {
            Some(Event::Start { kind, .. }) => *kind,
            _ => None,
        }
    }
}
