# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is dual-licensed under either the MIT license found in the
# LICENSE-MIT file in the root directory of this source tree or the Apache
# License, Version 2.0 found in the LICENSE-APACHE file in the root directory
# of this source tree. You may select, at your option, one of the
# above-listed licenses.

load("@prelude//rust:linkable_symbol.bzl", prelude_rust_linkable_symbol = "rust_linkable_symbol")
load("@shim//:shims.bzl", _rust_library = "rust_library")

def rust_linkable_symbol(
        visibility = ["PUBLIC"],
        **kwargs):
    prelude_rust_linkable_symbol(
        visibility = visibility,
        rust_library_macro = _rust_library,
        **kwargs
    )
