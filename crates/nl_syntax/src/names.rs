//! Case classes of names.

use crate::SyntaxKind;

/// The case class of a word that is not a keyword.
///
/// The lexer only puts the coarse class on tokens (`LNAME`, `CNAME`,
/// `ONAME`, see [`NameCase::token_kind`]); call [`name_case`] on the token
/// text for the finer class, for example for the naming lints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NameCase {
    /// No uppercase letter: `health`, `x1`, `_`, `_tmp`. Token `LNAME`.
    Snake,
    /// First character uppercase, at least one lowercase letter, no `_`:
    /// `Player`, `HTTPServer`, `Vec2`. Token `CNAME`.
    Pascal,
    /// First character uppercase, no lowercase letter, no `_`: `IO`, `T`,
    /// `T1`, `MAX`. Both PascalCase (an acronym) and SCREAMING (a constant).
    /// Token `CNAME`.
    AllCaps,
    /// First character uppercase, no lowercase letter, contains `_`:
    /// `MAX_HP`, `TILE_SIZE_2`. Token `CNAME`.
    Screaming,
    /// Any other style: `getHealth`, `Get_Health`, `_Foo`. Token `ONAME`.
    Other,
}

impl NameCase {
    /// The token kind the lexer gives a name of this class.
    pub fn token_kind(self) -> SyntaxKind {
        match self {
            NameCase::Snake => SyntaxKind::LNAME,
            NameCase::Pascal | NameCase::AllCaps | NameCase::Screaming => SyntaxKind::CNAME,
            NameCase::Other => SyntaxKind::ONAME,
        }
    }

    /// True for the classes that give a `CNAME`.
    pub fn is_capitalized(self) -> bool {
        matches!(self, NameCase::Pascal | NameCase::AllCaps | NameCase::Screaming)
    }
}

/// Classifies a word by its letters. Does not check for
/// keywords; `name_case("Self")` is `Pascal`. Any text is accepted: only
/// ASCII letters and `_` are looked at.
pub fn name_case(word: &str) -> NameCase {
    let has_upper = word.bytes().any(|b| b.is_ascii_uppercase());
    let has_lower = word.bytes().any(|b| b.is_ascii_lowercase());
    let has_underscore = word.contains('_');
    let first_upper = word.bytes().next().is_some_and(|b| b.is_ascii_uppercase());
    if !has_upper {
        NameCase::Snake
    } else if first_upper && has_lower && !has_underscore {
        NameCase::Pascal
    } else if first_upper && !has_lower && !has_underscore {
        NameCase::AllCaps
    } else if first_upper && !has_lower {
        NameCase::Screaming
    } else {
        NameCase::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_examples() {
        for w in ["health", "x1", "_", "_tmp", "build_mesh", "from", "step", "then", "__x"] {
            assert_eq!(name_case(w), NameCase::Snake, "{w}");
        }
        for w in ["Player", "HTTPServer", "Vec2", "Self"] {
            assert_eq!(name_case(w), NameCase::Pascal, "{w}");
        }
        for w in ["IO", "OS", "T", "T1", "MAX"] {
            assert_eq!(name_case(w), NameCase::AllCaps, "{w}");
        }
        for w in ["MAX_HP", "TILE_SIZE_2", "A_"] {
            assert_eq!(name_case(w), NameCase::Screaming, "{w}");
        }
        for w in ["getHealth", "Get_Health", "_Foo", "maxPlayers", "x_Y"] {
            assert_eq!(name_case(w), NameCase::Other, "{w}");
        }
        assert_eq!(NameCase::Screaming.token_kind(), SyntaxKind::CNAME);
        assert_eq!(NameCase::Other.token_kind(), SyntaxKind::ONAME);
        assert_eq!(name_case(""), NameCase::Snake);
    }
}
