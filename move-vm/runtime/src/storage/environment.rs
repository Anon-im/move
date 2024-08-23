// Copyright (c) The Move Contributors
// SPDX-License-Identifier: Apache-2.0

use crate::{
    config::VMConfig,
    native_functions::{NativeFunction, NativeFunctions},
    storage::{struct_name_index_map::StructNameIndexMap, verifier::VerifierExtension},
    Module, Script,
};
use move_binary_format::{
    access::ModuleAccess, errors::PartialVMResult, file_format::CompiledScript, CompiledModule,
};
use move_bytecode_verifier::dependencies;
use move_core_types::{
    account_address::AccountAddress,
    identifier::{IdentStr, Identifier},
};
use std::sync::Arc;

/// Wrapper around partially verified compiled module, i.e., one that passed
/// local bytecode verification, but not the dependency checks yet.
pub struct PartiallyVerifiedModule(Arc<CompiledModule>);

impl PartiallyVerifiedModule {
    pub fn immediate_dependencies_iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&AccountAddress, &IdentStr)> {
        self.0.immediate_dependencies_iter()
    }

    pub fn immediate_friends_iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&AccountAddress, &IdentStr)> {
        self.0.immediate_friends_iter()
    }
}

/// Wrapper around partially verified compiled script, i.e., one that passed
/// local bytecode verification, but not the dependency checks yet.
pub struct PartiallyVerifiedScript(Arc<CompiledScript>);

/// [MoveVM] runtime environment encapsulating different configurations. Shared
/// between the VM and the code cache.
pub struct RuntimeEnvironment {
    /// Configuration for the VM. Contains information about enabled checks,
    /// verification, deserialization, etc.
    vm_config: VMConfig,
    /// All registered native functions in the current context (binary). When
    /// a verified [Module] is constructed, existing native functions are inlined
    /// in the module representation, so that the interpreter can call them directly.
    natives: NativeFunctions,
    /// Optional verifier extension to run passes on modules and scripts provided externally.
    verifier_extension: Option<Arc<dyn VerifierExtension>>,

    /// Map from struct names to indices, to save on unnecessary cloning and reduce
    /// memory consumption. Used by all struct type creations in the VM and in code cache.
    struct_name_index_map: StructNameIndexMap,
}

impl RuntimeEnvironment {
    /// Creates a new runtime environment with native functions and VM configurations.
    /// If there are duplicated natives, creation panics. Also, callers can provide
    /// verification extensions to add hooks on top of a bytecode verifier.
    pub(crate) fn new(
        vm_config: VMConfig,
        natives: impl IntoIterator<Item = (AccountAddress, Identifier, Identifier, NativeFunction)>,
        verifier_extension: Option<Arc<dyn VerifierExtension>>,
    ) -> Self {
        let natives = NativeFunctions::new(natives)
            .unwrap_or_else(|e| panic!("Failed to create native functions: {}", e));
        Self {
            vm_config,
            natives,
            struct_name_index_map: StructNameIndexMap::empty(),
            verifier_extension,
        }
    }

    pub fn test() -> Self {
        Self {
            vm_config: VMConfig::default(),
            natives: NativeFunctions::new(vec![]).unwrap(),
            struct_name_index_map: StructNameIndexMap::empty(),
            verifier_extension: None,
        }
    }

    /// Returns the config currently used by this runtime environment.
    pub fn vm_config(&self) -> &VMConfig {
        &self.vm_config
    }

    /// Returns native functions available to this runtime.
    pub(crate) fn natives(&self) -> &NativeFunctions {
        &self.natives
    }

    /// Returns the re-indexing map currently used by this runtime environment
    /// to remap struct identifiers into indices.
    pub(crate) fn struct_name_index_map(&self) -> &StructNameIndexMap {
        &self.struct_name_index_map
    }

    /// Creates a partially verified compiled script by running:
    ///   1. Move bytecode verifier,
    ///   2. Verifier extension, if provided.
    pub fn build_partially_verified_script(
        &self,
        compiled_script: Arc<CompiledScript>,
    ) -> PartialVMResult<PartiallyVerifiedScript> {
        move_bytecode_verifier::verify_script(compiled_script.as_ref())
            .map_err(|e| e.to_partial())?;
        if let Some(verifier) = &self.verifier_extension {
            verifier.verify_script(compiled_script.as_ref())?;
        }
        Ok(PartiallyVerifiedScript(compiled_script))
    }

    /// Creates a fully verified script by running dependency verification
    /// pass. The caller must provide verified module dependencies.
    pub fn build_verified_script(
        &self,
        partially_verified_script: PartiallyVerifiedScript,
        immediate_dependencies: &[Arc<Module>],
    ) -> PartialVMResult<Script> {
        dependencies::verify_script(
            partially_verified_script.0.as_ref(),
            immediate_dependencies.iter().map(|m| m.module()),
        )
        .map_err(|e| e.to_partial())?;
        Script::new(partially_verified_script.0, self.struct_name_index_map())
    }

    /// Creates a partially verified compiled module by running:
    ///   1. Move bytecode verifier,
    ///   2. Verifier extension, if provided.
    pub fn build_partially_verified_module(
        &self,
        compiled_module: Arc<CompiledModule>,
    ) -> PartialVMResult<PartiallyVerifiedModule> {
        move_bytecode_verifier::verify_module(compiled_module.as_ref())
            .map_err(|e| e.to_partial())?;
        if let Some(verifier) = &self.verifier_extension {
            verifier.verify_module(compiled_module.as_ref())?;
        }
        Ok(PartiallyVerifiedModule(compiled_module))
    }

    /// Creates a fully verified module by running dependency verification
    /// pass. The caller must provide verified module dependencies.
    pub fn build_verified_module(
        &self,
        partially_verified_module: PartiallyVerifiedModule,
        immediate_dependencies: &[Arc<Module>],
    ) -> PartialVMResult<Module> {
        dependencies::verify_module(
            partially_verified_module.0.as_ref(),
            immediate_dependencies.iter().map(|m| m.module()),
        )
        .map_err(|e| e.to_partial())?;
        Module::new(
            &self.natives,
            partially_verified_module.0,
            self.struct_name_index_map(),
        )
    }
}

/// Represents any type that contains a [RuntimeEnvironment].
pub trait WithEnvironment {
    fn runtime_environment(&self) -> &RuntimeEnvironment;
}