mod cleanup;
mod error_guard;
mod error_wrap;
mod recovery;

pub(crate) use cleanup::CleanupMutator;
pub(crate) use error_guard::ErrorGuardMutator;
pub(crate) use error_wrap::ErrorWrapMutator;
pub(crate) use recovery::RecoveryMutator;

fn crate_root_aliases(items: &[syn::Item]) -> (bool, bool) {
    let mut core = false;
    let mut std = false;
    for item in items {
        let syn::Item::ExternCrate(item) = item else {
            continue;
        };
        let local = item
            .rename
            .as_ref()
            .map_or(&item.ident, |(_, rename)| rename);
        core |= local == "core" && item.ident != "core";
        std |= local == "std" && item.ident != "std";
    }
    (core, std)
}
