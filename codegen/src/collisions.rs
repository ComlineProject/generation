//! Does a name a generator is about to invent already mean something?
//!
//! Every generator derives names a schema never wrote. Rust turns a
//! `protocol Gateway` into a `GatewayError` union and a `GatewayDispatcher`;
//! TypeScript turns an `error Rude` into a `class RudeError`. None of those
//! names is in the schema, so nothing stopped a schema from declaring one
//! itself — `protocol Gateway` plus `error GatewayError` emitted two types of
//! the same name, and the generated file simply did not compile. The author
//! got the target language's redefinition error, pointing at generated code
//! they did not write.
//!
//! Derived names can also collide with *each other*: Rust's per-function enum
//! is `<Proto><Fn>Error`, so `protocol Chat { function send(…) }` and a
//! `protocol ChatSend` both reach for `ChatSendError`.
//!
//! The mechanism is shared; the names are not. `symbols` draws the same line —
//! "this module resolves and records, and each generator writes" — because
//! what a generator derives is a fact about that language's output, not about
//! the schema. So each generator hands over the names it is about to use,
//! described in its own terms, and this module decides whether any of them is
//! already taken.

use std::collections::BTreeMap;

use comline_core::schema::ir::frozen::unit::FrozenUnit;

use eyre::{bail, Result};

/// A name a generator intends to emit, and where it came from.
///
/// `from` is read by a person looking at a build failure, so it should name
/// the declaration *and* what was derived from it — "protocol `Chat`, function
/// `send`" rather than "a function".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derived {
    /// The identifier that will appear in the generated output.
    pub name: String,
    /// What the generator derived it from, phrased for a diagnostic.
    pub from: String,
}

impl Derived {
    pub fn new(name: impl Into<String>, from: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            from: from.into(),
        }
    }
}

/// Every name this schema declares, mapped to the keyword that declared it.
///
/// One flat namespace, which is core's own model: its validator reports a
/// struct and an error of the same name as a duplicate declaration, so a
/// generator does not have to care which kind a name is — only that it is
/// taken.
pub fn declared_names(units: &[FrozenUnit]) -> BTreeMap<&str, &'static str> {
    units
        .iter()
        .filter_map(|unit| match unit {
            FrozenUnit::Struct { name, .. } => Some((name.as_str(), "struct")),
            FrozenUnit::Enum { name, .. } => Some((name.as_str(), "enum")),
            FrozenUnit::Error { name, .. } => Some((name.as_str(), "error")),
            FrozenUnit::Protocol { name, .. } => Some((name.as_str(), "protocol")),
            FrozenUnit::Constant { name, .. } => Some((name.as_str(), "const")),
            FrozenUnit::Validator { name, .. } => Some((name.as_str(), "validator")),
            FrozenUnit::TypeAlias { name, .. } => Some((name.as_str(), "type alias")),
            _ => None,
        })
        .collect()
}

/// Refuse to generate when a derived name is already taken — by a declaration
/// in `units`, or by another derived name.
///
/// `namespace` is the schema being generated, for a message that says which
/// file to go and edit.
pub fn check(
    namespace: &str,
    units: &[FrozenUnit],
    derived: impl IntoIterator<Item = Derived>,
) -> Result<()> {
    let declared = declared_names(units);
    let mut seen: BTreeMap<String, String> = BTreeMap::new();

    for item in derived {
        if let Some(kind) = declared.get(item.name.as_str()) {
            bail!(
                "schema `{namespace}` declares {kind} `{name}`, but generating \
                 {from} needs that name too.\n\
                 Rename the {kind} — the generated name is derived from the \
                 declaration it belongs to and cannot be changed.",
                name = item.name,
                from = item.from,
            );
        }

        if let Some(first) = seen.get(&item.name) {
            bail!(
                "schema `{namespace}` generates the name `{name}` twice — once \
                 for {first}, once for {second}.\n\
                 Rename one of the two declarations.",
                name = item.name,
                second = item.from,
            );
        }

        seen.insert(item.name.clone(), item.from);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error_unit(name: &str) -> FrozenUnit {
        FrozenUnit::Error {
            docstring: None,
            parameters: vec![],
            ordinal: 0,
            imported_from: None,
            name: name.to_string(),
            message: String::new(),
            fields: vec![],
            span: None,
        }
    }

    fn struct_unit(name: &str) -> FrozenUnit {
        FrozenUnit::Struct {
            docstring: None,
            parameters: vec![],
            name: name.to_string(),
            fields: vec![],
            span: (0, 1),
        }
    }

    #[test]
    fn a_free_name_generates() {
        let units = vec![error_unit("Rude")];
        assert!(check(
            "chat",
            &units,
            [Derived::new("ChatError", "protocol `Chat`'s error union")],
        )
        .is_ok());
    }

    #[test]
    fn a_declared_name_is_refused_and_says_which_declaration() {
        let units = vec![error_unit("GatewayError")];
        let err = check(
            "gateway",
            &units,
            [Derived::new("GatewayError", "protocol `Gateway`'s error union")],
        )
        .expect_err("the name is taken");
        let message = err.to_string();
        assert!(message.contains("declares error `GatewayError`"), "{message}");
        assert!(message.contains("protocol `Gateway`'s error union"), "{message}");
    }

    /// The kind does not matter — core keeps one flat declaration namespace.
    #[test]
    fn a_struct_takes_the_name_just_as_an_error_would() {
        let units = vec![struct_unit("RudeError")];
        let err = check(
            "chat",
            &units,
            [Derived::new("RudeError", "the thrown class for error `Rude`")],
        )
        .expect_err("the name is taken");
        assert!(err.to_string().contains("declares struct `RudeError`"));
    }

    #[test]
    fn two_derived_names_that_agree_are_refused() {
        let err = check(
            "chat",
            &[],
            [
                Derived::new("ChatSendError", "protocol `Chat`, function `send`"),
                Derived::new("ChatSendError", "protocol `ChatSend`'s error union"),
            ],
        )
        .expect_err("both want the same name");
        let message = err.to_string();
        assert!(message.contains("twice"), "{message}");
        assert!(message.contains("protocol `Chat`, function `send`"), "{message}");
        assert!(message.contains("protocol `ChatSend`'s error union"), "{message}");
    }

    #[test]
    fn declared_names_covers_every_declaring_unit() {
        let units = vec![struct_unit("S"), error_unit("E")];
        let names = declared_names(&units);
        assert_eq!(names.get("S"), Some(&"struct"));
        assert_eq!(names.get("E"), Some(&"error"));
        assert_eq!(names.get("absent"), None);
    }
}
