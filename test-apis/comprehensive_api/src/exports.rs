pub mod v0 {
    pub fn foo() {}
}

pub mod v1 {
    // Make v1 compatible with v0 by using a wildcard import like this
    pub use super::v0::*;

    pub fn foo2() {
        foo();
    }
}

pub mod recursion_1 {
    pub use super::recursion_2;
}

pub mod recursion_2 {
    pub use super::recursion_1;
}

pub mod recursion_glob_1 {
    pub use super::recursion_glob_2::*;
}

pub mod recursion_glob_2 {
    pub use super::recursion_glob_1::*;
}

/// Test code from <https://github.com/Enselic/cargo-public-api/issues/145>
pub mod issue_145 {
    use example_api as internal_name_for_a_crate;
    use example_api::Struct;
    use example_api::Struct as WeirdName;

    pub fn test1(_transform: Struct) {}
    pub fn test2(_transform: WeirdName) {}
    pub fn test3(_transform: example_api::Struct) {}
    pub fn test4(_transform: internal_name_for_a_crate::Struct) {}

    pub enum Test {
        V1(Struct),
        V2(WeirdName),
        V3(example_api::Struct),
        V4(internal_name_for_a_crate::Struct),
    }
}
