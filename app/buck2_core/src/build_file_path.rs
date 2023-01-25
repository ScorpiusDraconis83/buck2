/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use allocative::Allocative;
use dupe::Dupe;

use crate::bzl::ModuleID;
use crate::cells::build_file_cell::BuildFileCell;
use crate::cells::cell_path::CellPath;
use crate::cells::name::CellName;
use crate::cells::paths::CellRelativePathBuf;
use crate::fs::paths::file_name::FileName;
use crate::fs::paths::file_name::FileNameBuf;
use crate::package::PackageLabel;

/// Path of a build file (e.g. `BUCK`) only. (`bzl` files are not included).
#[derive(Clone, Hash, Eq, PartialEq, Debug, derive_more::Display, Allocative)]
#[display(fmt = "{}", id)]
pub struct BuildFilePath {
    /// The package of this build file
    package: PackageLabel,
    /// The build file's filename (which can be configured). i.e. `BUCK`
    filename: FileNameBuf,
    /// A ModuleID for the import.
    id: ModuleID,
}

impl BuildFilePath {
    pub fn new(package: PackageLabel, filename: FileNameBuf) -> Self {
        let id = ModuleID(format!("{}:{}", package, filename));
        Self {
            package,
            filename,
            id,
        }
    }

    pub fn unchecked_new(cell: &str, package: &str, filename: &str) -> Self {
        let package = PackageLabel::new(
            CellName::unchecked_new(cell),
            &CellRelativePathBuf::unchecked_new(package.to_owned()),
        );
        let filename = FileNameBuf::unchecked_new(filename);
        Self::new(package, filename)
    }

    pub fn cell(&self) -> CellName {
        self.package.cell_name()
    }

    pub fn package(&self) -> PackageLabel {
        self.package.dupe()
    }

    pub fn path(&self) -> CellPath {
        self.package.as_cell_path().join(&self.filename)
    }

    pub fn build_file_cell(&self) -> BuildFileCell {
        BuildFileCell::new(self.cell())
    }

    pub fn filename(&self) -> &FileName {
        &self.filename
    }

    pub fn id(&self) -> &ModuleID {
        &self.id
    }
}
