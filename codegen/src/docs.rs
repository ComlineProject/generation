//! The documentation pages a generator points users at.
//!
//! A generator knows its own documentation, so it supplies the links
//! instead of the CLI (or the editor) keeping a list per language. Each link
//! is for a [`Topic`], and a generator's [`DocsTable`] is handed to
//! [`crate::Registry::register`] on its [`crate::Generator`]. It reaches an
//! external generator's callers through [`crate::protocol::LanguageInfo`].

/// A subject a generator may have a documentation page for.
///
/// Its [`id`](Topic::id) is stable: it's the key a generator's links travel
/// under in the protocol. A new variant goes at the end of [`Topic::ALL`];
/// a generator without a page for it simply has no entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Topic {
    /// The page that says when a package's `Services` / `serve` / `connect`
    /// are generated and when they aren't. The CLI links it when it notes
    /// their absence.
    Connections,
}

impl Topic {
    /// Every topic, in a stable order.
    pub const ALL: &'static [Topic] = &[Topic::Connections];

    /// The stable id: `connections`.
    pub fn id(self) -> &'static str {
        match self {
            Topic::Connections => "connections",
        }
    }

    /// The topic an [`id`](Topic::id) names.
    pub fn from_id(id: &str) -> Option<Topic> {
        Topic::ALL.iter().copied().find(|t| t.id() == id)
    }
}

/// A generator's documentation pages: each topic with the absolute URL of
/// its page (an anchor is allowed). A topic it has no page for is left out.
pub type DocsTable = &'static [(Topic, &'static str)];

/// `topic`'s page in `table`, if the generator has one.
pub fn link(table: DocsTable, topic: Topic) -> Option<&'static str> {
    table.iter().find(|(t, _)| *t == topic).map(|(_, url)| *url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for t in Topic::ALL {
            assert_eq!(Topic::from_id(t.id()), Some(*t));
        }
        assert_eq!(Topic::from_id("nope"), None);
    }

    #[test]
    fn a_topic_without_a_page_has_no_link() {
        const TABLE: DocsTable = &[(Topic::Connections, "https://x/connections#generated")];
        assert_eq!(
            link(TABLE, Topic::Connections),
            Some("https://x/connections#generated")
        );
        assert_eq!(link(&[], Topic::Connections), None);
    }
}
