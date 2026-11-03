//! Bounds on types (core#98), as a generator reads them.
//!
//! A bound is part of the type: `char[max 64]`, `char[2]`, `T[max N]`, and a
//! struct's `@max_depth`. A generator that renders a bounded alias as its
//! plain target (`type Name = char[max 32]` as Rust's `type Name = String`)
//! still has to check the alias's bound wherever it's used:
//! [`Validation::expand_bounds`](crate::validation::Validation::expand_bounds)
//! inlines every alias that carries one, so each use site sees its bounds.

use comline_core::schema::ir::frozen::types::{PrimitiveType, TypeRef};
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};

/// Whether `ty` carries a bound anywhere in it: a string's maximum or exact
/// length (any array of `char`), or a dynamic array's maximum. A fixed
/// array's length (`u8[4]`) isn't one: its type already holds it.
pub fn has_bounds(ty: &TypeRef) -> bool {
    match ty {
        TypeRef::Array { element, .. } if **element == TypeRef::Primitive(PrimitiveType::Char) => true,
        TypeRef::Array { max: Some(_), .. } => true,
        TypeRef::Array { element, .. } => has_bounds(element),
        TypeRef::Union(members) => members.iter().any(has_bounds),
        _ => false,
    }
}

/// A struct's depth limit, `@max_depth = D`: how deep a type that contains
/// itself may nest, the outermost value being level 1.
pub fn max_depth(parameters: &[FrozenUnit]) -> Option<u64> {
    parameters.iter().find_map(|p| match p {
        FrozenUnit::Property { name, value: ConfigValue::Integer(d) } if name == "max_depth" => {
            u64::try_from(*d).ok()
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::Validation;

    fn array(element: TypeRef, length: Option<u64>, max: Option<u64>) -> TypeRef {
        TypeRef::Array { element: Box::new(element), length, max }
    }

    const CHAR: TypeRef = TypeRef::Primitive(PrimitiveType::Char);

    #[test]
    fn strings_and_maximums_are_bounds_and_fixed_lengths_are_not() {
        assert!(has_bounds(&array(CHAR, None, Some(8))));
        assert!(has_bounds(&array(CHAR, Some(2), None)));
        assert!(has_bounds(&array(TypeRef::Primitive(PrimitiveType::U8), None, Some(8))));
        assert!(has_bounds(&array(array(CHAR, None, Some(8)), Some(3), None)));
        assert!(!has_bounds(&array(TypeRef::Primitive(PrimitiveType::U8), Some(4), None)));
        assert!(!has_bounds(&TypeRef::Primitive(PrimitiveType::String)));
        assert!(!has_bounds(&TypeRef::Named("Name".into())));
    }

    fn alias(name: &str, target: TypeRef) -> FrozenUnit {
        FrozenUnit::TypeAlias { docstring: None, parameters: vec![], name: name.into(), target, span: (0, 0) }
    }

    #[test]
    fn bounded_aliases_expand_and_plain_ones_stay_named() {
        let schemas = vec![(
            "a".to_string(),
            vec![
                alias("Name", array(CHAR, None, Some(32))),
                alias("Names", array(TypeRef::Named("Name".into()), None, Some(4))),
                alias("Id", TypeRef::Primitive(PrimitiveType::U64)),
            ],
        )];
        let model = Validation::new(&schemas);
        assert_eq!(
            model.expand_bounds("a", &TypeRef::Named("Names".into())),
            array(array(CHAR, None, Some(32)), None, Some(4))
        );
        assert_eq!(
            model.expand_bounds("a", &array(TypeRef::Named("Name".into()), Some(2), None)),
            array(array(CHAR, None, Some(32)), Some(2), None)
        );
        assert_eq!(model.expand_bounds("a", &TypeRef::Named("Id".into())), TypeRef::Named("Id".into()));
    }

    #[test]
    fn a_depth_limit_is_read() {
        let p = vec![FrozenUnit::Property { name: "max_depth".into(), value: ConfigValue::Integer(16) }];
        assert_eq!(max_depth(&p), Some(16));
        assert_eq!(max_depth(&[]), None);
    }
}
