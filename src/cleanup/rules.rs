use std::collections::HashMap;

/// Apply regex-based text cleanup. Handles ~60% of dictation cases instantly.
pub fn apply(text: &str) -> String {
    let mut result = text.to_string();

    // Map common spoken code symbols
    let replacements: HashMap<&str, &str> = [
        ("open curly brace", "{"),
        ("close curly brace", "}"),
        ("open parenthesis", "("),
        ("open paren", "("),
        ("close parenthesis", ")"),
        ("close paren", ")"),
        ("open square bracket", "["),
        ("close square bracket", "]"),
        ("open angle bracket", "<"),
        ("close angle bracket", ">"),
        ("open tag", "<"),
        ("close tag", ">"),
        ("semicolon", ";"),
        ("colon", ":"),
        ("comma", ","),
        ("dot dot dot", "..."),
        ("spread", "..."),
        ("equals sign", "="),
        ("double equals", "=="),
        ("equals equals", "=="),
        ("triple equals", "==="),
        ("not equals", "!="),
        ("bang equals", "!="),
        ("ampersand ampersand", "&&"),
        ("and and", "&&"),
        ("pipe pipe", "||"),
        ("or or", "||"),
        ("double quote", "\""),
        ("open quote", "\""),
        ("single quote", "'"),
        ("back tick", "`"),
        ("hash", "#"),
        ("pound", "#"),
        ("slash slash", "//"),
        ("dash", "-"),
        ("underscore", "_"),
        ("at sign", "@"),
        ("fat arrow", "=>"),
        ("arrow", "=>"),
        ("ternary", "?"),
        ("percent", "%"),
        ("plus plus", "++"),
        ("minus minus", "--"),
        ("ampersand", "&"),
        ("pipe", "|"),
        ("tilde", "~"),
        ("backslash", "\\"),
    ]
    .iter()
    .cloned()
    .collect();

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

    #[test]
    fn test_symbol_replacement() {
        assert_eq!(apply("open curly brace"), "{");
        assert_eq!(apply("close curly brace"), "}");
        assert_eq!(apply("semicolon"), ";");
    }

    #[test]
    fn test_capitalize_i() {
        assert_eq!(apply("i went to the store"), "I went to the store.");
    }

    #[test]
    fn test_add_period() {
        assert_eq!(apply("hello world"), "Hello world.");
    }
}
