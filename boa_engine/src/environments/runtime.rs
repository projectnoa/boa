use crate::{
    environments::CompileTimeEnvironment, error::JsNativeError, object::JsObject, Context,
    JsResult, JsString, JsSymbol, JsValue,
};
use boa_ast::expression::Identifier;
use boa_gc::{Finalize, Gc, GcRefCell, Trace};
use rustc_hash::FxHashSet;
use std::cell::Cell;

/// A declarative environment holds binding values at runtime.
///
/// Bindings are stored in a fixed size list of optional values.
/// If a binding is not initialized, the value is `None`.
///
/// Optionally, an environment can hold a `this` value.
/// The `this` value is present only if the environment is a function environment.
///
/// Code evaluation at runtime (e.g. the `eval` built-in function) can add
/// bindings to existing, compiled function environments.
/// This makes it impossible to determine the location of all bindings at compile time.
/// To dynamically check for added bindings at runtime, a reference to the
/// corresponding compile time environment is needed.
///
/// Checking all environments for potential added bindings at runtime on every get/set
/// would offset the performance improvement of determining binding locations at compile time.
/// To minimize this, each environment holds a `poisoned` flag.
/// If bindings where added at runtime, the current environment and all inner environments
/// are marked as poisoned.
/// All poisoned environments have to be checked for added bindings.
#[derive(Debug, Trace, Finalize)]
pub(crate) struct DeclarativeEnvironment {
    bindings: GcRefCell<Vec<Option<JsValue>>>,
    compile: Gc<GcRefCell<CompileTimeEnvironment>>,
    #[unsafe_ignore_trace]
    poisoned: Cell<bool>,
    #[unsafe_ignore_trace]
    with: Cell<bool>,
    slots: Option<EnvironmentSlots>,
}

impl DeclarativeEnvironment {
    /// Creates a new, global `DeclarativeEnvironment`.
    pub(crate) fn new_global() -> Self {
        DeclarativeEnvironment {
            bindings: GcRefCell::new(Vec::new()),
            compile: Gc::new(GcRefCell::new(CompileTimeEnvironment::new_global())),
            poisoned: Cell::new(false),
            with: Cell::new(false),
            slots: Some(EnvironmentSlots::Global),
        }
    }

    /// Gets the compile time environment of this environment.
    pub(crate) fn compile_env(&self) -> Gc<GcRefCell<CompileTimeEnvironment>> {
        self.compile.clone()
    }

    /// Gets the bindings of this environment.
    pub(crate) const fn bindings(&self) -> &GcRefCell<Vec<Option<JsValue>>> {
        &self.bindings
    }
}

/// Describes the different types of internal slot data that an environment can hold.
#[derive(Clone, Debug, Trace, Finalize)]
pub(crate) enum EnvironmentSlots {
    Function(GcRefCell<FunctionSlots>),
    Global,
}

impl EnvironmentSlots {
    /// Return the slots if they are part of a function environment.
    pub(crate) const fn as_function_slots(&self) -> Option<&GcRefCell<FunctionSlots>> {
        if let Self::Function(env) = &self {
            Some(env)
        } else {
            None
        }
    }
}

/// Holds the internal slots of a function environment.
#[derive(Clone, Debug, Trace, Finalize)]
pub(crate) struct FunctionSlots {
    /// The `[[ThisValue]]` internal slot.
    this: JsValue,

    /// The `[[ThisBindingStatus]]` internal slot.
    #[unsafe_ignore_trace]
    this_binding_status: ThisBindingStatus,

    /// The `[[FunctionObject]]` internal slot.
    function_object: JsObject,

    /// The `[[NewTarget]]` internal slot.
    new_target: Option<JsObject>,
}

impl FunctionSlots {
    /// Returns the value of the `[[FunctionObject]]` internal slot.
    pub(crate) const fn function_object(&self) -> &JsObject {
        &self.function_object
    }

    /// Returns the value of the `[[NewTarget]]` internal slot.
    pub(crate) const fn new_target(&self) -> Option<&JsObject> {
        self.new_target.as_ref()
    }

    /// `BindThisValue`
    ///
    /// Sets the given value as the `this` binding of the environment.
    /// Returns `false` if the `this` binding has already been initialized.
    ///
    /// More information:
    ///  - [ECMAScript specification][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-bindthisvalue
    pub(crate) fn bind_this_value(&mut self, this: &JsObject) -> bool {
        // 1. Assert: envRec.[[ThisBindingStatus]] is not lexical.
        debug_assert!(self.this_binding_status != ThisBindingStatus::Lexical);

        // 2. If envRec.[[ThisBindingStatus]] is initialized, throw a ReferenceError exception.
        if self.this_binding_status == ThisBindingStatus::Initialized {
            return false;
        }

        // 3. Set envRec.[[ThisValue]] to V.
        self.this = this.clone().into();

        // 4. Set envRec.[[ThisBindingStatus]] to initialized.
        self.this_binding_status = ThisBindingStatus::Initialized;

        // 5. Return V.
        true
    }

    /// `HasThisBinding`
    ///
    /// Returns if the environment has a `this` binding.
    ///
    /// More information:
    ///  - [ECMAScript specification][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-function-environment-records-hasthisbinding
    pub(crate) fn has_this_binding(&self) -> bool {
        // 1. If envRec.[[ThisBindingStatus]] is lexical, return false; otherwise, return true.
        self.this_binding_status != ThisBindingStatus::Lexical
    }

    /// `HasSuperBinding`
    ///
    /// Returns if the environment has a `super` binding.
    ///
    /// More information:
    ///  - [ECMAScript specification][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-function-environment-records-hassuperbinding
    ///
    /// # Panics
    ///
    /// Panics if the function object of the environment is not a function.
    pub(crate) fn has_super_binding(&self) -> bool {
        // 1.If envRec.[[ThisBindingStatus]] is lexical, return false.
        if self.this_binding_status == ThisBindingStatus::Lexical {
            return false;
        }

        // 2. If envRec.[[FunctionObject]].[[HomeObject]] is undefined, return false; otherwise, return true.
        self.function_object
            .borrow()
            .as_function()
            .expect("function object must be function")
            .get_home_object()
            .is_some()
    }

    /// `GetThisBinding`
    ///
    /// Returns the `this` binding on the function environment.
    ///
    /// More information:
    ///  - [ECMAScript specification][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-function-environment-records-getthisbinding
    pub(crate) fn get_this_binding(&self) -> Result<&JsValue, JsNativeError> {
        // 1. Assert: envRec.[[ThisBindingStatus]] is not lexical.
        debug_assert!(self.this_binding_status != ThisBindingStatus::Lexical);

        // 2. If envRec.[[ThisBindingStatus]] is uninitialized, throw a ReferenceError exception.
        if self.this_binding_status == ThisBindingStatus::Uninitialized {
            Err(JsNativeError::reference().with_message("Must call super constructor in derived class before accessing 'this' or returning from derived constructor"))
        } else {
            // 3. Return envRec.[[ThisValue]].
            Ok(&self.this)
        }
    }
}

/// Describes the status of a `this` binding in function environments.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ThisBindingStatus {
    Lexical,
    Initialized,
    Uninitialized,
}

impl DeclarativeEnvironment {
    /// Returns the internal slot data of the current environment.
    pub(crate) const fn slots(&self) -> Option<&EnvironmentSlots> {
        self.slots.as_ref()
    }

    /// Get the binding value from the environment by it's index.
    ///
    /// # Panics
    ///
    /// Panics if the binding value is out of range or not initialized.
    pub(crate) fn get(&self, index: usize) -> JsValue {
        self.bindings
            .borrow()
            .get(index)
            .expect("binding index must be in range")
            .clone()
            .expect("binding must be initialized")
    }

    /// Set the binding value at the specified index.
    ///
    /// # Panics
    ///
    /// Panics if the binding value is out of range or not initialized.
    pub(crate) fn set(&self, index: usize, value: JsValue) {
        let mut bindings = self.bindings.borrow_mut();
        let binding = bindings
            .get_mut(index)
            .expect("binding index must be in range");
        assert!(!binding.is_none(), "binding must be initialized");
        *binding = Some(value);
    }
}

/// The environment stack holds all environments at runtime.
///
/// Environments themselves are garbage collected,
/// because they must be preserved for function calls.
#[derive(Clone, Debug, Trace, Finalize)]
pub(crate) struct DeclarativeEnvironmentStack {
    stack: Vec<Environment>,
}

/// A runtime environment.
#[derive(Clone, Debug, Trace, Finalize)]
pub(crate) enum Environment {
    Declarative(Gc<DeclarativeEnvironment>),
    Object(JsObject),
}

impl Environment {
    /// Returns the declarative environment if it is one.
    pub(crate) const fn as_declarative(&self) -> Option<&Gc<DeclarativeEnvironment>> {
        match self {
            Self::Declarative(env) => Some(env),
            Self::Object(_) => None,
        }
    }

    /// Returns the declarative environment and panic if it is not one.
    #[track_caller]
    pub(crate) fn declarative_expect(&self) -> &Gc<DeclarativeEnvironment> {
        self.as_declarative()
            .expect("environment must be declarative")
    }
}

impl DeclarativeEnvironmentStack {
    /// Create a new environment stack.
    pub(crate) fn new(global: Gc<DeclarativeEnvironment>) -> Self {
        Self {
            stack: vec![Environment::Declarative(global)],
        }
    }

    /// Replaces the current global with a new global environment.
    pub(crate) fn replace_global(&mut self, global: Gc<DeclarativeEnvironment>) {
        self.stack[0] = Environment::Declarative(global);
    }

    /// Extends the length of the next outer function environment to the number of compiled bindings.
    ///
    /// This is only useful when compiled bindings are added after the initial compilation (eval).
    pub(crate) fn extend_outer_function_environment(&mut self) {
        for env in self
            .stack
            .iter()
            .filter_map(Environment::as_declarative)
            .rev()
        {
            if let Some(EnvironmentSlots::Function(_)) = env.slots {
                let compile_bindings_number = env.compile.borrow().num_bindings();
                let mut bindings_mut = env.bindings.borrow_mut();

                if compile_bindings_number > bindings_mut.len() {
                    let diff = compile_bindings_number - bindings_mut.len();
                    bindings_mut.extend(vec![None; diff]);
                }
                break;
            }
        }
    }

    /// Check if any of the provided binding names are defined as lexical bindings.
    ///
    /// Start at the current environment.
    /// Stop at the next outer function environment.
    pub(crate) fn has_lex_binding_until_function_environment(
        &self,
        names: &FxHashSet<Identifier>,
    ) -> Option<Identifier> {
        for env in self
            .stack
            .iter()
            .filter_map(Environment::as_declarative)
            .rev()
        {
            let compile = env.compile.borrow();
            for name in names {
                if compile.has_lex_binding(*name) {
                    return Some(*name);
                }
            }
            if compile.is_function() {
                break;
            }
        }
        None
    }

    /// Pop all current environments except the global environment.
    pub(crate) fn pop_to_global(&mut self) -> Vec<Environment> {
        self.stack.split_off(1)
    }

    /// Get the number of current environments.
    pub(crate) fn len(&self) -> usize {
        self.stack.len()
    }

    /// Truncate current environments to the given number.
    pub(crate) fn truncate(&mut self, len: usize) {
        self.stack.truncate(len);
    }

    /// Extend the current environment stack with the given environments.
    pub(crate) fn extend(&mut self, other: Vec<Environment>) {
        self.stack.extend(other);
    }

    /// `GetThisEnvironment`
    ///
    /// Returns the environment that currently provides a `this` biding.
    ///
    /// More information:
    ///  - [ECMAScript specification][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-getthisenvironment
    ///
    /// # Panics
    ///
    /// Panics if no environment exists on the stack.
    pub(crate) fn get_this_environment(&self) -> &EnvironmentSlots {
        for env in self
            .stack
            .iter()
            .filter_map(Environment::as_declarative)
            .rev()
        {
            if let Some(slots) = &env.slots {
                match slots {
                    EnvironmentSlots::Function(function_env) => {
                        if function_env.borrow().has_this_binding() {
                            return slots;
                        }
                    }
                    EnvironmentSlots::Global => return slots,
                }
            }
        }

        panic!("global environment must exist")
    }

    /// Push a new object environment on the environments stack and return it's index.
    pub(crate) fn push_object(&mut self, object: JsObject) -> usize {
        let index = self.stack.len();
        self.stack.push(Environment::Object(object));
        index
    }

    /// Push a declarative environment on the environments stack and return it's index.
    ///
    /// # Panics
    ///
    /// Panics if no environment exists on the stack.
    #[track_caller]
    pub(crate) fn push_declarative(
        &mut self,
        num_bindings: usize,
        compile_environment: Gc<GcRefCell<CompileTimeEnvironment>>,
    ) -> usize {
        let (poisoned, with) = {
            let with = self
                .stack
                .last()
                .expect("global environment must always exist")
                .as_declarative()
                .is_none();

            let environment = self
                .stack
                .iter()
                .rev()
                .find_map(Environment::as_declarative)
                .expect("global environment must always exist");
            (environment.poisoned.get(), with || environment.with.get())
        };

        let index = self.stack.len();

        self.stack
            .push(Environment::Declarative(Gc::new(DeclarativeEnvironment {
                bindings: GcRefCell::new(vec![None; num_bindings]),
                compile: compile_environment,
                poisoned: Cell::new(poisoned),
                with: Cell::new(with),
                slots: None,
            })));

        index
    }

    /// Push a function environment on the environments stack.
    ///
    /// # Panics
    ///
    /// Panics if no environment exists on the stack.
    #[track_caller]
    pub(crate) fn push_function(
        &mut self,
        num_bindings: usize,
        compile_environment: Gc<GcRefCell<CompileTimeEnvironment>>,
        this: Option<JsValue>,
        function_object: JsObject,
        new_target: Option<JsObject>,
        lexical: bool,
    ) {
        let (poisoned, with) = {
            let with = self
                .stack
                .last()
                .expect("global environment must always exist")
                .as_declarative()
                .is_none();

            let environment = self
                .stack
                .iter()
                .rev()
                .find_map(Environment::as_declarative)
                .expect("global environment must always exist");
            (environment.poisoned.get(), with || environment.with.get())
        };

        let this_binding_status = if lexical {
            ThisBindingStatus::Lexical
        } else if this.is_some() {
            ThisBindingStatus::Initialized
        } else {
            ThisBindingStatus::Uninitialized
        };

        let this = this.unwrap_or(JsValue::Null);

        let mut bindings = vec![None; num_bindings];
        for index in compile_environment.borrow().var_binding_indices() {
            bindings[index] = Some(JsValue::Undefined);
        }

        self.stack
            .push(Environment::Declarative(Gc::new(DeclarativeEnvironment {
                bindings: GcRefCell::new(bindings),
                compile: compile_environment,
                poisoned: Cell::new(poisoned),
                with: Cell::new(with),
                slots: Some(EnvironmentSlots::Function(GcRefCell::new(FunctionSlots {
                    this,
                    this_binding_status,
                    function_object,
                    new_target,
                }))),
            })));
    }

    /// Push a function environment that inherits it's internal slots from the outer function
    /// environment.
    ///
    /// # Panics
    ///
    /// Panics if no environment exists on the stack.
    #[track_caller]
    pub(crate) fn push_function_inherit(
        &mut self,
        num_bindings: usize,
        compile_environment: Gc<GcRefCell<CompileTimeEnvironment>>,
    ) {
        debug_assert!(
            self.stack.len() == compile_environment.borrow().environment_index(),
            "tried to push an invalid compile environment"
        );

        let (poisoned, with, slots) = {
            let with = self
                .stack
                .last()
                .expect("global environment must always exist")
                .as_declarative()
                .is_none();

            let environment = self
                .stack
                .iter()
                .rev()
                .find_map(|env| env.as_declarative().filter(|decl| decl.slots().is_some()))
                .expect("global environment must always exist");
            (
                environment.poisoned.get(),
                with || environment.with.get(),
                environment.slots.clone(),
            )
        };

        let mut bindings = vec![None; num_bindings];
        for index in compile_environment.borrow().var_binding_indices() {
            bindings[index] = Some(JsValue::Undefined);
        }

        self.stack
            .push(Environment::Declarative(Gc::new(DeclarativeEnvironment {
                bindings: GcRefCell::new(bindings),
                compile: compile_environment,
                poisoned: Cell::new(poisoned),
                with: Cell::new(with),
                slots,
            })));
    }

    /// Pop environment from the environments stack.
    #[track_caller]
    pub(crate) fn pop(&mut self) -> Environment {
        debug_assert!(self.stack.len() > 1);
        self.stack
            .pop()
            .expect("environment stack is cannot be empty")
    }

    /// Get the most outer environment.
    ///
    /// # Panics
    ///
    /// Panics if no environment exists on the stack.
    #[track_caller]
    pub(crate) fn current(&mut self) -> Environment {
        self.stack
            .last()
            .expect("global environment must always exist")
            .clone()
    }

    /// Get the compile environment for the current runtime environment.
    ///
    /// # Panics
    ///
    /// Panics if no environment exists on the stack.
    pub(crate) fn current_compile_environment(&self) -> Gc<GcRefCell<CompileTimeEnvironment>> {
        self.stack
            .iter()
            .filter_map(Environment::as_declarative)
            .last()
            .expect("global environment must always exist")
            .compile
            .clone()
    }

    /// Mark that there may be added bindings from the current environment to the next function
    /// environment.
    pub(crate) fn poison_until_last_function(&mut self) {
        for env in self
            .stack
            .iter()
            .rev()
            .filter_map(Environment::as_declarative)
        {
            env.poisoned.set(true);
            if env.compile_env().borrow().is_function() {
                return;
            }
        }
    }

    /// Set the value of a declarative binding.
    ///
    /// # Panics
    ///
    /// Panics if the environment or binding index are out of range.
    #[track_caller]
    pub(crate) fn put_declarative_value(
        &mut self,
        environment_index: usize,
        binding_index: usize,
        value: JsValue,
    ) {
        let mut bindings = self
            .stack
            .get(environment_index)
            .expect("environment index must be in range")
            .declarative_expect()
            .bindings
            .borrow_mut();
        let binding = bindings
            .get_mut(binding_index)
            .expect("binding index must be in range");
        *binding = Some(value);
    }

    /// Set the value of a binding if it is uninitialized.
    ///
    /// # Panics
    ///
    /// Panics if the environment or binding index are out of range.
    #[track_caller]
    pub(crate) fn put_value_if_uninitialized(
        &mut self,
        environment_index: usize,
        binding_index: usize,
        value: JsValue,
    ) {
        let mut bindings = self
            .stack
            .get(environment_index)
            .expect("environment index must be in range")
            .declarative_expect()
            .bindings
            .borrow_mut();
        let binding = bindings
            .get_mut(binding_index)
            .expect("binding index must be in range");
        if binding.is_none() {
            *binding = Some(value);
        }
    }
}

/// A binding locator contains all information about a binding that is needed to resolve it at runtime.
///
/// Binding locators get created at bytecode compile time and are accessible at runtime via the [`crate::vm::CodeBlock`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct BindingLocator {
    name: Identifier,
    environment_index: usize,
    binding_index: usize,
    global: bool,
    mutate_immutable: bool,
    silent: bool,
}

impl BindingLocator {
    /// Creates a new declarative binding locator that has knows indices.
    pub(crate) const fn declarative(
        name: Identifier,
        environment_index: usize,
        binding_index: usize,
    ) -> Self {
        Self {
            name,
            environment_index,
            binding_index,
            global: false,
            mutate_immutable: false,
            silent: false,
        }
    }

    /// Creates a binding locator that indicates that the binding is on the global object.
    pub(in crate::environments) const fn global(name: Identifier) -> Self {
        Self {
            name,
            environment_index: 0,
            binding_index: 0,
            global: true,
            mutate_immutable: false,
            silent: false,
        }
    }

    /// Creates a binding locator that indicates that it was attempted to mutate an immutable binding.
    /// At runtime this should always produce a type error.
    pub(in crate::environments) const fn mutate_immutable(name: Identifier) -> Self {
        Self {
            name,
            environment_index: 0,
            binding_index: 0,
            global: false,
            mutate_immutable: true,
            silent: false,
        }
    }

    /// Creates a binding locator that indicates that any action is silently ignored.
    pub(in crate::environments) const fn silent(name: Identifier) -> Self {
        Self {
            name,
            environment_index: 0,
            binding_index: 0,
            global: false,
            mutate_immutable: false,
            silent: true,
        }
    }

    /// Returns the name of the binding.
    pub(crate) const fn name(&self) -> Identifier {
        self.name
    }

    /// Returns if the binding is located on the global object.
    pub(crate) const fn is_global(&self) -> bool {
        self.global
    }

    /// Returns the environment index of the binding.
    pub(crate) const fn environment_index(&self) -> usize {
        self.environment_index
    }

    /// Returns the binding index of the binding.
    pub(crate) const fn binding_index(&self) -> usize {
        self.binding_index
    }

    /// Returns if the binding is a silent operation.
    pub(crate) const fn is_silent(&self) -> bool {
        self.silent
    }

    /// Helper method to throws an error if the binding access is illegal.
    pub(crate) fn throw_mutate_immutable(
        &self,
        context: &mut Context<'_>,
    ) -> Result<(), JsNativeError> {
        if self.mutate_immutable {
            Err(JsNativeError::typ().with_message(format!(
                "cannot mutate an immutable binding '{}'",
                context.interner().resolve_expect(self.name.sym())
            )))
        } else {
            Ok(())
        }
    }
}

impl Context<'_> {
    /// Gets the corresponding runtime binding of the provided `BindingLocator`, modifying
    /// its indexes in place.
    ///
    /// This readjusts a `BindingLocator` to the correct binding if a `with` environment or
    /// `eval` call modified the compile-time bindings.
    ///
    /// Only use if the binding origin is unknown or comes from a `var` declaration. Lexical bindings
    /// are completely removed of runtime checks because the specification guarantees that runtime
    /// semantics cannot add or remove lexical bindings.
    pub(crate) fn find_runtime_binding(&mut self, locator: &mut BindingLocator) -> JsResult<()> {
        let current = self.vm.environments.current();
        if let Some(env) = current.as_declarative() {
            if !env.with.get() && !env.poisoned.get() {
                return Ok(());
            }
        }

        for env_index in (locator.environment_index..self.vm.environments.stack.len()).rev() {
            match self.environment_expect(env_index) {
                Environment::Declarative(env) => {
                    if env.poisoned.get() {
                        let compile = env.compile.borrow();
                        if compile.is_function() {
                            if let Some(b) = compile.get_binding(locator.name) {
                                locator.environment_index = b.environment_index;
                                locator.binding_index = b.binding_index;
                                locator.global = false;
                                break;
                            }
                        }
                    } else if !env.with.get() {
                        break;
                    }
                }
                Environment::Object(o) => {
                    let o = o.clone();
                    let key: JsString = self
                        .interner()
                        .resolve_expect(locator.name.sym())
                        .into_common(false);
                    if o.has_property(key.clone(), self)? {
                        if let Some(unscopables) = o.get(JsSymbol::unscopables(), self)?.as_object()
                        {
                            if unscopables.get(key.clone(), self)?.to_boolean() {
                                continue;
                            }
                        }
                        locator.environment_index = env_index;
                        locator.global = false;
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Checks if the binding pointed by `locator` is initialized.
    ///
    /// # Panics
    ///
    /// Panics if the environment or binding index are out of range.
    pub(crate) fn is_initialized_binding(&mut self, locator: &BindingLocator) -> JsResult<bool> {
        if locator.global {
            let key: JsString = self
                .interner()
                .resolve_expect(locator.name.sym())
                .into_common(false);
            self.global_object().has_property(key, self)
        } else {
            match self.environment_expect(locator.environment_index) {
                Environment::Declarative(env) => {
                    Ok(env.bindings.borrow()[locator.binding_index].is_some())
                }
                Environment::Object(_) => Ok(true),
            }
        }
    }

    /// Get the value of a binding.
    ///
    /// # Panics
    ///
    /// Panics if the environment or binding index are out of range.
    pub(crate) fn get_binding(&mut self, locator: BindingLocator) -> JsResult<Option<JsValue>> {
        if locator.global {
            let global = self.global_object();
            let key: JsString = self
                .interner()
                .resolve_expect(locator.name.sym())
                .into_common(false);
            if global.has_property(key.clone(), self)? {
                global.get(key, self).map(Some)
            } else {
                Ok(None)
            }
        } else {
            match self.environment_expect(locator.environment_index) {
                Environment::Declarative(env) => {
                    Ok(env.bindings.borrow()[locator.binding_index].clone())
                }
                Environment::Object(obj) => {
                    let obj = obj.clone();
                    let key: JsString = self
                        .interner()
                        .resolve_expect(locator.name.sym())
                        .into_common(false);
                    obj.get(key, self).map(Some)
                }
            }
        }
    }

    /// Sets the value of a binding.
    ///
    /// # Panics
    ///
    /// Panics if the environment or binding index are out of range.
    #[track_caller]
    pub(crate) fn set_binding(
        &mut self,
        locator: BindingLocator,
        value: JsValue,
        strict: bool,
    ) -> JsResult<()> {
        if locator.global {
            let key = self
                .interner()
                .resolve_expect(locator.name().sym())
                .into_common::<JsString>(false);

            self.global_object().set(key, value, strict, self)?;
        } else {
            match self.environment_expect(locator.environment_index) {
                Environment::Declarative(decl) => {
                    decl.bindings.borrow_mut()[locator.binding_index] = Some(value);
                }
                Environment::Object(obj) => {
                    let obj = obj.clone();
                    let key: JsString = self
                        .interner()
                        .resolve_expect(locator.name.sym())
                        .into_common(false);

                    obj.set(key, value, strict, self)?;
                }
            }
        }

        Ok(())
    }

    /// Deletes a binding if it exists.
    ///
    /// Returns `true` if the binding was deleted.
    ///
    /// # Panics
    ///
    /// Panics if the environment or binding index are out of range.
    pub(crate) fn delete_binding(&mut self, locator: BindingLocator) -> JsResult<bool> {
        if locator.is_global() {
            let key: JsString = self
                .interner()
                .resolve_expect(locator.name().sym())
                .into_common::<JsString>(false);
            self.global_object().__delete__(&key.into(), self)
        } else {
            match self.environment_expect(locator.environment_index) {
                Environment::Declarative(_) => Ok(false),
                Environment::Object(obj) => {
                    let obj = obj.clone();
                    let key: JsString = self
                        .interner()
                        .resolve_expect(locator.name.sym())
                        .into_common(false);

                    obj.__delete__(&key.into(), self)
                }
            }
        }
    }

    /// Return the environment at the given index. Panics if the index is out of range.
    fn environment_expect(&self, index: usize) -> &Environment {
        self.vm
            .environments
            .stack
            .get(index)
            .expect("environment index must be in range")
    }
}
