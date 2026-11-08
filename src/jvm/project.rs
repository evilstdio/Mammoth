//! Project-wide declaration identities with a separate name scope per source.
//! Bodies are emitted only in their declaring module. Shared Java and function
//! type tables keep signatures meaningful when they cross a module boundary.

use super::*;

pub(super) struct NativeNames {
    pub classes: Vec<String>,
    pub interfaces: Vec<String>,
}

struct Scope<'a> {
    types: Types<'a>,
    classes: Vec<usize>,
    interfaces: Vec<usize>,
    functions: HashMap<&'a str, Signature<'a>>,
    extensions: HashMap<&'a str, Vec<&'a str>>,
    imports: HashMap<&'a str, JavaTarget<'a>>,
    java: HashMap<&'a str, JavaTarget<'a>>,
    java_fields: HashMap<&'a str, (Kind, JavaTarget<'a>)>,
}

pub(super) fn resolve<'a>(
    programs: &[&'a Program<'a>],
) -> Result<Vec<Symbols<'a>>, (usize, Error)> {
    let mut modules = HashMap::new();
    let mut classes = Vec::new();
    let mut interfaces = Vec::new();
    let mut class_owners = Vec::new();
    let mut interface_owners = Vec::new();
    let mut native_names = NativeNames {
        classes: Vec::new(),
        interfaces: Vec::new(),
    };
    let java = Rc::new(RefCell::new(Vec::new()));
    let functions = Rc::new(RefCell::new(Vec::new()));
    let parameters = Rc::new(RefCell::new(Vec::new()));
    let class_parameters = Rc::new(RefCell::new(Vec::new()));
    let interface_parameters = Rc::new(RefCell::new(Vec::new()));
    let bounds = Rc::new(RefCell::new(Vec::new()));
    let variances = Rc::new(RefCell::new(Vec::new()));
    let stars = Rc::new(RefCell::new(Vec::new()));
    let captures = Rc::new(RefCell::new(HashMap::new()));
    let pending_bounds = Rc::new(RefCell::new(Vec::new()));
    let applications = Rc::new(RefCell::new(Vec::new()));
    let classpath = Rc::new(OnceCell::new());
    let mut scopes = Vec::new();
    for (owner, program) in programs.iter().enumerate() {
        if let Some(name) = program.module.as_deref() {
            modules.insert(name, owner);
        }
        let own_classes = (classes.len()..classes.len() + program.classes.len()).collect();
        let own_interfaces =
            (interfaces.len()..interfaces.len() + program.interfaces.len()).collect();
        for (local, class) in program.classes.iter().enumerate() {
            classes.push(class);
            class_owners.push(owner);
            native_names.classes.push(binary_name(program, "", local));
        }
        for (local, interface) in program.interfaces.iter().enumerate() {
            interfaces.push(interface);
            interface_owners.push(owner);
            native_names
                .interfaces
                .push(binary_name(program, "Interface", local));
        }
        scopes.push(Scope {
            types: Types {
                module: program.module.as_deref(),
                native_names: Rc::new(NativeNames {
                    classes: Vec::new(),
                    interfaces: Vec::new(),
                }),
                names: HashMap::new(),
                java: java.clone(),
                functions: functions.clone(),
                parameters: parameters.clone(),
                class_parameters: class_parameters.clone(),
                interface_parameters: interface_parameters.clone(),
                bounds: bounds.clone(),
                variances: variances.clone(),
                stars: stars.clone(),
                captures: captures.clone(),
                pending_bounds: pending_bounds.clone(),
                applications: applications.clone(),
                classpath: classpath.clone(),
                packages: Vec::new(),
            },
            classes: own_classes,
            interfaces: own_interfaces,
            functions: HashMap::new(),
            extensions: HashMap::new(),
            imports: HashMap::new(),
            java: HashMap::new(),
            java_fields: HashMap::new(),
        });
    }
    let native_names = Rc::new(native_names);
    class_parameters
        .borrow_mut()
        .resize_with(classes.len(), Vec::new);
    interface_parameters
        .borrow_mut()
        .resize_with(interfaces.len(), Vec::new);
    // Register every native declaration before resolving any imports or types.
    for (owner, (scope, program)) in scopes.iter_mut().zip(programs).enumerate() {
        let result = (|| {
            scope.types.native_names = native_names.clone();
            for (&index, class) in scope.classes.iter().zip(&program.classes) {
                declare_type(&mut scope.types, class.name, Kind::Object(index))?;
            }
            for (&index, interface) in scope.interfaces.iter().zip(&program.interfaces) {
                declare_type(&mut scope.types, interface.name, Kind::Interface(index))?;
            }
            let mut java_names = HashSet::new();
            for class in &program.java_classes {
                let binary = java_class_name(class.target.text).ok_or_else(|| {
                    at(class.target, "Expected a fully qualified Java class name.")
                })?;
                if binary.len() > u16::MAX as usize - 3 {
                    return Err(at(
                        class.target,
                        "This Java class name is too long for a JVM type.",
                    ));
                }
                // Declared bindings have their own identity so aliases in one
                // source do not change Java calls made in another source.
                if binary == "java/lang/String" || !java_names.insert(binary.clone()) {
                    return Err(at(
                        class.target,
                        "This Java class already has a Mammoth type.",
                    ));
                }
                let index = scope.types.java.borrow().len();
                declare_type(&mut scope.types, class.name, Kind::Java(index))?;
                scope.types.java.borrow_mut().push(JavaEntry {
                    binary,
                    arguments: Vec::new(),
                    declared: true,
                });
            }
            Ok(())
        })();
        result.map_err(|error| (owner, error))?;
    }
    // Imports expose names only in the importing file. Class identities remain
    // global, so two files may use the same simple name without colliding.
    for (owner, program) in programs.iter().enumerate() {
        for import in &program.imports {
            if let Some(&source) = modules.get(import.module.as_str()) {
                if import.wildcard {
                    return Err((
                        owner,
                        at(
                            import.name,
                            "Name each project import, such as import example.Models.Player.",
                        ),
                    ));
                }
                let kind = scopes[source]
                    .classes
                    .iter()
                    .copied()
                    .find(|&index| classes[index].name.text == import.name.text)
                    .map(Kind::Object)
                    .or_else(|| {
                        scopes[source]
                            .interfaces
                            .iter()
                            .copied()
                            .find(|&index| interfaces[index].name.text == import.name.text)
                            .map(Kind::Interface)
                    });
                if let Some(kind) = kind {
                    declare_type(&mut scopes[owner].types, import.local, kind)
                        .map_err(|error| (owner, error))?;
                } else if !programs[source].functions.iter().any(|function| {
                    function.exported
                        && (function.name.text == import.name.text
                            || function.extension_of == Some(import.name.text)
                            || function.overload_of == Some(import.name.text))
                }) {
                    return Err((
                        owner,
                        at(
                            import.name,
                            "This module has no class, interface, or exported function with that name.",
                        ),
                    ));
                }
                continue;
            }
            let types = &mut scopes[owner].types;
            let result = (|| {
                if import.wildcard {
                    if !types.classpath().has_package(&import.module) {
                        return Err(at(import.name, "Could not find this Java package."));
                    }
                    types.packages.push(import.module.clone());
                    return Ok(());
                }
                let mut binary = format!("{}/{}", import.module, import.name.text);
                // `import a.b.Outer.Inner` names the nested class `a/b/Outer$Inner`.
                if types.classpath().load(&binary).is_none() {
                    let mut candidate = binary.clone();
                    while let Some(slash) = candidate.rfind('/') {
                        candidate.replace_range(slash..=slash, "$");
                        if types.classpath().load(&candidate).is_some() {
                            binary = candidate;
                            break;
                        }
                    }
                }
                match types.classpath().load(&binary) {
                    Some(info) if info.is_public() => {
                        let kind = types.java_kind(&binary);
                        declare_type(types, import.local, kind)
                    }
                    Some(_) => Err(at(import.name, "This Java class is not public.")),
                    None => Err(at(
                        import.name,
                        if programs.len() > 1 {
                            "Could not find this Java class or project module."
                        } else {
                            "Could not find this Java class. Check the name and the CLASSPATH."
                        },
                    )),
                }
            })();
            result.map_err(|error| (owner, error))?;
        }
    }
    for (owner, (scope, program)) in scopes.iter().zip(programs).enumerate() {
        for (&index, class) in scope.classes.iter().zip(&program.classes) {
            if class.type_parameters.is_empty() {
                continue;
            }
            let mut local = scope.types.clone();
            let names = scope
                .types
                .push_parameters(&mut local, &class.type_parameters)
                .map_err(|error| (owner, error))?;
            for (&(_, kind), &variance) in names.iter().zip(&class.variances) {
                let Kind::TypeParameter(parameter) = kind else {
                    unreachable!()
                };
                variances.borrow_mut()[parameter] = variance;
            }
            class_parameters.borrow_mut()[index] = names;
        }
        for (&index, interface) in scope.interfaces.iter().zip(&program.interfaces) {
            let mut local = scope.types.clone();
            let names = scope
                .types
                .push_parameters(&mut local, &interface.type_parameters)
                .map_err(|error| (owner, error))?;
            for (&(_, kind), &variance) in names.iter().zip(&interface.variances) {
                let Kind::TypeParameter(parameter) = kind else {
                    unreachable!()
                };
                variances.borrow_mut()[parameter] = variance;
            }
            interface_parameters.borrow_mut()[index] = names;
        }
    }
    // Bounds may name any generic class or interface, so they are resolved
    // once every declaration's parameter count is known.
    for (owner, (scope, program)) in scopes.iter().zip(programs).enumerate() {
        for (&index, class) in scope.classes.iter().zip(&program.classes) {
            let local = scope.types.class_scope(index);
            let names = class_parameters.borrow()[index].clone();
            scope
                .types
                .resolve_bounds(&local, &names, &class.bounds)
                .map_err(|error| (owner, error))?;
        }
        for (&index, interface) in scope.interfaces.iter().zip(&program.interfaces) {
            let local = scope.types.interface_scope(index);
            let names = interface_parameters.borrow()[index].clone();
            scope
                .types
                .resolve_bounds(&local, &names, &interface.bounds)
                .map_err(|error| (owner, error))?;
        }
    }
    let mut java_classes = HashMap::new();
    for (owner, (scope, program)) in scopes.iter_mut().zip(programs).enumerate() {
        let result = (|| {
            let top_level = || {
                program
                    .functions
                    .iter()
                    .chain(program.java.iter().map(|binding| &binding.function))
            };
            for (alias, target) in &program.aliases {
                let kind = resolve_type(&scope.types, *target)?;
                declare_type(&mut scope.types, *alias, kind)?;
            }
            scope.functions = resolve_functions(top_level(), 255, &scope.types, false)?;
            for function in &program.functions {
                if let Some(plain) = function.extension_of {
                    scope
                        .extensions
                        .entry(plain)
                        .or_default()
                        .push(function.name.text);
                }
            }
            for function in top_level() {
                if scope.types.names.contains_key(function.name.text) {
                    return Err(at(
                        function.name,
                        "A class or interface already uses this name.",
                    ));
                }
            }
            for binding in &program.java {
                scope.functions[binding.function.name.text].check_java(binding.function.name)?;
                scope.java.insert(
                    binding.function.name.text,
                    JavaTarget::parse(
                        binding.target,
                        "Expected a fully qualified Java method name such as java.lang.Math.max.",
                    )?,
                );
            }
            for field in &program.java_fields {
                let kind = resolve_type(&scope.types, field.annotation)?;
                if !kind.is_java_value() {
                    return Err(at(
                        field.annotation,
                        "Java fields use Text, Int, Bool, or a declared Java type.",
                    ));
                }
                let target = JavaTarget::parse(
                    field.target,
                    "Expected a fully qualified Java field name such as java.lang.Integer.MAX_VALUE.",
                )?;
                if scope
                    .java_fields
                    .insert(field.name.text, (kind, target))
                    .is_some()
                {
                    return Err(at(field.name, "This Java field name is already in use."));
                }
            }
            for class in &program.java_classes {
                let methods = resolve_functions(&class.methods, 254, &scope.types, true)?;
                let mut aliases = HashMap::new();
                for method in &class.methods {
                    if let Some(name) = method.foreign_name {
                        if name.text.len() > u16::MAX as usize || !java_identifier(name.text) {
                            return Err(at(name, "Expected a Java method name without its class."));
                        }
                        aliases.insert(method.name.text, name.text);
                    }
                }
                let constructor = class
                    .constructor
                    .as_ref()
                    .map(|function| {
                        let signature = Signature::resolve(
                            function.name,
                            &function.parameters,
                            None,
                            254,
                            &scope.types,
                        )?;
                        signature.check_java(function.name)?;
                        Ok(signature)
                    })
                    .transpose()?;
                let Kind::Java(index) = scope.types.names[class.name.text] else {
                    unreachable!()
                };
                java_classes.insert(
                    index,
                    JavaClassRef {
                        declaration: class,
                        interface: class.interface,
                        aliases,
                        constructor,
                        methods,
                    },
                );
            }
            Ok(())
        })();
        result.map_err(|error| (owner, error))?;
    }
    // Copy already-resolved signatures, never reinterpret an exported type in
    // the caller's scope. This also covers aliased and transitive object types.
    for (owner, program) in programs.iter().enumerate() {
        for import in &program.imports {
            let Some(&source) = modules.get(import.module.as_str()) else {
                continue;
            };
            for function in &programs[source].functions {
                if function.exported && function.overload_of == Some(import.name.text) {
                    let signature = scopes[source].functions[function.name.text].clone();
                    scopes[owner]
                        .functions
                        .insert(function.name.text, signature);
                    scopes[owner].imports.insert(
                        function.name.text,
                        JavaTarget {
                            class: import.module.clone(),
                            member: function.name.text,
                        },
                    );
                }
            }
            let extensions: Vec<_> = programs[source]
                .functions
                .iter()
                .filter(|function| {
                    function.exported && function.extension_of == Some(import.name.text)
                })
                .collect();
            for function in &extensions {
                let signature = scopes[source].functions[function.name.text].clone();
                scopes[owner]
                    .functions
                    .insert(function.name.text, signature);
                scopes[owner].imports.insert(
                    function.name.text,
                    JavaTarget {
                        class: import.module.clone(),
                        member: function.name.text,
                    },
                );
                scopes[owner]
                    .extensions
                    .entry(import.local.text)
                    .or_default()
                    .push(function.name.text);
                let prefix = format!("{}$default$", function.name.text);
                for helper in &programs[source].functions {
                    if helper.name.text.starts_with(prefix.as_str()) {
                        let signature = scopes[source].functions[helper.name.text].clone();
                        scopes[owner].functions.insert(helper.name.text, signature);
                        scopes[owner].imports.insert(
                            helper.name.text,
                            JavaTarget {
                                class: import.module.clone(),
                                member: helper.name.text,
                            },
                        );
                    }
                }
            }
            if !extensions.is_empty() {
                continue;
            }
            let Some(function) = programs[source]
                .functions
                .iter()
                .find(|function| function.exported && function.name.text == import.name.text)
            else {
                continue;
            };
            let local = import.local;
            if local.text == "print"
                || scopes[owner].functions.contains_key(local.text)
                || scopes[owner].types.names.contains_key(local.text)
            {
                return Err((
                    owner,
                    at(local, "This import name is already in use or reserved."),
                ));
            }
            let signature = scopes[source].functions[function.name.text].clone();
            scopes[owner].functions.insert(local.text, signature);
            scopes[owner].imports.insert(
                local.text,
                JavaTarget {
                    class: import.module.clone(),
                    member: function.name.text,
                },
            );
            // Default values travel with the function, under the local name.
            let prefix = format!("{}$default$", function.name.text);
            for helper in &programs[source].functions {
                let Some(index) = helper.name.text.strip_prefix(prefix.as_str()) else {
                    continue;
                };
                let name: &str =
                    Box::leak(format!("{}$default${index}", local.text).into_boxed_str());
                let signature = scopes[source].functions[helper.name.text].clone();
                scopes[owner].functions.insert(name, signature);
                scopes[owner].imports.insert(
                    name,
                    JavaTarget {
                        class: import.module.clone(),
                        member: helper.name.text,
                    },
                );
            }
        }
    }
    let mut overload_table: HashMap<&str, Vec<&str>> = HashMap::new();
    for program in programs.iter() {
        let members = program
            .classes
            .iter()
            .flat_map(|class| class.methods.iter().chain(&class.static_methods))
            .chain(
                program
                    .interfaces
                    .iter()
                    .flat_map(|interface| &interface.methods),
            )
            .chain(&program.functions);
        for function in members {
            if let Some(plain) = function.overload_of {
                let hidden = overload_table.entry(plain).or_default();
                if !hidden.contains(&function.name.text) {
                    hidden.push(function.name.text);
                }
            }
        }
    }
    let overloads = Rc::new(overload_table);
    let mut methods = Vec::new();
    let mut constructors = Vec::new();
    let mut secondary_constructors = Vec::new();
    let mut implementations = Vec::new();
    let mut superclasses = Vec::new();
    let mut superclass_kinds = Vec::new();
    let mut java_supers = Vec::new();
    let mut method_flags = Vec::new();
    let mut protected_methods = Vec::new();
    let mut static_methods = Vec::new();
    let mut static_fields = Vec::new();
    let mut interface_methods = Vec::new();
    let mut interface_parents = Vec::new();
    for (owner, (scope, program)) in scopes.iter().zip(programs).enumerate() {
        let types = &scope.types;
        let result = (|| {
            for &index in &scope.interfaces {
                let interface = interfaces[index];
                let local = types.interface_scope(index);
                let resolved = resolve_functions(&interface.methods, 254, &local, false)?;
                for method in &interface.methods {
                    types.check_variance(&resolved[method.name.text], method.name)?;
                }
                interface_methods.push(resolved);
                let mut parents = Vec::new();
                for &name in &interface.extends {
                    let kind = resolve_type(&local, name)?;
                    local.check_parent_variance(kind, name)?;
                    let java_interface = matches!(kind, Kind::Java(index)
                    if java_classes.get(&index).map_or_else(
                        || types.is_java_interface(&types.java_name(index)),
                        |class| class.interface,
                    ));
                    if !java_interface
                        && !matches!(kind, Kind::Interface(_) | Kind::AppliedInterface(_))
                    {
                        return Err(at(name, "An interface can only extend other interfaces."));
                    }
                    if java_interface
                        && parents.iter().any(|&other| {
                            matches!(other, Kind::Java(_))
                                && types.runtime_class_name(other) == types.runtime_class_name(kind)
                        })
                    {
                        return Err(at(name, "This interface is already listed here."));
                    }
                    if parents
                        .iter()
                        .any(|&other| types.same_interface_identity(other, kind))
                    {
                        return Err(at(name, "This interface is already listed here."));
                    }
                    parents.push(kind);
                }
                interface_parents.push(parents);
            }
            for (&index, class) in scope.classes.iter().zip(&program.classes) {
                // Static members belong to the class, not to an object, so
                // they cannot use the class's type parameters.
                for function in &class.static_methods {
                    if !function.type_parameters.is_empty() {
                        return Err(at(
                            function.name,
                            "Static methods cannot have type parameters yet.",
                        ));
                    }
                }
                static_methods.push(resolve_functions(&class.static_methods, 255, types, false)?);
                let mut fields = Vec::new();
                for field in &class.static_fields {
                    if class.enum_values.is_some() {
                        return Err(at(field.name, "Enums cannot declare static fields yet."));
                    }
                    let kind = match (field.annotation, &field.value) {
                        (Some(annotation), _) => resolve_type(types, annotation)?,
                        // Other starting values are inferred once every
                        // declaration is known; `Null` marks them until then.
                        (None, Some(value)) => literal_kind(value).unwrap_or(Kind::Null),
                        (None, None) => unreachable!("the parser requires a type or a value"),
                    };
                    let duplicate = class
                        .methods
                        .iter()
                        .map(|method| method.name)
                        .chain(class.static_methods.iter().map(|method| method.name))
                        .chain(class.fields.iter().map(|field| field.name))
                        .chain(class.parameters.iter().map(|(name, _)| *name))
                        .any(|name| name.text == field.name.text)
                        || fields
                            .iter()
                            .any(|other: &super::StaticField| other.name == field.name.text);
                    if duplicate {
                        return Err(at(field.name, "This name is already used in the class."));
                    }
                    fields.push(super::StaticField {
                        name: field.name.text,
                        kind,
                        mutable: field.mutable,
                        private: field.private,
                    });
                }
                for function in &class.static_methods {
                    if class
                        .methods
                        .iter()
                        .any(|method| method.name.text == function.name.text)
                    {
                        return Err(at(
                            function.name,
                            "A static method cannot have the same name as an instance method.",
                        ));
                    }
                }
                static_fields.push(fields);
                let local = types.class_scope(index);
                let types = &local;
                let resolved = resolve_functions(&class.methods, 254, types, false)?;
                for method in &class.methods {
                    types.check_variance(&resolved[method.name.text], method.name)?;
                }
                methods.push(resolved);
                constructors.push(Signature::resolve(
                    class.name,
                    &class.parameters,
                    None,
                    254,
                    types,
                )?);
                let mut secondary: Vec<Signature<'_>> = Vec::new();
                for item in &class.secondary {
                    let signature =
                        Signature::resolve(item.keyword, &item.parameters, None, 254, types)?;
                    let primary: &Signature<'_> = &constructors[constructors.len() - 1];
                    if signature.descriptor == primary.descriptor
                        || secondary
                            .iter()
                            .any(|other| other.descriptor == signature.descriptor)
                    {
                        return Err(at(
                            item.keyword,
                            "Another constructor of this class takes the same parameter types.",
                        ));
                    }
                    secondary.push(signature);
                }
                secondary_constructors.push(secondary);
                let mut implements = Vec::new();
                for &name in &class.implements {
                    let kind = resolve_type(types, name)?;
                    match kind {
                        Kind::Interface(_) | Kind::AppliedInterface(_) => {}
                        Kind::Java(index)
                            if java_classes.get(&index).map_or_else(
                                || types.is_java_interface(&types.java_name(index)),
                                |class| class.interface,
                            ) => {}
                        _ => return Err(at(name, "A class can only implement an interface.")),
                    }
                    if implements
                        .iter()
                        .any(|&other| types.same_interface_identity(other, kind))
                    {
                        return Err(at(name, "This interface is already listed here."));
                    }
                    implements.push(kind);
                }
                let mut java_super = None;
                let mut superclass_kind = None;
                let superclass = match &class.extends {
                    None => None,
                    Some((name, _)) => match resolve_type(types, *name)? {
                        kind @ Kind::Object(index) => {
                            superclass_kind = Some(kind);
                            Some(index)
                        }
                        kind @ Kind::Applied(_) => {
                            superclass_kind = Some(kind);
                            types.native_class(kind)
                        }
                        Kind::Interface(_) => {
                            return Err(at(
                                *name,
                                "Use implements for an interface. Use extends for a class.",
                            ));
                        }
                        Kind::Java(index) => {
                            let binary = types.java_name(index);
                            match types.java_class(&binary) {
                                Some(info) if info.is_interface() => {
                                    return Err(at(
                                        *name,
                                        "Use implements for an interface. Use extends for a class.",
                                    ));
                                }
                                Some(info) if info.access & crate::classfile::ACC_FINAL != 0 => {
                                    return Err(at(
                                        *name,
                                        "This Java class is final, so it cannot be extended.",
                                    ));
                                }
                                Some(_) => java_super = Some(index),
                                None => return Err(at(*name, "Could not find this Java class.")),
                            }
                            None
                        }
                        _ => return Err(at(*name, "A class can only extend another class.")),
                    },
                };
                if class.enum_values.is_some() {
                    // Enum<E>, so E satisfies bounds such as the one on EnumMap.
                    java_super =
                        Some(types.java_generic("java/lang/Enum", vec![Kind::Object(index)]));
                }
                if let Some(kind) = superclass_kind {
                    types.check_parent_variance(kind, class.name)?;
                }
                for &kind in &implements {
                    types.check_parent_variance(kind, class.name)?;
                }
                superclasses.push(superclass);
                superclass_kinds.push(superclass_kind);
                java_supers.push(java_super);
                implementations.push(implements);
                method_flags.push(
                    class
                        .methods
                        .iter()
                        .map(|method| (method.name.text, (method.private, method.is_abstract)))
                        .collect(),
                );
                protected_methods.push(
                    class
                        .methods
                        .iter()
                        .filter(|method| method.protected)
                        .map(|method| method.name.text)
                        .collect(),
                );
            }
            Ok(())
        })();
        result.map_err(|error| (owner, error))?;
    }
    // Reject every inheritance cycle before any ancestor traversal, including
    // cycles formed by declarations in different files.
    let mut state = vec![0u8; classes.len()];
    for start in 0..classes.len() {
        let mut chain = Vec::new();
        let mut current = Some(start);
        while let Some(index) = current {
            if state[index] == 2 {
                break;
            }
            if state[index] == 1 {
                let class = classes[index];
                let name = class.extends.as_ref().map_or(class.name, |(name, _)| *name);
                return Err((
                    class_owners[index],
                    at(
                        name,
                        "A class cannot extend itself, directly or indirectly.",
                    ),
                ));
            }
            state[index] = 1;
            chain.push(index);
            current = superclasses[index];
        }
        for index in chain {
            state[index] = 2;
        }
    }
    // Interfaces may extend several parents, so cycles are found by a
    // depth-first search over every parent edge.
    let roots = |kind: Kind| match kind {
        Kind::Interface(index) => index,
        Kind::AppliedInterface(index) => applications.borrow()[index].class,
        _ => usize::MAX,
    };
    let mut state = vec![0u8; interfaces.len()];
    for start in 0..interfaces.len() {
        if state[start] != 0 {
            continue;
        }
        let mut stack = vec![(start, 0)];
        state[start] = 1;
        while let Some(&mut (index, ref mut next)) = stack.last_mut() {
            let parents = &interface_parents[index];
            if *next == parents.len() {
                state[index] = 2;
                stack.pop();
                continue;
            }
            let position = *next;
            *next += 1;
            let parent = roots(parents[position]);
            if parent == usize::MAX {
                // Java parents cannot lead back to Mammoth interfaces.
                continue;
            }
            match state[parent] {
                0 => {
                    state[parent] = 1;
                    stack.push((parent, 0));
                }
                1 => {
                    return Err((
                        interface_owners[index],
                        at(
                            interfaces[index].extends[position],
                            "An interface cannot extend itself, directly or indirectly.",
                        ),
                    ));
                }
                _ => {}
            }
        }
    }
    let mut template = Symbols {
        types: scopes[0].types.clone(),
        functions: HashMap::new(),
        java: HashMap::new(),
        imports: HashMap::new(),
        java_fields: HashMap::new(),
        methods: Rc::new(methods),
        constructors: Rc::new(constructors),
        secondary: Rc::new(secondary_constructors),
        interfaces: Rc::new(interface_methods),
        interface_parents: Rc::new(interface_parents),
        implementations: Rc::new(implementations),
        java_classes: Rc::new(java_classes),
        superclasses: Rc::new(superclasses),
        superclass_kinds: Rc::new(superclass_kinds),
        java_supers: Rc::new(java_supers),
        method_flags: Rc::new(method_flags),
        protected_methods: Rc::new(protected_methods),
        static_methods: Rc::new(static_methods),
        static_fields: Rc::new(static_fields),
        enum_values: Rc::new(
            classes
                .iter()
                .map(|class| {
                    class
                        .enum_values
                        .as_ref()
                        .map(|values| values.iter().map(|(name, _)| name.text).collect())
                })
                .collect(),
        ),
        abstract_classes: Rc::new(classes.iter().map(|class| class.is_abstract).collect()),
        class_fields: Rc::new(classes.iter().map(|_| Vec::new()).collect()),
        globals: Vec::new(),
        class_declarations: Rc::new(classes),
        interface_declarations: Rc::new(interfaces),
        own_classes: Vec::new(),
        own_interfaces: Vec::new(),
        extensions: HashMap::new(),
        overloads: overloads.clone(),
    };
    for (owner, (scope, program)) in scopes.iter().zip(programs).enumerate() {
        template.types = scope.types.clone();
        template.own_classes = scope.classes.clone();
        template.own_interfaces = scope.interfaces.clone();
        template
            .check_interfaces(&scope.interfaces)
            .map_err(|error| (owner, error))?;
        template
            .check_classes(program)
            .map_err(|error| (owner, error))?;
    }
    // Inferred fields may depend on a field in a later source file. Retry
    // unresolved inference after making progress, while reporting other errors
    // immediately. Cyclic inference still asks for an explicit annotation.
    let mut pending = template.classes_by_depth();
    while !pending.is_empty() {
        let count = pending.len();
        let mut deferred = Vec::new();
        let mut first_error = None;
        for index in pending {
            let owner = class_owners[index];
            template.types = scopes[owner].types.class_scope(index).into_owned();
            template.functions = scopes[owner].functions.clone();
            match template.resolve_fields(index) {
                Ok(fields) => {
                    for (field, declaration) in fields
                        .iter()
                        .zip(&template.class_declarations[index].fields)
                    {
                        template
                            .types
                            .check_field_variance(field.kind, field.mutable, declaration.name)
                            .map_err(|error| (owner, error))?;
                    }
                    Rc::get_mut(&mut template.class_fields).unwrap()[index] = fields;
                }
                Err(error) if error.message.starts_with("Add a type to this field") => {
                    deferred.push(index);
                    first_error.get_or_insert((owner, error));
                }
                Err(error) => return Err((owner, error)),
            }
        }
        if deferred.len() == count {
            return Err(first_error.unwrap());
        }
        pending = deferred;
    }
    // Static fields without a type take the type of their starting value.
    for (owner, (scope, program)) in scopes.iter().zip(programs).enumerate() {
        template.types = scope.types.clone();
        template.functions = scope.functions.clone();
        for (&index, class) in scope.classes.iter().zip(&program.classes) {
            for (position, declaration) in class.static_fields.iter().enumerate() {
                if template.static_fields[index][position].kind != Kind::Null {
                    continue;
                }
                let known = template.static_fields[index].clone();
                let lookup = |name: &str| {
                    known
                        .iter()
                        .find(|field| field.name == name && field.kind != Kind::Null)
                        .map(|field| field.kind)
                };
                let kind = declaration
                    .value
                    .as_ref()
                    .and_then(|value| match value {
                        // A call to one of the class's own static methods.
                        Expr::Call(name, _) => template.static_methods[index]
                            .get(name.text)
                            .and_then(|signature| signature.result)
                            .or_else(|| template.static_kind(value, &lookup)),
                        _ => template.static_kind(value, &lookup),
                    })
                    .filter(|kind| *kind != Kind::Null)
                    .ok_or_else(|| {
                        (
                            owner,
                            at(
                                declaration.name,
                                "Give this static field a type, such as static var count: Int = start().",
                            ),
                        )
                    })?;
                Rc::get_mut(&mut template.static_fields).unwrap()[index][position].kind = kind;
            }
        }
    }
    template.functions.clear();
    let mut symbols: Vec<_> = scopes
        .into_iter()
        .map(|scope| Symbols {
            types: scope.types,
            functions: scope.functions,
            extensions: scope.extensions,
            overloads: overloads.clone(),
            java: scope.java,
            imports: scope.imports,
            java_fields: scope.java_fields,
            own_classes: scope.classes,
            own_interfaces: scope.interfaces,
            ..template.clone()
        })
        .collect();
    for (owner, (symbol, program)) in symbols.iter_mut().zip(programs).enumerate() {
        symbol.globals =
            globals::resolve_globals(program, symbol).map_err(|error| (owner, error))?;
        globals::check_global_order(program, &symbol.globals).map_err(|error| (owner, error))?;
    }
    Ok(symbols)
}

fn binary_name(program: &Program<'_>, label: &str, index: usize) -> String {
    if let Some(module) = &program.module {
        format!("{module}${label}${index}")
    } else {
        format!("Mammoth{label}${index}")
    }
}

impl Types<'_> {
    /// Java bindings may give the same JVM type different local names. Compare
    /// those identities without dropping nullability or generic arguments.
    pub(super) fn same_kind(&self, left: Kind, right: Kind) -> bool {
        if left == right {
            return true;
        }
        match (left, right) {
            (Kind::Applied(left), Kind::Applied(right))
            | (Kind::AppliedInterface(left), Kind::AppliedInterface(right)) => {
                let left = self.application(left);
                let right = self.application(right);
                left.class == right.class && self.same_kinds(&left.arguments, &right.arguments)
            }
            (Kind::Java(left), Kind::Java(right)) => {
                self.java_name(left) == self.java_name(right)
                    && self.same_kinds(
                        &self.type_arguments(Kind::Java(left)),
                        &self.type_arguments(Kind::Java(right)),
                    )
            }
            (Kind::Nullable(_), Kind::Nullable(_)) => {
                self.same_kind(left.non_null(), right.non_null())
            }
            (Kind::List(left), Kind::List(right))
            | (Kind::MutableList(left), Kind::MutableList(right)) => {
                (left == ListKind::Byte) == (right == ListKind::Byte)
                    && self.same_kind(left.element(), right.element())
            }
            (Kind::Function(left), Kind::Function(right)) => {
                let left = self.function_type(left);
                let right = self.function_type(right);
                self.same_kinds(&left.parameters, &right.parameters)
                    && self.same_result(left.result, right.result)
            }
            _ => false,
        }
    }

    fn same_kinds(&self, left: &[Kind], right: &[Kind]) -> bool {
        left.len() == right.len()
            && left
                .iter()
                .zip(right)
                .all(|(&left, &right)| self.same_kind(left, right))
    }

    pub(super) fn same_result(&self, left: Option<Kind>, right: Option<Kind>) -> bool {
        match (left, right) {
            (None, None) => true,
            (Some(left), Some(right)) => self.same_kind(left, right),
            _ => false,
        }
    }
}

/// The type of a literal starting value, for fields declared without a type.
fn literal_kind(value: &Expr<'_>) -> Option<Kind> {
    Some(match value {
        Expr::Int(..) => Kind::Int,
        Expr::Long(..) => Kind::Long,
        Expr::Double(..) => Kind::Double,
        Expr::Bool(..) => Kind::Bool,
        Expr::Char(..) => Kind::Char,
        Expr::Text(..) => Kind::Text,
        Expr::Negate(_, inner) => match literal_kind(inner)? {
            kind @ (Kind::Int | Kind::Long | Kind::Double) => kind,
            _ => return None,
        },
        _ => return None,
    })
}

impl Types<'_> {
    /// Native roots are absent for Java interfaces; two absent roots do not
    /// mean that unrelated Java interfaces are the same declaration.
    fn same_interface_identity(&self, left: Kind, right: Kind) -> bool {
        match (left, right) {
            (Kind::Java(_), Kind::Java(_)) => {
                self.runtime_class_name(left) == self.runtime_class_name(right)
            }
            _ => self
                .interface_root(left)
                .is_some_and(|root| self.interface_root(right) == Some(root)),
        }
    }
}
