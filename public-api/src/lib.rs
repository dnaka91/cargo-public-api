//! This library gives you the public API of a library crate, in the form of a
//! list of public items in the crate. Public items are items that other crates
//! can use. Diffing is also supported.
//!
//! If you want a convenient CLI for this library, you should use [cargo
//! public-api](https://github.com/Enselic/cargo-public-api).
//!
//! As input to the library, a special output format from `cargo doc` is used,
//! which goes by the name **rustdoc JSON**. Currently, only `cargo doc` from
//! the Nightly toolchain can produce **rustdoc JSON** for a library. You build
//! **rustdoc JSON** like this:
//!
//! ```bash
//! cargo +nightly rustdoc --lib -- -Z unstable-options --output-format json
//! ```
//!
//! Consider using [`rustdoc_json`](https://crates.io/crates/rustdoc_json)
//! instead of invoking the above command yourself.
//!
//! The main entry point to the library is [`PublicApi::from_rustdoc_json_str`],
//! so please read its documentation.
//!
//! # Examples
//!
//! The two main use cases are listing the public API and diffing different
//! versions of the same public APIs.
//!
//! ## List all public items of a crate (the public API)
//! ```no_run
#![doc = include_str!("../examples/list_public_api.rs")]
//! ```
//!
//! ## Diff two versions of a public API
//! ```no_run
#![doc = include_str!("../examples/diff_public_api.rs")]
//! ```
//!
//! The most comprehensive example code on how to use the library can be found
//! in the thin binary wrapper around the library, see
//! <https://github.com/Enselic/cargo-public-api/blob/main/public-api/src/main.rs>.

// deny in CI, only warn here
#![warn(clippy::all, clippy::pedantic, missing_docs)]

mod crate_wrapper;
mod error;
mod intermediate_public_item;
mod item_iterator;
mod public_item;
mod render;
pub mod tokens;

pub mod diff;

use std::path::Path;

// Documented at the definition site so cargo doc picks it up
pub use error::{Error, Result};

// Documented at the definition site so cargo doc picks it up
pub use public_item::PublicItem;

/// This constant defines the minimum version of nightly that is required in
/// order for the rustdoc JSON output to be parsable by this library. Note that
/// this library is implemented with stable Rust. But the rustdoc JSON that this
/// library parses can currently only be produced by nightly.
///
/// The rustdoc JSON format is still changing, so every now and then we update
/// this library to support the latest format. If you use this version of
/// nightly or later, you should be fine.
pub const MINIMUM_RUSTDOC_JSON_VERSION: &str = "nightly-2022-09-28";

/// Contains various options that you can pass to [`PublicApi::from_rustdoc_json_str`].
#[derive(Copy, Clone, Debug)]
#[non_exhaustive] // More options are likely to be added in the future
pub struct Options {
    /// If `true`, items part of
    ///
    /// * Blanket Implementations (such as `impl<T> Any for T`, `impl<T>
    ///   Borrow<T> for T`, and `impl<T, U> Into<U> for T where U: From<T>`)
    ///
    /// * Auto Trait Implementations (such as `impl Sync for Foo`)
    ///
    /// are excluded in the list of public items of a crate. The default value
    /// is `false`, to make that the default behavior is to fully list the
    /// public API.
    ///
    /// For casual use, it is easy to see the presence of these items to
    /// constitute noise however, even if they formally are part of the public
    /// API of a crate.
    ///
    /// TODO: What prior art is there for a single-letter arg switch for this
    /// kind of arg? Maybe we need another long term for this to make the short
    /// term make sense.
    pub simplified: bool,

    /// If `true`, items will be sorted before being returned. If you will pass
    /// on the return value to [`diff::PublicApiDiff::between`], it is
    /// currently unnecessary to sort first, because the sorting will be
    /// performed/ensured inside of that function.
    ///
    /// The default value is `true`, because usually the performance impact is
    /// negligible, and is is generally more practical to work with sorted data.
    pub sorted: bool,
}

/// Enables options to be set up like this (note that `Options` is marked
/// `#[non_exhaustive]`):
///
/// ```
/// # use public_api::Options;
/// let mut options = Options::default();
/// options.sorted = true;
/// // ...
/// ```
impl Default for Options {
    fn default() -> Self {
        Self {
            simplified: false,
            sorted: true,
        }
    }
}

/// The public API of a crate
///
/// Return type of [`public_api_from_rustdoc_json_str`].
///
/// ## Rendering the items
///
/// To render the items in the public API you can iterate over the [items](PublicItem).
///
/// You get the `rustdoc_json_str` in the example below as explained in the [crate] documentation, either via
/// [`rustdoc_json`](https://crates.io/crates/rustdoc_json) or by calling `cargo rustdoc` yourself.
///
/// ```no_run
/// use public_api::{PublicApi, Options};
///
/// # let rustdoc_json_str: String = todo!();
/// let options = Options::default();
/// // Gather the rustdoc content as described in this crates top-level documentation.
/// let public_api = PublicApi::from_rustdoc_json_str(&rustdoc_json_str, options)?;
///
/// for public_item in public_api.items() {
///     // here we print the items to stdout, we could also write to a string or a file.
///     println!("{}", public_item);
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug)]
#[non_exhaustive] // More fields might be added in the future
pub struct PublicApi {
    /// The items that constitutes the public API. An "item" is for example a
    /// function, a struct, a struct field, an enum, an enum variant, a module,
    /// etc... Note that this Vec does not include `impl`s items, but the
    /// iterator returned by [`Self::items()`] do include them.
    pub(crate) items: Vec<PublicItem>,

    /// See [`Self::missing_item_ids()`]
    pub(crate) missing_item_ids: Vec<String>,
}

impl PublicApi {
    /// Takes a [`Path`] to a rustdoc JSON file and returns a [`PublicApi`] with
    /// [`PublicItem`]s where each [`PublicItem`] is one public item of the
    /// crate, i.e. part of the crate's public API. Use [`Self::items()`] or
    /// `[Self::into_items()` to get the items.
    ///
    /// There exists a convenient `cargo public-api` subcommand wrapper for this
    /// function found at <https://github.com/Enselic/cargo-public-api> that
    /// builds the rustdoc JSON for you and then invokes this function. If you don't
    /// want to use that wrapper, use [`rustdoc_json`](https://crates.io/crates/rustdoc_json)
    /// to build and return the path to the rustdoc json or call
    /// ```bash
    /// cargo +nightly rustdoc --lib -- -Z unstable-options --output-format json
    /// ```
    /// to generate the rustdoc JSON that this function takes as input. The output
    /// is put in `./target/doc/your_library.json`. As mentioned,
    /// [`rustdoc_json`](https://crates.io/crates/rustdoc_json) does this for you.
    ///
    /// For reference, the rustdoc JSON format is documented at
    /// <https://rust-lang.github.io/rfcs/2963-rustdoc-json.html>. But the format is
    /// still a moving target. Open PRs and issues for rustdoc JSON itself can be
    /// found at <https://github.com/rust-lang/rust/labels/A-rustdoc-json>.
    ///
    /// # Errors
    ///
    /// E.g. if the JSON is invalid or if the file can't be read.
    pub fn from_rustdoc_json(path: impl AsRef<Path>, options: Options) -> Result<PublicApi> {
        Self::from_rustdoc_json_str(&std::fs::read_to_string(path)?, options)
    }

    /// Same as [`Self::from_rustdoc_json`], but the rustdoc JSON is read from a
    /// `&str` rather than a file.
    ///
    /// # Errors
    ///
    /// E.g. if the JSON is invalid.
    pub fn from_rustdoc_json_str(
        rustdoc_json_str: impl AsRef<str>,
        options: Options,
    ) -> Result<PublicApi> {
        let crate_ = deserialize_without_recursion_limit(rustdoc_json_str.as_ref())?;

        let mut public_api = item_iterator::public_api_in_crate(&crate_, options);

        if options.sorted {
            for item in &mut public_api.items {
                item.impls.sort();
            }
            public_api.items.sort();
        }

        Ok(public_api)
    }

    /// Returns an iterator over all public items in the public API
    pub fn items(&self) -> impl Iterator<Item = &'_ PublicItem> {
        self.items.iter().flat_map(|item| {
            let mut all = vec![];
            all.push(item);
            all.extend(item.impls.iter());
            all
        })
    }

    /// Like [`Self::items()`], but ownership of all `PublicItem`s are
    /// transferred to the caller.
    pub fn into_items(self) -> impl Iterator<Item = PublicItem> {
        self.items.into_iter().flat_map(|item| {
            let PublicItem {
                path,
                tokens,
                children,
                impls,
            } = item;

            let children_stripped = PublicItem {
                path,
                tokens,
                children: vec![],
                impls: vec![],
            };

            let mut all = vec![];
            all.push(children_stripped);
            all.extend(children.into_iter());
            all.extend(impls.into_iter());
            all
        })
    }

    /// The rustdoc JSON IDs of missing but referenced items. Intended for use
    /// with `--verbose` flags or similar.
    ///
    /// In some cases, a public item might be referenced from another public
    /// item (e.g. a `mod`), but is missing from the rustdoc JSON file. This
    /// occurs for example in the case of re-exports of external modules (see
    /// <https://github.com/Enselic/cargo-public-api/issues/103>). The entries
    /// in this Vec are what IDs that could not be found.
    ///
    /// The exact format of IDs are to be considered an implementation detail
    /// and must not be be relied on.
    pub fn missing_item_ids(&self) -> impl Iterator<Item = &String> {
        self.missing_item_ids.iter()
    }
}

#[allow(clippy::missing_errors_doc, missing_docs)]
#[deprecated(
    note = "use `PublicApi::from_rustdoc_json` or `PublicApi::from_rustdoc_json_str` instead"
)]
pub fn public_api_from_rustdoc_json_str(
    rustdoc_json_str: &str,
    options: Options,
) -> Result<PublicApi> {
    PublicApi::from_rustdoc_json_str(rustdoc_json_str, options)
}

/// Helper to deserialize the JSON with `serde_json`, but with the recursion
/// limit disabled. Otherwise we hit the recursion limit on crates such as
/// `diesel`.
fn deserialize_without_recursion_limit(rustdoc_json_str: &str) -> Result<rustdoc_types::Crate> {
    let mut deserializer = serde_json::Deserializer::from_str(rustdoc_json_str);
    deserializer.disable_recursion_limit();
    Ok(serde::de::Deserialize::deserialize(&mut deserializer)?)
}
