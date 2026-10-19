//! [`Diagnostic`]: an error or warning about a range of the source.

use rowan::TextRange;

/// How serious a [`Diagnostic`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// The program is rejected.
    Error,
    /// The program is accepted, but probably not what was meant.
    Warning,
}

/// A message about a byte range of the source, shared by the lexer, the
/// parser and later stages.
///
/// `message` is written for the person who made the mistake: it says what is
/// wrong and, where there is an obvious fix, how to write it. It does not
/// repeat the location, which a renderer derives from `range`. The range may
/// be empty (a point, such as "missing `}` here").
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Diagnostic {
    pub severity: Severity,
    pub range: TextRange,
    pub message: String,
}

impl Diagnostic {
    /// An error at `range`.
    pub fn error(range: TextRange, message: impl Into<String>) -> Self {
        Diagnostic { severity: Severity::Error, range, message: message.into() }
    }

    /// A warning at `range`.
    pub fn warning(range: TextRange, message: impl Into<String>) -> Self {
        Diagnostic { severity: Severity::Warning, range, message: message.into() }
    }

    /// True for [`Severity::Error`].
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }
}
