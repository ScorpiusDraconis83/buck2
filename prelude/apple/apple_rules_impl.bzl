# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is dual-licensed under either the MIT license found in the
# LICENSE-MIT file in the root directory of this source tree or the Apache
# License, Version 2.0 found in the LICENSE-APACHE file in the root directory
# of this source tree. You may select, at your option, one of the
# above-listed licenses.

load("@prelude//:attrs_validators.bzl", "validation_common")
load(
    "@prelude//:validation_deps.bzl",
    "VALIDATION_DEPS_ATTR_NAME",
    "VALIDATION_DEPS_ATTR_TYPE",
)
load("@prelude//apple:apple_common.bzl", "apple_common")
# @oss-disable[end= ]: load("@prelude//apple/meta_only:meta_only_rules.bzl", "meta_only_apple_rule_attributes", "meta_only_apple_rule_implementations")
load("@prelude//apple/swift:swift_incremental_support.bzl", "SwiftCompilationMode")
load("@prelude//apple/swift:swift_toolchain_types.bzl", "SwiftObjectFormat")
load("@prelude//cxx:headers.bzl", "CPrecompiledHeaderInfo", "HeaderMode")
load("@prelude//cxx:link_groups_types.bzl", "LINK_GROUP_MAP_ATTR")
load("@prelude//linking:execution_preference.bzl", "link_execution_preference_attr")
load("@prelude//linking:link_info.bzl", "LinkOrdering")
load("@prelude//linking:types.bzl", "Linkage")
load(":apple_library.bzl", "AppleSharedLibraryMachOFileType")
load(
    ":apple_rules_impl_utility.bzl",
    "APPLE_ARCHIVE_OBJECTS_LOCALLY_OVERRIDE_ATTR_NAME",
    "apple_bundle_extra_attrs",
    "apple_dsymutil_attrs",
    "get_apple_toolchain_attr",
    "get_apple_xctoolchain_attr",
    "get_apple_xctoolchain_bundle_id_attr",
    "get_enable_library_evolution",
    "get_skip_swift_incremental_outputs_attrs",
    "get_swift_incremental_file_hashing_attrs",
)

implemented_rules = {
# @oss-disable[end= ]: } | meta_only_apple_rule_implementations()
} # @oss-enable

_APPLE_TOOLCHAIN_ATTR = get_apple_toolchain_attr()

def _apple_library_extra_attrs():
    attribs = {
        "dist_thin_lto_codegen_flags": attrs.list(attrs.arg(), default = []),
        "enable_distributed_thinlto": attrs.bool(default = select({
            "DEFAULT": False,
            "config//build_mode/constraints:distributed-thin-lto-enabled": True,
        })),
        "enable_library_evolution": attrs.option(attrs.bool(), default = None),
        "header_mode": attrs.option(attrs.enum(HeaderMode.values()), default = None),
        "link_execution_preference": link_execution_preference_attr(),
        "link_group_map": LINK_GROUP_MAP_ATTR,
        "link_ordering": attrs.option(attrs.enum(LinkOrdering.values()), default = None),
        "precompiled_header": attrs.option(attrs.dep(providers = [CPrecompiledHeaderInfo]), default = None),
        "preferred_linkage": attrs.enum(Linkage.values(), default = "any"),
        "propagated_target_sdk_version": attrs.option(attrs.string(), default = None),
        # Mach-O file type for binary when the target is built as a shared library.
        "shared_library_macho_file_type": attrs.enum(AppleSharedLibraryMachOFileType.values(), default = "dylib"),
        "stripped": attrs.option(attrs.bool(), default = None),
        "supports_header_symlink_subtarget": attrs.bool(default = False),
        "supports_shlib_interfaces": attrs.bool(default = True),
        "swift_compilation_mode": attrs.enum(SwiftCompilationMode.values(), default = "wmo"),
        "swift_package_name": attrs.option(attrs.string(), default = None),
        "use_archive": attrs.option(attrs.bool(), default = None),
        "_apple_toolchain": _APPLE_TOOLCHAIN_ATTR,
        "_apple_xctoolchain": get_apple_xctoolchain_attr(),
        "_apple_xctoolchain_bundle_id": get_apple_xctoolchain_bundle_id_attr(),
        "_enable_library_evolution": get_enable_library_evolution(),
        "_stripped_default": attrs.bool(default = False),
        "_swift_enable_testing": attrs.bool(default = select({
            "DEFAULT": False,
            "config//features/apple:swift_enable_testing_enabled": True,
        })),
        APPLE_ARCHIVE_OBJECTS_LOCALLY_OVERRIDE_ATTR_NAME: attrs.option(attrs.bool(), default = None),
        VALIDATION_DEPS_ATTR_NAME: VALIDATION_DEPS_ATTR_TYPE,
    } | validation_common.attrs_validators_arg()
    attribs.update(apple_common.apple_tools_arg())
    attribs.update(apple_dsymutil_attrs())
    attribs.update(get_swift_incremental_file_hashing_attrs())
    attribs.update(get_skip_swift_incremental_outputs_attrs())
    return attribs

extra_attributes = {
    "apple_bundle": apple_bundle_extra_attrs(),
    "apple_library": _apple_library_extra_attrs(),
    "apple_toolchain": {
        # The Buck v1 attribute specs defines those as `attrs.source()` but
        # we want to properly handle any runnable tools that might have
        # addition runtime requirements.
        "actool": attrs.exec_dep(providers = [RunInfo]),
        "codesign": attrs.exec_dep(providers = [RunInfo]),
        "codesign_allocate": attrs.exec_dep(providers = [RunInfo]),
        "codesign_identities_command": attrs.option(attrs.exec_dep(providers = [RunInfo]), default = None),
        # Controls invocations of `ibtool`, `actool` `mapc` and `momc`
        "compile_resources_locally": attrs.bool(default = False),
        "copy_scene_kit_assets": attrs.exec_dep(providers = [RunInfo]),
        "cxx_toolchain": attrs.toolchain_dep(),
        "dsymutil": attrs.exec_dep(providers = [RunInfo]),
        "dwarfdump": attrs.option(attrs.exec_dep(providers = [RunInfo]), default = None),
        "extra_linker_outputs": attrs.set(attrs.string(), default = []),
        "ibtool": attrs.exec_dep(providers = [RunInfo]),
        "installer": attrs.default_only(attrs.label(default = "fbsource//xplat/buck2/platform/apple/installer/src/com/facebook/buck/apple/installer:apple_installer")),
        "libtool": attrs.exec_dep(providers = [RunInfo]),
        "lipo": attrs.exec_dep(providers = [RunInfo]),
        "mapc": attrs.option(attrs.exec_dep(providers = [RunInfo]), default = None),
        "merge_index_store": attrs.default_only(attrs.dep(providers = [RunInfo], default = "prelude//apple/tools/index:merge_index_store")),
        "momc": attrs.exec_dep(providers = [RunInfo]),
        "objdump": attrs.option(attrs.exec_dep(providers = [RunInfo]), default = None),
        # A placeholder tool that can be used to set up toolchain constraints.
        # Useful when fat and thin toolchahins share the same underlying tools via `command_alias()`,
        # which requires setting up separate platform-specific aliases with the correct constraints.
        "placeholder_tool": attrs.option(attrs.exec_dep(providers = [RunInfo]), default = None),
        "platform_path": attrs.option(attrs.source(), default = None),  # Mark as optional until we remove `_internal_platform_path`
        # Defines whether the Xcode project generator needs to check
        # that the selected Xcode version matches the one defined
        # by the `xcode_build_version` fields.
        "requires_xcode_version_match": attrs.bool(default = False),
        "sdk_path": attrs.option(attrs.source(), default = None),  # Mark as optional until we remove `_internal_sdk_path`
        "swift_toolchain": attrs.option(attrs.toolchain_dep(), default = None),
        "version": attrs.option(attrs.string(), default = None),
        "xcode_build_version": attrs.option(attrs.string(), default = None),
        "xcode_version": attrs.string(),
        "xctest": attrs.exec_dep(providers = [RunInfo]),
        # TODO(T111858757): Mirror of `platform_path` but treated as a string. It allows us to
        #                   pass abs paths during development and using the currently selected Xcode.
        "_internal_platform_path": attrs.option(attrs.string(), default = None),
        # TODO(T111858757): Mirror of `sdk_path` but treated as a string. It allows us to
        #                   pass abs paths during development and using the currently selected Xcode.
        "_internal_sdk_path": attrs.option(attrs.string(), default = None),
    },
    "swift_toolchain": {
        "architecture": attrs.string(),
        "make_swift_comp_db": attrs.default_only(attrs.dep(providers = [RunInfo], default = "prelude//apple/tools:make_swift_comp_db")),
        "make_swift_interface": attrs.default_only(attrs.dep(providers = [RunInfo], default = "prelude//apple/tools:make_swift_interface")),
        "object_format": attrs.enum(SwiftObjectFormat.values(), default = "object"),
        # A placeholder tool that can be used to set up toolchain constraints.
        # Useful when fat and thin toolchahins share the same underlying tools via `command_alias()`,
        # which requires setting up separate platform-specific aliases with the correct constraints.
        "placeholder_tool": attrs.option(attrs.exec_dep(providers = [RunInfo]), default = None),
        "platform_path": attrs.option(attrs.source(), default = None),
        "provide_swift_debug_info": attrs.bool(default = True),
        "sdk_module_path_prefixes": attrs.dict(key = attrs.string(), value = attrs.source(), default = {}),
        "sdk_modules": attrs.list(attrs.exec_dep(), default = []),  # A list or a root target that represent a graph of sdk modules (e.g Frameworks)
        "sdk_path": attrs.option(attrs.source(), default = None),  # Mark as optional until we remove `_internal_sdk_path`
        "serialized_diags_to_json": attrs.option(attrs.exec_dep(providers = [RunInfo]), default = None),
        "swift_ide_test_tool": attrs.option(attrs.exec_dep(providers = [RunInfo]), default = None),
        "swift_stdlib_tool": attrs.exec_dep(providers = [RunInfo]),
        "swiftc": attrs.exec_dep(providers = [RunInfo]),
        "use_depsfiles": attrs.bool(default = False),
        # TODO(T111858757): Mirror of `sdk_path` but treated as a string. It allows us to
        #                   pass abs paths during development and using the currently selected Xcode.
        "_internal_sdk_path": attrs.option(attrs.string(), default = None),
        "_swiftc_wrapper": attrs.exec_dep(providers = [RunInfo], default = "prelude//apple/tools:swift_exec"),
    },
# @oss-disable[end= ]: } | meta_only_apple_rule_attributes()
} # @oss-enable
