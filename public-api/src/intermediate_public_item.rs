use std::rc::Rc;

use rustdoc_types::{Id, Item};

use crate::{public_item::PublicItemPath, render::RenderingContext, tokens::Token};

/// This struct represents one public item of a crate, but in intermediate form.
/// It wraps a single [Item] but adds additional calculated values to make it
/// easier to work with. Later, one [`Self`] will be converted to exactly one
/// [`crate::PublicItem`].
/// TODO@
#[derive(Clone, Debug)]
pub struct IntermediatePublicItem<'c> {
    /// The item we are effectively wrapping.
    pub item: &'c Item,

    /// If `Some`, this overrides [Item::name], which happens in the case of
    /// renamed imports (`pub use other::Item as Foo;`).
    pub overridden_name: Option<String>,
}

impl<'c> IntermediatePublicItem<'c> {
    pub fn name(&self) -> Option<&str> {
        self.overridden_name
            .as_deref()
            .or(self.item.name.as_deref())
    }


    // pub fn iter() -> ChildIter {

    // }
}

// struct ChildIter<'a> {

// }

// impl<'c> Iterator for IntermediatePublicItem<'c> {
//     type Item = &'c IntermediatePublicItem<'c>;

//     fn next(&mut self) -> Option<Self::Item> {
//         todo!()
//     }
// }
