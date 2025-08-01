# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is dual-licensed under either the MIT license found in the
# LICENSE-MIT file in the root directory of this source tree or the Apache
# License, Version 2.0 found in the LICENSE-APACHE file in the root directory
# of this source tree. You may select, at your option, one of the
# above-listed licenses.

load("@fbsource//tools/build_defs:platform_defs.bzl", "APPLE", "CXX", "MACOSX", "WINDOWS")
load("@shim//build_defs:python_library.bzl", "python_library")

CYTHON_DEFAULT_PLATFORMS = (CXX, WINDOWS, APPLE)
CYTHON_DEFAULT_APPLE_SDKS = (MACOSX,)

def cython_library(name, visibility = ["PUBLIC"], **_):
    python_library(name = name, visibility = visibility)
