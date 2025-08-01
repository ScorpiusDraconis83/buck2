# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is dual-licensed under either the MIT license found in the
# LICENSE-MIT file in the root directory of this source tree or the Apache
# License, Version 2.0 found in the LICENSE-APACHE file in the root directory
# of this source tree. You may select, at your option, one of the
# above-listed licenses.

load("@prelude//apple/user:apple_ipa_package.bzl", "make_apple_ipa_package_target")
load("@prelude//utils:selects.bzl", "selects")
load(":apple_bundle_config.bzl", "apple_bundle_config")
load(":apple_dsym_config.bzl", "apple_dsym_config")
load(":apple_info_plist_substitutions_parsing.bzl", "parse_codesign_entitlements")
load(":apple_package_config.bzl", "apple_package_config")
load(":apple_resource_bundle.bzl", "make_resource_bundle_rule")
load(
    ":apple_rules_impl_utility.bzl",
    "APPLE_ARCHIVE_OBJECTS_LOCALLY_OVERRIDE_ATTR_NAME",
)

AppleBuckConfigAttributeOverride = record(
    name = field(str),
    section = field(str, default = "apple"),
    key = field(str),
    positive_values = field([list[str], list[bool]], default = ["True", "true"]),
    value_if_true = field([str, bool, None], default = True),
    value_if_false = field([str, bool, None], default = False),
    skip_if_false = field(bool, default = False),
)

APPLE_LINK_LIBRARIES_LOCALLY_OVERRIDE = AppleBuckConfigAttributeOverride(
    name = "link_execution_preference",
    key = "link_libraries_locally_override",
    value_if_true = "local",
    skip_if_false = True,
)

APPLE_LINK_LIBRARIES_REMOTELY_OVERRIDE = AppleBuckConfigAttributeOverride(
    name = "link_execution_preference",
    key = "link_libraries_remotely_override",
    value_if_true = "remote",
    skip_if_false = True,
)

APPLE_STRIPPED_DEFAULT = AppleBuckConfigAttributeOverride(
    name = "_stripped_default",
    key = "stripped_default",
    skip_if_false = True,
)

_APPLE_LIBRARY_LOCAL_EXECUTION_OVERRIDES = [
    APPLE_LINK_LIBRARIES_LOCALLY_OVERRIDE,
    APPLE_LINK_LIBRARIES_REMOTELY_OVERRIDE,
    AppleBuckConfigAttributeOverride(name = APPLE_ARCHIVE_OBJECTS_LOCALLY_OVERRIDE_ATTR_NAME, key = "archive_objects_locally_override"),
]

# If both configs are set the last one wins
_APPLE_BINARY_EXECUTION_OVERRIDES = [
    AppleBuckConfigAttributeOverride(
        name = "link_execution_preference",
        key = "link_binaries_locally_override",
        value_if_true = "local",
        skip_if_false = True,
    ),
    AppleBuckConfigAttributeOverride(
        name = "link_execution_preference",
        key = "link_binaries_remotely_override",
        value_if_true = "remote",
        skip_if_false = True,
    ),
]

_APPLE_TEST_LOCAL_EXECUTION_OVERRIDES = [
    APPLE_LINK_LIBRARIES_LOCALLY_OVERRIDE,
    APPLE_LINK_LIBRARIES_REMOTELY_OVERRIDE,
]

def apple_macro_layer_set_bool_override_attrs_from_config(overrides: list[AppleBuckConfigAttributeOverride]) -> dict[str, Select]:
    attribs = {}
    for override in overrides:
        config_value = read_root_config(override.section, override.key, None)
        if config_value != None:
            config_is_true = config_value in override.positive_values
            if not config_is_true and override.skip_if_false:
                continue
            attribs[override.name] = select({
                "DEFAULT": override.value_if_true if config_is_true else override.value_if_false,
                # Do not set attribute value for host tools
                "ovr_config//platform/execution/constraints:execution-platform-transitioned": None,
            })
    return attribs

def apple_test_macro_impl(apple_test_rule, apple_resource_bundle_rule, **kwargs):
    _transform_apple_minimum_os_version_to_propagated_target_sdk_version(kwargs)
    kwargs.update(apple_bundle_config())
    kwargs.update(apple_dsym_config())
    kwargs.update(apple_macro_layer_set_bool_override_attrs_from_config(_APPLE_TEST_LOCAL_EXECUTION_OVERRIDES))

    # `extension` is used both by `apple_test` and `apple_resource_bundle`, so provide default here
    kwargs["extension"] = kwargs.pop("extension", "xctest")

    apple_test_rule(
        _resource_bundle = make_resource_bundle_rule(apple_resource_bundle_rule, **kwargs),
        **kwargs
    )

def apple_xcuitest_macro_impl(apple_xcuitest_rule, **kwargs):
    kwargs.update(apple_bundle_config())
    apple_xcuitest_rule(
        **kwargs
    )

def apple_bundle_macro_impl(apple_bundle_rule, apple_resource_bundle_rule, **kwargs):
    _transform_apple_minimum_os_version_to_propagated_target_sdk_version(kwargs)
    info_plist_substitutions = kwargs.get("info_plist_substitutions")
    kwargs.update(apple_bundle_config())
    kwargs.update(apple_dsym_config())
    codesign_entitlements = selects.apply(info_plist_substitutions, parse_codesign_entitlements)

    apple_bundle_rule(
        _codesign_entitlements = codesign_entitlements,
        _resource_bundle = make_resource_bundle_rule(apple_resource_bundle_rule, **kwargs),
        **kwargs
    )

def apple_library_macro_impl(apple_library_rule = None, **kwargs):
    kwargs.update(apple_dsym_config())
    kwargs.update(apple_macro_layer_set_bool_override_attrs_from_config(_APPLE_LIBRARY_LOCAL_EXECUTION_OVERRIDES))
    kwargs.update(apple_macro_layer_set_bool_override_attrs_from_config([APPLE_STRIPPED_DEFAULT]))
    apple_library_rule(**kwargs)

def prebuilt_apple_framework_macro_impl(prebuilt_apple_framework_rule = None, **kwargs):
    kwargs.update(apple_macro_layer_set_bool_override_attrs_from_config([APPLE_STRIPPED_DEFAULT]))
    prebuilt_apple_framework_rule(**kwargs)

def apple_binary_macro_impl(apple_binary_rule = None, apple_universal_executable = None, **kwargs):
    _transform_apple_minimum_os_version_to_propagated_target_sdk_version(kwargs)
    dsym_args = apple_dsym_config()
    kwargs.update(dsym_args)
    kwargs.update(apple_macro_layer_set_bool_override_attrs_from_config(_APPLE_BINARY_EXECUTION_OVERRIDES))
    kwargs.update(apple_macro_layer_set_bool_override_attrs_from_config([APPLE_STRIPPED_DEFAULT]))

    original_binary_name = kwargs.pop("name")

    if kwargs.pop("supports_universal", False):
        binary_name = original_binary_name + "ThinBinary"
        apple_universal_executable(
            name = original_binary_name,
            executable = ":" + binary_name,
            executable_name = original_binary_name,
            labels = kwargs.get("labels"),
            visibility = kwargs.get("visibility"),
            default_target_platform = kwargs.get("default_target_platform"),
            **dsym_args
        )
    else:
        binary_name = original_binary_name

    apple_binary_rule(name = binary_name, **kwargs)

def apple_package_macro_impl(apple_package_rule = None, apple_ipa_package_rule = None, **kwargs):
    kwargs.update(apple_package_config())
    apple_package_rule(
        _ipa_package = make_apple_ipa_package_target(apple_ipa_package_rule, **kwargs),
        **kwargs
    )

def apple_universal_executable_macro_impl(apple_universal_executable_rule = None, **kwargs):
    kwargs.update(apple_dsym_config())
    apple_universal_executable_rule(
        **kwargs
    )

# TODO: T197775809 Rename `target_sdk_version` to `minimum_os_version`
def _move_attribute_value(new_name, old_name, kwargs):
    if new_name in kwargs:
        if old_name in kwargs:
            fail("Cannot specify both `{}` and `{}`".format(old_name, new_name))
        kwargs[old_name] = kwargs.pop(new_name)

def _transform_apple_minimum_os_version_to_propagated_target_sdk_version(kwargs):
    # During the transition periods, allow either `minimum_os_version` or
    # `propagated_target_sdk_version` to be used on targets.
    # Under the hood, `propagated_target_sdk_version` is the actual field on rules.
    #
    # At the end of the transition, `propagated_target_sdk_version` rule fields be renamed
    # to `minimum_os_version` and this transformer removed.
    _move_attribute_value("minimum_os_version", "propagated_target_sdk_version", kwargs)
