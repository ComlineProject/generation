//! What runs when a value is validated, resolved once for every generator.
//!
//! A field's checks come from two places: its own `@validators`, and the
//! `@validators` on any `type` alias its type goes through. Core stores an
//! alias's validators once, on the alias, so working out a field's checks
//! means walking the alias chain - across schemas, since an alias or a
//! validator can be imported (std's `StringBounds` always is). Every target
//! needs exactly that walk, so it lives here; each generator only lowers the
//! resulting [`Plan`] into its own language.
//!
//! Properties are bound here too: the argument a use site wrote, else the
//! property's declared default, with constant references (`MAX_NAME`,
//! `u32::MAX`, `limits::DEFAULT`) resolved to their values. A generator never
//! sees an unbound property or a reference.

use std::cell::RefCell;
use std::collections::HashMap;

use comline_core::schema::ir::frozen::types::{Literal, PrimitiveType, TypeRef};
use comline_core::schema::ir::frozen::unit::{ConfigValue, FrozenUnit};
use eyre::{eyre, Result};

use crate::symbols::SymbolTable;

/// Whether decoding a value runs its checks: `comline.toml`'s
/// `[generate] validate_on_decode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ValidateOnDecode {
    /// Emit the checks; never run them on the caller's behalf.
    #[default]
    Off,
    /// Decoding fails with every failed check.
    Collect,
    /// Decoding fails with the first failed check.
    First,
}

impl ValidateOnDecode {
    pub const NAMES: [&'static str; 3] = ["off", "collect", "first"];

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "off" => Some(Self::Off),
            "collect" => Some(Self::Collect),
            "first" => Some(Self::First),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Collect => "collect",
            Self::First => "first",
        }
    }
}

/// A property's bound value. Wider than core's [`Literal`]: `u64::MAX` is a
/// valid bound and doesn't fit an `i64`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Integer(i128),
    Bool(bool),
    String(String),
}

/// One validator applied to one value, every property bound.
#[derive(Debug, Clone)]
pub struct Check<'a> {
    /// The validator's declared name.
    pub validator: &'a str,
    /// The schema declaring it.
    pub namespace: &'a str,
    /// Its `validate` block: `FrozenUnit::Assert`s, in order.
    pub asserts: &'a [FrozenUnit],
    /// Every property, in declaration order: the argument given, else the
    /// default.
    pub params: Vec<(&'a str, Value)>,
}

impl Check<'_> {
    pub fn param(&self, name: &str) -> Option<&Value> {
        self.params.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }
}

/// Everything to check on one value.
#[derive(Debug, Clone, Default)]
pub struct Plan<'a> {
    /// Checks on the value itself: the alias chain's, innermost alias
    /// first, then the use site's own `@validators`.
    pub checks: Vec<Check<'a>>,
    /// When the value is an array: what to check on each element.
    pub elements: Option<Box<Plan<'a>>>,
    /// The value is a struct or error that has checks of its own, which run
    /// through its own validation.
    pub nested: bool,
}

impl Plan<'_> {
    pub fn is_empty(&self) -> bool {
        self.checks.is_empty() && self.elements.is_none() && !self.nested
    }
}

/// The resolution model for one request.
pub struct Validation<'a> {
    schemas: &'a [(String, Vec<FrozenUnit>)],
    tables: HashMap<&'a str, SymbolTable>,
    /// `(namespace, type)` → whether it needs validation. `false` is also
    /// the in-progress marker, so a recursive type terminates.
    needs: RefCell<HashMap<(String, String), bool>>,
}

/// Alias chains and nesting are bounded; core already reports a cycle.
const MAX_DEPTH: usize = 32;

impl<'a> Validation<'a> {
    pub fn new(schemas: &'a [(String, Vec<FrozenUnit>)]) -> Self {
        let tables = schemas
            .iter()
            .map(|(ns, units)| (ns.as_str(), SymbolTable::new(units, schemas)))
            .collect();
        Self { schemas, tables, needs: RefCell::new(HashMap::new()) }
    }

    /// What to check on a value of type `ty`, written in schema `ns`, whose
    /// use site carries `parameters` (a field's annotations; empty for a
    /// function argument). `at` names the use site in errors.
    pub fn plan(
        &self,
        ns: &str,
        ty: &TypeRef,
        parameters: &'a [FrozenUnit],
        at: &str,
    ) -> Result<Plan<'a>> {
        self.plan_at(ns, ty, parameters, at, 0)
    }

    /// Whether the struct or error `name` (as written in `ns`) has any check
    /// on a field, directly or further down.
    pub fn needs_validation(&self, ns: &str, name: &str) -> bool {
        let Some((decl_ns, unit)) = self.resolve(ns, name) else {
            return false;
        };
        let (FrozenUnit::Struct { name, fields, .. } | FrozenUnit::Error { name, fields, .. }) = unit
        else {
            return false;
        };
        let key = (decl_ns.to_string(), name.clone());
        if let Some(&known) = self.needs.borrow().get(&key) {
            return known;
        }
        self.needs.borrow_mut().insert(key.clone(), false);
        let needs = fields.iter().any(|field| match field {
            FrozenUnit::Field { ty, parameters, .. } => self
                .plan(decl_ns, ty, parameters, name)
                // An unresolvable check is reported where the type is
                // generated; it still means there's something to run.
                .map_or(true, |plan| !plan.is_empty()),
            _ => false,
        });
        self.needs.borrow_mut().insert(key, needs);
        needs
    }

    fn plan_at(
        &self,
        ns: &str,
        ty: &TypeRef,
        parameters: &'a [FrozenUnit],
        at: &str,
        depth: usize,
    ) -> Result<Plan<'a>> {
        // Follow the alias chain down to what it stands for.
        let mut chain: Vec<(&'a str, &'a [FrozenUnit])> = Vec::new();
        let (mut cur_ns, mut cur): (&str, &TypeRef) = (ns, ty);
        while let TypeRef::Named(name) = cur {
            let Some((alias_ns, FrozenUnit::TypeAlias { parameters, target, .. })) =
                self.resolve(cur_ns, name)
            else {
                break;
            };
            if chain.len() == MAX_DEPTH {
                break;
            }
            chain.push((alias_ns, parameters));
            (cur_ns, cur) = (alias_ns, target);
        }

        let mut checks = Vec::new();
        for (alias_ns, alias_parameters) in chain.iter().rev() {
            checks.extend(self.checks(alias_ns, alias_parameters, at)?);
        }
        checks.extend(self.checks(ns, parameters, at)?);

        let mut plan = Plan { checks, ..Plan::default() };
        match cur {
            TypeRef::Array { element, .. } if depth < MAX_DEPTH => {
                let elements = self.plan_at(cur_ns, element, &[], at, depth + 1)?;
                if !elements.is_empty() {
                    plan.elements = Some(Box::new(elements));
                }
            }
            TypeRef::Named(name) => plan.nested = self.needs_validation(cur_ns, name),
            _ => {}
        }
        Ok(plan)
    }

    /// The `@validators` among `parameters`, resolved from schema `ns`.
    fn checks(&self, ns: &str, parameters: &'a [FrozenUnit], at: &str) -> Result<Vec<Check<'a>>> {
        let mut checks = Vec::new();
        for parameter in parameters {
            let FrozenUnit::ValidatorRef { name, args } = parameter else {
                continue;
            };
            let Some((
                validator_ns,
                FrozenUnit::Validator { name: declared, properties, expression_block, .. },
            )) = self.resolve(ns, name)
            else {
                return Err(eyre!("{at}: validator '{name}' isn't declared in any schema being generated"));
            };
            let asserts = match &**expression_block {
                FrozenUnit::ExpressionBlock { asserts } => asserts.as_slice(),
                _ => &[],
            };

            let mut params = Vec::new();
            for property in properties {
                let FrozenUnit::Field { name: prop, default, .. } = property else {
                    continue;
                };
                let given = args.iter().find_map(|arg| match arg {
                    FrozenUnit::Property { name, value } if name == prop => Some(value),
                    _ => None,
                });
                let value = match (given, default) {
                    (Some(value), _) => self.config_value(ns, value),
                    (None, Some(default)) => self.literal(validator_ns, default),
                    (None, None) => Err(eyre!("no value given, and it has no default")),
                }
                .map_err(|e| eyre!("{at}: validator '{name}', property '{prop}': {e}"))?;
                params.push((prop.as_str(), value));
            }

            checks.push(Check { validator: declared, namespace: validator_ns, asserts, params });
        }
        Ok(checks)
    }

    /// An argument as a use site wrote it.
    fn config_value(&self, ns: &str, value: &ConfigValue) -> Result<Value> {
        match value {
            ConfigValue::Integer(i) => Ok(Value::Integer(i128::from(*i))),
            ConfigValue::Bool(b) => Ok(Value::Bool(*b)),
            ConfigValue::Str(s) => Ok(Value::String(s.clone())),
            ConfigValue::Name(r) | ConfigValue::Path(r) => self.reference(ns, r),
            ConfigValue::FString(_) => Err(eyre!("an f-string isn't a validator argument")),
            ConfigValue::Marker => Err(eyre!("no value given")),
        }
    }

    /// A default as a validator declared it.
    fn literal(&self, ns: &str, literal: &Literal) -> Result<Value> {
        match literal {
            Literal::Integer(i) => Ok(Value::Integer(i128::from(*i))),
            Literal::Bool(b) => Ok(Value::Bool(*b)),
            Literal::String(s) => Ok(Value::String(s.clone())),
            Literal::Reference(r) => self.reference(ns, r),
        }
    }

    /// A constant by name (`MAX`, `limits::MAX`), or a primitive's bound
    /// (`u32::MAX`, `s8::MIN`).
    fn reference(&self, ns: &str, reference: &str) -> Result<Value> {
        if let Some((ty, bound)) = reference.split_once("::") {
            if let (Some(p), "MIN" | "MAX") = (PrimitiveType::from_name(ty), bound) {
                let (min, max) =
                    p.integer_range().ok_or_else(|| eyre!("`{ty}` has no `{bound}`"))?;
                return Ok(Value::Integer(if bound == "MIN" { min } else { max }));
            }
        }
        match self.resolve(ns, reference) {
            Some((const_ns, FrozenUnit::Constant { value: Some(value), .. })) => {
                self.literal(const_ns, value)
            }
            _ => Err(eyre!("`{reference}` isn't a constant in any schema being generated")),
        }
    }

    /// The declaration `name` refers to, from schema `ns`: a qualified path
    /// (`a::b::Name`), a name `ns` declares itself, or one it imports.
    fn resolve(&self, ns: &str, name: &str) -> Option<(&'a str, &'a FrozenUnit)> {
        if let Some((path, declared)) = name.rsplit_once("::") {
            return self.declared_in(&path.replace("::", "/"), declared);
        }
        if let Some(found) = self.declared_in(ns, name) {
            return Some(found);
        }
        let origin = self.tables.get(ns)?.origin(name)?;
        self.declared_in(&origin.namespace, &origin.declared)
    }

    fn declared_in(&self, ns: &str, name: &str) -> Option<(&'a str, &'a FrozenUnit)> {
        let (ns, units) = self.schemas.iter().find(|(n, _)| n == ns)?;
        let unit = units.iter().find(|unit| match unit {
            FrozenUnit::Struct { name: n, .. }
            | FrozenUnit::TypeAlias { name: n, .. }
            | FrozenUnit::Validator { name: n, .. }
            | FrozenUnit::Constant { name: n, .. } => n == name,
            // A re-export slot names another schema's error; it isn't the
            // declaration.
            FrozenUnit::Error { name: n, imported_from: None, .. } => n == name,
            _ => false,
        })?;
        Some((ns.as_str(), unit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use comline_core::schema::ir::compiler::interpreter::incremental::IncrementalInterpreter;
    use comline_core::schema::ir::compiler::Compile;

    const STD: &str = "validator StringBounds for string {\n    min_chars: u32 = 0\n    \
                       max_chars: u32 = 1024\n    validate {\n        \
                       assert(value.length >= params.min_chars, \"{value.name} is too short\")\n    }\n}\n";

    fn schemas(user: &str) -> Vec<(String, Vec<FrozenUnit>)> {
        vec![
            ("std/validators".to_string(), IncrementalInterpreter::from_source(STD)),
            ("app".to_string(), IncrementalInterpreter::from_source(user)),
        ]
    }

    fn field<'a>(units: &'a [FrozenUnit], strukt: &str, field: &str) -> (&'a TypeRef, &'a [FrozenUnit]) {
        units
            .iter()
            .find_map(|u| match u {
                FrozenUnit::Struct { name, fields, .. } if name == strukt => {
                    fields.iter().find_map(|f| match f {
                        FrozenUnit::Field { name, ty, parameters, .. } if name == field => {
                            Some((ty, parameters.as_slice()))
                        }
                        _ => None,
                    })
                }
                _ => None,
            })
            .expect("the field")
    }

    fn plan_of<'a>(model: &Validation<'a>, units: &'a [FrozenUnit], strukt: &str, name: &str) -> Plan<'a> {
        let (ty, parameters) = field(units, strukt, name);
        model.plan("app", ty, parameters, name).expect("resolves")
    }

    #[test]
    fn an_imported_validator_binds_arguments_and_defaults() {
        let schemas = schemas(
            "use std::validators::StringBounds\nstruct S {\n    \
             @validators = [StringBounds(min_chars = 3)]\n    name: string\n}\n",
        );
        let model = Validation::new(&schemas);
        let plan = plan_of(&model, &schemas[1].1, "S", "name");
        let [check] = plan.checks.as_slice() else { panic!("{plan:?}") };
        assert_eq!((check.validator, check.namespace), ("StringBounds", "std/validators"));
        assert_eq!(check.param("min_chars"), Some(&Value::Integer(3)));
        assert_eq!(check.param("max_chars"), Some(&Value::Integer(1024)));
        assert_eq!(check.asserts.len(), 1);
    }

    #[test]
    fn alias_checks_come_first_innermost_alias_first() {
        let schemas = schemas(
            "use std::validators::StringBounds\n\
             @validators = [StringBounds(min_chars = 1)]\ntype Text = string\n\
             @validators = [StringBounds(min_chars = 2)]\ntype Name = Text\n\
             struct S {\n    @validators = [StringBounds(min_chars = 3)]\n    name: Name\n}\n",
        );
        let model = Validation::new(&schemas);
        let plan = plan_of(&model, &schemas[1].1, "S", "name");
        let mins: Vec<_> = plan.checks.iter().map(|c| c.param("min_chars").cloned()).collect();
        assert_eq!(
            mins,
            [Some(Value::Integer(1)), Some(Value::Integer(2)), Some(Value::Integer(3))]
        );
    }

    #[test]
    fn an_array_of_an_alias_checks_each_element() {
        let schemas = schemas(
            "use std::validators::StringBounds\n\
             @validators = [StringBounds(min_chars = 1)]\ntype Name = string\n\
             struct S {\n    names: Name[]\n}\n",
        );
        let model = Validation::new(&schemas);
        let plan = plan_of(&model, &schemas[1].1, "S", "names");
        assert!(plan.checks.is_empty());
        assert_eq!(plan.elements.expect("element checks").checks.len(), 1);
    }

    #[test]
    fn a_nested_struct_with_checks_is_marked_and_one_without_is_not() {
        let schemas = schemas(
            "use std::validators::StringBounds\n\
             struct User {\n    @validators = [StringBounds(min_chars = 1)]\n    name: string\n}\n\
             struct Plain {\n    n: u32\n}\n\
             struct Post {\n    author: User\n    plain: Plain\n    replies: Post[]\n}\n",
        );
        let model = Validation::new(&schemas);
        let units = &schemas[1].1;
        assert!(plan_of(&model, units, "Post", "author").nested);
        assert!(plan_of(&model, units, "Post", "plain").is_empty());
        // Recursive through `Post[]`, and terminates.
        assert!(model.needs_validation("app", "Post"));
        assert!(plan_of(&model, units, "Post", "replies").elements.expect("elements").nested);
        assert!(!model.needs_validation("app", "Plain"));
    }

    #[test]
    fn constant_and_primitive_bound_arguments_resolve() {
        let schemas = schemas(
            "use std::validators::StringBounds\nconst MIN: u32 = 4\n\
             struct S {\n    @validators = [StringBounds(min_chars = MIN, max_chars = u64::MAX)]\n    \
             name: string\n}\n",
        );
        let model = Validation::new(&schemas);
        let plan = plan_of(&model, &schemas[1].1, "S", "name");
        assert_eq!(plan.checks[0].param("min_chars"), Some(&Value::Integer(4)));
        assert_eq!(
            plan.checks[0].param("max_chars"),
            Some(&Value::Integer(i128::from(u64::MAX)))
        );
    }

    #[test]
    fn a_validator_the_request_lacks_is_an_error_naming_the_use_site() {
        let schemas = schemas("struct S {\n    @validators = [Nope()]\n    name: string\n}\n");
        let model = Validation::new(&schemas);
        let (ty, parameters) = field(&schemas[1].1, "S", "name");
        let error = model.plan("app", ty, parameters, "struct 'S', field 'name'").unwrap_err();
        assert_eq!(
            error.to_string(),
            "struct 'S', field 'name': validator 'Nope' isn't declared in any schema being generated"
        );
    }

    #[test]
    fn validate_on_decode_names_round_trip() {
        for name in ValidateOnDecode::NAMES {
            assert_eq!(ValidateOnDecode::parse(name).map(ValidateOnDecode::name), Some(name));
        }
        assert_eq!(ValidateOnDecode::parse("sometimes"), None);
        assert_eq!(ValidateOnDecode::default(), ValidateOnDecode::Off);
    }
}
