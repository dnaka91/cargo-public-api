#![allow(clippy::unused_self)]
use crate::intermediate_public_item::IntermediatePublicItem;
use std::rc::Rc;

use rustdoc_types::{
    Abi, Constant, Crate, FnDecl, FunctionPointer, GenericArg, GenericArgs, GenericBound,
    GenericParamDef, GenericParamDefKind, Generics, Header, ItemEnum, MacroKind, Path, PolyTrait,
    StructKind, Term, Type, TypeBinding, TypeBindingKind, Variant, WherePredicate,
};

/// A simple macro to write `Token::Whitespace` in less characters.
macro_rules! ws {
    () => {
        Token::Whitespace
    };
}

use crate::tokens::Token;

enum Binding<'a> {
    GenericArg(&'a GenericArg),
    TypeBinding(&'a TypeBinding),
}

pub struct RenderingContext<'a> {
    pub crate_: &'a Crate,
}

impl<'a> RenderingContext<'a> {
    #[allow(clippy::too_many_lines)]
    pub fn token_stream(&self, item: &IntermediatePublicItem) -> Vec<Token> {
        let mut tokens = vec![];

        for attr in &item.item.attrs {
            if attr_relevant_for_public_apis(attr) {
                tokens.push(Token::Annotation(attr.clone()));
                tokens.push(ws!());
            }
        }

        let inner_tokens = match &item.item.inner {
            ItemEnum::Module(_) => self.render_simple(&["mod"], &item.path()),
            ItemEnum::ExternCrate { .. } => self.render_simple(&["extern", "crate"], &item.path()),
            ItemEnum::Import(_) => self.render_simple(&["use"], &item.path()),
            ItemEnum::Union(_) => self.render_simple(&["union"], &item.path()),
            ItemEnum::Struct(s) => {
                let mut output = self.render_simple(&["struct"], &item.path());
                output.extend(self.render_generics(&s.generics));
                if matches!(s.kind, StructKind::Tuple(_)) {
                    output
                        .extend(self.render_option_tuple(&item.pre_resolved_fields, Some(&pub_())));
                }
                output
            }
            ItemEnum::StructField(inner) => {
                let mut output = self.render_simple(&["struct", "field"], &item.path());
                output.extend(colon());
                output.extend(self.render_type(inner));
                output
            }
            ItemEnum::Enum(e) => {
                let mut output = self.render_simple(&["enum"], &item.path());
                output.extend(self.render_generics(&e.generics));
                output
            }
            ItemEnum::Variant(inner) => {
                let mut output = self.render_simple(&["enum", "variant"], &item.path());
                match inner {
                    Variant::Struct { .. } => {} // Each struct field is printed individually
                    Variant::Plain(discriminant) => {
                        if let Some(discriminant) = discriminant {
                            output.extend(equals());
                            output.push(Token::identifier(&discriminant.value));
                        }
                    }
                    Variant::Tuple(_) => {
                        output.extend(self.render_option_tuple(&item.pre_resolved_fields, None));
                    }
                }
                output
            }
            ItemEnum::Function(inner) => self.render_function(
                self.render_path(&item.path()),
                &inner.decl,
                &inner.generics,
                &inner.header,
            ),
            ItemEnum::Method(inner) => self.render_function(
                self.render_path(&item.path()),
                &inner.decl,
                &inner.generics,
                &inner.header,
            ),
            ItemEnum::Trait(inner) => {
                let tags = if inner.is_unsafe {
                    vec!["unsafe", "trait"]
                } else {
                    vec!["trait"]
                };
                let mut output = self.render_simple(&tags, &item.path());
                output.extend(self.render_generics(&inner.generics));
                output
            }
            ItemEnum::TraitAlias(_) => self.render_simple(&["trait", "alias"], &item.path()),
            ItemEnum::Impl(_) => self.render_simple(&["impl"], &item.path()),
            ItemEnum::Typedef(inner) => {
                let mut output = self.render_simple(&["type"], &item.path());
                output.extend(self.render_generics(&inner.generics));
                output.extend(equals());
                output.extend(self.render_type(&inner.type_));
                output
            }
            ItemEnum::AssocType {
                generics,
                bounds,
                default,
            } => {
                let mut output = self.render_simple(&["type"], &item.path());
                output.extend(self.render_generics(generics));
                output.extend(self.render_generic_bounds(bounds));
                if let Some(ty) = default {
                    output.extend(equals());
                    output.extend(self.render_type(ty));
                }
                output
            }
            ItemEnum::OpaqueTy(_) => self.render_simple(&["opaque", "type"], &item.path()),
            ItemEnum::Constant(con) => {
                let mut output = self.render_simple(&["const"], &item.path());
                output.extend(colon());
                output.extend(self.render_constant(con));
                output
            }
            ItemEnum::AssocConst { type_, .. } => {
                let mut output = self.render_simple(&["const"], &item.path());
                output.extend(colon());
                output.extend(self.render_type(type_));
                output
            }
            ItemEnum::Static(inner) => {
                let tags = if inner.mutable {
                    vec!["mut", "static"]
                } else {
                    vec!["static"]
                };
                let mut output = self.render_simple(&tags, &item.path());
                output.extend(colon());
                output.extend(self.render_type(&inner.type_));
                output
            }
            ItemEnum::ForeignType => self.render_simple(&["type"], &item.path()),
            ItemEnum::Macro(_definition) => {
                // TODO: _definition contains the whole definition, it would be really neat to get out all possible ways to invoke it
                let mut output = self.render_simple(&["macro"], &item.path());
                output.push(Token::symbol("!"));
                output
            }
            ItemEnum::ProcMacro(inner) => {
                let mut output = self.render_simple(&["proc", "macro"], &item.path());
                output.pop(); // Remove name of macro to possibly wrap it in `#[]`
                let name = Token::identifier(item.item.name.as_ref().unwrap_or(&"".to_string()));
                match inner.kind {
                    MacroKind::Bang => output.extend(vec![name, Token::symbol("!()")]),
                    MacroKind::Attr => {
                        output.extend(vec![Token::symbol("#["), name, Token::symbol("]")]);
                    }
                    MacroKind::Derive => {
                        output.extend(vec![Token::symbol("#[derive("), name, Token::symbol(")]")]);
                    }
                }
                output
            }
            ItemEnum::PrimitiveType(_) => self.render_simple(&["primitive", "type"], &item.path()),
        };

        tokens.extend(inner_tokens);

        tokens
    }

    fn render_simple(&self, tags: &[&str], path: &[Rc<IntermediatePublicItem<'_>>]) -> Vec<Token> {
        let mut output = pub_();
        output.extend(
            tags.iter()
                .flat_map(|t| [Token::kind(*t), ws!()])
                .collect::<Vec<Token>>(),
        );
        output.extend(self.render_path(path));
        output
    }

    fn render_path(&self, path: &[Rc<IntermediatePublicItem<'_>>]) -> Vec<Token> {
        let mut output = vec![];
        for item in path {
            let token_fn = if matches!(item.item.inner, ItemEnum::Function(_) | ItemEnum::Method(_))
            {
                Token::function
            } else if matches!(
                item.item.inner,
                ItemEnum::Trait(_)
                    | ItemEnum::Struct(_)
                    | ItemEnum::Union(_)
                    | ItemEnum::Enum(_)
                    | ItemEnum::Typedef(_)
            ) {
                Token::type_
            } else {
                Token::identifier
            };
            output.push(token_fn(item.name.clone()));
            output.push(Token::symbol("::"));
        }
        if !path.is_empty() {
            output.pop();
        }
        output
    }

    #[allow(clippy::needless_pass_by_value)]
    fn render_sequence<T>(
        &self,
        start: Vec<Token>,
        end: Vec<Token>,
        between: Vec<Token>,
        sequence: &[T],
        render: impl Fn(&T) -> Vec<Token>,
    ) -> Vec<Token> {
        self.render_sequence_impl(start, end, between, false, sequence, render)
    }

    #[allow(clippy::needless_pass_by_value)]
    fn render_sequence_if_not_empty<T>(
        &self,
        start: Vec<Token>,
        end: Vec<Token>,
        between: Vec<Token>,
        sequence: &[T],
        render: impl Fn(&T) -> Vec<Token>,
    ) -> Vec<Token> {
        self.render_sequence_impl(start, end, between, true, sequence, render)
    }

    #[allow(clippy::needless_pass_by_value)]
    fn render_sequence_impl<T>(
        &self,
        start: Vec<Token>,
        end: Vec<Token>,
        between: Vec<Token>,
        return_nothing_if_empty: bool,
        sequence: &[T],
        render: impl Fn(&T) -> Vec<Token>,
    ) -> Vec<Token> {
        if return_nothing_if_empty && sequence.is_empty() {
            return vec![];
        }
        let mut output = start;
        for (index, seq) in sequence.iter().enumerate() {
            output.extend(render(seq));
            if index < sequence.len() - 1 {
                output.extend(between.clone());
            }
        }
        output.extend(end);
        output
    }

    fn render_type(&self, ty: &Type) -> Vec<Token> {
        self.render_option_type(&Some(ty))
    }

    #[allow(clippy::ref_option_ref, clippy::trivially_copy_pass_by_ref)] // Because of `render_sequence()` arg types
    fn render_option_type(&self, ty: &Option<&Type>) -> Vec<Token> {
        let ty = match ty {
            Some(ty) => ty,
            None => return vec![Token::symbol("_")], // The `_` in `EnumWithStrippedTupleVariants::DoubleFirstHidden(_, bool)`
        };
        match ty {
            Type::ResolvedPath(path) => self.render_resolved_path(path),
            Type::DynTrait(dyn_trait) => self.render_dyn_trait(dyn_trait),
            Type::Generic(name) => vec![Token::generic(name)],
            Type::Primitive(name) => vec![Token::primitive(name)],
            Type::FunctionPointer(ptr) => self.render_function_pointer(ptr),
            Type::Tuple(types) => self.render_tuple(types),
            Type::Slice(ty) => self.render_slice(ty),
            Type::Array { type_, len } => self.render_array(type_, len),
            Type::ImplTrait(bounds) => self.render_impl_trait(bounds),
            Type::Infer => vec![Token::symbol("_")],
            Type::RawPointer { mutable, type_ } => self.render_raw_pointer(*mutable, type_),
            Type::BorrowedRef {
                lifetime,
                mutable,
                type_,
            } => self.render_borrowed_ref(lifetime.as_deref(), *mutable, type_),
            Type::QualifiedPath {
                name,
                args: _,
                self_type,
                trait_,
            } => self.render_qualified_path(self_type, trait_, name),
        }
    }

    fn render_dyn_trait(&self, dyn_trait: &rustdoc_types::DynTrait) -> Vec<Token> {
        let mut output = vec![];

        let more_than_one = dyn_trait.traits.len() > 1 || dyn_trait.lifetime.is_some();
        if more_than_one {
            output.push(Token::symbol("("));
        }

        output.extend(self.render_sequence_if_not_empty(
            vec![Token::keyword("dyn"), ws!()],
            vec![],
            plus(),
            &dyn_trait.traits,
            |p| self.render_poly_trait(p),
        ));

        if let Some(lt) = &dyn_trait.lifetime {
            output.extend(plus());
            output.extend(vec![Token::lifetime(lt)]);
        }

        if more_than_one {
            output.push(Token::symbol(")"));
        }

        output
    }

    fn render_function(
        &self,
        name: Vec<Token>,
        decl: &FnDecl,
        generics: &Generics,
        header: &Header,
    ) -> Vec<Token> {
        let mut output = pub_();
        if header.unsafe_ {
            output.extend(vec![Token::qualifier("unsafe"), ws!()]);
        };
        if header.const_ {
            output.extend(vec![Token::qualifier("const"), ws!()]);
        };
        if header.async_ {
            output.extend(vec![Token::qualifier("async"), ws!()]);
        };
        if header.abi != Abi::Rust {
            output.push(match &header.abi {
                Abi::C { .. } => Token::qualifier("c"),
                Abi::Cdecl { .. } => Token::qualifier("cdecl"),
                Abi::Stdcall { .. } => Token::qualifier("stdcall"),
                Abi::Fastcall { .. } => Token::qualifier("fastcall"),
                Abi::Aapcs { .. } => Token::qualifier("aapcs"),
                Abi::Win64 { .. } => Token::qualifier("win64"),
                Abi::SysV64 { .. } => Token::qualifier("sysV64"),
                Abi::System { .. } => Token::qualifier("system"),
                Abi::Other(text) => Token::qualifier(text),
                Abi::Rust => unreachable!(),
            });
            output.push(ws!());
        }

        output.extend(vec![Token::kind("fn"), ws!()]);
        output.extend(name);

        // Generic parameters
        output.extend(self.render_generic_param_defs(&generics.params));

        // Regular parameters and return type
        output.extend(self.render_fn_decl(decl));

        // Where predicates
        output.extend(self.render_where_predicates(&generics.where_predicates));

        output
    }

    fn render_fn_decl(&self, decl: &FnDecl) -> Vec<Token> {
        let mut output = vec![];
        // Main arguments
        output.extend(self.render_sequence(
            vec![Token::symbol("(")],
            vec![Token::symbol(")")],
            comma(),
            &decl.inputs,
            |(name, ty)| {
                self.simplified_self(name, ty).unwrap_or_else(|| {
                    let mut output = vec![];
                    if name != "_" {
                        output.extend(vec![Token::identifier(name), Token::symbol(":"), ws!()]);
                    }
                    output.extend(self.render_type(ty));
                    output
                })
            },
        ));
        // Return type
        if let Some(ty) = &decl.output {
            output.extend(arrow());
            output.extend(self.render_type(ty));
        }
        output
    }

    fn simplified_self(&self, name: &str, ty: &Type) -> Option<Vec<Token>> {
        if name == "self" {
            match ty {
                Type::Generic(name) if name == "Self" => Some(vec![Token::self_("self")]),
                Type::BorrowedRef {
                    lifetime,
                    mutable,
                    type_,
                } => match type_.as_ref() {
                    Type::Generic(name) if name == "Self" => {
                        let mut output = vec![Token::symbol("&")];
                        if let Some(lt) = lifetime {
                            output.extend(vec![Token::lifetime(lt), ws!()]);
                        }
                        if *mutable {
                            output.extend(vec![Token::keyword("mut"), ws!()]);
                        }
                        output.push(Token::self_("self"));
                        Some(output)
                    }
                    _ => None,
                },
                _ => None,
            }
        } else {
            None
        }
    }

    fn render_resolved_path(&self, path: &Path) -> Vec<Token> {
        let mut output = vec![];
        let name = &path.name;
        if !name.is_empty() {
            let split: Vec<_> = name.split("::").collect();
            let len = split.len();
            for (index, part) in split.into_iter().enumerate() {
                if index == 0 && part == "$crate" {
                    output.push(Token::identifier("$crate"));
                } else if index == len - 1 {
                    output.push(Token::type_(part));
                } else {
                    output.push(Token::identifier(part));
                }
                output.push(Token::symbol("::"));
            }
            if len > 0 {
                output.pop();
            }
            if let Some(args) = &path.args {
                output.extend(self.render_generic_args(args));
            }
        }
        output
    }

    fn render_function_pointer(&self, ptr: &FunctionPointer) -> Vec<Token> {
        let mut output = self.render_higher_rank_trait_bounds(&ptr.generic_params);
        output.push(Token::kind("fn"));
        output.extend(self.render_fn_decl(&ptr.decl));
        output
    }

    fn render_tuple(&self, types: &[Type]) -> Vec<Token> {
        let option_tuple: Vec<Option<&Type>> = types.iter().map(Some).collect();
        self.render_option_tuple(&option_tuple, None)
    }

    /// `prefix` is to handle the difference  between tuple structs and enum variant
    /// tuple structs. The former marks public fields as `pub ` whereas all fields
    /// of enum tuple structs are always implicitly `pub`.
    fn render_option_tuple(&self, types: &[Option<&Type>], prefix: Option<&[Token]>) -> Vec<Token> {
        self.render_sequence(
            vec![Token::symbol("(")],
            vec![Token::symbol(")")],
            comma(),
            types,
            |type_| {
                let mut output: Vec<Token> = vec![];
                if let (Some(prefix), Some(_)) = (prefix, type_) {
                    output.extend(prefix.to_owned());
                }
                output.extend(self.render_option_type(type_));
                output
            },
        )
    }

    fn render_slice(&self, ty: &Type) -> Vec<Token> {
        let mut output = vec![Token::symbol("[")];
        output.extend(self.render_type(ty));
        output.push(Token::symbol("]"));
        output
    }

    fn render_array(&self, type_: &Type, len: &str) -> Vec<Token> {
        let mut output = vec![Token::symbol("[")];
        output.extend(self.render_type(type_));
        output.extend(vec![
            Token::symbol(";"),
            ws!(),
            Token::primitive(len),
            Token::symbol("]"),
        ]);
        output
    }

    fn render_impl_trait(&self, bounds: &[GenericBound]) -> Vec<Token> {
        let mut output = vec![Token::keyword("impl")];
        output.push(ws!());
        output.extend(self.render_generic_bounds(bounds));
        output
    }

    fn render_raw_pointer(&self, mutable: bool, type_: &Type) -> Vec<Token> {
        let mut output = vec![Token::symbol("*")];
        output.push(Token::keyword(if mutable { "mut" } else { "const" }));
        output.push(ws!());
        output.extend(self.render_type(type_));
        output
    }

    fn render_borrowed_ref(
        &self,
        lifetime: Option<&str>,
        mutable: bool,
        type_: &Type,
    ) -> Vec<Token> {
        let mut output = vec![Token::symbol("&")];
        if let Some(lt) = lifetime {
            output.extend(vec![Token::lifetime(lt), ws!()]);
        }
        if mutable {
            output.extend(vec![Token::keyword("mut"), ws!()]);
        }
        output.extend(self.render_type(type_));
        output
    }

    fn render_qualified_path(&self, type_: &Type, trait_: &Path, name: &str) -> Vec<Token> {
        let mut output = vec![];
        match type_ {
            Type::Generic(name) if name == "Self" && trait_.name.is_empty() => {
                output.push(Token::keyword("Self"));
            }
            _ => {
                output.push(Token::symbol("<"));
                output.extend(self.render_type(type_));
                output.extend(vec![ws!(), Token::keyword("as"), ws!()]);
                output.extend(self.render_resolved_path(trait_));
                output.push(Token::symbol(">"));
            }
        }
        output.push(Token::symbol("::"));
        output.push(Token::identifier(name));
        output
    }

    fn render_generic_args(&self, args: &GenericArgs) -> Vec<Token> {
        match args {
            GenericArgs::AngleBracketed { args, bindings } => self.render_sequence_if_not_empty(
                vec![Token::symbol("<")],
                vec![Token::symbol(">")],
                comma(),
                &args
                    .iter()
                    .map(Binding::GenericArg)
                    .chain(bindings.iter().map(Binding::TypeBinding))
                    .collect::<Vec<_>>(),
                |arg| match arg {
                    Binding::GenericArg(arg) => self.render_generic_arg(arg),
                    Binding::TypeBinding(binding) => self.render_type_binding(binding),
                },
            ),
            GenericArgs::Parenthesized {
                inputs,
                output: return_ty,
            } => {
                let mut output = self.render_sequence(
                    vec![Token::symbol("(")],
                    vec![Token::symbol(")")],
                    comma(),
                    inputs,
                    |type_| self.render_type(type_),
                );
                if let Some(return_ty) = return_ty {
                    output.extend(arrow());
                    output.extend(self.render_type(return_ty));
                }
                output
            }
        }
    }

    fn render_term(&self, term: &Term) -> Vec<Token> {
        match term {
            Term::Type(ty) => self.render_type(ty),
            Term::Constant(c) => self.render_constant(c),
        }
    }

    fn render_poly_trait(&self, poly_trait: &PolyTrait) -> Vec<Token> {
        let mut output = self.render_higher_rank_trait_bounds(&poly_trait.generic_params);
        output.extend(self.render_resolved_path(&poly_trait.trait_));
        output
    }

    fn render_generic_arg(&self, arg: &GenericArg) -> Vec<Token> {
        match arg {
            GenericArg::Lifetime(name) => vec![Token::lifetime(name)],
            GenericArg::Type(ty) => self.render_type(ty),
            GenericArg::Const(c) => self.render_constant(c),
            GenericArg::Infer => vec![Token::symbol("_")],
        }
    }

    fn render_type_binding(&self, binding: &TypeBinding) -> Vec<Token> {
        let mut output = vec![Token::identifier(&binding.name)];
        output.extend(self.render_generic_args(&binding.args));
        match &binding.binding {
            TypeBindingKind::Equality(term) => {
                output.extend(equals());
                output.extend(self.render_term(term));
            }
            TypeBindingKind::Constraint(bounds) => {
                output.extend(self.render_generic_bounds(bounds));
            }
        }
        output
    }

    fn render_constant(&self, constant: &Constant) -> Vec<Token> {
        let mut output = self.render_type(&constant.type_);
        if let Some(value) = &constant.value {
            output.extend(equals());
            if constant.is_literal {
                output.push(Token::primitive(value));
            } else {
                output.push(Token::identifier(value));
            }
        }
        output
    }

    fn render_generics(&self, generics: &Generics) -> Vec<Token> {
        let mut output = vec![];
        output.extend(self.render_generic_param_defs(&generics.params));
        output.extend(self.render_where_predicates(&generics.where_predicates));
        output
    }

    fn render_generic_param_defs(&self, params: &[GenericParamDef]) -> Vec<Token> {
        let params_without_synthetics: Vec<_> = params
            .iter()
            .filter(|p| {
                if let GenericParamDefKind::Type { synthetic, .. } = p.kind {
                    !synthetic
                } else {
                    true
                }
            })
            .collect();

        self.render_sequence_if_not_empty(
            vec![Token::symbol("<")],
            vec![Token::symbol(">")],
            comma(),
            &params_without_synthetics,
            |param| self.render_generic_param_def(param),
        )
    }

    fn render_generic_param_def(&self, generic_param_def: &GenericParamDef) -> Vec<Token> {
        let mut output = vec![];
        match &generic_param_def.kind {
            GenericParamDefKind::Lifetime { outlives } => {
                output.push(Token::lifetime(&generic_param_def.name));
                if !outlives.is_empty() {
                    output.extend(colon());
                    output.extend(self.render_sequence(vec![], vec![], plus(), outlives, |s| {
                        vec![Token::lifetime(s)]
                    }));
                }
            }
            GenericParamDefKind::Type { bounds, .. } => {
                output.push(Token::generic(&generic_param_def.name));
                if !bounds.is_empty() {
                    output.extend(colon());
                    output.extend(self.render_generic_bounds(bounds));
                }
            }
            GenericParamDefKind::Const { type_, .. } => {
                output.push(Token::qualifier("const"));
                output.push(ws!());
                output.push(Token::identifier(&generic_param_def.name));
                output.extend(colon());
                output.extend(self.render_type(type_));
            }
        }
        output
    }

    fn render_where_predicates(&self, where_predicates: &[WherePredicate]) -> Vec<Token> {
        let mut output = vec![];
        if !where_predicates.is_empty() {
            output.push(ws!());
            output.push(Token::Keyword("where".to_owned()));
            output.push(ws!());
            output.extend(
                self.render_sequence(vec![], vec![], comma(), where_predicates, |p| {
                    self.render_where_predicate(p)
                }),
            );
        }
        output
    }

    fn render_where_predicate(&self, where_predicate: &WherePredicate) -> Vec<Token> {
        let mut output = vec![];
        match where_predicate {
            WherePredicate::BoundPredicate {
                type_,
                bounds,
                generic_params,
            } => {
                output.extend(self.render_higher_rank_trait_bounds(generic_params));
                output.extend(self.render_type(type_));
                output.extend(colon());
                output.extend(self.render_generic_bounds(bounds));
            }
            WherePredicate::RegionPredicate {
                lifetime,
                bounds: _,
            } => output.push(Token::Lifetime(lifetime.clone())),
            WherePredicate::EqPredicate { lhs, rhs } => {
                output.extend(self.render_type(lhs));
                output.extend(equals());
                output.extend(self.render_term(rhs));
            }
        }
        output
    }

    fn render_generic_bounds(&self, bounds: &[GenericBound]) -> Vec<Token> {
        self.render_sequence_if_not_empty(vec![], vec![], plus(), bounds, |bound| match bound {
            GenericBound::TraitBound {
                trait_,
                generic_params,
                ..
            } => {
                let mut output = vec![];
                output.extend(self.render_higher_rank_trait_bounds(generic_params));
                output.extend(self.render_resolved_path(trait_));
                output
            }
            GenericBound::Outlives(id) => vec![Token::lifetime(id)],
        })
    }

    fn render_higher_rank_trait_bounds(&self, generic_params: &[GenericParamDef]) -> Vec<Token> {
        let mut output = vec![];
        if !generic_params.is_empty() {
            output.push(Token::keyword("for"));
            output.extend(self.render_generic_param_defs(generic_params));
            output.push(ws!());
        }
        output
    }
}

/// Our list of allowed attributes comes from
/// <https://github.com/rust-lang/rust/blob/68d0b29098/src/librustdoc/html/render/mod.rs#L941-L942>
fn attr_relevant_for_public_apis<S: AsRef<str>>(attr: S) -> bool {
    let prefixes = [
        "#[export_name",
        "#[link_section",
        "#[no_mangle",
        "#[non_exhaustive",
        "#[repr",
    ];

    for prefix in prefixes {
        if attr.as_ref().starts_with(prefix) {
            return true;
        }
    }

    false
}

fn pub_() -> Vec<Token> {
    vec![Token::qualifier("pub"), ws!()]
}

fn plus() -> Vec<Token> {
    vec![ws!(), Token::symbol("+"), ws!()]
}

fn colon() -> Vec<Token> {
    vec![Token::symbol(":"), ws!()]
}

fn comma() -> Vec<Token> {
    vec![Token::symbol(","), ws!()]
}

fn equals() -> Vec<Token> {
    vec![ws!(), Token::symbol("="), ws!()]
}

fn arrow() -> Vec<Token> {
    vec![ws!(), Token::symbol("->"), ws!()]
}

#[cfg(test)]
mod test {
    macro_rules! s {
        ($value:literal) => {
            $value.to_string()
        };
    }

    use std::collections::HashMap;

    use super::*;
    use rustdoc_types::Id;

    #[test]
    fn test_type_infer() {
        assert_render(
            |context| context.render_type(&Type::Infer),
            vec![Token::symbol("_")],
            "_",
        );
    }

    #[test]
    fn test_type_generic() {
        assert_render(
            |context| context.render_type(&Type::Generic(s!("name"))),
            vec![Token::generic("name")],
            "name",
        );
    }

    #[test]
    fn test_type_primitive() {
        assert_render(
            |context| context.render_type(&Type::Primitive(s!("name"))),
            vec![Token::primitive("name")],
            "name",
        );
    }

    #[test]
    fn test_type_resolved_simple() {
        assert_render(
            |context| {
                context.render_type(&Type::ResolvedPath(Path {
                    name: s!("name"),
                    args: None,
                    id: Id(s!("id")),
                }))
            },
            vec![Token::type_("name")],
            "name",
        );
    }

    #[test]
    fn test_type_resolved_long_name() {
        assert_render(
            |context| {
                context.render_type(&Type::ResolvedPath(Path {
                    name: s!("name::with::parts"),
                    args: None,
                    id: Id(s!("id")),
                }))
            },
            vec![
                Token::identifier("name"),
                Token::symbol("::"),
                Token::identifier("with"),
                Token::symbol("::"),
                Token::type_("parts"),
            ],
            "name::with::parts",
        );
    }

    #[test]
    fn test_type_resolved_crate_name() {
        assert_render(
            |context| {
                context.render_type(&Type::ResolvedPath(Path {
                    name: s!("$crate::name"),
                    args: None,
                    id: Id(s!("id")),
                }))
            },
            vec![
                Token::identifier("$crate"),
                Token::symbol("::"),
                Token::type_("name"),
            ],
            "$crate::name",
        );
    }

    #[test]
    fn test_type_resolved_name_crate() {
        assert_render(
            |context| {
                context.render_type(&Type::ResolvedPath(Path {
                    name: s!("name::$crate"),
                    args: None,
                    id: Id(s!("id")),
                }))
            },
            vec![
                Token::identifier("name"),
                Token::symbol("::"),
                Token::type_("$crate"),
            ],
            "name::$crate",
        );
    }

    #[test]
    fn test_type_tuple_empty() {
        assert_render(
            |context| context.render_type(&Type::Tuple(vec![])),
            vec![Token::symbol("("), Token::symbol(")")],
            "()",
        );
    }

    #[test]
    fn test_type_tuple() {
        assert_render(
            |context| {
                context.render_type(&Type::Tuple(vec![Type::Infer, Type::Generic(s!("gen"))]))
            },
            vec![
                Token::symbol("("),
                Token::symbol("_"),
                Token::symbol(","),
                ws!(),
                Token::generic("gen"),
                Token::symbol(")"),
            ],
            "(_, gen)",
        );
    }

    #[test]
    fn test_type_slice() {
        assert_render(
            |context| context.render_type(&Type::Slice(Box::new(Type::Infer))),
            vec![Token::symbol("["), Token::symbol("_"), Token::symbol("]")],
            "[_]",
        );
    }

    #[test]
    fn test_type_array() {
        assert_render(
            |context| {
                context.render_type(&Type::Array {
                    type_: Box::new(Type::Infer),
                    len: s!("20"),
                })
            },
            vec![
                Token::symbol("["),
                Token::symbol("_"),
                Token::symbol(";"),
                ws!(),
                Token::primitive("20"),
                Token::symbol("]"),
            ],
            "[_; 20]",
        );
    }

    #[test]
    fn test_type_pointer() {
        assert_render(
            |context| {
                context.render_type(&Type::RawPointer {
                    mutable: false,
                    type_: Box::new(Type::Infer),
                })
            },
            vec![
                Token::symbol("*"),
                Token::keyword("const"),
                ws!(),
                Token::symbol("_"),
            ],
            "*const _",
        );
    }

    #[test]
    fn test_type_pointer_mut() {
        assert_render(
            |context| {
                context.render_type(&Type::RawPointer {
                    mutable: true,
                    type_: Box::new(Type::Infer),
                })
            },
            vec![
                Token::symbol("*"),
                Token::keyword("mut"),
                ws!(),
                Token::symbol("_"),
            ],
            "*mut _",
        );
    }

    #[test]
    fn test_type_ref() {
        assert_render(
            |context| {
                context.render_type(&Type::BorrowedRef {
                    lifetime: None,
                    mutable: false,
                    type_: Box::new(Type::Infer),
                })
            },
            vec![Token::symbol("&"), Token::symbol("_")],
            "&_",
        );
    }

    #[test]
    fn test_type_ref_mut() {
        assert_render(
            |context| {
                context.render_type(&Type::BorrowedRef {
                    lifetime: None,
                    mutable: true,
                    type_: Box::new(Type::Infer),
                })
            },
            vec![
                Token::symbol("&"),
                Token::keyword("mut"),
                ws!(),
                Token::symbol("_"),
            ],
            "&mut _",
        );
    }

    #[test]
    fn test_type_ref_lt() {
        assert_render(
            |context| {
                context.render_type(&Type::BorrowedRef {
                    lifetime: Some(s!("'a")),
                    mutable: false,
                    type_: Box::new(Type::Infer),
                })
            },
            vec![
                Token::symbol("&"),
                Token::lifetime("'a"),
                ws!(),
                Token::symbol("_"),
            ],
            "&'a _",
        );
    }

    #[test]
    fn test_type_ref_lt_mut() {
        assert_render(
            |context| {
                context.render_type(&Type::BorrowedRef {
                    lifetime: Some(s!("'a")),
                    mutable: true,
                    type_: Box::new(Type::Infer),
                })
            },
            vec![
                Token::symbol("&"),
                Token::lifetime("'a"),
                ws!(),
                Token::keyword("mut"),
                ws!(),
                Token::symbol("_"),
            ],
            "&'a mut _",
        );
    }

    #[test]
    fn test_type_path() {
        assert_render(
            |context| {
                context.render_type(&Type::QualifiedPath {
                    name: s!("name"),
                    args: Box::new(GenericArgs::AngleBracketed {
                        args: vec![],
                        bindings: vec![],
                    }),
                    self_type: Box::new(Type::Generic(s!("type"))),
                    trait_: Path {
                        name: String::from("trait"),
                        args: None,
                        id: Id(s!("id")),
                    },
                })
            },
            vec![
                Token::symbol("<"),
                Token::generic("type"),
                ws!(),
                Token::keyword("as"),
                ws!(),
                Token::type_("trait"),
                Token::symbol(">"),
                Token::symbol("::"),
                Token::identifier("name"),
            ],
            "<type as trait>::name",
        );
    }

    #[allow(clippy::needless_pass_by_value)]
    fn assert_render(
        render_fn: impl Fn(RenderingContext) -> Vec<Token>,
        expected: Vec<Token>,
        expected_string: &str,
    ) {
        let c = Crate {
            root: Id(String::from("hej")),
            crate_version: None,
            includes_private: false,
            index: HashMap::new(),
            paths: HashMap::new(),
            external_crates: HashMap::new(),
            format_version: 0,
        };
        let r = RenderingContext { crate_: &c };
        let actual = render_fn(r);
        assert_eq!(actual, expected);
        assert_eq!(
            crate::item_iterator::tokens_to_string(&actual),
            expected_string.to_string()
        );
    }
}
