use std::collections::HashMap;

/// Apply regex-based text cleanup. Handles ~60% of dictation cases instantly.
pub fn apply(text: &str) -> String {
    let mut result = text.to_string();

    // Map common spoken code symbols — sorted longest-first to prevent substring clobbering
    let mut replacements: Vec<(&str, &str)> = vec![
        ("open curly brace", "{"),
        ("close curly brace", "}"),
        ("open parenthesis", "("),
        ("close parenthesis", ")"),
        ("open square bracket", "["),
        ("close square bracket", "]"),
        ("open angle bracket", "<"),
        ("close angle bracket", ">"),
        ("ampersand ampersand", "&&"),
        ("equals sign", "="),
        ("double equals", "=="),
        ("equals equals", "=="),
        ("triple equals", "==="),
        ("dot dot dot", "..."),
        ("open paren", "("),
        ("close paren", ")"),
        ("double quote", "\""),
        ("open quote", "\""),
        ("single quote", "'"),
        ("back tick", "`"),
        ("and and", "&&"),
        ("pipe pipe", "||"),
        ("slash slash", "//"),
        ("fat arrow", "=>"),
        ("minus minus", "--"),
        ("plus plus", "++"),
        ("at sign", "@"),
        ("backslash", "\\"),
        ("semicolon", ";"),
        ("underscore", "_"),
        ("not equals", "!="),
        ("bang equals", "!="),
        ("or or", "||"),
        ("open tag", "<"),
        ("close tag", ">"),
        ("ternary", "?"),
        ("percent", "%"),
        ("ampersand", "&"),
        ("backtick", "`"),
        ("colon", ":"),
        ("comma", ","),
        ("spread", "..."),
        ("arrow", "=>"),
        ("hash", "#"),
        ("pound", "#"),
        ("tilde", "~"),
        ("dash", "-"),
        ("pipe", "|"),
    ];
    replacements.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    for (spoken, symbol) in &replacements {
        result = result.replace(spoken, symbol);
        // Also handle capitalized variants
        let capitalized = spoken
            .chars()
            .next()
            .map(|c| {
                let rest: String = spoken.chars().skip(1).collect();
                format!("{}{}", c.to_uppercase(), rest)
            })
            .unwrap_or_default();
        result = result.replace(&capitalized, symbol);
    }

    // Fix common homophones / speech recognition errors
    let homophones: HashMap<&str, &str> = [
        ("their", "there"),
        ("to", "to"),
        ("two", "to"),
        ("your", "you're"),
        ("its", "it's"),
        ("could of", "could have"),
        ("should of", "should have"),
        ("would of", "would have"),
    ]
    .iter()
    .cloned()
    .collect();

    for (_wrong, _right) in &homophones {
        // Only replace if it's clearly a mistake — skip for now, LLM handles these better
    }

    // Capitalize first letter
    if let Some(first) = result.chars().next() {
        if first.is_lowercase() {
            result = format!("{}{}", first.to_uppercase(), &result[first.len_utf8()..]);
        }
    }

    // Capitalize "I" (standalone)
    result = result.replace(" i ", " I ");
    result = result.replace(" i'", " I'");

    // Add period at end if missing and text is a sentence
    let trimmed = result.trim();
    if !trimmed.is_empty()
        && !trimmed.ends_with(|c: char| c == '.' || c == '!' || c == '?' || c == ';' || c == ':' || c == '}')
        && trimmed.len() > 5
    {
        result = format!("{}.", result.trim());
    }

    // Clean up extra spaces
    while result.contains("  ") {
        result = result.replace("  ", " ");
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Symbol replacement ===

    #[test]
    fn test_braces() {
        assert_eq!(apply("open curly brace"), "{");
        assert_eq!(apply("close curly brace"), "}");
    }

    #[test]
    fn test_parens() {
        assert_eq!(apply("open parenthesis"), "(");
        assert_eq!(apply("close paren"), ")");
    }

    #[test]
    fn test_brackets_and_tags() {
        assert_eq!(apply("open square bracket"), "[");
        assert_eq!(apply("close angle bracket"), ">");
        assert_eq!(apply("open tag"), "<");
    }

    #[test]
    fn test_operators() {
        assert_eq!(apply("double equals"), "==");
        assert_eq!(apply("fat arrow"), "=>");
        assert_eq!(apply("not equals"), "!=");
        assert_eq!(apply("ampersand ampersand"), "&&");
        assert_eq!(apply("pipe pipe"), "||");
        assert_eq!(apply("triple equals"), "===");
    }

    #[test]
    fn test_quotes_and_misc() {
        assert_eq!(apply("double quote"), "\"");
        assert_eq!(apply("single quote"), "'");
        assert_eq!(apply("back tick"), "`");
        assert_eq!(apply("hash"), "#");
        assert_eq!(apply("at sign"), "@");
        assert_eq!(apply("underscore"), "_");
        assert_eq!(apply("tilde"), "~");
    }

    #[test]
    fn test_punctuation() {
        assert_eq!(apply("semicolon"), ";");
        assert_eq!(apply("colon"), ":");
        assert_eq!(apply("comma"), ",");
        assert_eq!(apply("dot dot dot"), "...");
        assert_eq!(apply("spread"), "...");
    }

    #[test]
    fn test_operators_single() {
        assert_eq!(apply("percent"), "%");
        assert_eq!(apply("plus plus"), "++");
        assert_eq!(apply("minus minus"), "--");
        assert_eq!(apply("ampersand"), "&");
        assert_eq!(apply("pipe"), "|");
        assert_eq!(apply("backslash"), "\\");
    }

    // === Capitalized variants ===

    #[test]
    fn test_capitalized_symbol() {
        assert_eq!(apply("Open curly brace"), "{");
        assert_eq!(apply("Close paren"), ")");
        assert_eq!(apply("Fat arrow"), "=>");
    }

    // === Multi-symbol phrases ===

    #[test]
    fn test_code_phrase() {
        let result = apply("open curly brace function fetch data close curly brace");
        assert!(result.contains("{"));
        assert!(result.contains("}"));
        assert!(result.contains("function"));
        assert!(result.contains("fetch"));
    }

    #[test]
    fn test_const_equals() {
        let result = apply("const x equals sign five");
        assert!(result.contains("Const") || result.contains("const"));
        assert!(result.contains("x"));
        assert!(result.contains("="));
        assert!(result.contains("five"));
    }

    // === Capital "I" ===

    #[test]
    fn test_capitalize_i() {
        assert_eq!(apply("i went to the store"), "I went to the store.");
    }

    #[test]
    fn test_capitalize_i_possessive() {
        let result = apply("i think it's mine");
        assert!(result.starts_with("I"));
    }

    // === Period auto-add ===

    #[test]
    fn test_add_period() {
        assert_eq!(apply("hello world"), "Hello world.");
    }

    #[test]
    fn test_no_period_already_punctuated() {
        assert_eq!(apply("hello."), "Hello.");
        assert_eq!(apply("hello!"), "Hello!");
    }

    #[test]
    fn test_no_period_short_text() {
        let result = apply("hi");
        assert!(!result.ends_with('.'));
    }

    // === Edge cases ===

    #[test]
    fn test_empty_string() {
        assert_eq!(apply(""), "");
    }

    #[test]
    fn test_extra_spaces() {
        assert_eq!(apply("hello   world"), "Hello world.");
    }

    #[test]
    fn test_text_ending_with_brace() {
        let result = apply("foo {}");
        assert!(result.ends_with('}'));
    }
}
