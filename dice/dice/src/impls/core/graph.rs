/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

//! The versioned dice graph of dependencies
#[allow(unused)]
pub(crate) mod introspection;
mod lazy_deps;
pub(crate) mod nodes;
pub(crate) mod storage;
pub(crate) mod types;
