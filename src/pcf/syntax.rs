//! Lexical facts about Pkl shared by the parser and the writer.

/// Words reserved by Pkl. They can only be used as property names when quoted with backticks.
pub(crate) const KEYWORDS: &[&str] = &[
    "abstract",
    "amends",
    "as",
    "case",
    "class",
    "const",
    "delete",
    "else",
    "extends",
    "external",
    "false",
    "fixed",
    "for",
    "function",
    "hidden",
    "if",
    "import",
    "in",
    "is",
    "let",
    "local",
    "module",
    "new",
    "nothing",
    "null",
    "open",
    "out",
    "outer",
    "override",
    "protected",
    "read",
    "record",
    "super",
    "switch",
    "this",
    "throw",
    "trace",
    "true",
    "typealias",
    "unknown",
    "vararg",
    "when",
];

/// Duration units and their length in nanoseconds, from largest to smallest.
pub(crate) const DURATION_UNITS: &[(&str, i64)] = &[
    ("d", 86_400_000_000_000),
    ("h", 3_600_000_000_000),
    ("min", 60_000_000_000),
    ("s", 1_000_000_000),
    ("ms", 1_000_000),
    ("us", 1_000),
    ("ns", 1),
];

/// Data size units and their size in bytes, from largest to smallest.
pub(crate) const DATA_SIZE_UNITS: &[(&str, i64)] = &[
    ("pib", 1 << 50),
    ("pb", 1_000_000_000_000_000),
    ("tib", 1 << 40),
    ("tb", 1_000_000_000_000),
    ("gib", 1 << 30),
    ("gb", 1_000_000_000),
    ("mib", 1 << 20),
    ("mb", 1_000_000),
    ("kib", 1 << 10),
    ("kb", 1_000),
    ("b", 1),
];

pub(crate) fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}

pub(crate) fn is_identifier_start(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphabetic()
}

pub(crate) fn is_identifier_part(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphanumeric()
}

/// Whether `name` can be written as a property name without backticks.
///
/// This is deliberately limited to ASCII so the output never depends on how a particular Pkl
/// version classifies Unicode characters.
pub(crate) fn is_plain_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || c == '$' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
        && !is_keyword(name)
}

/// Whether `name` can be written as a backtick-quoted identifier.
pub(crate) fn is_quotable_identifier(name: &str) -> bool {
    !name.contains(['`', '\n', '\r'])
}

pub(crate) fn duration_unit(name: &str) -> Option<i64> {
    DURATION_UNITS
        .iter()
        .find_map(|&(unit, scale)| (unit == name).then_some(scale))
}

pub(crate) fn data_size_unit(name: &str) -> Option<i64> {
    DATA_SIZE_UNITS
        .iter()
        .find_map(|&(unit, scale)| (unit == name).then_some(scale))
}
