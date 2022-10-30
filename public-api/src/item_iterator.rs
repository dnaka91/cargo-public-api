use std::{
    collections::{HashMap, VecDeque},
    vec,
};

use rustdoc_types::{Crate, Id, Impl, Import, Item, ItemEnum, Module, Struct, StructKind};

use super::intermediate_public_item::IntermediatePublicItem;
use crate::{
    crate_wrapper::CrateWrapper, public_item::PublicItem, render::RenderingContext, Options,
    PublicApi,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ImplKind {
    Normal,
    AutoTrait,
    Blanket,
}

impl ImplKind {
    fn is_active(&self, options: &Options) {
        match self {
            ImplKind::Blanket => options.with_blanket_implementations,
            ImplKind::AutoTrait | ImplKind::Normal => true,
        };
    }
}

// #[derive(Debug, Clone)]
// struct ImplItem<'c> {
//     item: &'c Item,
//     impl_: &'c Impl,
//     for_id: Option<&'c Id>,
//     kind: ImplKind,
// }

#[derive(Debug)]
struct UnprocessedItem<'c> {
    parent_path: Vec<IntermediatePublicItem<'c>>,
    id: &'c Id,
}

#[derive(Debug)]
struct ProcessedItem<'c> {
    path: Vec<IntermediatePublicItem<'c>>,
}

/// TODO
///
/// Iterating over items has the benefit of behaving properly when:
/// 1. A single item is imported several times.
/// 2. An item is (publicly) imported from another crate
///
/// Note that this implementation iterates over everything (with the exception
/// of `impl`s, see relevant code for more details), so if the rustdoc JSON is
/// generated with `--document-private-items`, then private items will also be
/// included in the output.
pub struct ItemProcessor<'c> {
    /// The original and unmodified rustdoc JSON, in deserialized form.
    crate_: CrateWrapper<'c>,

    options: Options,

    work_queue: VecDeque<UnprocessedItem<'c>>,

    output: Vec<ProcessedItem<'c>>,
}

impl<'c> ItemProcessor<'c> {
    pub fn new(crate_: &'c Crate, options: Options) -> Self {
        ItemProcessor {
            crate_: CrateWrapper::new(crate_),
            options,
            work_queue: VecDeque::new(),
            output: vec![],
        }
    }

    fn add_to_work_queue(&mut self, parent_path: Vec<IntermediatePublicItem<'c>>, id: &'c Id) {
        self.work_queue
            .push_front(UnprocessedItem { parent_path, id });
    }

    fn run(&mut self) {
        loop {
            match self.work_queue.pop_front() {
                Some(unprocessed_item) => {
                    if let Some(item) = self.crate_.get_item(unprocessed_item.id) {
                        self.process_any_item(item, unprocessed_item);
                    }
                }
                None => break,
            }
        }
    }

    fn process_any_item(&mut self, item: &'c Item, unprocessed_item: UnprocessedItem<'c>) {
        match &item.inner {
            ItemEnum::Import(import) => {
                if import.glob {
                    self.process_import_glob_item(import, unprocessed_item, item);
                } else {
                    self.process_import_item(item, import, unprocessed_item);
                }
            }
            ItemEnum::Impl(impl_) => {
                self.process_impl_item(unprocessed_item, impl_);
            }
            _ => {
                self.process_item(unprocessed_item, item);
            }
        }
    }

    /// We need to handle `pub use foo::*` specially. In case of such wildcard
    /// imports, `glob` will be `true` and `id` will be the module we should
    /// import all items from, but we should NOT add the module itself.
    fn process_import_glob_item(
        &mut self,
        import: &'c Import,
        unprocessed_item: UnprocessedItem<'c>,
        item: &'c Item,
    ) {
        // Before we inline this wildcard import, make sure that the module is
        // not indirectly trying to import itself. If we allow that, we'll get a
        // stack overflow.
        if let Some(Item {
            inner: ItemEnum::Module(Module { items, .. }),
            ..
        }) = import
            .id
            .as_ref()
            .and_then(|id| self.get_item_if_not_in_path(&unprocessed_item.parent_path, id))
        {
            for item_id in items {
                self.add_to_work_queue(unprocessed_item.parent_path.clone(), &item_id);
            }
        } else {
            self.finish_item(
                unprocessed_item,
                item,
                Some(format!("<<{}::*>>", import.source)),
            );
        }
    }

    /// Since public imports are part of the public API, we inline them, i.e.
    /// replace the item corresponding to an import with the item that is
    /// imported. If we didn't do this, publicly imported items would show up as
    /// just e.g. `pub use some::function`, which is not sufficient for the use
    /// cases of this tool. We want to show the actual API, and thus also show
    /// type information! There is one exception; for re-exports of primitive
    /// types, there is no item Id to inline with, so they remain as e.g. `pub
    /// use my_i32` in the output.
    fn process_import_item(
        &mut self,
        item: &'c Item,
        import: &'c Import,
        unprocessed_item: UnprocessedItem<'c>,
    ) {
        let mut actual_item = item;
        if let Some(imported_item) = import.id.as_ref().and_then(|imported_id| {
            self.get_item_if_not_in_path(&unprocessed_item.parent_path, imported_id)
        }) {
            actual_item = imported_item;
        }
        self.finish_item(unprocessed_item, actual_item, Some(import.name.clone()));
    }

    fn process_impl_item(&self, unprocessed_item: UnprocessedItem, impl_: &Impl) {
        todo!()
    }

    fn process_item(&mut self, unprocessed_item: UnprocessedItem<'c>, item: &'c Item) {
        let new = self.finish_item(unprocessed_item, item, None);

        // Note reversed so all items and up at the front but in preserved order
        for impl_ in impls_for_item(item).into_iter().flatten().rev() {
            self.work_queue.push_front(UnprocessedItem {
                parent_path: new.path.clone(),
                id: &impl_,
            });
        }

        for c in items_in_container(item).into_iter().flatten().rev() {
            self.work_queue.push_front(UnprocessedItem {
                parent_path: new.path.clone(),
                id: &c,
            });
        }

        self.output.push(new);
    }

    fn finish_item(
        &self,
        unprocessed_item: UnprocessedItem<'c>,
        item: &'c Item,
        overridden_name: Option<String>,
    ) -> ProcessedItem<'c> {
        // Transfer path ownership to output item
        let mut path = unprocessed_item.parent_path;

        // Complete the path with the last item
        path.push(IntermediatePublicItem {
            item,
            overridden_name,
        });

        // Done
        ProcessedItem { path }
    }

    fn finish_item_and_push(
        &mut self,
        unprocessed_item: UnprocessedItem<'c>,
        item: &'c Item,
        overridden_name: Option<String>,
    ) {
        let x = self.finish_item(unprocessed_item, item, overridden_name);
        self.output.push(x);
    }

    // #[must_use]
    // pub fn path(&self) -> Vec<&IntermediatePublicItem<'c>> {
    //     let mut path = vec![self];

    //     let mut current_item = self;
    //     while let Some(parent) = current_item.parent.as_ref() {
    //         path.insert(0, parent);
    //         current_item = parent;
    //     }

    //     path
    // }

    // #[must_use]
    // pub fn path_vec(&self) -> PublicItemPath {
    //     self.path()
    //         .iter()
    //         .filter_map(|i| i.name())
    //         .map(ToOwned::to_owned)
    //         .collect()
    // }

    // #[must_use]
    // pub fn path_contains_id(&self, id: &'c Id) -> bool {
    //     self.path().iter().any(|m| m.item.id == *id)
    // }

    // #[must_use]
    // pub fn path_contains_renamed_item(&self) -> bool {
    //     self.path().iter().any(|m| m.overridden_name.is_some())
    // }

    /// Get the rustdoc JSON item with `id`, but only if it is not already part
    /// of the path. This can happen in the case of recursive re-exports, in
    /// which case we need to break the recursion.
    fn get_item_if_not_in_path(
        &mut self,
        parent_path: &[IntermediatePublicItem<'c>],
        id: &'c Id,
    ) -> Option<&'c Item> {
        if parent_path.iter().any(|m| m.item.id == *id) {
            // The item is already in the path! Break import recursion...
            return None;
        }

        self.crate_.get_item(id)
    }
}

// fn all_impls(crate_: &Crate) -> impl Iterator<Item = ImplItem> {
//     crate_.index.values().filter_map(|item| match &item.inner {
//         ItemEnum::Impl(impl_) => Some(ImplItem {
//             item,
//             impl_,
//             kind: impl_kind(impl_),
//             for_id: match &impl_.for_ {
//                 Type::ResolvedPath(path) => Some(&path.id),
//                 _ => None,
//             },
//         }),
//         _ => None,
//     })
// }

const fn impl_kind(impl_: &Impl) -> ImplKind {
    let has_blanket_impl = matches!(impl_.blanket_impl, Some(_));

    // See https://github.com/rust-lang/rust/blob/54f20bbb8a7aeab93da17c0019c1aaa10329245a/src/librustdoc/json/conversions.rs#L589-L590
    match (impl_.synthetic, has_blanket_impl) {
        (true, false) => ImplKind::AutoTrait,
        (false, true) => ImplKind::Blanket,
        _ => ImplKind::Normal,
    }
}

// fn active_impls(all_impls: Vec<ImplItem>, options: Options) -> Impls {
//     let mut impls = HashMap::new();

//     for impl_item in all_impls {
//         let for_id = match impl_item.for_id {
//             Some(id) => id,
//             None => continue,
//         };

//         let active = match impl_item.kind {
//             ImplKind::Blanket => options.with_blanket_implementations,
//             ImplKind::AutoTrait | ImplKind::Normal => true,
//         };

//         if active {
//             impls
//                 .entry(for_id)
//                 .or_insert_with(Vec::new)
//                 .push(impl_item.impl_);
//         }
//     }

//     impls
// }

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

pub fn public_api_in_crate(crate_: &Crate, options: Options) -> super::PublicApi {
    let mut item_processor = ItemProcessor::new(crate_, options);
    item_processor.add_to_work_queue(vec![], &crate_.root);
    item_processor.run();

    // Given a rustdoc JSON Id, keeps track of what public items that have this
    // ID. The reason this is a one-to-many mapping is because of re-exports.
    // If an API re-exports a public item in a different place, the same item
    // will be reachable by different paths, and thus the Vec will contain many
    // [`IntermediatePublicItem`]s for that ID.
    //
    // You might think this is rare, but it is actually a common thing in
    // real-world code.
    let mut id_to_items: HashMap<&Id, Vec<&IntermediatePublicItem>> = HashMap::new();

    let context = RenderingContext {
        crate_,
        id_to_items,
    };

    PublicApi {
        items: item_processor
            .output
            .iter()
            .map(|processed_item| PublicItem {
                path: processed_item
                    .path
                    .iter()
                    .filter_map(|i| i.name())
                    .map(ToOwned::to_owned)
                    .collect(),
                tokens: context.token_stream(&processed_item.path),
            })
            .collect::<Vec<_>>(),
        missing_item_ids: item_processor.crate_.missing_item_ids(),
    }
}
