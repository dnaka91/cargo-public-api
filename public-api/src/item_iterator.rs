use std::{collections::HashMap, rc::Rc};

use rustdoc_types::{Crate, Id, Impl, Import, Item, ItemEnum, Module, Struct, StructKind, Type};

use super::crate_wrapper::CrateWrapper;
use super::intermediate_public_item::{IntermediatePublicItem, Parent};
use crate::{public_item::PublicItem, render::RenderingContext, Options, PublicApi};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImplKind {
    Normal,
    AutoTrait,
    Blanket,
}

#[derive(Debug, Clone)]
struct ImplItem<'a> {
    item: &'a Item,
    for_id: Option<&'a Id>,
    kind: ImplKind,
}

/// Iterates over all items in a crate. Iterating over items has the benefit of
/// behaving properly when:
/// 1. A single item is imported several times.
/// 2. An item is (publicly) imported from another crate
///
/// Note that this implementation iterates over everything. So if the rustdoc
/// JSON is generated with `--document-private-items`, then private items will
/// also be included in the output. Usage of `--document-private-items` is not
/// supported.
pub struct ItemIterator<'a> {
    /// The original and unmodified rustdoc JSON, in deserialized form.
    crate_: CrateWrapper<'a>,

    /// What items left to visit (and possibly add more items from)
    items_left: Vec<Rc<IntermediatePublicItem<'a>>>,

    /// Given a rustdoc JSON Id, keeps track of what public items that have this
    /// ID. The reason this is a one-to-many mapping is because of re-exports.
    /// If an API re-exports a public item in a different place, the same item
    /// will be reachable by different paths, and thus the Vec will contain many
    /// [`IntermediatePublicItem`]s for that ID.
    ///
    /// You might think this is rare, but it is actually a common thing in
    /// real-world code.
    id_to_items: HashMap<&'a Id, Vec<Rc<IntermediatePublicItem<'a>>>>,
}

impl<'a> ItemIterator<'a> {
    pub fn new(crate_: &'a Crate) -> Self {
        let mut s = ItemIterator {
            crate_: CrateWrapper::new(crate_),
            items_left: vec![],
            id_to_items: HashMap::new(),
        };

        // Bootstrap with the root item
        s.visit_item_with_id(&crate_.root, None);

        s
    }

    /// Tries to add an item to visit. The only time this fails is if the [`Id`]
    /// is missing from the rustdoc JSON for some reason.
    fn visit_item_with_id(&mut self, id: &'a Id, parent: Parent<'a>) {
        if let Some(item) = self.crate_.get_item(id) {
            self.visit_item(item, parent);
        }
    }

    fn visit_item(&mut self, item: &'a Item, parent: Parent<'a>) {
        match &item.inner {
            ItemEnum::Import(import) => self.visit_import_item(item, &import, parent),
            _ => self.map_and_add(item, None, parent),
        }
    }

    // Since public imports are part of the public API, we inline them, i.e.
    // replace the item corresponding to an import with the item that is
    // imported. If we didn't do this, publicly imported items would show up
    // as just e.g. `pub use some::function`, which is not sufficient for
    // the use cases of this tool. We want to show the actual API, and thus
    // also show type information! There is one exception; for re-exports of
    // primitive types, there is no item Id to inline with, so they remain
    // as e.g. `pub use my_i32` in the output.
    fn visit_import_item(&mut self, item: &'a Item, import: &'a Import, parent: Parent<'a>) {
        match import {
            // We need to handle `pub use foo::*` specially. In case of such
            // wildcard imports, `glob` will be `true` and `id` will be the
            // module we should import all items from, but we should NOT add
            // the module itself.
            Import {
                id: Some(mod_id),
                glob: true,
                ..
            } => {
                // We try to inline glob imports, but that might fail, and we want to
                // keep track of when that happens.
                let mut glob_import_inlined = false;

                // Before we inline this wildcard import, make sure that the module
                // is not indirectly trying to import itself. If we allow that,
                // we'll get a stack overflow. Note that `glob_import_inlined`
                // remains `false` in that case, which means that the output will
                // use a special syntax to indicate that we broke recursion.
                if let Some(Item {
                    inner: ItemEnum::Module(Module { items, .. }),
                    ..
                }) = self.get_item_if_not_in_path(&parent, &mod_id)
                {
                    for item in items {
                        self.visit_item_with_id(item, parent.clone());
                    }
                    glob_import_inlined = true;
                }

                // if we inlined a glob import earlier, we should not add the import
                // item itself. All other items we can go ahead and add.
                if !glob_import_inlined {
                    // Items should have been inlined in maybe_add_item_to_visit(),
                    // but since we got here that must have failed, typically
                    // because the built rustdoc JSON omitted some items from the
                    // output, or to break import recursion.
                    self.map_and_add(item, Some(format!("<<{}::*>>", import.source)), parent);
                }
            }
            import => {
                // Normally we add the original item, but in the case of imports we
                // replace this with the *imported* item.
                let mut actual_item = item;

                if let Some(imported_item) = import
                    .id
                    .as_ref()
                    .and_then(|imported_id| self.get_item_if_not_in_path(&parent, imported_id))
                {
                    actual_item = imported_item;
                }

                self.map_and_add(actual_item, Some(import.name.clone()), parent);
            }
        }
    }

    fn map_and_add(
        &mut self,
        item: &'a Item,
        overridden_name: Option<String>,
        parent: Option<Rc<IntermediatePublicItem<'a>>>,
    ) {
        let public_item = self.map_new_intermediate_public_item(item, overridden_name, parent);
        self.items_left.push(public_item);
    }

    /// Creates a new [`IntermediatePublicItem`] and adds it to the
    /// [`Self::id_to_items`] map.
    pub fn map_new_intermediate_public_item(
        &mut self,
        item: &'a Item,
        overridden_name: Option<String>,
        parent: Parent<'a>,
    ) -> Rc<IntermediatePublicItem<'a>> {
        let public_item = Rc::new(IntermediatePublicItem {
            item,
            overridden_name,
            parent,
        });

        //eprintln!("map {:?}", item.id);
        self.id_to_items
            .entry(&item.id)
            .or_default()
            .push(public_item.clone());

        public_item
    }

    /// Get the rustdoc JSON item with `id`, but only if it is not already part
    /// of the path. This can happen in the case of recursive re-exports, in
    /// which case we need to break the recursion.
    fn get_item_if_not_in_path(&mut self, parent: &Parent<'a>, id: &'a Id) -> Option<&'a Item> {
        if parent.clone().map_or(false, |p| p.path_contains_id(id)) {
            // The item is already in the path! Break import recursion...
            return None;
        }

        self.crate_.get_item(id)
    }

    fn try_add_children_for_item(&mut self, public_item: &Rc<IntermediatePublicItem<'a>>) {
        // Handle any impls
        for id in impls_for_item(public_item.item).into_iter().flatten() {
            self.visit_item_with_id(&id, None);
            // TODO: Visit in beginning instead
        }

        // Handle regular children of the item
        for child in items_in_container(public_item.item).into_iter().flatten() {
            self.visit_item_with_id(child, Some(public_item.clone()));
        }
    }
}

impl<'a> Iterator for ItemIterator<'a> {
    type Item = Rc<IntermediatePublicItem<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut result = None;

        if let Some(public_item) = self.items_left.pop() {
            self.try_add_children_for_item(&public_item.clone());

            result = Some(public_item);
        }

        result
    }
}

fn all_impls(crate_: &Crate) -> impl Iterator<Item = ImplItem> {
    crate_.index.values().filter_map(|item| match &item.inner {
        ItemEnum::Impl(impl_) => {
            eprintln!("saw {:?}", item.id);
            Some(ImplItem {
                item,
                kind: impl_kind(impl_),
                for_id: match &impl_.for_ {
                    Type::ResolvedPath(path) => Some(&path.id),
                    _ => None,
                },
            })
        }
        _ => None,
    })
}

const fn impl_kind(impl_: &Impl) -> ImplKind {
    let has_blanket_impl = matches!(impl_.blanket_impl, Some(_));

    // See https://github.com/rust-lang/rust/blob/54f20bbb8a7aeab93da17c0019c1aaa10329245a/src/librustdoc/json/conversions.rs#L589-L590
    match (impl_.synthetic, has_blanket_impl) {
        (true, false) => ImplKind::AutoTrait,
        (false, true) => ImplKind::Blanket,
        _ => ImplKind::Normal,
    }
}

fn is_impl_item_active(impl_item: &ImplItem, options: Options) -> bool {
    if impl_item.for_id.is_none() {
        eprintln!("for nothing for {:?}", impl_item.item.id);
        return false;
    };

    match impl_item.kind {
        ImplKind::Normal => true,
        ImplKind::Blanket | ImplKind::AutoTrait => !options.simplified,
    }
}

/// Some items contain other items, which is relevant for analysis. Keep track
/// of such relationships.
pub const fn items_in_container(item: &Item) -> Option<&Vec<Id>> {
    match &item.inner {
        ItemEnum::Module(m) => Some(&m.items),
        ItemEnum::Union(u) => Some(&u.fields),
        ItemEnum::Struct(Struct {
            kind: StructKind::Plain { fields, .. },
            ..
        })
        | ItemEnum::Variant(rustdoc_types::Variant::Struct { fields, .. }) => Some(fields),
        ItemEnum::Enum(e) => Some(&e.variants),
        ItemEnum::Trait(t) => Some(&t.items),
        ItemEnum::Impl(i) => Some(&i.items),
        _ => None,
    }
}

// TODO: Option<&Vec<Id>>?
pub fn impls_for_item(item: &Item) -> Option<&[Id]> {
    match &item.inner {
        ItemEnum::Union(union_) => Some(&union_.impls),
        ItemEnum::Struct(struct_) => Some(&struct_.impls),
        ItemEnum::Enum(enum_) => Some(&enum_.impls),
        ItemEnum::Primitive(primitive) => Some(&primitive.impls),
        // TODO? ItemEnum::Trait(trait_) => trait_.im,
        _ => None,
    }
}

pub fn public_api_in_crate(crate_: &Crate, _options: Options) -> super::PublicApi {
    let mut item_iterator = ItemIterator::new(crate_);
    let items: Vec<_> = item_iterator.by_ref().collect();

    // `impl`s are a bit special. They do not need to be reachable by the crate
    // root in order to matter. All that matters is that the trait and type
    // involved are both public.
    //
    // Since the rustdoc JSON by definition only includes public items, all
    // `impl`s we see are potentially relevant. We do some filtering though.
    // For example, we do not care about blanket implementations by default.
    // for active_impl in all_impls(crate_).filter(|i| {
    //     let b = is_impl_item_active(i, options);
    //     eprintln!("active  {:?} {}", i.item.id, b);
    //     b || true
    // }) {
    //     item_iterator.map_new_intermediate_public_item(active_impl.item, None, None);
    // }

    let context = RenderingContext {
        crate_,
        id_to_items: item_iterator.id_to_items,
    };

    PublicApi {
        items: items
            .iter()
            .filter(|item| !is_part_of_impl(item))
            .map(|item| PublicItem::from_intermediate_public_item(&context, item))
            .collect(),
        missing_item_ids: item_iterator.crate_.missing_item_ids(),
    }
}

fn is_part_of_impl<'a, 'b>(item: &'b Rc<IntermediatePublicItem<'a>>) -> bool {
    if let Some(parent) = &item.parent {
        if is_part_of_impl(&parent) {
            return true;
        }
    }
    matches!(item.item.inner, ItemEnum::Impl(_))
}
