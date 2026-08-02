use syn::{GenericParam, Item, Stmt, UseTree};

#[derive(Clone, Copy, Default)]
pub(super) struct Bindings {
    std_shadowed: bool,
    tokio_shadowed: bool,
    thread_imported: bool,
    root_aliases: RootAliases,
}

impl Bindings {
    pub(super) fn for_crate(items: &[Item]) -> Self {
        let root_aliases = RootAliases::from_crate_items(items);
        let mut bindings = Self {
            std_shadowed: root_aliases.std,
            tokio_shadowed: root_aliases.tokio,
            thread_imported: false,
            root_aliases,
        };
        bindings.add_scope_items(items.iter());
        bindings
    }

    pub(super) fn for_nested_module(self, items: &[Item]) -> Self {
        let mut bindings = Self {
            std_shadowed: self.root_aliases.std,
            tokio_shadowed: self.root_aliases.tokio,
            thread_imported: false,
            root_aliases: self.root_aliases,
        };
        bindings.add_scope_items(items.iter());
        bindings
    }

    pub(super) fn with_block_items(mut self, statements: &[Stmt]) -> Self {
        self.add_scope_items(statements.iter().filter_map(|statement| match statement {
            Stmt::Item(item) => Some(item),
            _ => None,
        }));
        self
    }

    pub(super) fn with_generics(mut self, generics: &syn::Generics) -> Self {
        for parameter in &generics.params {
            let GenericParam::Type(parameter) = parameter else {
                continue;
            };
            self.shadow(parameter.ident.to_string().as_str());
        }
        self
    }

    pub(super) fn standard_path_available(self, root_qualified: bool) -> bool {
        if root_qualified {
            !self.root_aliases.std
        } else {
            !self.std_shadowed
        }
    }

    pub(super) fn tokio_path_available(self, root_qualified: bool) -> bool {
        if root_qualified {
            !self.root_aliases.tokio
        } else {
            !self.tokio_shadowed
        }
    }

    pub(super) fn standard_thread_imported(self) -> bool {
        self.thread_imported
    }

    fn add_scope_items<'item>(&mut self, items: impl Iterator<Item = &'item Item> + Clone) {
        for item in items.clone() {
            self.add_type_bindings(item);
        }
        let mut thread_binding = ThreadBinding::default();
        for item in items {
            thread_binding.add_item(item);
        }
        if thread_binding.present {
            let standard_path_available = if thread_binding.root_qualified {
                !self.root_aliases.std
            } else {
                !self.std_shadowed
            };
            self.thread_imported =
                thread_binding.standard && !thread_binding.other && standard_path_available;
        }
    }

    fn add_type_bindings(&mut self, item: &Item) {
        if let Some(name) = type_binding_name(item) {
            self.shadow(name.to_string().as_str());
            return;
        }
        if let Item::ExternCrate(item) = item {
            self.add_extern_crate(item);
        }
        if let Item::Use(item) = item {
            self.add_use_names(item);
        }
    }

    fn add_extern_crate(&mut self, item: &syn::ItemExternCrate) {
        let local = extern_crate_binding(item);
        if local == "std" && item.ident != "std" {
            self.std_shadowed = true;
        }
        if local == "tokio" && item.ident != "tokio" {
            self.tokio_shadowed = true;
        }
    }

    fn add_use_names(&mut self, item: &syn::ItemUse) {
        let mut names = Vec::new();
        collect_use_names(&item.tree, &mut names);
        for name in names {
            if name != "thread" {
                self.shadow(&name);
            }
        }
    }

    fn shadow(&mut self, name: &str) {
        match name {
            "std" => self.std_shadowed = true,
            "tokio" => self.tokio_shadowed = true,
            "thread" => self.thread_imported = false,
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Default)]
struct RootAliases {
    std: bool,
    tokio: bool,
}

impl RootAliases {
    fn from_crate_items(items: &[Item]) -> Self {
        let mut aliases = Self::default();
        for item in items {
            let Item::ExternCrate(item) = item else {
                continue;
            };
            let local = extern_crate_binding(item);
            if local == "std" && item.ident != "std" {
                aliases.std = true;
            }
            if local == "tokio" && item.ident != "tokio" {
                aliases.tokio = true;
            }
        }
        aliases
    }
}

fn extern_crate_binding(item: &syn::ItemExternCrate) -> &syn::Ident {
    item.rename
        .as_ref()
        .map_or(&item.ident, |(_, rename)| rename)
}

#[derive(Default)]
struct ThreadBinding {
    present: bool,
    standard: bool,
    other: bool,
    root_qualified: bool,
}

impl ThreadBinding {
    fn add_item(&mut self, item: &Item) {
        if let Some(name) = type_binding_name(item) {
            self.add_named_item(name);
            return;
        }
        match item {
            Item::ExternCrate(item) => self.add_named_item(extern_crate_binding(item)),
            Item::Use(item) => {
                let mut path = Vec::new();
                self.add_use_tree(&item.tree, &mut path, item.leading_colon.is_some());
            }
            _ => {}
        }
    }

    fn add_named_item(&mut self, name: &syn::Ident) {
        if name == "thread" {
            self.present = true;
            self.other = true;
        }
    }

    fn add_use_tree(&mut self, tree: &UseTree, path: &mut Vec<String>, root_qualified: bool) {
        match tree {
            UseTree::Path(node) => {
                path.push(node.ident.to_string());
                self.add_use_tree(&node.tree, path, root_qualified);
                path.pop();
            }
            UseTree::Name(node) => {
                path.push(node.ident.to_string());
                self.add_use_leaf(path, node.ident == "thread", root_qualified);
                path.pop();
            }
            UseTree::Rename(node) => {
                path.push(node.ident.to_string());
                self.add_use_leaf(path, node.rename == "thread", root_qualified);
                path.pop();
            }
            UseTree::Group(group) => {
                for tree in &group.items {
                    self.add_use_tree(tree, path, root_qualified);
                }
            }
            UseTree::Glob(_) => {
                self.present = true;
                self.other = true;
            }
        }
    }

    fn add_use_leaf(&mut self, path: &[String], binds_thread: bool, root_qualified: bool) {
        if !binds_thread {
            return;
        }
        self.present = true;
        if path.iter().map(String::as_str).eq(["std", "thread"]) {
            self.standard = true;
            self.root_qualified |= root_qualified;
        } else {
            self.other = true;
        }
    }
}

fn type_binding_name(item: &Item) -> Option<&syn::Ident> {
    match item {
        Item::Enum(item) => Some(&item.ident),
        Item::Mod(item) => Some(&item.ident),
        Item::Struct(item) => Some(&item.ident),
        Item::Trait(item) => Some(&item.ident),
        Item::TraitAlias(item) => Some(&item.ident),
        Item::Type(item) => Some(&item.ident),
        Item::Union(item) => Some(&item.ident),
        _ => None,
    }
}

fn collect_use_names(tree: &UseTree, names: &mut Vec<String>) {
    match tree {
        UseTree::Path(path) => collect_use_names(&path.tree, names),
        UseTree::Name(name) => names.push(name.ident.to_string()),
        UseTree::Rename(rename) => names.push(rename.rename.to_string()),
        UseTree::Group(group) => {
            for tree in &group.items {
                collect_use_names(tree, names);
            }
        }
        UseTree::Glob(_) => {
            names.extend(["std", "tokio", "thread"].map(str::to_owned));
        }
    }
}
