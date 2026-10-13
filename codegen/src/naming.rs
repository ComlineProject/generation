//! Identifier case conversions every generator needs.
//!
//! These were character-identical copies in `comline-codegen-rust` and
//! `comline-codegen-typescript`. They are here because two targets agreeing
//! on them is not a coincidence: the input is an IDL identifier, whose shape
//! the IDL fixes, so the conversion has nothing language-specific in it.
//!
//! What a *target* does with the result is its own business — which case a
//! generator reaches for is a language convention, not a shared one.

/// `get_user` → `GetUser`. Underscore-separated words, each capitalised,
/// joined. Empty segments are dropped, so `get__user` and `_get_user` behave.
pub fn pascal(s: &str) -> String {
    s.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// `get_user` → `getUser`. [`pascal`] with the first character lowered —
/// which is not the same as lowering the whole first word (`HTTPServer`
/// stays `hTTPServer`, not `httpServer`; that distinction belongs to a
/// target that cares).
pub fn camel(s: &str) -> String {
    let pascal = pascal(s);
    let mut chars = pascal.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `UserService` → `USER_SERVICE`. Splits before each interior uppercase
/// character, so an already-snake name is only uppercased.
pub fn screaming_snake(s: &str) -> String {
    let mut out = String::new();
    for (i, ch) in s.char_indices() {
        if ch.is_uppercase() && i != 0 {
            out.push('_');
        }
        out.extend(ch.to_uppercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_joins_underscore_words() {
        assert_eq!(pascal("get_user"), "GetUser");
        assert_eq!(pascal("lookup"), "Lookup");
        assert_eq!(pascal(""), "");
    }

    #[test]
    fn pascal_drops_empty_segments() {
        assert_eq!(pascal("get__user"), "GetUser");
        assert_eq!(pascal("_get_user_"), "GetUser");
    }

    #[test]
    fn camel_lowers_only_the_first_character() {
        assert_eq!(camel("get_user"), "getUser");
        // Deliberately not `httpServer`: collapsing a run of capitals is a
        // judgement call, and this helper doesn't make it.
        assert_eq!(camel("HTTP_server"), "hTTPServer");
        assert_eq!(camel(""), "");
    }

    #[test]
    fn screaming_snake_splits_before_interior_capitals() {
        assert_eq!(screaming_snake("UserService"), "USER_SERVICE");
        assert_eq!(screaming_snake("Service"), "SERVICE");
    }

    #[test]
    fn screaming_snake_leaves_an_existing_snake_name_alone() {
        assert_eq!(screaming_snake("user_service"), "USER_SERVICE");
    }
}
