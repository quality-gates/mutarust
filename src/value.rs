use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::Span;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, ExprAssign, ExprCall, ExprField, ExprMethodCall, ExprStruct, FieldValue, Member};

use crate::mutator::span_range;
use crate::{Mutation, Mutator};

const NONE: &str = "::core::option::Option::None";

#[derive(Clone, Copy)]
enum ValueKind {
    CompositeFieldClear,
    ContextNil,
    RemoveSelfAssign,
}

pub(crate) struct ValueMutator {
    name: &'static str,
    kind: ValueKind,
}

impl ValueMutator {
    pub(crate) const fn composite_field_clear() -> Self {
        Self {
            name: "composite/field-clear",
            kind: ValueKind::CompositeFieldClear,
        }
    }

    pub(crate) const fn context_nil() -> Self {
        Self {
            name: "expression/context-nil",
            kind: ValueKind::ContextNil,
        }
    }

    pub(crate) const fn remove_self_assign() -> Self {
        Self {
            name: "statement/remove-self-assign",
            kind: ValueKind::RemoveSelfAssign,
        }
    }
}

impl Mutator for ValueMutator {
    fn name(&self) -> &str {
        self.name
    }

    fn mutations(&self, source: &str) -> Vec<Mutation> {
        let Ok(file) = syn::parse_file(source) else {
            return Vec::new();
        };
        let shadows = ShadowedNames::collect_items(file.items.iter());

        match self.kind {
            ValueKind::CompositeFieldClear => CompositeVisitor::mutations(
                source,
                &file,
                shadows,
                derived_default_names(file.items.iter()),
            ),
            ValueKind::ContextNil => ContextVisitor::mutations(
                source,
                &file,
                shadows,
                context_signatures_for_items(&file.items),
            ),
            ValueKind::RemoveSelfAssign => SelfAssignVisitor::mutations(source, &file),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ShadowedNames {
    alloc: bool,
    core: bool,
    default: bool,
    none: bool,
    option: bool,
    some: bool,
    string: bool,
    std: bool,
    vector: bool,
}

impl ShadowedNames {
    pub(crate) fn collect_items<'a>(items: impl Iterator<Item = &'a syn::Item>) -> Self {
        let mut names = Self::default();
        for item in items {
            names.record_type_identifier(type_namespace_identifier(item));
            names.record_value_identifier(value_namespace_identifier(item));
            if let syn::Item::Use(item) = item {
                names.record_use(&item.tree);
            }
        }
        names
    }

    fn record_type_identifier(&mut self, identifier: Option<&syn::Ident>) {
        if let Some(identifier) = identifier {
            self.alloc |= identifier == "alloc";
            self.core |= identifier == "core";
            self.default |= identifier == "Default";
            self.option |= identifier == "Option";
            self.string |= identifier == "String";
            self.std |= identifier == "std";
            self.vector |= identifier == "Vec";
        }
    }

    fn record_value_identifier(&mut self, identifier: Option<&syn::Ident>) {
        if let Some(identifier) = identifier {
            self.none |= identifier == "None";
            self.some |= identifier == "Some";
        }
    }

    fn record_use(&mut self, tree: &syn::UseTree) {
        if use_tree_has_glob(tree) {
            self.alloc = true;
            self.core = true;
            self.default = true;
            self.none = true;
            self.option = true;
            self.some = true;
            self.string = true;
            self.std = true;
            self.vector = true;
            return;
        }
        self.alloc |= use_tree_imports(tree, "alloc", None);
        self.core |= use_tree_imports(tree, "core", None);
        self.default |= use_tree_imports(tree, "Default", None);
        self.none |= use_tree_imports(tree, "None", None);
        self.option |= use_tree_imports(tree, "Option", None);
        self.some |= use_tree_imports(tree, "Some", None);
        self.string |= use_tree_imports(tree, "String", None);
        self.std |= use_tree_imports(tree, "std", None);
        self.vector |= use_tree_imports(tree, "Vec", None);
    }

    pub(crate) fn merged(self, other: Self) -> Self {
        let mut merged = self;
        macro_rules! merge_fields {
            ($($field:ident),+ $(,)?) => {
                $(merged.$field |= other.$field;)+
            };
        }
        merge_fields!(
            alloc, core, default, none, option, some, string, std, vector
        );
        merged
    }
}

fn type_namespace_identifier(item: &syn::Item) -> Option<&syn::Ident> {
    match item {
        syn::Item::ExternCrate(item) => item.rename.as_ref().map(|(_, identifier)| identifier),
        syn::Item::Mod(item) => Some(&item.ident),
        _ => type_item_identifier(item),
    }
}

fn value_namespace_identifier(item: &syn::Item) -> Option<&syn::Ident> {
    match item {
        syn::Item::Const(item) => Some(&item.ident),
        syn::Item::Fn(item) => Some(&item.sig.ident),
        syn::Item::Static(item) => Some(&item.ident),
        syn::Item::Struct(item) if !matches!(item.fields, syn::Fields::Named(_)) => {
            Some(&item.ident)
        }
        _ => None,
    }
}

fn type_item_identifier(item: &syn::Item) -> Option<&syn::Ident> {
    match item {
        syn::Item::Enum(item) => Some(&item.ident),
        syn::Item::Struct(item) => Some(&item.ident),
        syn::Item::Trait(item) => Some(&item.ident),
        syn::Item::TraitAlias(item) => Some(&item.ident),
        syn::Item::Type(item) => Some(&item.ident),
        syn::Item::Union(item) => Some(&item.ident),
        _ => None,
    }
}

fn use_tree_has_glob(tree: &syn::UseTree) -> bool {
    match tree {
        syn::UseTree::Path(path) => use_tree_has_glob(&path.tree),
        syn::UseTree::Group(group) => group.items.iter().any(use_tree_has_glob),
        syn::UseTree::Glob(_) => true,
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) => false,
    }
}

pub(crate) fn block_shadows(block: &syn::Block) -> ShadowedNames {
    ShadowedNames::collect_items(block.stmts.iter().filter_map(|statement| match statement {
        syn::Stmt::Item(item) => Some(item),
        _ => None,
    }))
}

pub(crate) fn generic_shadows(generics: &syn::Generics) -> ShadowedNames {
    let mut shadows = ShadowedNames::default();
    for parameter in &generics.params {
        if let syn::GenericParam::Type(parameter) = parameter {
            shadows.record_type_identifier(Some(&parameter.ident));
        }
    }
    shadows
}

fn derived_default_names<'a>(items: impl Iterator<Item = &'a syn::Item>) -> BTreeSet<String> {
    let items = items.collect::<Vec<_>>();
    let fixed_shadows = ShadowedNames::collect_items(items.iter().copied());
    items
        .into_iter()
        .filter_map(|item| match item {
            syn::Item::Struct(item) if has_default_derive(&item.attrs, fixed_shadows) => {
                Some(item.ident.to_string())
            }
            _ => None,
        })
        .collect()
}

fn has_default_derive(attributes: &[syn::Attribute], shadows: ShadowedNames) -> bool {
    attributes.iter().any(|attribute| {
        if !attribute.path().is_ident("derive") {
            return false;
        }
        let mut found = false;
        let _ = attribute.parse_nested_meta(|meta| {
            let path = meta
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            found |= match path.as_str() {
                "Default" => !shadows.default,
                "core::default::Default" => meta.path.leading_colon.is_some() || !shadows.core,
                "std::default::Default" => meta.path.leading_colon.is_some() || !shadows.std,
                _ => false,
            };
            Ok(())
        });
        found
    })
}

fn block_derived_defaults(block: &syn::Block) -> BTreeSet<String> {
    derived_default_names(block.stmts.iter().filter_map(|statement| match statement {
        syn::Stmt::Item(item) => Some(item),
        _ => None,
    }))
}

#[derive(Clone, Default)]
struct ContextSignatures {
    functions: BTreeMap<String, Option<Vec<bool>>>,
    all_ambiguous: bool,
}

impl ContextSignatures {
    fn collect_items<'a>(items: impl Iterator<Item = &'a syn::Item>) -> Self {
        let mut signatures = Self::default();
        for item in items {
            if let syn::Item::Fn(function) = item {
                signatures.record(&function.sig);
            }
        }
        signatures
    }

    fn record(&mut self, signature: &syn::Signature) {
        let name = signature.ident.to_string();
        let parameters = concrete_option_parameters(signature);
        self.functions
            .entry(name)
            .and_modify(|known| *known = None)
            .or_insert(Some(parameters));
    }

    fn overlay(&mut self, other: Self) {
        if other.all_ambiguous {
            self.shadow_all();
        }
        self.functions.extend(other.functions);
    }

    fn shadow(&mut self, names: &BTreeSet<String>) {
        for name in names {
            self.functions.insert(name.clone(), None);
        }
    }

    fn shadow_all(&mut self) {
        self.all_ambiguous = true;
        for signature in self.functions.values_mut() {
            *signature = None;
        }
    }

    fn parameters(&self, function: &Expr) -> Option<&[bool]> {
        if self.all_ambiguous {
            return None;
        }
        let Expr::Path(function) = strip_expression(function) else {
            return None;
        };
        if function.qself.is_some() || function.path.segments.len() != 1 {
            return None;
        }
        let name = function.path.segments[0].ident.to_string();
        self.functions.get(&name)?.as_deref()
    }
}

fn concrete_option_parameters(signature: &syn::Signature) -> Vec<bool> {
    let generic_names = signature
        .generics
        .params
        .iter()
        .filter_map(|parameter| match parameter {
            syn::GenericParam::Type(parameter) => Some(parameter.ident.to_string()),
            syn::GenericParam::Const(parameter) => Some(parameter.ident.to_string()),
            syn::GenericParam::Lifetime(_) => None,
        })
        .collect::<BTreeSet<_>>();
    signature
        .inputs
        .iter()
        .filter_map(|input| match input {
            syn::FnArg::Typed(input) => Some(
                is_option_type(&input.ty)
                    && !type_uses_generic_parameter(&input.ty, &generic_names),
            ),
            syn::FnArg::Receiver(_) => None,
        })
        .collect()
}

fn type_uses_generic_parameter(kind: &syn::Type, names: &BTreeSet<String>) -> bool {
    let mut collector = GenericReferenceCollector {
        names,
        found: false,
    };
    collector.visit_type(kind);
    collector.found
}

struct GenericReferenceCollector<'names> {
    names: &'names BTreeSet<String>,
    found: bool,
}

impl<'ast> Visit<'ast> for GenericReferenceCollector<'_> {
    fn visit_type_macro(&mut self, _kind: &'ast syn::TypeMacro) {
        self.found = true;
    }

    fn visit_expr_macro(&mut self, _expression: &'ast syn::ExprMacro) {
        self.found = true;
    }

    fn visit_type_impl_trait(&mut self, _kind: &'ast syn::TypeImplTrait) {
        self.found = true;
    }

    fn visit_type_path(&mut self, kind: &'ast syn::TypePath) {
        if kind
            .path
            .segments
            .first()
            .is_some_and(|segment| self.names.contains(&segment.ident.to_string()))
        {
            self.found = true;
        }
        visit::visit_type_path(self, kind);
    }

    fn visit_expr_path(&mut self, expression: &'ast syn::ExprPath) {
        if expression
            .path
            .segments
            .first()
            .is_some_and(|segment| self.names.contains(&segment.ident.to_string()))
        {
            self.found = true;
        }
        visit::visit_expr_path(self, expression);
    }
}

fn is_option_type(kind: &syn::Type) -> bool {
    let syn::Type::Path(path) = kind else {
        return false;
    };
    path.qself.is_none()
        && path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Option")
}

fn block_context_signatures(block: &syn::Block) -> ContextSignatures {
    let mut signatures = ContextSignatures::collect_items(block.stmts.iter().filter_map(
        |statement| match statement {
            syn::Stmt::Item(item) => Some(item),
            _ => None,
        },
    ));
    let bindings = block_item_bindings(block);
    signatures.shadow(&bindings.names);
    if bindings.all {
        signatures.shadow_all();
    }
    signatures
}

fn context_signatures_for_items(items: &[syn::Item]) -> ContextSignatures {
    let mut signatures = ContextSignatures::collect_items(items.iter());
    let bindings = item_bindings(items.iter());
    signatures.shadow(&bindings.names);
    if bindings.all {
        signatures.shadow_all();
    }
    signatures
}

#[derive(Default)]
struct ScopeBindings {
    names: BTreeSet<String>,
    all: bool,
}

#[derive(Default)]
struct PatternBindingCollector {
    names: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for PatternBindingCollector {
    fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
        self.names.insert(pattern.ident.to_string());
        visit::visit_pat_ident(self, pattern);
    }

    fn visit_pat_guard(&mut self, pattern: &'ast syn::PatGuard) {
        // Match-arm guards are Pat::Guard in syn 3. The default Visit walk enters the
        // guard expression and would collect closure or if-let bindings as arm bindings.
        self.visit_pat(&pattern.pat);
    }
}

fn pattern_bindings<'a>(patterns: impl Iterator<Item = &'a syn::Pat>) -> BTreeSet<String> {
    let mut bindings = PatternBindingCollector::default();
    for pattern in patterns {
        bindings.visit_pat(pattern);
    }
    bindings.names
}

fn item_bindings<'a>(items: impl Iterator<Item = &'a syn::Item>) -> ScopeBindings {
    let mut bindings = ScopeBindings::default();
    for item in items {
        if let Some(identifier) = value_item_identifier(item) {
            bindings.names.insert(identifier.to_string());
        }
        if let syn::Item::Use(item) = item {
            bindings.all |= collect_use_bindings(&item.tree, None, &mut bindings.names);
        }
    }
    bindings
}

fn value_item_identifier(item: &syn::Item) -> Option<&syn::Ident> {
    match item {
        syn::Item::Fn(_) => None,
        _ => value_namespace_identifier(item),
    }
}

fn type_item_bindings<'a>(items: impl Iterator<Item = &'a syn::Item>) -> ScopeBindings {
    let mut bindings = ScopeBindings::default();
    for item in items {
        let identifier = match item {
            syn::Item::ExternCrate(item) => Some(
                item.rename
                    .as_ref()
                    .map_or(&item.ident, |(_, identifier)| identifier),
            ),
            syn::Item::Mod(item) => Some(&item.ident),
            _ => type_item_identifier(item),
        };
        if let Some(identifier) = identifier {
            bindings.names.insert(identifier.to_string());
        }
        if let syn::Item::Use(item) = item {
            bindings.all |= collect_use_bindings(&item.tree, None, &mut bindings.names);
        }
    }
    bindings
}

fn block_item_bindings(block: &syn::Block) -> ScopeBindings {
    item_bindings(block.stmts.iter().filter_map(|statement| match statement {
        syn::Stmt::Item(item) => Some(item),
        _ => None,
    }))
}

fn collect_use_bindings(
    tree: &syn::UseTree,
    parent: Option<&syn::Ident>,
    names: &mut BTreeSet<String>,
) -> bool {
    match tree {
        syn::UseTree::Path(path) => collect_use_bindings(&path.tree, Some(&path.ident), names),
        syn::UseTree::Name(name) => {
            let identifier = if name.ident == "self" {
                parent.unwrap_or(&name.ident)
            } else {
                &name.ident
            };
            names.insert(identifier.to_string());
            false
        }
        syn::UseTree::Rename(rename) => {
            names.insert(rename.rename.to_string());
            false
        }
        syn::UseTree::Group(group) => group
            .items
            .iter()
            .any(|tree| collect_use_bindings(tree, parent, names)),
        syn::UseTree::Glob(_) => true,
    }
}

fn use_tree_imports(tree: &syn::UseTree, target: &str, parent: Option<&syn::Ident>) -> bool {
    match tree {
        syn::UseTree::Path(path) => use_tree_imports(&path.tree, target, Some(&path.ident)),
        syn::UseTree::Name(name) => {
            name.ident == target
                || (name.ident == "self" && parent.is_some_and(|name| name == target))
        }
        syn::UseTree::Rename(rename) => rename.rename == target,
        syn::UseTree::Group(group) => group
            .items
            .iter()
            .any(|tree| use_tree_imports(tree, target, parent)),
        syn::UseTree::Glob(_) => false,
    }
}

struct CompositeVisitor<'source> {
    source: &'source str,
    shadows: ShadowedNames,
    derived_defaults: BTreeSet<String>,
    mutations: Vec<Mutation>,
}

impl<'source> CompositeVisitor<'source> {
    fn mutations(
        source: &'source str,
        file: &syn::File,
        shadows: ShadowedNames,
        derived_defaults: BTreeSet<String>,
    ) -> Vec<Mutation> {
        let mut visitor = Self {
            source,
            shadows,
            derived_defaults,
            mutations: Vec::new(),
        };
        visitor.visit_file(file);
        visitor.mutations
    }

    fn add_field_removal(&mut self, expression: &ExprStruct, index: usize) {
        let field = &expression.fields[index];
        if known_default(&field.expr, self.shadows) {
            return;
        }
        let start = field.span().byte_range().start;
        let end = expression.fields.get(index + 1).map_or_else(
            || {
                expression
                    .dot2_token
                    .map(|token| token.span().byte_range().start)
            },
            |next| Some(next.span().byte_range().start),
        );
        if let Some(end) = end {
            self.add_range(start..end, "");
        }
    }

    fn add_field_default(&mut self, field: &FieldValue) {
        if field.colon_token.is_none() || known_default(&field.expr, self.shadows) {
            return;
        }
        if let Some(replacement) = direct_default(&field.expr, self.shadows) {
            self.add_span(field.expr.span(), &replacement);
        }
    }

    fn add_range(&mut self, range: std::ops::Range<usize>, replacement: &str) {
        if range.start < range.end && range.end <= self.source.len() {
            self.mutations.push(Mutation::new(range, replacement));
        }
    }

    fn add_span(&mut self, span: Span, replacement: &str) {
        if let Some(range) = span_range(self.source, span) {
            self.mutations.push(Mutation::new(range, replacement));
        }
    }

    fn with_generics(&mut self, generics: &syn::Generics, visit: impl FnOnce(&mut Self)) {
        let previous_shadows = self.shadows;
        let previous_defaults = self.derived_defaults.clone();
        self.shadows = previous_shadows.merged(generic_shadows(generics));
        for parameter in &generics.params {
            if let syn::GenericParam::Type(parameter) = parameter {
                self.derived_defaults.remove(&parameter.ident.to_string());
            }
        }
        visit(self);
        self.shadows = previous_shadows;
        self.derived_defaults = previous_defaults;
    }
}

impl<'ast> Visit<'ast> for CompositeVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_block(&mut self, block: &'ast syn::Block) {
        let previous_shadows = self.shadows;
        let previous_defaults = self.derived_defaults.clone();
        let bindings =
            type_item_bindings(block.stmts.iter().filter_map(|statement| match statement {
                syn::Stmt::Item(item) => Some(item),
                _ => None,
            }));
        self.shadows = previous_shadows.merged(block_shadows(block));
        if bindings.all {
            self.derived_defaults.clear();
        } else {
            self.derived_defaults
                .retain(|name| !bindings.names.contains(name));
        }
        self.derived_defaults.extend(block_derived_defaults(block));
        visit::visit_block(self, block);
        self.shadows = previous_shadows;
        self.derived_defaults = previous_defaults;
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let previous_shadows = self.shadows;
        let previous_defaults = self.derived_defaults.clone();
        if let Some((_, items)) = &item.content {
            self.shadows = ShadowedNames::collect_items(items.iter());
            self.derived_defaults = derived_default_names(items.iter());
        }
        visit::visit_item_mod(self, item);
        self.shadows = previous_shadows;
        self.derived_defaults = previous_defaults;
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.with_generics(&item.sig.generics, |visitor| {
            visitor.visit_block(&item.block)
        });
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.with_generics(&item.sig.generics, |visitor| {
            visitor.visit_block(&item.block)
        });
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        if let Some(block) = &item.default {
            self.with_generics(&item.sig.generics, |visitor| visitor.visit_block(block));
        }
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        self.with_generics(&item.generics, |visitor| {
            visit::visit_item_impl(visitor, item)
        });
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        self.with_generics(&item.generics, |visitor| {
            visit::visit_item_trait(visitor, item)
        });
    }

    fn visit_expr_struct(&mut self, expression: &'ast ExprStruct) {
        if has_default_rest(expression, &self.derived_defaults, self.shadows) {
            for (index, _) in expression.fields.iter().enumerate() {
                self.add_field_removal(expression, index);
            }
        } else if expression.rest.is_none() {
            for field in &expression.fields {
                self.add_field_default(field);
            }
        }
        visit::visit_expr_struct(self, expression);
    }
}

fn has_default_rest(
    expression: &ExprStruct,
    derived_defaults: &BTreeSet<String>,
    shadows: ShadowedNames,
) -> bool {
    let Some(Expr::Call(call)) = expression.rest.as_deref().map(strip_expression) else {
        return false;
    };
    if !call.args.is_empty() {
        return false;
    }
    let Expr::Path(function) = strip_expression(&call.func) else {
        return false;
    };
    if expression.path.leading_colon.is_some() || expression.path.segments.len() != 1 {
        return false;
    }
    let type_name = &expression.path.segments[0].ident;
    derived_defaults.contains(&type_name.to_string()) && is_default_trait_call(function, shadows)
}

fn direct_default(expression: &Expr, shadows: ShadowedNames) -> Option<String> {
    match strip_expression(expression) {
        Expr::Lit(literal) => direct_literal_default(&literal.lit),
        Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Neg(_)) => {
            negative_numeric_default(&unary.expr)
        }
        expression if is_some_call(expression, shadows) => Some(NONE.to_owned()),
        _ => None,
    }
}

fn negative_numeric_default(expression: &Expr) -> Option<String> {
    match strip_expression(expression) {
        Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(value),
            ..
        }) => nonzero_integer_default(value),
        Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Float(value),
            ..
        }) => value
            .base10_parse::<f64>()
            .ok()
            .map(|_| format!("0.0{}", value.suffix())),
        _ => None,
    }
}

fn direct_literal_default(literal: &syn::Lit) -> Option<String> {
    match literal {
        syn::Lit::Bool(value) => value.value.then(|| "false".to_owned()),
        syn::Lit::Int(value) => nonzero_integer_default(value),
        syn::Lit::Float(value) => nonzero_float_default(value),
        syn::Lit::Char(value) => (value.value() != '\0').then(|| "'\\0'".to_owned()),
        syn::Lit::Str(value) => (!value.value().is_empty()).then(|| "\"\"".to_owned()),
        syn::Lit::ByteStr(value) => (!value.value().is_empty()).then(|| "b\"\"".to_owned()),
        _ => None,
    }
}

fn nonzero_integer_default(value: &syn::LitInt) -> Option<String> {
    value
        .base10_parse::<u128>()
        .ok()
        .is_some_and(|number| number != 0)
        .then(|| format!("0{}", value.suffix()))
}

fn nonzero_float_default(value: &syn::LitFloat) -> Option<String> {
    value
        .base10_parse::<f64>()
        .ok()
        .is_some_and(|number| number != 0.0)
        .then(|| format!("0.0{}", value.suffix()))
}

fn known_default(expression: &Expr, shadows: ShadowedNames) -> bool {
    match strip_expression(expression) {
        Expr::Array(array) => array.elems.is_empty(),
        Expr::Reference(reference) => {
            matches!(strip_expression(&reference.expr), Expr::Array(array) if array.elems.is_empty())
        }
        Expr::Tuple(tuple) => tuple.elems.is_empty(),
        Expr::Lit(literal) => literal_is_default(&literal.lit),
        Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Neg(_)) => {
            negative_integer_is_zero(&unary.expr)
        }
        Expr::Path(path) => is_known_none(path, shadows),
        Expr::Call(call) => is_known_default_call(call, shadows),
        _ => false,
    }
}

pub(crate) fn negative_integer_is_zero(expression: &Expr) -> bool {
    match strip_expression(expression) {
        Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(value),
            ..
        }) => value.base10_parse::<u128>().ok() == Some(0),
        _ => false,
    }
}

pub(crate) fn is_known_none(expression: &syn::ExprPath, shadows: ShadowedNames) -> bool {
    let path = expression
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");
    match path.as_str() {
        "None" => !shadows.none,
        "Option::None" => !shadows.option,
        "core::option::Option::None" => expression.path.leading_colon.is_some() || !shadows.core,
        "std::option::Option::None" => expression.path.leading_colon.is_some() || !shadows.std,
        _ => false,
    }
}

fn literal_is_default(literal: &syn::Lit) -> bool {
    match literal {
        syn::Lit::Bool(value) => !value.value,
        syn::Lit::Int(value) => value.base10_parse::<u128>().ok() == Some(0),
        syn::Lit::Float(value) => value.base10_parse::<f64>().ok() == Some(0.0),
        syn::Lit::Char(value) => value.value() == '\0',
        syn::Lit::Str(value) => value.value().is_empty(),
        syn::Lit::ByteStr(value) => value.value().is_empty(),
        _ => false,
    }
}

pub(crate) fn is_known_default_call(call: &ExprCall, shadows: ShadowedNames) -> bool {
    if !call.args.is_empty() {
        return false;
    }
    let Expr::Path(path) = strip_expression(&call.func) else {
        return false;
    };
    if is_default_trait_call(path, shadows) {
        return true;
    }
    let segments = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");
    is_known_collection_constructor(path, &segments, shadows)
}

fn is_known_collection_constructor(
    path: &syn::ExprPath,
    segments: &str,
    shadows: ShadowedNames,
) -> bool {
    match segments {
        "String::new" => path.path.leading_colon.is_none() && !shadows.string,
        "Vec::new" => path.path.leading_colon.is_none() && !shadows.vector,
        "std::string::String::new" | "std::vec::Vec::new" => {
            path.path.leading_colon.is_some() || !shadows.std
        }
        "alloc::string::String::new" | "alloc::vec::Vec::new" => {
            path.path.leading_colon.is_some() || !shadows.alloc
        }
        _ => false,
    }
}

fn is_default_trait_call(path: &syn::ExprPath, shadows: ShadowedNames) -> bool {
    if path.qself.is_some() {
        return false;
    }
    let segments = path
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");
    match segments.as_str() {
        "Default::default" => path.path.leading_colon.is_none() && !shadows.default,
        "core::default::Default::default" => path.path.leading_colon.is_some() || !shadows.core,
        "std::default::Default::default" => path.path.leading_colon.is_some() || !shadows.std,
        _ => false,
    }
}

struct ContextVisitor<'source> {
    source: &'source str,
    shadows: ShadowedNames,
    signatures: ContextSignatures,
    mutations: Vec<Mutation>,
}

impl<'source> ContextVisitor<'source> {
    fn mutations(
        source: &'source str,
        file: &syn::File,
        shadows: ShadowedNames,
        signatures: ContextSignatures,
    ) -> Vec<Mutation> {
        let mut visitor = Self {
            source,
            shadows,
            signatures,
            mutations: Vec::new(),
        };
        visitor.visit_file(file);
        visitor.mutations
    }

    fn add_arguments(
        &mut self,
        arguments: &Punctuated<Expr, syn::token::Comma>,
        concrete_options: Option<&[bool]>,
    ) {
        for (index, argument) in arguments.iter().enumerate() {
            if is_some_call(argument, self.shadows) {
                if let Some(range) = span_range(self.source, argument.span()) {
                    let mut mutation = Mutation::new(range, NONE);
                    if concrete_options.and_then(|parameters| parameters.get(index)) != Some(&true)
                    {
                        mutation = mutation.requiring_compile_validation();
                    }
                    self.mutations.push(mutation);
                }
            }
        }
    }
}

fn with_context_bindings<Result>(
    visitor: &mut ContextVisitor<'_>,
    generics: Option<&syn::Generics>,
    bindings: BTreeSet<String>,
    visit: impl FnOnce(&mut ContextVisitor<'_>) -> Result,
) -> Result {
    let previous_shadows = visitor.shadows;
    let previous_signatures = visitor.signatures.clone();
    if let Some(generics) = generics {
        visitor.shadows = previous_shadows.merged(generic_shadows(generics));
    }
    visitor.signatures.shadow(&bindings);
    let result = visit(visitor);
    visitor.shadows = previous_shadows;
    visitor.signatures = previous_signatures;
    result
}

fn visit_condition(visitor: &mut ContextVisitor<'_>, expression: &Expr) -> BTreeSet<String> {
    match strip_expression(expression) {
        Expr::Binary(binary) if matches!(binary.op, syn::BinOp::And(_)) => {
            let mut bindings = visit_condition(visitor, &binary.left);
            let right = with_context_bindings(visitor, None, bindings.clone(), |visitor| {
                visit_condition(visitor, &binary.right)
            });
            bindings.extend(right);
            bindings
        }
        Expr::Let(expression) => {
            visitor.visit_expr(&expression.expr);
            pattern_bindings(std::iter::once(expression.pat.as_ref()))
        }
        _ => {
            visitor.visit_expr(expression);
            BTreeSet::new()
        }
    }
}

fn input_bindings(inputs: &Punctuated<syn::FnArg, syn::token::Comma>) -> BTreeSet<String> {
    pattern_bindings(inputs.iter().filter_map(|input| match input {
        syn::FnArg::Typed(input) => Some(input.pat.as_ref()),
        syn::FnArg::Receiver(_) => None,
    }))
}

impl<'ast> Visit<'ast> for ContextVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_block(&mut self, block: &'ast syn::Block) {
        let previous_shadows = self.shadows;
        let previous_signatures = self.signatures.clone();
        self.shadows = previous_shadows.merged(block_shadows(block));
        self.signatures.overlay(block_context_signatures(block));
        for statement in &block.stmts {
            visit::visit_stmt(self, statement);
            if let syn::Stmt::Local(local) = statement {
                self.signatures
                    .shadow(&pattern_bindings(std::iter::once(&local.pat)));
            }
        }
        self.shadows = previous_shadows;
        self.signatures = previous_signatures;
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let previous_shadows = self.shadows;
        let previous_signatures = self.signatures.clone();
        if let Some((_, items)) = &item.content {
            self.shadows = ShadowedNames::collect_items(items.iter());
            self.signatures = context_signatures_for_items(items);
        }
        visit::visit_item_mod(self, item);
        self.shadows = previous_shadows;
        self.signatures = previous_signatures;
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        with_context_bindings(
            self,
            Some(&item.sig.generics),
            input_bindings(&item.sig.inputs),
            |visitor| visitor.visit_block(&item.block),
        );
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        with_context_bindings(
            self,
            Some(&item.sig.generics),
            input_bindings(&item.sig.inputs),
            |visitor| visitor.visit_block(&item.block),
        );
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        if let Some(block) = &item.default {
            with_context_bindings(
                self,
                Some(&item.sig.generics),
                input_bindings(&item.sig.inputs),
                |visitor| visitor.visit_block(block),
            );
        }
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        with_context_bindings(self, Some(&item.generics), BTreeSet::new(), |visitor| {
            visit::visit_item_impl(visitor, item);
        });
    }

    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        with_context_bindings(self, Some(&item.generics), BTreeSet::new(), |visitor| {
            visit::visit_item_trait(visitor, item);
        });
    }

    fn visit_expr_closure(&mut self, expression: &'ast syn::ExprClosure) {
        let bindings = pattern_bindings(expression.inputs.iter());
        with_context_bindings(self, None, bindings, |visitor| {
            visitor.visit_expr(&expression.body);
        });
    }

    fn visit_arm(&mut self, arm: &'ast syn::Arm) {
        let bindings = pattern_bindings(std::iter::once(&arm.pat));
        with_context_bindings(self, None, bindings, |visitor| {
            if let syn::Pat::Guard(pattern) = &arm.pat {
                visitor.visit_expr(&pattern.guard);
            }
            visitor.visit_expr(&arm.body);
        });
    }

    fn visit_expr_if(&mut self, expression: &'ast syn::ExprIf) {
        let bindings = visit_condition(self, &expression.cond);
        with_context_bindings(self, None, bindings, |visitor| {
            visitor.visit_block(&expression.then_branch);
        });
        if let Some((_, alternative)) = &expression.else_branch {
            self.visit_expr(alternative);
        }
    }

    fn visit_expr_while(&mut self, expression: &'ast syn::ExprWhile) {
        let bindings = visit_condition(self, &expression.cond);
        with_context_bindings(self, None, bindings, |visitor| {
            visitor.visit_block(&expression.body);
        });
    }

    fn visit_expr_for_loop(&mut self, expression: &'ast syn::ExprForLoop) {
        self.visit_expr(&expression.expr);
        let bindings = pattern_bindings(std::iter::once(expression.pat.as_ref()));
        with_context_bindings(self, None, bindings, |visitor| {
            visitor.visit_block(&expression.body);
        });
    }

    fn visit_expr_call(&mut self, expression: &'ast ExprCall) {
        let parameters = self
            .signatures
            .parameters(&expression.func)
            .map(<[bool]>::to_vec);
        self.add_arguments(&expression.args, parameters.as_deref());
        visit::visit_expr_call(self, expression);
    }

    fn visit_expr_method_call(&mut self, expression: &'ast ExprMethodCall) {
        self.add_arguments(&expression.args, None);
        visit::visit_expr_method_call(self, expression);
    }
}

fn is_some_call(expression: &Expr, shadows: ShadowedNames) -> bool {
    let Expr::Call(call) = strip_expression(expression) else {
        return false;
    };
    if call.args.len() != 1 {
        return false;
    }
    let Expr::Path(function) = strip_expression(&call.func) else {
        return false;
    };
    let path = function
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");
    match path.as_str() {
        "Some" => !shadows.some,
        "Option::Some" => !shadows.option,
        "core::option::Option::Some" | "std::option::Option::Some" => {
            function.path.leading_colon.is_some()
        }
        _ => false,
    }
}

struct SelfAssignVisitor<'source> {
    source: &'source str,
    mutations: Vec<Mutation>,
}

impl<'source> SelfAssignVisitor<'source> {
    fn mutations(source: &'source str, file: &syn::File) -> Vec<Mutation> {
        let mut visitor = Self {
            source,
            mutations: Vec::new(),
        };
        visitor.visit_file(file);
        visitor.mutations
    }
}

impl<'ast> Visit<'ast> for SelfAssignVisitor<'_> {
    skip_non_expression_syntax!();

    fn visit_stmt(&mut self, statement: &'ast syn::Stmt) {
        if let syn::Stmt::Expr(Expr::Assign(assignment), Some(_)) = statement {
            if is_safe_self_assignment(assignment) {
                if let Some(range) = span_range(self.source, statement.span()) {
                    self.mutations.push(Mutation::new(range, ""));
                }
            }
        }
        visit::visit_stmt(self, statement);
    }
}

pub(crate) fn is_safe_self_assignment(assignment: &ExprAssign) -> bool {
    same_place(&assignment.left, &assignment.right)
}

fn same_place(left: &Expr, right: &Expr) -> bool {
    match (strip_expression(left), strip_expression(right)) {
        (Expr::Path(left), Expr::Path(right)) => same_local_path(left, right),
        (Expr::Field(left), Expr::Field(right)) => same_field(left, right),
        (Expr::Tuple(left), Expr::Tuple(right)) => same_tuple(left, right),
        _ => false,
    }
}

fn same_local_path(left: &syn::ExprPath, right: &syn::ExprPath) -> bool {
    left.qself.is_none()
        && right.qself.is_none()
        && left.path.leading_colon.is_none()
        && right.path.leading_colon.is_none()
        && left.path.segments.len() == 1
        && right.path.segments.len() == 1
        && left.path.segments[0].ident == right.path.segments[0].ident
}

fn same_tuple(left: &syn::ExprTuple, right: &syn::ExprTuple) -> bool {
    left.elems.len() == right.elems.len()
        && left
            .elems
            .iter()
            .zip(&right.elems)
            .all(|(left, right)| same_place(left, right))
}

fn same_field(left: &ExprField, right: &ExprField) -> bool {
    member_name(&left.member) == member_name(&right.member) && same_place(&left.base, &right.base)
}

fn member_name(member: &Member) -> String {
    match member {
        Member::Named(identifier) => identifier.to_string(),
        Member::Unnamed(index) => index.index.to_string(),
    }
}

fn strip_expression(mut expression: &Expr) -> &Expr {
    loop {
        expression = match expression {
            Expr::Group(group) => &group.expr,
            Expr::Paren(parenthesized) => &parenthesized.expr,
            _ => return expression,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mutator;

    #[test]
    fn context_nil_ignores_bindings_from_match_arm_guard_expressions() {
        let source = "fn consume(_: Option<i32>) {} fn run(value: Option<i32>, preds: &[i32]) { match value { Some(n) if preds.iter().any(|consume| consume == &n) => { consume(Some(1)); }, _ => {} } }";
        let mutations = ValueMutator::context_nil().mutations(source);

        assert_eq!(mutations.len(), 1);
        assert!(!mutations[0].requires_compile_validation());
        assert_eq!(
            mutations[0].apply(source),
            Some(source.replacen(
                "consume(Some(1))",
                "consume(::core::option::Option::None)",
                1,
            ))
        );
    }
}
