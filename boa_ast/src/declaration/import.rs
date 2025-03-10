//! Import declaration AST nodes.
//!
//! This module contains `import` declaration AST nodes.
//!
//! More information:
//! - [MDN documentation][mdn]
//!  - [ECMAScript specification][spec]
//!
//! [spec]: https://tc39.es/ecma262/#sec-imports
//! [mdn]: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Statements/import

use std::ops::ControlFlow;

use crate::{
    expression::Identifier,
    try_break,
    visitor::{VisitWith, Visitor, VisitorMut},
};
use boa_interner::Sym;

use super::ModuleSpecifier;

/// The kind of import in an [`ImportDeclaration`].
#[derive(Debug, Clone)]
pub enum ImportKind {
    /// Default (`import defaultName from "module-name"`) or unnamed (`import "module-name").
    DefaultOrUnnamed,
    /// Namespaced import (`import * as name from "module-name"`).
    Namespaced {
        /// Binding for the namespace created from the exports of the imported module.
        binding: Identifier,
    },
    /// Import list (`import { export1, export2 as alias2 } from "module-name"`).
    Named {
        /// List of the required exports of the imported module.
        names: Box<[ImportSpecifier]>,
    },
}

impl VisitWith for ImportKind {
    fn visit_with<'a, V>(&'a self, visitor: &mut V) -> ControlFlow<V::BreakTy>
    where
        V: Visitor<'a>,
    {
        match self {
            Self::DefaultOrUnnamed => ControlFlow::Continue(()),
            Self::Namespaced { binding } => visitor.visit_identifier(binding),
            Self::Named { names } => {
                for name in &**names {
                    try_break!(visitor.visit_import_specifier(name));
                }
                ControlFlow::Continue(())
            }
        }
    }

    fn visit_with_mut<'a, V>(&'a mut self, visitor: &mut V) -> ControlFlow<V::BreakTy>
    where
        V: VisitorMut<'a>,
    {
        match self {
            Self::DefaultOrUnnamed => ControlFlow::Continue(()),
            Self::Namespaced { binding } => visitor.visit_identifier_mut(binding),
            Self::Named { names } => {
                for name in &mut **names {
                    try_break!(visitor.visit_import_specifier_mut(name));
                }
                ControlFlow::Continue(())
            }
        }
    }
}

/// An import declaration AST node.
///
/// More information:
///  - [ECMAScript specification][spec]
///
/// [spec]: https://tc39.es/ecma262/#prod-ImportDeclaration
#[derive(Debug, Clone)]
pub struct ImportDeclaration {
    /// Binding for the default export of `specifier`.
    default: Option<Identifier>,
    /// See [`ImportKind`].
    kind: ImportKind,
    /// Module specifier.
    specifier: ModuleSpecifier,
}

impl ImportDeclaration {
    /// Creates a new import declaration.
    #[inline]
    #[must_use]
    pub const fn new(
        default: Option<Identifier>,
        kind: ImportKind,
        specifier: ModuleSpecifier,
    ) -> Self {
        Self {
            default,
            kind,
            specifier,
        }
    }

    /// Gets the binding for the default export of the module.
    #[inline]
    #[must_use]
    pub const fn default(&self) -> Option<Identifier> {
        self.default
    }

    /// Gets the module specifier of the import declaration.
    #[inline]
    #[must_use]
    pub const fn specifier(&self) -> ModuleSpecifier {
        self.specifier
    }

    /// Gets the import kind of the import declaration
    #[inline]
    #[must_use]
    pub const fn kind(&self) -> &ImportKind {
        &self.kind
    }
}

impl VisitWith for ImportDeclaration {
    fn visit_with<'a, V>(&'a self, visitor: &mut V) -> ControlFlow<V::BreakTy>
    where
        V: Visitor<'a>,
    {
        if let Some(default) = &self.default {
            try_break!(visitor.visit_identifier(default));
        }
        try_break!(visitor.visit_import_kind(&self.kind));
        visitor.visit_module_specifier(&self.specifier)
    }

    fn visit_with_mut<'a, V>(&'a mut self, visitor: &mut V) -> ControlFlow<V::BreakTy>
    where
        V: VisitorMut<'a>,
    {
        if let Some(default) = &mut self.default {
            try_break!(visitor.visit_identifier_mut(default));
        }
        try_break!(visitor.visit_import_kind_mut(&mut self.kind));
        visitor.visit_module_specifier_mut(&mut self.specifier)
    }
}

/// Import specifier
///
/// More information:
///  - [ECMAScript specification][spec]
///
/// [spec]: https://tc39.es/ecma262/#prod-ImportSpecifier
#[derive(Debug, Clone, Copy)]
pub struct ImportSpecifier {
    binding: Identifier,
    export_name: Sym,
}

impl ImportSpecifier {
    /// Creates a new [`ImportSpecifier`].
    #[inline]
    #[must_use]
    pub const fn new(binding: Identifier, export_name: Sym) -> Self {
        Self {
            binding,
            export_name,
        }
    }

    /// Gets the binding of the import specifier.
    #[inline]
    #[must_use]
    pub const fn binding(self) -> Identifier {
        self.binding
    }

    /// Gets the optional export name of the import.
    #[inline]
    #[must_use]
    pub const fn export_name(self) -> Sym {
        self.export_name
    }
}

impl VisitWith for ImportSpecifier {
    fn visit_with<'a, V>(&'a self, visitor: &mut V) -> ControlFlow<V::BreakTy>
    where
        V: Visitor<'a>,
    {
        try_break!(visitor.visit_identifier(&self.binding));
        visitor.visit_sym(&self.export_name)
    }

    fn visit_with_mut<'a, V>(&'a mut self, visitor: &mut V) -> ControlFlow<V::BreakTy>
    where
        V: VisitorMut<'a>,
    {
        try_break!(visitor.visit_identifier_mut(&mut self.binding));
        visitor.visit_sym_mut(&mut self.export_name)
    }
}
