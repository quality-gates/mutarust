use std::collections::BTreeSet;

use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ExprReturn, Generics, ReturnType, Type};

use crate::mutator::span_range;
use crate::value::{ShadowedNames, block_shadows, generic_shadows};
use crate::{Mutation, Mutator};

const DEFAULT: &str = "::core::default::Default::default()";
const NONE: &str = "::core::option::Option::None";

pub(crate) struct ReturnValueMutator;

impl Mutator for ReturnValueMutator {
    fn name(&self) -> &str {
        "statement/return"
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(file) = syn::parse_file(source) else {
            return Vec::new();
        };
        let default_types = collect_default_types(&file);
        let mut visitor = ReturnVisitor {
            source,
            default_types: &default_types,
            shadows: ShadowedNames::collect_items(file.items.iter()),
            scope: None,
            impl_default: false,
            mutations: Vec::new(),
        };
        visitor.visit_file(&file);
        visitor.mutations
    }
}

#[derive(Clone)]
struct ReturnScope {
    kind: Type,
    generic_defaults: BTreeSet<String>,
    self_default: bool,
}

struct ReturnVisitor<'source, 'types> {
    source: &'source str,
    default_types: &'types BTreeSet<String>,
    shadows: ShadowedNames,
    scope: Option<ReturnScope>,
    impl_default: bool,
    mutations: Vec<Mutation>,
}

impl ReturnVisitor<'_, '_> {
    fn with_scope(
        &mut self,
        output: &ReturnType,
        generics: &Generics,
        visit: impl FnOnce(&mut Self),
    ) {
        let previous = self.scope.take();
        let previous_shadows = self.shadows;
        self.shadows = previous_shadows.merged(generic_shadows(generics));
        self.scope = return_type(output).map(|kind| ReturnScope {
            kind: kind.clone(),
            generic_defaults: generic_defaults(generics),
            self_default: self.impl_default,
        });
        visit(self);
        self.scope = previous;
        self.shadows = previous_shadows;
    }

    fn add_return(&mut self, expression: &ExprReturn) {
        let Some(scope) = self.scope.clone() else {
            return;
        };
        let Some(value) = expression.expr.as_deref() else {
            return;
        };
        if let (Type::Tuple(kind), Expr::Tuple(values)) =
            (strip_type(&scope.kind), strip_expr(value))
        {
            if kind.elems.len() == values.elems.len() {
                for (kind, value) in kind.elems.iter().zip(&values.elems) {
                    self.add_value(kind, value, &scope);
                }
            }
            return;
        }
        self.add_value(&scope.kind, value, &scope);
    }

    fn add_value(&mut self, kind: &Type, value: &Expr, scope: &ReturnScope) {
        let Some(replacement) = default_for_type(
            kind,
            &scope.generic_defaults,
            self.default_types,
            scope.self_default,
        ) else {
            return;
        };
        if expression_is_default(value, &replacement, self.shadows) {
            return;
        }
        if let Some(range) = span_range(self.source, value.span()) {
            let needs_validation = replacement.contains("::core::default::Default")
                || replacement.contains("::core::option::Option");
            let mut mutation = Mutation::new(range, replacement);
            if needs_validation {
                mutation = mutation.requiring_compile_validation();
            }
            self.mutations.push(mutation);
        }
    }
}

impl<'ast> Visit<'ast> for ReturnVisitor<'_, '_> {
    fn visit_block(&mut self, block: &'ast syn::Block) {
        let previous = self.shadows;
        self.shadows = previous.merged(block_shadows(block));
        visit::visit_block(self, block);
        self.shadows = previous;
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let previous = self.shadows;
        if let Some((_, items)) = &item.content {
            self.shadows = ShadowedNames::collect_items(items.iter());
        }
        visit::visit_item_mod(self, item);
        self.shadows = previous;
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.with_scope(&item.sig.output, &item.sig.generics, |visitor| {
            visitor.visit_block(&item.block);
        });
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.with_scope(&item.sig.output, &item.sig.generics, |visitor| {
            visitor.visit_block(&item.block);
        });
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        let Some(block) = &item.default else {
            return;
        };
        self.with_scope(&item.sig.output, &item.sig.generics, |visitor| {
            visitor.visit_block(block);
        });
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let previous = self.impl_default;
        let previous_shadows = self.shadows;
        self.shadows = previous_shadows.merged(generic_shadows(&item.generics));
        let trait_default = item.trait_.as_ref().is_some_and(|(_, path, _)| {
            path.segments
                .last()
                .is_some_and(|segment| segment.ident == "Default")
        });
        let type_default = match strip_type(&item.self_ty) {
            Type::Path(path) => path
                .path
                .segments
                .last()
                .is_some_and(|segment| self.default_types.contains(&segment.ident.to_string())),
            _ => false,
        };
        self.impl_default = trait_default || type_default;
        visit::visit_item_impl(self, item);
        self.impl_default = previous;
        self.shadows = previous_shadows;
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        let previous_shadows = self.shadows;
        self.shadows = previous_shadows.merged(generic_shadows(&item.generics));
        visit::visit_item_trait(self, item);
        self.shadows = previous_shadows;
    }

    fn visit_expr_closure(&mut self, expression: &'ast syn::ExprClosure) {
        let previous = self.scope.take();
        self.scope = return_type(&expression.output).map(|kind| ReturnScope {
            kind: kind.clone(),
            generic_defaults: previous
                .as_ref()
                .map_or_else(BTreeSet::new, |scope| scope.generic_defaults.clone()),
            self_default: previous.as_ref().is_some_and(|scope| scope.self_default),
        });
        visit::visit_expr_closure(self, expression);
        self.scope = previous;
    }

    fn visit_expr_async(&mut self, expression: &'ast syn::ExprAsync) {
        let previous = self.scope.take();
        visit::visit_expr_async(self, expression);
        self.scope = previous;
    }

    fn visit_expr_return(&mut self, expression: &'ast ExprReturn) {
        self.add_return(expression);
        visit::visit_expr_return(self, expression);
    }
}

fn return_type(output: &ReturnType) -> Option<&Type> {
    match output {
        ReturnType::Default => None,
        ReturnType::Type(_, kind) => Some(kind),
    }
}

fn generic_defaults(generics: &Generics) -> BTreeSet<String> {
    let mut defaults = generics
        .params
        .iter()
        .filter_map(default_type_parameter)
        .collect::<BTreeSet<_>>();
    if let Some(clause) = &generics.where_clause {
        defaults.extend(clause.predicates.iter().filter_map(default_where_type));
    }
    defaults
}

fn default_type_parameter(parameter: &syn::GenericParam) -> Option<String> {
    let syn::GenericParam::Type(parameter) = parameter else {
        return None;
    };
    has_default_bound(&parameter.bounds).then(|| parameter.ident.to_string())
}

fn default_where_type(predicate: &syn::WherePredicate) -> Option<String> {
    let syn::WherePredicate::Type(predicate) = predicate else {
        return None;
    };
    if !has_default_bound(&predicate.bounds) {
        return None;
    }
    let Type::Path(path) = strip_type(&predicate.bounded_ty) else {
        return None;
    };
    (path.qself.is_none() && path.path.segments.len() == 1)
        .then(|| path.path.segments[0].ident.to_string())
}

fn has_default_bound(
    bounds: &syn::punctuated::Punctuated<syn::TypeParamBound, syn::token::Plus>,
) -> bool {
    bounds.iter().any(|bound| {
        matches!(bound, syn::TypeParamBound::Trait(bound) if bound.path.segments.last().is_some_and(|segment| segment.ident == "Default"))
    })
}

fn default_for_type(
    kind: &Type,
    generic_defaults: &BTreeSet<String>,
    default_types: &BTreeSet<String>,
    self_default: bool,
) -> Option<String> {
    match strip_type(kind) {
        Type::Path(path) => default_for_path(path, generic_defaults, default_types, self_default),
        Type::Reference(reference) => default_for_reference(reference),
        Type::Tuple(tuple) => {
            default_for_tuple(tuple, generic_defaults, default_types, self_default)
        }
        _ => None,
    }
}

fn default_for_path(
    path: &syn::TypePath,
    generic_defaults: &BTreeSet<String>,
    default_types: &BTreeSet<String>,
    self_default: bool,
) -> Option<String> {
    if path.qself.is_some() {
        return None;
    }
    let name = path.path.segments.last()?.ident.to_string();
    if let Some(default) = primitive_default(&name) {
        return Some(default.to_owned());
    }
    if name == "Option" {
        return Some(NONE.to_owned());
    }
    if name == "String" || name == "Vec" {
        return Some(DEFAULT.to_owned());
    }
    if name == "Self" && self_default {
        return Some(DEFAULT.to_owned());
    }
    (generic_defaults.contains(&name) || default_types.contains(&name)).then(|| DEFAULT.to_owned())
}

fn primitive_default(name: &str) -> Option<&'static str> {
    const INTEGERS: [&str; 12] = [
        "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
    ];
    match name {
        "bool" => Some("false"),
        "f32" | "f64" => Some("0.0"),
        "char" => Some("'\\0'"),
        name if INTEGERS.contains(&name) => Some("0"),
        _ => None,
    }
}

fn default_for_reference(reference: &syn::TypeReference) -> Option<String> {
    if reference.mutability.is_some() {
        return None;
    }
    match strip_type(&reference.elem) {
        Type::Path(path) if plain_path_name(path, "str") => Some("\"\"".to_owned()),
        Type::Slice(_) => Some("&[]".to_owned()),
        _ => None,
    }
}

fn plain_path_name(path: &syn::TypePath, expected: &str) -> bool {
    path.qself.is_none() && path.path.segments.len() == 1 && path.path.segments[0].ident == expected
}

fn default_for_tuple(
    tuple: &syn::TypeTuple,
    generic_defaults: &BTreeSet<String>,
    default_types: &BTreeSet<String>,
    self_default: bool,
) -> Option<String> {
    let values = tuple
        .elems
        .iter()
        .map(|kind| default_for_type(kind, generic_defaults, default_types, self_default))
        .collect::<Option<Vec<_>>>()?;
    let suffix = if values.len() == 1 { "," } else { "" };
    Some(format!("({}{suffix})", values.join(", ")))
}

fn expression_is_default(expression: &Expr, replacement: &str, shadows: ShadowedNames) -> bool {
    let expression = strip_expr(expression);
    if matches!(expression, Expr::Call(call) if crate::value::is_known_default_call(call, shadows))
    {
        return true;
    }
    match expression {
        Expr::Lit(literal) => literal_matches_default(&literal.lit, replacement),
        Expr::Unary(unary) => negative_expression_matches_default(unary, replacement),
        Expr::Path(path) => path_matches_default(path, replacement, shadows),
        Expr::Reference(reference) => reference_matches_default(reference, replacement),
        Expr::Tuple(tuple) => replacement == "()" && tuple.elems.is_empty(),
        _ => false,
    }
}

fn negative_expression_matches_default(expression: &syn::ExprUnary, replacement: &str) -> bool {
    matches!(expression.op, syn::UnOp::Neg(_))
        && matches!(replacement, "0" | "0.0")
        && crate::value::negative_integer_is_zero(&expression.expr)
}

fn literal_matches_default(literal: &syn::Lit, replacement: &str) -> bool {
    match literal {
        syn::Lit::Bool(value) => boolean_matches_default(value, replacement),
        syn::Lit::Int(value) => integer_matches_default(value, replacement),
        syn::Lit::Float(value) => float_matches_default(value, replacement),
        syn::Lit::Char(value) => character_matches_default(value, replacement),
        syn::Lit::Str(value) => string_matches_default(value, replacement),
        _ => false,
    }
}

fn boolean_matches_default(value: &syn::LitBool, replacement: &str) -> bool {
    replacement == "false" && !value.value
}

fn integer_matches_default(value: &syn::LitInt, replacement: &str) -> bool {
    replacement == "0" && value.base10_parse::<u128>().ok() == Some(0)
}

fn float_matches_default(value: &syn::LitFloat, replacement: &str) -> bool {
    replacement == "0.0" && value.base10_parse::<f64>().ok() == Some(0.0)
}

fn character_matches_default(value: &syn::LitChar, replacement: &str) -> bool {
    replacement == "'\\0'" && value.value() == '\0'
}

fn string_matches_default(value: &syn::LitStr, replacement: &str) -> bool {
    replacement == "\"\"" && value.value().is_empty()
}

fn path_matches_default(path: &syn::ExprPath, replacement: &str, shadows: ShadowedNames) -> bool {
    replacement == NONE && crate::value::is_known_none(path, shadows)
}

fn reference_matches_default(reference: &syn::ExprReference, replacement: &str) -> bool {
    replacement == "&[]"
        && matches!(strip_expr(&reference.expr), Expr::Array(array) if array.elems.is_empty())
}

fn collect_default_types(file: &syn::File) -> BTreeSet<String> {
    let mut collector = DefaultTypeCollector::default();
    collector.visit_file(file);
    collector.types
}

#[derive(Default)]
struct DefaultTypeCollector {
    types: BTreeSet<String>,
}

impl DefaultTypeCollector {
    fn add_derived(&mut self, identifier: &syn::Ident, attributes: &[syn::Attribute]) {
        let derived = attributes.iter().any(|attribute| {
            if !attribute.path().is_ident("derive") {
                return false;
            }
            let mut found = false;
            let _ = attribute.parse_nested_meta(|meta| {
                found |= meta
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "Default");
                Ok(())
            });
            found
        });
        if derived {
            self.types.insert(identifier.to_string());
        }
    }
}

impl<'ast> Visit<'ast> for DefaultTypeCollector {
    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        self.add_derived(&item.ident, &item.attrs);
        visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        self.add_derived(&item.ident, &item.attrs);
        visit::visit_item_enum(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if item.trait_.as_ref().is_some_and(|(_, path, _)| {
            path.segments
                .last()
                .is_some_and(|segment| segment.ident == "Default")
        }) {
            if let Type::Path(path) = strip_type(&item.self_ty) {
                if let Some(segment) = path.path.segments.last() {
                    self.types.insert(segment.ident.to_string());
                }
            }
        }
        visit::visit_item_impl(self, item);
    }
}

fn strip_type(mut kind: &Type) -> &Type {
    loop {
        kind = match kind {
            Type::Group(group) => &group.elem,
            Type::Paren(parenthesized) => &parenthesized.elem,
            _ => return kind,
        };
    }
}

fn strip_expr(mut expression: &Expr) -> &Expr {
    loop {
        expression = match expression {
            Expr::Group(group) => &group.expr,
            Expr::Paren(parenthesized) => &parenthesized.expr,
            _ => return expression,
        };
    }
}
