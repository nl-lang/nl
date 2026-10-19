//! Minimizes a failing program by deleting statements while it still
//! fails the same way, so that a finding is reported with the smallest
//! program that shows it.
//!
//! Generated programs indent every block by two spaces, so a statement
//! and everything it owns are found from the indentation alone: the line,
//! the deeper lines after it, and for a construct its `else`, `elseif`,
//! `when`, `catch` and `finally` clauses and its `end` at the same
//! indentation. Each such unit is tried for deletion, from the top, until
//! a whole pass deletes nothing; units that leave a broken program simply
//! fail the test and stay. A clause line is a unit of its own (its line
//! and its deeper lines), so a branch can go while its construct stays.

/// Keywords that continue a construct at its own indentation.
const CLAUSES: &[&str] = &["else", "elseif", "when", "catch", "finally"];

/// The smallest program found that still `fails`, after at most
/// `max_tests` calls of it.
pub fn minimize(source: &str, max_tests: usize, mut fails: impl FnMut(&str) -> bool) -> String {
    let mut lines: Vec<String> = source.lines().map(str::to_string).collect();
    let mut tests = 0;
    // Chunks of top-level statements first, halving the chunk size as in
    // delta debugging (Zeller), so that a long program shrinks in few
    // tests.
    let mut n = 2;
    loop {
        let units = top_units(&lines);
        if units.len() < 2 {
            break;
        }
        let size = units.len().div_ceil(n);
        let mut removed = false;
        for chunk in units.chunks(size) {
            if tests >= max_tests {
                return join(&lines);
            }
            let (Some(first), Some(last)) = (chunk.first(), chunk.last()) else { continue };
            let mut candidate = lines.clone();
            candidate.drain(first.0..last.1);
            tests += 1;
            if !candidate.is_empty() && fails(&join(&candidate)) {
                lines = candidate;
                n = (n - 1).max(2);
                removed = true;
                break;
            }
        }
        if !removed {
            if n >= units.len() {
                break;
            }
            n = (n * 2).min(units.len());
        }
    }
    // Then whole units at every depth, then single lines (a crash need
    // not keep the program well-formed), until neither deletes anything.
    loop {
        let mut changed = false;
        for single in [false, true] {
            let mut i = 0;
            while i < lines.len() {
                if tests >= max_tests {
                    return join(&lines);
                }
                let end = if single { i + 1 } else { unit_end(&lines, i) };
                let mut candidate = lines.clone();
                candidate.drain(i..end);
                tests += 1;
                if !candidate.is_empty() && fails(&join(&candidate)) {
                    lines = candidate;
                    changed = true;
                } else {
                    i += 1;
                }
            }
        }
        if !changed {
            return join(&lines);
        }
    }
}

/// The spans of the top-level units: statements at indentation 0 with
/// everything they own.
fn top_units(lines: &[String]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let end = unit_end(lines, i).max(i + 1);
        out.push((i, end));
        i = end;
    }
    out
}

fn join(lines: &[String]) -> String {
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The line starts with `word` as a whole word.
fn starts_with_word(line: &str, word: &str) -> bool {
    let t = line.trim_start();
    t.strip_prefix(word).is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
}

/// One past the last line of the unit that starts at line `i`.
fn unit_end(lines: &[String], i: usize) -> usize {
    let Some(first) = lines.get(i) else { return i };
    let ind = indent_of(first);
    let is_clause = CLAUSES.iter().any(|k| starts_with_word(first, k));
    let mut j = i + 1;
    while let Some(line) = lines.get(j) {
        if line.trim().is_empty() || indent_of(line) > ind {
            j += 1;
            continue;
        }
        if indent_of(line) == ind && !is_clause {
            if starts_with_word(line, "end") {
                j += 1;
                break;
            }
            if CLAUSES.iter().any(|k| starts_with_word(line, k)) {
                j += 1;
                continue;
            }
        }
        break;
    }
    j
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    #[test]
    fn units() {
        let l = lines("let a = 1\nif a\n  print(a)\nelse\n  print(2)\nend\nprint(3)");
        assert_eq!(unit_end(&l, 0), 1);
        assert_eq!(unit_end(&l, 1), 6);
        assert_eq!(unit_end(&l, 2), 3);
        assert_eq!(unit_end(&l, 3), 5);
        let c = lines("case x\nwhen 1\n  a()\nwhen 2\n  b()\nend.upcase");
        assert_eq!(unit_end(&c, 0), 6);
        assert_eq!(unit_end(&c, 1), 3);
    }

    #[test]
    fn keeps_what_fails() {
        let src = "let a = 1\nlet b = 2\nif a\n  print(b)\nend\nprint(\"boom\")\nprint(a)\n";
        let out = minimize(src, 1000, |s| s.contains("boom"));
        assert_eq!(out, "print(\"boom\")\n");
    }
}
