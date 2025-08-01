/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

//! Command line arguments definition for bxl functions

use std::collections::HashSet;
use std::fmt::Formatter;
use std::hash::Hash;
use std::sync::Arc;

use allocative::Allocative;
use buck2_build_api::configure_targets::load_compatible_patterns_with_modifiers;
use buck2_core::global_cfg_options::GlobalCfgOptions;
use buck2_core::pattern::pattern::ModifiersError;
use buck2_core::pattern::pattern::ParsedPattern;
use buck2_core::pattern::pattern::ParsedPatternWithModifiers;
use buck2_core::pattern::pattern::lex_target_pattern;
use buck2_core::pattern::pattern_type::ProvidersPatternExtra;
use buck2_core::pattern::pattern_type::TargetPatternExtra;
use buck2_core::provider::label::ProvidersLabel;
use buck2_core::target::configured_target_label::ConfiguredTargetLabel;
use buck2_core::target::label::label::TargetLabel;
use buck2_error::BuckErrorContext;
use buck2_error::conversion::from_any_with_tag;
use buck2_interpreter::types::configured_providers_label::StarlarkProvidersLabel;
use buck2_interpreter::types::target_label::StarlarkConfiguredTargetLabel;
use buck2_interpreter::types::target_label::StarlarkTargetLabel;
use buck2_node::load_patterns::MissingTargetBehavior;
use buck2_node::load_patterns::load_patterns;
use buck2_node::target_calculation::ConfiguredTargetCalculation;
use clap::ArgAction;
use derive_more::Display;
use dupe::Dupe;
use futures::FutureExt;
use futures::future::BoxFuture;
use gazebo::variants::VariantName;
use itertools::Itertools;
use num_bigint::BigInt;
use serde::Serialize;
use starlark::any::ProvidesStaticType;
use starlark::environment::GlobalsBuilder;
use starlark::environment::Methods;
use starlark::environment::MethodsBuilder;
use starlark::environment::MethodsStatic;
use starlark::starlark_module;
use starlark::starlark_simple_value;
use starlark::values::Heap;
use starlark::values::NoSerialize;
use starlark::values::StarlarkValue;
use starlark::values::UnpackValue;
use starlark::values::Value;
use starlark::values::ValueError;
use starlark::values::ValueLike;
use starlark::values::dict::Dict;
use starlark::values::float::StarlarkFloat;
use starlark::values::list::AllocList;
use starlark::values::list::ListRef;
use starlark::values::list_or_tuple::UnpackListOrTuple;
use starlark::values::none::NoneType;
use starlark::values::starlark_value;
use starlark_map::ordered_map::OrderedMap;
use starlark_map::small_map::SmallMap;

use crate::bxl::eval::CliResolutionCtx;

/// Defines the cli args for the bxl function
#[derive(Clone, Debug, Display, ProvidesStaticType, NoSerialize, Allocative)]
#[display("{:?}", self)]
pub(crate) struct CliArgs {
    /// The default value. If None, the value is not optional and must be provided by the user
    pub(crate) default: Option<Arc<CliArgValue>>,
    /// Documentation for what the attribute actually means
    doc: String,
    /// The coercer to take this parameter's value from Starlark value -> an
    /// internal representation
    coercer: CliArgType,
    /// The shorthand representation of the CLI arg
    #[allocative(skip)]
    pub(crate) short: Option<char>,
}

starlark_simple_value!(CliArgs);

/// Type of the bxl.CliArgs object returned by methods under [`cli_args`](../cli_args) namespace, e. g. `cli_args.string()`.
#[starlark_module]
fn starlark_attribute_methods(builder: &mut MethodsBuilder) {}

#[starlark_value(type = "bxl.CliArgs")]
impl<'v> StarlarkValue<'v> for CliArgs {
    // Used to add type documentation to the generated documentation
    fn get_methods() -> Option<&'static Methods> {
        static RES: MethodsStatic = MethodsStatic::new();
        RES.methods(starlark_attribute_methods)
    }
}

impl CliArgs {
    /// Helper to create an attribute from attrs.foo functions
    fn new<'v>(
        default: Option<Value<'v>>,
        doc: &str,
        coercer: CliArgType,
        short: Option<Value<'v>>,
    ) -> buck2_error::Result<Self> {
        let default = match default {
            None => None,
            Some(x) => Some(Arc::new(coercer.coerce_value(x).map_err(|_| {
                starlark::Error::from(ValueError::IncorrectParameterType)
            })?)),
        };

        let short = match short {
            None => None,
            Some(s) => match s.unpack_str() {
                Some(s) => {
                    if s.len() != 1 {
                        return Err(
                            starlark::Error::from(ValueError::IncorrectParameterType).into()
                        );
                    }
                    Some(s.chars().next().unwrap())
                }
                None => {
                    return Err(starlark::Error::from(ValueError::IncorrectParameterType).into());
                }
            },
        };

        Ok(Self {
            default,
            doc: doc.to_owned(),
            coercer,
            short,
        })
    }

    pub(crate) fn to_clap<'a>(&'a self, arg: clap::Arg) -> clap::Arg {
        let mut arg = self.coercer.to_clap(arg.help(self.doc.clone()));
        if let Some(short) = self.short {
            arg = arg.short(short);
        }

        if self.default.is_some() {
            arg = arg.required(false);
        }

        arg
    }

    pub(crate) async fn parse_clap<'a>(
        &self,
        clap: ArgAccessor<'a>,
        ctx: &CliResolutionCtx<'a>,
    ) -> buck2_error::Result<CliArgValue> {
        Ok(match self.coercer.parse_clap(clap, ctx).await? {
            None => (**self.default.as_ref().ok_or(CliArgError::MissingCliArg)?).clone(),
            Some(v) => v,
        })
    }
}

// Wrapper around `serde_json::Value`s, making sure that we keep the values ordered when working with
// `serde_json::Object`s so that hashing is deterministic for BXL keys.
#[derive(
    derive_more::Display,
    Debug,
    Clone,
    Eq,
    PartialEq,
    Hash,
    Ord,
    PartialOrd,
    Allocative,
    Serialize,
    strong_hash::StrongHash
)]
pub(crate) enum JsonCliArgValueData {
    None,
    Bool(bool),
    // Preserve BigInts and floats as strings for eq/hash for BXL key
    Float(String),
    Int(String),
    String(String),
    #[display("{}", _0.iter().map(|v| v.to_string()).join(","))]
    List(Vec<JsonCliArgValueData>),
    #[display(
        "{}",
        _0.iter().map(|(k, v)| format!("(k={k},v={v})")).join(",")
    )]
    Object(OrderedMap<String, JsonCliArgValueData>),
}

impl JsonCliArgValueData {
    fn from_serde_value(val: &serde_json::Value) -> Self {
        match val {
            serde_json::Value::Null => JsonCliArgValueData::None,
            serde_json::Value::Bool(x) => JsonCliArgValueData::Bool(*x),
            // serde_json::Number can be a u64, i64, or f64. Let's try i64 first, and if that doesn't
            // work then we will default to f64.
            serde_json::Value::Number(x) => {
                if x.is_i64() {
                    JsonCliArgValueData::Int(x.as_i64().unwrap().to_string())
                } else {
                    JsonCliArgValueData::Float(x.as_f64().unwrap().to_string())
                }
            }
            serde_json::Value::String(x) => JsonCliArgValueData::String(x.clone()),
            serde_json::Value::Array(xs) => {
                let mut vec = Vec::with_capacity(xs.len());
                for x in xs {
                    vec.push(Self::from_serde_value(x))
                }
                JsonCliArgValueData::List(vec)
            }
            serde_json::Value::Object(mp) => {
                let mut res = OrderedMap::new();
                for (k, v) in mp.into_iter() {
                    res.insert(k.clone(), Self::from_serde_value(v));
                }
                JsonCliArgValueData::Object(res)
            }
        }
    }

    pub(crate) fn as_starlark<'v>(&self, heap: &'v Heap) -> Value<'v> {
        match self {
            Self::Bool(b) => Value::new_bool(*b),
            // Verified when we constructed these from the `serde_json::Value`s
            Self::Float(i) => heap.alloc(i.parse::<f64>().expect("already verified")),
            Self::Int(i) => heap.alloc(BigInt::from(i.parse::<i64>().expect("already verified"))),
            Self::String(s) => heap.alloc(s),
            Self::List(l) => heap.alloc(AllocList(l.iter().map(|v| v.as_starlark(heap)))),
            Self::None => Value::new_none(),
            Self::Object(mp) => {
                let mut res = SmallMap::with_capacity(mp.len());
                for (k, v) in mp.into_iter() {
                    res.insert_hashed(
                        heap.alloc_str(k).to_value().get_hashed().unwrap(),
                        v.as_starlark(heap),
                    );
                }
                heap.alloc(Dict::new(res))
            }
        }
    }
}

#[derive(
    Debug,
    derive_more::Display,
    PartialEq,
    Eq,
    Clone,
    Hash,
    Ord,
    PartialOrd,
    Allocative,
    strong_hash::StrongHash
)]
pub(crate) enum CliArgValue {
    Bool(bool),
    Int(BigInt),
    // store this as a string here for eq, hash since generally this comes from cmdline.
    // Note that this means `3.0` and `3.00` would not be equal. The string should already have
    // been verified to be a f64.
    Float(String),
    String(String),
    // Type of list elements is used to verify that concatenation is valid.
    // That only can be checked after configuration took place,
    // so pass the type info together with values to be used later.
    #[display("{}", _0.iter().map(|v| v.to_string()).join(","))]
    List(Vec<CliArgValue>),
    None,
    TargetLabel(TargetLabel),
    ConfiguredTargetLabel(ConfiguredTargetLabel),
    ProvidersLabel(ProvidersLabel),
    // For json CLI arg, we do not allow defaults, and we only allow primitives that can be
    // deserialized into a `serde_json::Value`.
    Json(JsonCliArgValueData),
}

impl CliArgValue {
    pub(crate) fn as_starlark<'v>(&self, heap: &'v Heap) -> Value<'v> {
        match self {
            CliArgValue::Bool(b) => Value::new_bool(*b),
            CliArgValue::Int(i) => heap.alloc(i.clone()),
            CliArgValue::Float(f) => heap.alloc(f.parse::<f64>().expect("already verified")),
            CliArgValue::String(s) => heap.alloc(s),
            CliArgValue::List(l) => heap.alloc(AllocList(l.iter().map(|v| v.as_starlark(heap)))),
            CliArgValue::None => Value::new_none(),
            CliArgValue::TargetLabel(t) => heap.alloc(StarlarkTargetLabel::new(t.dupe())),
            CliArgValue::ConfiguredTargetLabel(c) => {
                heap.alloc(StarlarkConfiguredTargetLabel::new(c.dupe()))
            }
            CliArgValue::ProvidersLabel(p) => heap.alloc(StarlarkProvidersLabel::new(p.clone())),
            CliArgValue::Json(j) => heap.alloc(j.as_starlark(heap)),
        }
    }
}

#[derive(Debug, VariantName, Clone, Dupe, Allocative)]
pub(crate) enum CliArgType {
    Bool,
    Int,
    Float,
    String,
    Enumeration(Arc<HashSet<String>>),
    List(Arc<CliArgType>),
    Option(Arc<CliArgType>),
    TargetLabel,
    TargetExpr,
    ConfiguredTargetLabel,
    ConfiguredTargetExpr,
    SubTarget,
    SubTargetExpr,
    Json,
}

impl Display for CliArgType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            CliArgType::Enumeration(t) => {
                write!(
                    f,
                    "{}(variants={})",
                    self.variant_name(),
                    t.iter().join(",")
                )
            }
            CliArgType::List(t) => {
                write!(f, "{}({})", self.variant_name(), t)
            }
            CliArgType::Option(t) => {
                write!(f, "{}({})", self.variant_name(), t)
            }
            x => write!(f, "{}", x.variant_name()),
        }
    }
}

impl CliArgType {
    fn bool() -> Self {
        CliArgType::Bool
    }

    fn int() -> Self {
        CliArgType::Int
    }

    fn float() -> Self {
        CliArgType::Float
    }

    fn string() -> Self {
        CliArgType::String
    }

    fn list(t: CliArgType) -> Self {
        CliArgType::List(Arc::new(t))
    }

    fn option(t: CliArgType) -> Self {
        CliArgType::Option(Arc::new(t))
    }

    fn target_label() -> Self {
        CliArgType::TargetLabel
    }

    fn target_expr() -> Self {
        CliArgType::TargetExpr
    }

    fn configured_target_label() -> Self {
        CliArgType::ConfiguredTargetLabel
    }

    fn configured_target_expr() -> Self {
        CliArgType::ConfiguredTargetExpr
    }

    fn sub_target() -> Self {
        CliArgType::SubTarget
    }

    fn sub_target_expr() -> Self {
        CliArgType::SubTargetExpr
    }

    fn enumeration(vs: HashSet<String>) -> Self {
        CliArgType::Enumeration(Arc::new(vs))
    }

    fn json() -> Self {
        CliArgType::Json
    }
}

#[derive(Debug, buck2_error::Error)]
#[buck2(input)]
pub(crate) enum CliArgError {
    #[error("Expected default value of type `{}`, but got `{}`", _0, _1)]
    DefaultValueTypeError(CliArgType, String),
    #[error("Missing cli arg from command line that isn't optional nor has any default values")]
    MissingCliArg,
    #[error("`{0}` is not a {1} label.")]
    NotALabel(String, &'static str),
    #[error("Defaults are not allowed for cli arg type `{0}`.")]
    NoDefaultsAllowed(CliArgType),
    #[error("Duplicate short args are not allowed: `{0}` was already used")]
    DuplicateShort(char),
    #[error("An argument can be kebab-case OR snake-case, not both: `{0}`")]
    DefinedBothKebabAndSnakeCase(String),
    #[error("Expecting json object. Got: `{0}`")]
    NotAJsonObject(String),
}

impl CliArgType {
    fn coerce_value<'v>(&self, value: Value<'v>) -> buck2_error::Result<CliArgValue> {
        Ok(match self {
            CliArgType::Bool => CliArgValue::Bool(value.unpack_bool().ok_or_else(|| {
                CliArgError::DefaultValueTypeError(self.dupe(), value.get_type().to_owned())
            })?),
            CliArgType::Int => CliArgValue::Int(BigInt::unpack_value(value)?.ok_or_else(|| {
                CliArgError::DefaultValueTypeError(self.dupe(), value.get_type().to_owned())
            })?),
            CliArgType::Float => CliArgValue::Float(
                StarlarkFloat::unpack_value(value)?
                    .ok_or_else(|| {
                        CliArgError::DefaultValueTypeError(self.dupe(), value.get_type().to_owned())
                    })?
                    .0
                    .to_string(),
            ),
            CliArgType::String => CliArgValue::String(
                value
                    .unpack_str()
                    .ok_or_else(|| {
                        CliArgError::DefaultValueTypeError(self.dupe(), value.get_type().to_owned())
                    })?
                    .to_owned(),
            ),
            CliArgType::Enumeration(vs) => {
                let v = value
                    .unpack_str()
                    .ok_or_else(|| {
                        CliArgError::DefaultValueTypeError(self.dupe(), value.get_type().to_owned())
                    })?
                    .to_owned();
                if vs.contains(&v) {
                    CliArgValue::String(v)
                } else {
                    return Err(CliArgError::DefaultValueTypeError(self.dupe(), v).into());
                }
            }
            CliArgType::List(inner) => {
                let list = ListRef::from_value(value).ok_or_else(|| {
                    CliArgError::DefaultValueTypeError(self.dupe(), value.get_type().to_owned())
                })?;
                let mut result = vec![];
                for item in list.iter() {
                    result.push(inner.coerce_value(item)?);
                }

                CliArgValue::List(result)
            }
            CliArgType::Option(inner) => {
                if value.is_none() {
                    CliArgValue::None
                } else {
                    inner.coerce_value(value)?
                }
            }
            CliArgType::TargetLabel => {
                let label = value.downcast_ref::<StarlarkTargetLabel>().ok_or_else(|| {
                    CliArgError::DefaultValueTypeError(self.dupe(), value.get_type().to_owned())
                })?;
                CliArgValue::TargetLabel(label.label().dupe())
            }
            CliArgType::ConfiguredTargetLabel => {
                let configured_label = value
                    .downcast_ref::<StarlarkConfiguredTargetLabel>()
                    .ok_or_else(|| {
                        CliArgError::DefaultValueTypeError(self.dupe(), value.get_type().to_owned())
                    })?;
                CliArgValue::ConfiguredTargetLabel(configured_label.label().dupe())
            }
            CliArgType::SubTarget => {
                let label = value
                    .downcast_ref::<StarlarkProvidersLabel>()
                    .ok_or_else(|| {
                        CliArgError::DefaultValueTypeError(self.dupe(), value.get_type().to_owned())
                    })?;
                CliArgValue::ProvidersLabel((*label.label()).clone())
            }
            CliArgType::TargetExpr => {
                return Err(CliArgError::NoDefaultsAllowed(CliArgType::TargetExpr).into());
            }
            CliArgType::ConfiguredTargetExpr => {
                return Err(
                    CliArgError::NoDefaultsAllowed(CliArgType::ConfiguredTargetExpr).into(),
                );
            }
            CliArgType::SubTargetExpr => {
                return Err(CliArgError::NoDefaultsAllowed(CliArgType::SubTargetExpr).into());
            }
            CliArgType::Json => {
                return Err(CliArgError::NoDefaultsAllowed(CliArgType::Json).into());
            }
        })
    }

    pub(crate) fn to_clap<'a>(&'a self, clap: clap::Arg) -> clap::Arg {
        match self {
            CliArgType::Bool => clap
                .num_args(1)
                .value_parser(|x: &str| x.parse::<bool>().map(|_| x.to_owned())),
            CliArgType::Int => clap
                .num_args(1)
                .value_parser(|x: &str| x.parse::<BigInt>().map(|_| x.to_owned())),
            CliArgType::Float => clap
                .num_args(1)
                .value_parser(|x: &str| x.parse::<f64>().map(|_| x.to_owned())),
            CliArgType::String => clap.num_args(1),
            CliArgType::Enumeration(variants) => clap
                .num_args(1)
                .value_parser(variants.iter().cloned().collect::<Vec<_>>()),
            CliArgType::List(inner) => inner.to_clap(clap).num_args(0..).action(ArgAction::Append),
            CliArgType::Option(inner) => inner.to_clap(clap).required(false),
            CliArgType::TargetLabel => clap.num_args(1).value_parser(|x: &str| {
                anyhow::Ok(
                    lex_target_pattern::<TargetPatternExtra>(x, false)
                        .and_then(|parsed| parsed.pattern.infer_target())
                        .map(|parsed| {
                            parsed
                                .target()
                                .buck_error_context(CliArgError::NotALabel(x.to_owned(), "target"))
                                .map(|_| ())
                        })
                        .map(|_| x.to_owned())?,
                )
            }),
            CliArgType::ConfiguredTargetLabel => clap.num_args(1).value_parser(|x: &str| {
                anyhow::Ok(
                    lex_target_pattern::<TargetPatternExtra>(x, false)
                        .and_then(|parsed| parsed.pattern.infer_target())
                        .map(|parsed| {
                            parsed
                                .target()
                                .buck_error_context(CliArgError::NotALabel(x.to_owned(), "target"))
                                .map(|_| ())
                        })
                        .map(|_| x.to_owned())?,
                )
            }),
            CliArgType::SubTarget => clap.num_args(1).value_parser(|x: &str| {
                anyhow::Ok(
                    lex_target_pattern::<ProvidersPatternExtra>(x, false)
                        .and_then(|parsed| parsed.pattern.infer_target())
                        .map(|parsed| {
                            parsed
                                .target()
                                .buck_error_context(CliArgError::NotALabel(x.to_owned(), "target"))
                                .map(|_| ())
                        })
                        .map(|_| x.to_owned())?,
                )
            }),
            CliArgType::TargetExpr => clap.num_args(1),
            CliArgType::ConfiguredTargetExpr => clap.num_args(1),
            CliArgType::SubTargetExpr => clap.num_args(1),
            CliArgType::Json => clap.num_args(1),
        }
    }

    pub(crate) fn parse_clap<'a>(
        &'a self,
        clap: ArgAccessor<'a>,
        ctx: &'a CliResolutionCtx<'a>,
    ) -> BoxFuture<'a, buck2_error::Result<Option<CliArgValue>>> {
        async move {
            Ok(match self {
                CliArgType::Bool => clap.value_of().map_or(Ok(None), |x| {
                    let r: buck2_error::Result<_> = try { CliArgValue::Bool(x.parse::<bool>()?) };
                    r.map(Some)
                })?,
                CliArgType::Int => clap.value_of().map_or(Ok(None), |x| {
                    let r: buck2_error::Result<_> = try {
                        CliArgValue::Int(
                            x.parse::<BigInt>()
                                .map_err(|e| from_any_with_tag(e, buck2_error::ErrorTag::Tier0))?,
                        )
                    };
                    r.map(Some)
                })?,
                CliArgType::Float => clap.value_of().map_or(Ok(None), |x| {
                    let r: buck2_error::Result<_> = try {
                        CliArgValue::Float({
                            x.parse::<f64>()?;
                            x.to_owned()
                        })
                    };
                    r.map(Some)
                })?,
                CliArgType::String => clap.value_of().map(|s| CliArgValue::String(s.to_owned())),
                CliArgType::Enumeration(_variants) => {
                    // the validators should have already made sure the strings are only of the variants
                    clap.value_of().map(|s| CliArgValue::String(s.to_owned()))
                }
                CliArgType::List(inner) => match clap.values_of() {
                    None => None,
                    Some(values) => Some(CliArgValue::List(
                        futures::future::join_all(values.map(|v| async move {
                            try {
                                inner
                                    .parse_clap(ArgAccessor::Literal(v), ctx)
                                    .await?
                                    .expect("shouldn't be empty when parsing list items")
                            }
                        }))
                        .await
                        .into_iter()
                        .collect::<buck2_error::Result<_>>()?,
                    )),
                },
                CliArgType::Option(inner) => Some(if clap.value_of().is_some() {
                    inner
                        .parse_clap(clap, ctx)
                        .await?
                        .expect("arg is verified to be present")
                } else {
                    CliArgValue::None
                }),
                CliArgType::TargetLabel => clap.value_of().map_or(Ok(None), |x| {
                    let r: buck2_error::Result<_> = try {
                        CliArgValue::TargetLabel(
                            ParsedPattern::<TargetPatternExtra>::parse_relaxed(
                                &ctx.target_alias_resolver,
                                ctx.relative_dir.as_cell_path(),
                                x,
                                &ctx.cell_resolver,
                                &ctx.cell_alias_resolver,
                            )?
                            .as_target_label(x)?,
                        )
                    };
                    r.map(Some)
                })?,
                CliArgType::ConfiguredTargetLabel => {
                    let x = clap.value_of().unwrap_or("");

                    let ParsedPatternWithModifiers {
                        parsed_pattern,
                        modifiers,
                    } = ParsedPatternWithModifiers::<TargetPatternExtra>::parse_relaxed(
                        &ctx.target_alias_resolver,
                        ctx.relative_dir.as_cell_path(),
                        x,
                        &ctx.cell_resolver,
                        &ctx.cell_alias_resolver,
                    )?;

                    let global_cfg_options = &ctx.global_cfg_options;
                    let local_cfg_options = match modifiers.as_slice() {
                        Some(modifiers) => {
                            if !ctx.global_cfg_options.cli_modifiers.is_empty() {
                                return Err(
                                    ModifiersError::PatternModifiersWithGlobalModifiers.into()
                                );
                            }
                            &GlobalCfgOptions {
                                target_platform: global_cfg_options.target_platform.dupe(),
                                cli_modifiers: modifiers.to_vec().into(),
                            }
                        }
                        None => global_cfg_options,
                    };

                    Some(CliArgValue::ConfiguredTargetLabel(
                        ctx.dice
                            .clone()
                            .get_configured_target(
                                &parsed_pattern.as_target_label(x)?,
                                local_cfg_options,
                            )
                            .await?,
                    ))
                }
                CliArgType::SubTarget => clap.value_of().map_or(Ok(None), |x| {
                    let r: buck2_error::Result<_> = try {
                        CliArgValue::ProvidersLabel(
                            ParsedPattern::<ProvidersPatternExtra>::parse_relaxed(
                                &ctx.target_alias_resolver,
                                ctx.relative_dir.as_cell_path(),
                                x,
                                &ctx.cell_resolver,
                                &ctx.cell_alias_resolver,
                            )?
                            .as_providers_label(x)?,
                        )
                    };
                    r.map(Some)
                })?,
                CliArgType::TargetExpr => {
                    let x = clap.value_of().unwrap_or("");
                    let pattern = ParsedPattern::<TargetPatternExtra>::parse_relaxed(
                        &ctx.target_alias_resolver,
                        ctx.relative_dir.as_cell_path(),
                        x,
                        &ctx.cell_resolver,
                        &ctx.cell_alias_resolver,
                    )?;
                    let loaded = load_patterns(
                        &mut ctx.dice.clone(),
                        vec![pattern],
                        MissingTargetBehavior::Fail,
                    )
                    .await?;
                    Some(CliArgValue::List(
                        loaded
                            .iter_loaded_targets()
                            .map_ok(|t| CliArgValue::TargetLabel(t.label().dupe()))
                            .collect::<buck2_error::Result<_>>()?,
                    ))
                }
                CliArgType::ConfiguredTargetExpr => {
                    let arg = clap.value_of().unwrap_or("");
                    let pattern_with_modifiers =
                        ParsedPatternWithModifiers::<TargetPatternExtra>::parse_relaxed(
                            &ctx.target_alias_resolver,
                            ctx.relative_dir.as_cell_path(),
                            arg,
                            &ctx.cell_resolver,
                            &ctx.cell_alias_resolver,
                        )?;
                    let loaded = load_compatible_patterns_with_modifiers(
                        &mut ctx.dice.clone(),
                        vec![pattern_with_modifiers],
                        &ctx.global_cfg_options,
                        MissingTargetBehavior::Fail,
                    )
                    .await?;
                    Some(CliArgValue::List(
                        loaded
                            .iter()
                            .map(|t| CliArgValue::ConfiguredTargetLabel(t.label().dupe()))
                            .collect(),
                    ))
                }
                CliArgType::SubTargetExpr => {
                    let x = clap.value_of().unwrap_or("");
                    let pattern = ParsedPattern::<ProvidersPatternExtra>::parse_relaxed(
                        &ctx.target_alias_resolver,
                        ctx.relative_dir.as_cell_path(),
                        x,
                        &ctx.cell_resolver,
                        &ctx.cell_alias_resolver,
                    )?;
                    let loaded = load_patterns(
                        &mut ctx.dice.clone(),
                        vec![pattern],
                        MissingTargetBehavior::Fail,
                    )
                    .await?;

                    Some(CliArgValue::List(
                        loaded
                            .into_iter()
                            .flat_map(|(pkg, result)| match result {
                                Ok(res) => res
                                    .keys()
                                    .map(|(target, pattern)| {
                                        Ok(CliArgValue::ProvidersLabel(
                                            pattern
                                                .to_owned()
                                                .into_providers_label(pkg.dupe(), target.as_ref()),
                                        ))
                                    })
                                    .collect::<Vec<_>>(),
                                Err(e) => vec![Err(e.dupe())],
                            })
                            .collect::<buck2_error::Result<Vec<_>>>()?,
                    ))
                }
                CliArgType::Json => match clap.value_of() {
                    None => None,
                    Some(value) => {
                        let json: serde_json::Value = serde_json::from_str(value)?;
                        let data = JsonCliArgValueData::from_serde_value(&json);
                        if let JsonCliArgValueData::Object(_) = data {
                            Some(CliArgValue::Json(data))
                        } else {
                            return Err(CliArgError::NotAJsonObject(json.to_string()).into());
                        }
                    }
                },
            })
        }
        .boxed()
    }
}

#[starlark_module]
pub(crate) fn cli_args_module(registry: &mut GlobalsBuilder) {
    /// Takes an arg from cli, and gets a string in bxl
    fn string<'v>(
        default: Option<Value<'v>>,
        #[starlark(default = "")] doc: &str,
        #[starlark(require = named)] short: Option<Value<'v>>,
    ) -> starlark::Result<CliArgs> {
        Ok(CliArgs::new(default, doc, CliArgType::string(), short)?)
    }

    /// Takes a list of args from cli, and gets a list of inner type in bxl.
    ///
    /// e.g. `cli.args.list(cli.args.string())` declares that the cli flag takes a list of args from cli, and gets a list of strings in bxl.
    fn list<'v>(
        #[starlark(require = pos)] inner: &CliArgs,
        default: Option<Value<'v>>,
        #[starlark(default = "")] doc: &str,
        #[starlark(require = named)] short: Option<Value<'v>>,
    ) -> starlark::Result<CliArgs> {
        let coercer = CliArgType::list(inner.coercer.dupe());
        Ok(CliArgs::new(default, doc, coercer, short)?)
    }

    /// Accepts "true" or "false" from cli, and get a `bool` in bxl. If not given, will get `false`
    fn bool<'v>(
        #[starlark(default = false)] default: Value<'v>,
        #[starlark(default = "")] doc: &str,
        #[starlark(require = named)] short: Option<Value<'v>>,
    ) -> starlark::Result<CliArgs> {
        Ok(CliArgs::new(Some(default), doc, CliArgType::bool(), short)?)
    }

    /// Takes an arg from cli, and gets a `int` in bxl
    fn int<'v>(
        default: Option<Value<'v>>,
        #[starlark(default = "")] doc: &str,
        #[starlark(require = named)] short: Option<Value<'v>>,
    ) -> starlark::Result<CliArgs> {
        Ok(CliArgs::new(default, doc, CliArgType::int(), short)?)
    }

    /// Takes an arg from cli, and gets a `float` in bxl
    fn float<'v>(
        default: Option<Value<'v>>,
        #[starlark(default = "")] doc: &str,
        #[starlark(require = named)] short: Option<Value<'v>>,
    ) -> starlark::Result<CliArgs> {
        Ok(CliArgs::new(default, doc, CliArgType::float(), short)?)
    }

    /// Takes a arg from cli, and gets an inner type in bxl. If not given, will get `None` in bxl.
    ///
    /// e.g. `cli.args.option(cli.args.int())` defines an optional int arg.
    fn option<'v>(
        inner: &CliArgs,
        #[starlark(default = "")] doc: &str,
        #[starlark(default = NoneType)] default: Value<'v>,
        #[starlark(require = named)] short: Option<Value<'v>>,
    ) -> starlark::Result<CliArgs> {
        let coercer = CliArgType::option(inner.coercer.dupe());
        Ok(CliArgs::new(Some(default), doc, coercer, short)?)
    }

    /// Takes a set of defined values in `variants`, and gets a `str` in bxl.
    ///
    /// e.g. `cli.args.enumeration(["foo", "bar"])` defines an arg that only accepts "foo" or "bar".
    fn r#enum<'v>(
        #[starlark(require = pos)] variants: UnpackListOrTuple<String>,
        default: Option<Value<'v>>,
        #[starlark(default = "")] doc: &str,
        #[starlark(require = named)] short: Option<Value<'v>>,
    ) -> starlark::Result<CliArgs> {
        // Value seems to usually be a `[String]`, listing the possible values of the
        // enumeration. Unfortunately, for things like `exported_lang_preprocessor_flags`
        // it ends up being `Type` which doesn't match the data we see.
        Ok(CliArgs::new(
            default,
            doc,
            CliArgType::enumeration(variants.into_iter().collect()),
            short,
        )?)
    }

    /// Takes an arg from cli, and gets a parsed `TargetLabel` in bxl.
    ///
    /// **Note**: this will not check if the target is valid.
    fn target_label<'v>(
        #[starlark(default = "")] doc: &str,
        #[starlark(require = named)] short: Option<Value<'v>>,
    ) -> starlark::Result<CliArgs> {
        Ok(CliArgs::new(None, doc, CliArgType::target_label(), short)?)
    }

    /// Takes an arg from cli with `?modifier` modifiers,
    /// and gets a parsed `ConfiguredTargetLabel` in bxl.
    fn configured_target_label<'v>(
        #[starlark(default = "")] doc: &str,
        #[starlark(require = named)] short: Option<Value<'v>>,
    ) -> starlark::Result<CliArgs> {
        Ok(CliArgs::new(
            None,
            doc,
            CliArgType::configured_target_label(),
            short,
        )?)
    }

    /// Takes an arg from cli, and gets a parsed `ProvidersLabel` in bxl.
    ///
    /// **Note**: this will not check if the target is valid.
    fn sub_target<'v>(
        #[starlark(default = "")] doc: &str,
        #[starlark(require = named)] short: Option<Value<'v>>,
    ) -> starlark::Result<CliArgs> {
        Ok(CliArgs::new(None, doc, CliArgType::sub_target(), short)?)
    }

    /// Takes an arg from the cli, and treats it as a target pattern, e.g. "cell//foo:bar", "cell//foo:", or "cell//foo/...".
    /// We will get a list of `TargetLabel` in bxl.
    fn target_expr<'v>(
        #[starlark(default = "")] doc: &str,
        #[starlark(require = named)] short: Option<Value<'v>>,
    ) -> starlark::Result<CliArgs> {
        Ok(CliArgs::new(None, doc, CliArgType::target_expr(), short)?)
    }

    /// Takes an arg from the cli, and treats it as a target pattern, e.g. "cell//foo:bar", "cell//foo:", or "cell//foo/..."
    /// with `?modifier` modifiers.
    /// We will get a list of `ConfiguredTargetLabel` in bxl.
    fn configured_target_expr<'v>(
        #[starlark(default = "")] doc: &str,
        #[starlark(require = named)] short: Option<Value<'v>>,
    ) -> starlark::Result<CliArgs> {
        Ok(CliArgs::new(
            None,
            doc,
            CliArgType::configured_target_expr(),
            short,
        )?)
    }

    /// Takes an arg from cli, and would be treated as a sub target pattern. We will get a list of `ProvidersLabel` in bxl.
    fn sub_target_expr<'v>(
        #[starlark(default = "")] doc: &str,
        #[starlark(require = named)] short: Option<Value<'v>>,
    ) -> starlark::Result<CliArgs> {
        Ok(CliArgs::new(
            None,
            doc,
            CliArgType::sub_target_expr(),
            short,
        )?)
    }

    /// Takes an arg from cli, and would be treated as a json string, and return a json object in bxl.
    ///
    /// **Note**: It will not accept a json file path, if you want to pass a json file path, you can use like in cli `--flag "$(cat foo.json)"`
    fn json<'v>(
        #[starlark(default = "")] doc: &str,
        #[starlark(require = named)] short: Option<Value<'v>>,
    ) -> starlark::Result<CliArgs> {
        Ok(CliArgs::new(None, doc, CliArgType::json(), short)?)
    }
}

pub(crate) fn register_cli_args_module(registry: &mut GlobalsBuilder) {
    cli_args_module(registry)
}

pub(crate) enum ArgAccessor<'a> {
    Clap {
        clap: &'a clap::ArgMatches,
        arg: &'a str,
    },
    Literal(&'a str),
}

impl<'a> ArgAccessor<'a> {
    fn value_of(&self) -> Option<&str> {
        match self {
            ArgAccessor::Clap { clap, arg } => clap.get_one::<String>(arg).map(|x| x.as_str()),
            ArgAccessor::Literal(s) => Some(s),
        }
    }

    fn values_of(&self) -> Option<impl Iterator<Item = &str>> {
        match self {
            ArgAccessor::Clap { clap, arg } => clap
                .get_many::<String>(arg)
                .map(|x| itertools::Either::Left(x.map(|y| y.as_str()))),
            ArgAccessor::Literal(s) => Some(itertools::Either::Right(std::iter::once(*s))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use buck2_core::configuration::data::ConfigurationData;
    use buck2_core::provider::label::ProvidersLabel;
    use buck2_core::provider::label::testing::ProvidersLabelTestExt;
    use buck2_core::target::configured_target_label::ConfiguredTargetLabel;
    use buck2_core::target::label::label::TargetLabel;
    use buck2_interpreter::types::configured_providers_label::StarlarkProvidersLabel;
    use buck2_interpreter::types::target_label::StarlarkConfiguredTargetLabel;
    use buck2_interpreter::types::target_label::StarlarkTargetLabel;
    use num_bigint::BigInt;
    use starlark::values::Heap;
    use starlark::values::Value;
    use starlark_map::ordered_map::OrderedMap;

    use crate::bxl::starlark_defs::cli_args::CliArgType;
    use crate::bxl::starlark_defs::cli_args::CliArgValue;
    use crate::bxl::starlark_defs::cli_args::JsonCliArgValueData;

    #[test]
    fn print_cli_arg_list() -> buck2_error::Result<()> {
        let args = vec![
            CliArgValue::Bool(true),
            CliArgValue::String("test".to_owned()),
            CliArgValue::Int(BigInt::from(1)),
        ];

        let cli_arg = CliArgValue::List(args);
        let printed = format!("{cli_arg}");
        assert_eq!(printed, "true,test,1");

        Ok(())
    }

    #[test]
    fn print_cli_arg_json() -> buck2_error::Result<()> {
        let mut args = OrderedMap::new();
        args.insert("my_bool".to_owned(), JsonCliArgValueData::Bool(true));
        args.insert(
            "my_string".to_owned(),
            JsonCliArgValueData::String("test".to_owned()),
        );
        args.insert(
            "my_array".to_owned(),
            JsonCliArgValueData::List(vec![
                JsonCliArgValueData::String("a".to_owned()),
                JsonCliArgValueData::String("b".to_owned()),
                JsonCliArgValueData::String("c".to_owned()),
            ]),
        );

        args.insert(
            "nested".to_owned(),
            JsonCliArgValueData::Object(args.clone()),
        );

        let printed = format!("{}", JsonCliArgValueData::Object(args));
        assert_eq!(
            printed,
            "(k=my_bool,v=true),(k=my_string,v=test),(k=my_array,v=a,b,c),(k=nested,v=(k=my_bool,v=true),(k=my_string,v=test),(k=my_array,v=a,b,c))"
        );

        Ok(())
    }

    #[test]
    fn coerce_starlark() -> buck2_error::Result<()> {
        let heap = Heap::new();

        assert_eq!(
            CliArgType::bool().coerce_value(Value::new_bool(true))?,
            CliArgValue::Bool(true)
        );

        assert_eq!(
            CliArgType::int().coerce_value(heap.alloc(42))?,
            CliArgValue::Int(BigInt::from(42))
        );

        assert_eq!(
            CliArgType::float().coerce_value(heap.alloc(4.2))?,
            CliArgValue::Float("4.2".to_owned())
        );

        assert_eq!(
            CliArgType::string().coerce_value(heap.alloc("foobar"))?,
            CliArgValue::String("foobar".to_owned())
        );

        assert_eq!(
            CliArgType::enumeration(HashSet::from_iter([
                "a".to_owned(),
                "b".to_owned(),
                "c".to_owned()
            ]))
            .coerce_value(heap.alloc("a"))?,
            CliArgValue::String("a".to_owned())
        );

        assert_eq!(
            CliArgType::option(CliArgType::int()).coerce_value(Value::new_none())?,
            CliArgValue::None
        );

        assert_eq!(
            CliArgType::option(CliArgType::bool()).coerce_value(Value::new_bool(true))?,
            CliArgValue::Bool(true)
        );

        assert_eq!(
            CliArgType::bool().coerce_value(Value::new_bool(false))?,
            CliArgValue::Bool(false)
        );

        assert_eq!(
            CliArgType::list(CliArgType::int()).coerce_value(heap.alloc(vec![1, 4, 2]))?,
            CliArgValue::List(vec![
                CliArgValue::Int(BigInt::from(1)),
                CliArgValue::Int(BigInt::from(4)),
                CliArgValue::Int(BigInt::from(2))
            ])
        );

        assert_eq!(
            CliArgType::target_label().coerce_value(heap.alloc(StarlarkTargetLabel::new(
                TargetLabel::testing_parse("root//foo:bar")
            )))?,
            CliArgValue::TargetLabel(TargetLabel::testing_parse("root//foo:bar"))
        );

        assert_eq!(
            CliArgType::configured_target_label().coerce_value(heap.alloc(
                StarlarkConfiguredTargetLabel::new(ConfiguredTargetLabel::testing_parse(
                    "root//foo:bar",
                    ConfigurationData::testing_new(),
                ))
            ))?,
            CliArgValue::ConfiguredTargetLabel(ConfiguredTargetLabel::testing_parse(
                "root//foo:bar",
                ConfigurationData::testing_new(),
            ))
        );

        assert_eq!(
            CliArgType::sub_target().coerce_value(heap.alloc(StarlarkProvidersLabel::new(
                ProvidersLabel::testing_new("foo", "pkg", "bar", Some(&["a", "b"]))
            )))?,
            CliArgValue::ProvidersLabel(ProvidersLabel::testing_new(
                "foo",
                "pkg",
                "bar",
                Some(&["a", "b"])
            ))
        );

        Ok(())
    }
}
