/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use crate::context_value::ContextValue;
use crate::format::into_anyhow_for_format;
use crate::root::ErrorRoot;
use crate::Category;
use crate::ErrorType;
use crate::UniqueRootId;

pub type DynLateFormat = dyn Fn(&mut fmt::Formatter<'_>) -> fmt::Result + Send + Sync + 'static;

/// The core error type provided by this crate.
///
/// While this type has many of the features of `anyhow::Error`, in most places you should continue
/// to use `anyhow`. This type is only expected to appear on a small number of APIs which require a
/// clonable error.
///
/// Unlike `anyhow::Error`, this type supports no downcasting. That is an intentional choice -
/// downcasting errors is fragile and becomes difficult to support in conjunction with anyhow
/// compatibility.
#[derive(allocative::Allocative, Clone, dupe::Dupe)]
pub struct Error(pub(crate) Arc<ErrorKind>);

/// The actual error representation.
///
/// The representation is expected to take on a significant bit of additional complexity in the
/// future - the current version is an initial MVP.
///
/// Right now, this type can represent an error root, together with a stack of context information.
#[derive(allocative::Allocative)]
pub(crate) enum ErrorKind {
    Root(Box<ErrorRoot>),
    /// For now we use untyped context to maximize compatibility with anyhow.
    WithContext(ContextValue, Error),
    /// Indicates that the error has been emitted, ie shown to the user.
    // This `Arc` should ideally be a `Box`. However, that doesn't work right now because of the
    // implementation of `into_anyhow_for_format`.
    #[allocative(skip)] // FIXME(JakobDegen): "Implementation is not general enough"
    Emitted(Arc<DynLateFormat>, Error),
}

impl Error {
    #[track_caller]
    pub fn new<E: StdError + Send + Sync + 'static>(e: E) -> Self {
        let source_location =
            crate::source_location::from_file(std::panic::Location::caller().file(), None);
        crate::any::recover_crate_error(&e, source_location)
    }

    fn iter_kinds<'a>(&'a self) -> impl Iterator<Item = &'a ErrorKind> {
        let mut cur = Some(self);
        std::iter::from_fn(move || {
            let out = cur?;
            match &*out.0 {
                ErrorKind::WithContext(_, next) | ErrorKind::Emitted(_, next) => cur = Some(next),
                ErrorKind::Root(_) => cur = None,
            };
            Some(out.0.as_ref())
        })
    }

    fn root(&self) -> &ErrorRoot {
        let Some(ErrorKind::Root(r)) = self.iter_kinds().last() else {
            unreachable!()
        };
        r
    }

    pub fn action_error(&self) -> Option<&buck2_data::ActionError> {
        self.root().action_error()
    }

    pub(crate) fn iter_context<'a>(&'a self) -> impl Iterator<Item = &'a ContextValue> {
        self.iter_kinds().filter_map(|kind| match kind {
            ErrorKind::WithContext(ctx, _) => Some(ctx),
            _ => None,
        })
    }

    pub fn mark_emitted(self, late_format: Arc<DynLateFormat>) -> Self {
        // Have to write this kind of weird to get the compiler to infer a higher ranked closure
        Self(Arc::new(ErrorKind::Emitted(late_format, self)))
    }

    /// If the error has not been emitted yet, returns `None`, otherwise `Some`.
    ///
    /// Most errors are only shown to the user once. However, some errors, specifically action
    /// errors, are shown to the user twice: Once when the error occurs, and again at the end of the
    /// build in the form of a short "Failed to build target" summary.
    ///
    /// After the error has been shown to the user for the first time, it is marked as emitted. The
    /// late formatter that is returned here is what should be printed at the end of the build
    pub fn is_emitted<'a>(&'a self) -> Option<impl fmt::Debug + fmt::Display + 'a> {
        let (val, was_late_formatted) = into_anyhow_for_format(self, true);
        if was_late_formatted { Some(val) } else { None }
    }

    pub fn get_error_type(&self) -> Option<ErrorType> {
        self.root().error_type()
    }

    /// Only intended to be used for debugging, helps to understand the structure of the error
    pub fn get_stack_for_debug(&self) -> String {
        use fmt::Write;
        let mut s = String::new();
        for kind in self.iter_kinds() {
            match kind {
                ErrorKind::Root(r) => {
                    writeln!(s, "ROOT:\n{:#?}", r).unwrap();
                }
                ErrorKind::Emitted(_, _) => {
                    writeln!(s, "EMITTED").unwrap();
                }
                ErrorKind::WithContext(ctx, _) => {
                    writeln!(s, "CONTEXT: {:#}", ctx.as_display_for_debugging()).unwrap();
                }
            }
        }
        s
    }

    pub fn root_id(&self) -> UniqueRootId {
        self.root().id()
    }

    pub fn source_location(&self) -> Option<&str> {
        self.root().source_location()
    }

    pub fn context<C: Into<ContextValue>>(self, context: C) -> Self {
        Self(Arc::new(ErrorKind::WithContext(context.into(), self)))
    }

    #[cold]
    #[track_caller]
    pub(crate) fn new_anyhow_with_context<E, C: Into<ContextValue>>(e: E, c: C) -> anyhow::Error
    where
        Error: From<E>,
    {
        crate::Error::from(e).context(c).into()
    }

    pub fn tag(self, tags: impl IntoIterator<Item = crate::ErrorTag>) -> Self {
        self.context(ContextValue::Tags(tags.into_iter().collect()))
    }

    pub fn get_category(&self) -> Option<Category> {
        let mut out = None;
        for cat in self.iter_context().filter_map(|kind| match kind {
            ContextValue::Category(cat) => Some(*cat),
            _ => None,
        }) {
            // It's an infra error if it was ever marked as an infra error
            match cat {
                Category::Infra => return Some(cat),
                Category::User => out = Some(cat),
            }
        }
        out
    }

    /// Get all the tags that have been added to this error
    pub fn get_tags(&self) -> Vec<crate::ErrorTag> {
        let mut tags: Vec<_> = self
            .iter_context()
            .filter_map(|kind| match kind {
                ContextValue::Tags(tags) => Some(tags.iter().copied()),
                _ => None,
            })
            .flatten()
            .collect();
        tags.sort_unstable_by_key(|tag| tag.as_str_name());
        tags.dedup();
        tags
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    #[derive(Debug, thiserror::Error)]
    #[error("Test")]
    struct TestError;

    #[test]
    fn test_emitted_works() {
        let e: crate::Error = TestError.into();
        assert!(e.is_emitted().is_none());
        let e = e.mark_emitted(Arc::new(|_| Ok(())));
        assert!(e.is_emitted().is_some());
        let e: anyhow::Error = e.into();
        let e: crate::Error = e.context("context").into();
        assert!(e.is_emitted().is_some());
    }

    #[test]
    fn test_root_id() {
        let e1: crate::Error = TestError.into();
        let e1x = e1.clone().context("context");
        let e1y = e1.clone().context("context2");

        let e2: crate::Error = TestError.into();

        assert_eq!(e1.root_id(), e1x.root_id());
        assert_eq!(e1.root_id(), e1y.root_id());
        assert_eq!(e1x.root_id(), e1y.root_id());

        assert_ne!(e1.root_id(), e2.root_id());
    }
}
