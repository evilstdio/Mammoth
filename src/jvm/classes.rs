//! Class features: field types, inheritance checks, field access, and calls on
//! `this` and `super`.

use super::defaults::DefaultHost;
use crate::syntax::Statement;
use std::collections::{HashMap, HashSet};

use super::{
    Arithmetic, Emitter, Error, Expr, FieldInfo, Kind, ListKind, Method, Name, Program, Signature,
    Symbols, Types, at, instruction, resolve_type,
};

impl<'a> Symbols<'a> {
    /// Compare method constraints in the actual class/interface views, then
    /// rename the right method's parameters to the left method's parameters.
    fn same_method_bounds(
        &self,
        left: &Signature<'_>,
        right: &Signature<'_>,
        left_map: &std::collections::HashMap<Kind, Kind>,
        right_map: &std::collections::HashMap<Kind, Kind>,
    ) -> bool {
        if left.type_parameters.len() != right.type_parameters.len() {
            return false;
        }
        let mut substitutions = right_map.clone();
        substitutions.extend(
            right
                .type_parameters
                .iter()
                .zip(&left.type_parameters)
                .map(|((_, from), (_, to))| (*from, *to)),
        );
        left.type_parameters
            .iter()
            .zip(&right.type_parameters)
            .all(|((_, left), (_, right))| {
                let actual = self.types.all_bounds(*left);
                let expected = self.types.all_bounds(*right);
                actual.len() == expected.len()
                    && actual.iter().zip(&expected).all(|(&left, &right)| {
                        self.types.same_kind(
                            self.types.replace_parameters(left, left_map),
                            self.types.replace_parameters(right, &substitutions),
                        )
                    })
            })
    }

    /// The class itself followed by each of its superclasses.
    pub(super) fn ancestors(&self, class: usize) -> impl Iterator<Item = usize> + '_ {
        std::iter::successors(Some(class), move |&current| self.superclasses[current])
    }

    pub(super) fn find_method(&self, class: usize, name: &str) -> Option<(usize, &Signature<'a>)> {
        self.ancestors(class).find_map(|owner| {
            self.methods[owner]
                .get(name)
                .map(|signature| (owner, signature))
        })
    }

    pub(super) fn find_field(&self, class: usize, name: &str) -> Option<(usize, &FieldInfo<'a>)> {
        self.ancestors(class).find_map(|owner| {
            self.class_fields[owner]
                .iter()
                .find(|field| field.name == name)
                .map(|field| (owner, field))
        })
    }

    pub(super) fn is_abstract_class(&self, class: usize) -> bool {
        self.abstract_classes[class]
    }

    /// Whether an object of `class` can be used where `expected` is required.
    pub(super) fn object_is(&self, class: usize, expected: Kind) -> bool {
        match expected {
            Kind::Object(target) => self.ancestors(class).any(|owner| owner == target),
            Kind::Interface(target) => self.ancestors(class).any(|owner| {
                self.implementations[owner].iter().any(|&kind| {
                    self.interface_ancestors(kind)
                        .iter()
                        .any(|&ancestor| self.types.interface_root(ancestor) == Some(target))
                })
            }),
            Kind::Java(index) => {
                self.ancestors(class).any(|owner| {
                    self.java_interfaces(owner).iter().any(|kind| {
                        matches!(kind, Kind::Java(actual) if self.types.is_java_subclass(
                            &self.types.java_name(*actual), &self.types.java_name(index)))
                    })
                }) || self.java_root(class).is_some_and(|root| {
                    self.types
                        .is_java_subclass(&root, &self.types.java_name(index))
                })
            }
            _ => false,
        }
    }

    /// The type that a value of class type `actual` has as Mammoth interface
    /// `root`, with class arguments substituted, such as `Producer<Int>` for a
    /// `Box<Int>` whose class implements `Producer<T>`.
    pub(super) fn interface_view(&self, actual: Kind, root: usize) -> Option<Kind> {
        let find = |kind: Kind| {
            self.interface_ancestors(kind)
                .into_iter()
                .find(|&ancestor| self.types.interface_root(ancestor) == Some(root))
        };
        if self.types.interface_root(actual.non_null()).is_some() {
            return find(actual.non_null());
        }
        let class = self.types.native_class(actual.non_null())?;
        let mut map = self.types.class_map(actual.non_null());
        let mut current = Some(class);
        while let Some(owner) = current {
            for &kind in &self.implementations[owner] {
                if let Some(view) = find(self.types.replace_parameters(kind, &map)) {
                    return Some(view);
                }
            }
            current = self.superclasses[owner];
            if let Some(parent) = current {
                map = self.parent_map(owner, parent, &map);
            }
        }
        None
    }

    /// The interface type `kind` followed by every interface it extends,
    /// directly or indirectly, with type arguments substituted. Each
    /// interface appears once, at its first occurrence in depth-first order.
    pub(super) fn interface_ancestors(&self, kind: Kind) -> Vec<Kind> {
        let mut found: Vec<Kind> = Vec::new();
        let mut stack = vec![kind];
        while let Some(current) = stack.pop() {
            let Some(root) = self.types.interface_root(current) else {
                continue;
            };
            if found
                .iter()
                .any(|&other| self.types.interface_root(other) == Some(root))
            {
                continue;
            }
            found.push(current);
            let map = self.types.class_map(current);
            for &parent in self.interface_parents[root].iter().rev() {
                stack.push(self.types.replace_parameters(parent, &map));
            }
        }
        found
    }

    /// The Java interfaces that Mammoth interface type `kind` extends,
    /// directly or through its Mammoth ancestors, with arguments substituted.
    pub(super) fn java_parents(&self, kind: Kind) -> Vec<Kind> {
        let mut found = Vec::new();
        for ancestor in self.interface_ancestors(kind) {
            let root = self.types.interface_root(ancestor).unwrap();
            let map = self.types.class_map(ancestor);
            for &parent in &self.interface_parents[root] {
                if matches!(parent, Kind::Java(_)) {
                    let parent = self.types.replace_parameters(parent, &map);
                    if !found.contains(&parent) {
                        found.push(parent);
                    }
                }
            }
        }
        found
    }

    /// Every Java interface that `class` lists or reaches through the Mammoth
    /// interfaces it lists.
    pub(super) fn java_interfaces(&self, class: usize) -> Vec<Kind> {
        let mut found = Vec::new();
        for &kind in &self.implementations[class] {
            let reached = match kind {
                Kind::Java(_) => vec![kind],
                _ => self.java_parents(kind),
            };
            for kind in reached {
                if !found.contains(&kind) {
                    found.push(kind);
                }
            }
        }
        found
    }

    /// The interface among `kind` and its parents that declares method
    /// `name`, viewed with substituted type arguments, and that declaration.
    pub(super) fn interface_method(
        &self,
        kind: Kind,
        name: &str,
    ) -> Option<(Kind, &Signature<'a>)> {
        self.interface_method_views(self.interface_ancestors(kind), name)
            .into_iter()
            .next()
            .map(|owner| {
                let root = self.types.interface_root(owner).unwrap();
                (owner, &self.interfaces[root][name])
            })
    }

    /// Keep the most specific declarations of a method. A child declaration
    /// shadows its ancestors regardless of the order of the parent list.
    fn interface_method_views(&self, views: Vec<Kind>, name: &str) -> Vec<Kind> {
        let declarations: Vec<_> = views
            .into_iter()
            .filter(|&view| {
                let root = self.types.interface_root(view).unwrap();
                self.interfaces[root].contains_key(name)
            })
            .collect();
        let mut found = Vec::new();
        for &view in &declarations {
            let root = self.types.interface_root(view).unwrap();
            if !found
                .iter()
                .any(|&other| self.types.interface_root(other) == Some(root))
                && !declarations.iter().any(|&other| {
                    self.types.interface_root(other) != Some(root)
                        && self
                            .interface_ancestors(other)
                            .iter()
                            .any(|&ancestor| self.types.interface_root(ancestor) == Some(root))
                })
            {
                found.push(view);
            }
        }
        found
    }

    fn class_interface_methods(&self, class: usize, name: &str) -> Vec<Kind> {
        let views = self
            .ancestors(class)
            .flat_map(|owner| self.implementations[owner].iter().copied())
            .flat_map(|kind| self.interface_ancestors(kind))
            .collect();
        self.interface_method_views(views, name)
    }

    /// The Java interface, viewed with this object's type arguments, through
    /// which a Mammoth object calls a Java default method it inherits, such
    /// as `andThen` on a class implementing `Function<Int, Int>`. A method
    /// that the Java superclass chain has is reached through that chain.
    fn java_interface_receiver(&self, kind: Kind, class: usize, name: &str) -> Option<Kind> {
        if self
            .java_root(class)
            .is_some_and(|root| !self.types.java_inherited_methods(&root, name).is_empty())
        {
            return None;
        }
        self.java_generic_views(kind.non_null())
            .into_iter()
            .filter(|view| matches!(view, Kind::Java(_)))
            .find(|&view| {
                self.types.runtime_class_name(view).is_some_and(|binary| {
                    self.types.is_java_interface(&binary)
                        && !self.types.java_methods(&binary, name, false).is_empty()
                })
            })
    }

    /// The receiver type through which method `name` is called: an interface
    /// value calls an inherited method through the parent that declares it,
    /// and an object calls an inherited default method through its interface.
    pub(super) fn method_receiver(&self, kind: Kind, name: &str) -> Kind {
        let name = name.split('<').next().unwrap_or(name);
        if self.types.interface_root(kind).is_none() {
            return match self.types.native_class(kind.non_null()) {
                Some(class) if self.find_method(class, name).is_none() => self
                    .default_method(class, name)
                    .and_then(|root| self.interface_view(kind.non_null(), root))
                    .map(|view| {
                        self.interface_method(view, name)
                            .map_or(view, |(owner, _)| owner)
                    })
                    .or_else(|| self.java_interface_receiver(kind, class, name))
                    .unwrap_or(kind),
                _ => kind,
            };
        }
        if let Some((owner, _)) = self.interface_method(kind, name) {
            return owner;
        }
        // A method inherited from a Java parent interface.
        self.java_parents(kind)
            .into_iter()
            .find(|&parent| {
                self.types
                    .runtime_class_name(parent)
                    .is_some_and(|class| !self.types.java_methods(&class, name, false).is_empty())
            })
            .unwrap_or(kind)
    }

    /// Whether interface `root` gives method `name` a default body.
    pub(super) fn has_default(&self, root: usize, name: &str) -> bool {
        self.interface_declarations[root]
            .methods
            .iter()
            .any(|method| method.name.text == name && !method.is_abstract)
    }

    /// The interface whose default body class `class` inherits for method
    /// `name`, when the class and its superclasses do not declare it.
    pub(super) fn default_method(&self, class: usize, name: &str) -> Option<usize> {
        let owners = self.class_interface_methods(class, name);
        if owners.len() != 1 {
            return None;
        }
        let root = self.types.interface_root(owners[0])?;
        self.has_default(root, name).then_some(root)
    }

    /// The type a value of class type `actual` has as its ancestor class
    /// `target`, such as `Base<Int>` for a `Child` that extends `Base<Int>`.
    pub(super) fn class_view(&self, actual: Kind, target: usize) -> Option<Kind> {
        let class = self.types.native_class(actual.non_null())?;
        let mut map = self.types.class_map(actual.non_null());
        let mut current = class;
        while current != target {
            let parent = self.superclasses[current]?;
            map = self.parent_map(current, parent, &map);
            current = parent;
        }
        Some(if current == class {
            actual.non_null()
        } else {
            self.types
                .replace_parameters(self.types.self_kind(target), &map)
        })
    }

    /// Substitutions for the type parameters of class `owner` when a member it
    /// declares is used through a receiver of type `receiver`.
    pub(super) fn member_map(
        &self,
        receiver: Kind,
        owner: usize,
    ) -> std::collections::HashMap<Kind, Kind> {
        self.class_view(receiver, owner)
            .map(|view| self.types.class_map(view))
            .unwrap_or_default()
    }

    /// Whether a member declared in `owner` has types that depend on
    /// substitutions when used from class `class`.
    pub(super) fn generic_owner(&self, class: usize, owner: usize) -> bool {
        class != owner && !self.types.class_parameters.borrow()[owner].is_empty()
    }

    /// Infers type parameters in `pattern` from `actual`, first viewing a
    /// class value as the generic interface or superclass the pattern names.
    pub(super) fn infer_from(
        &self,
        pattern: Kind,
        actual: Kind,
        map: &mut std::collections::HashMap<Kind, Kind>,
    ) {
        let actual = self.supertype_view(pattern, actual);
        self.types.infer_parameters(pattern, actual, map);
    }

    /// Checks that `is`, `as`, or `as?` against `target` can be decided at
    /// runtime for a value of static type `subject`. Type arguments are
    /// erased, so a parameterized target is allowed only when the subject's
    /// type already implies every argument, as `Box<Int>` is implied for a
    /// `Source<Int>` when `Box<T>` implements `Source<T>`.
    pub(super) fn check_runtime_target(
        &self,
        target: Kind,
        subject: Kind,
        annotation: Name<'_>,
    ) -> Result<(), Error> {
        if self.implied_target(target, subject) {
            return Ok(());
        }
        if let Kind::Applied(index) | Kind::AppliedInterface(index) = target.non_null()
            && self
                .types
                .application(index)
                .arguments
                .iter()
                .all(|&argument| self.types.is_star(argument))
        {
            // `value is Box<*>` only asks for the erased class.
            return Ok(());
        }
        if matches!(target.non_null(), Kind::Java(_)) {
            let arguments = self.types.type_arguments(target.non_null());
            if !arguments.is_empty() && arguments.iter().all(|&item| self.types.is_star(item)) {
                return Ok(());
            }
        }
        if self.types.contains_parameter(target) {
            return Err(at(
                annotation,
                "Type parameters are erased at runtime. Use a concrete type for is, as, or as?.",
            ));
        }
        if (self.java_parameterized(target) || matches!(target.non_null(), Kind::MutableList(_)))
            && !self.implied_java_target(target, subject)
        {
            return Err(at(
                annotation,
                "Java type arguments are erased at runtime. Check only types whose arguments follow from the value's type, such as ArrayList<Text> for a Collection<Text>, or check the raw type such as ArrayList.",
            ));
        }
        if self.types.contains_application(target) {
            return Err(at(
                annotation,
                "Class type arguments are erased at runtime. Check only types whose arguments follow from the value's type, such as Box<Int> for a Source<Int>.",
            ));
        }
        Ok(())
    }

    /// Whether `kind` is, or contains, a Java class with type arguments.
    fn java_parameterized(&self, kind: Kind) -> bool {
        match kind.non_null() {
            kind @ Kind::Java(_) => !self.types.type_arguments(kind).is_empty(),
            Kind::List(list) | Kind::MutableList(list) => self.java_parameterized(list.element()),
            _ => false,
        }
    }

    /// Whether every type argument of the Java `target` follows from a value
    /// of static type `subject`: either `subject` already is that type (an
    /// upcast, ArrayList<Text> as Collection<Text>), or `target`'s class seen
    /// as `subject`'s class determines each argument and agrees with it (a
    /// downcast, Collection<Text> as ArrayList<Text>).
    fn implied_java_target(&self, target: Kind, subject: Kind) -> bool {
        let types = &self.types;
        let (target, subject) = (target.non_null(), subject.non_null());
        if types.same_kind(target, subject) {
            return true;
        }
        let (Some(target_class), Some(subject_class)) = (
            types.runtime_class_name(target),
            types.runtime_class_name(subject),
        ) else {
            return false;
        };
        let parameters = types.class_type_parameters(&target_class);
        let arguments = types.type_arguments(target);
        if parameters.len() != arguments.len() {
            return false;
        }
        let agrees = |map: &HashMap<String, Kind>, expected: &HashMap<String, Kind>| {
            !expected.is_empty()
                && expected.iter().all(|(name, &argument)| {
                    map.get(name)
                        .is_some_and(|&actual| types.same_kind(actual, argument))
                })
        };
        if types.is_java_subclass(&subject_class, &target_class) {
            let expected: HashMap<String, Kind> = parameters.into_iter().zip(arguments).collect();
            return self.java_generic_views(subject).into_iter().any(|view| {
                types
                    .runtime_class_name(view)
                    .is_some_and(|class| types.is_java_subclass(&class, &target_class))
                    && agrees(&types.substitution(view, &target_class), &expected)
            });
        }
        if !types.is_java_subclass(&target_class, &subject_class) {
            return false;
        }
        // Stand-ins for the target's arguments, to see which ones the
        // subject's type determines.
        let markers = types.cast_markers(arguments.len());
        let generic = Kind::Java(types.java_generic(&target_class, markers.clone()));
        let seen = types.substitution(generic, &subject_class);
        if markers
            .iter()
            .any(|&marker| !seen.values().any(|&kind| types.mentions(kind, marker)))
        {
            return false;
        }
        let replace: HashMap<Kind, Kind> = markers.into_iter().zip(arguments).collect();
        let derived: HashMap<String, Kind> = seen
            .into_iter()
            .map(|(name, kind)| (name, types.replace_parameters(kind, &replace)))
            .collect();
        let actual = types.substitution(subject, &subject_class);
        agrees(&actual, &derived) && actual.len() == derived.len()
    }

    /// Whether a parameterized `target` has exactly the type arguments that a
    /// value of static type `subject` would have as that class or interface.
    fn implied_target(&self, target: Kind, subject: Kind) -> bool {
        let target = target.non_null();
        let (parameters, generic) = match target {
            Kind::Applied(index) => {
                let class = self.types.application(index).class;
                (
                    self.types.class_parameters.borrow()[class].clone(),
                    self.types.self_kind(class),
                )
            }
            Kind::AppliedInterface(index) => {
                let interface = self.types.application(index).class;
                let parameters = self.types.interface_parameters.borrow()[interface].clone();
                let arguments = parameters.iter().map(|&(_, kind)| kind).collect();
                (parameters, self.types.apply_interface(interface, arguments))
            }
            _ => return false,
        };
        let subject = subject.non_null();
        let view = self.supertype_view(subject, generic);
        if view == generic {
            return false;
        }
        let mut map = std::collections::HashMap::new();
        self.types.infer_parameters(view, subject, &mut map);
        let written = self.types.application(match target {
            Kind::Applied(index) | Kind::AppliedInterface(index) => index,
            _ => unreachable!(),
        });
        parameters
            .iter()
            .zip(written.arguments)
            .all(|(&(_, parameter), argument)| {
                map.get(&parameter)
                    .is_some_and(|&inferred| self.types.same_kind(inferred, argument))
            })
    }

    pub(super) fn supertype_view(&self, pattern: Kind, actual: Kind) -> Kind {
        let nullable = matches!(actual, Kind::Nullable(_));
        let view = match pattern.non_null() {
            Kind::AppliedInterface(_) => {
                self.interface_view(actual, self.types.interface_root(pattern).unwrap())
            }
            Kind::Applied(_) if self.types.native_class(actual.non_null()).is_some() => {
                self.class_view(actual, self.types.native_class(pattern.non_null()).unwrap())
            }
            // A Mammoth class seen as a parameterized Java interface it implements.
            Kind::Java(_) if self.types.native_class(actual.non_null()).is_some() => {
                let owner = self.types.runtime_class_name(pattern.non_null());
                owner.and_then(|owner| {
                    let parameters = self.types.class_type_parameters(&owner);
                    if parameters.is_empty() {
                        return None;
                    }
                    self.java_generic_views(actual)
                        .into_iter()
                        .find_map(|view| {
                            let map = self.types.substitution(view, &owner);
                            let arguments = parameters
                                .iter()
                                .map(|name| map.get(name).copied())
                                .collect::<Option<Vec<_>>>()?;
                            Some(Kind::Java(self.types.java_generic(&owner, arguments)))
                        })
                })
            }
            _ => None,
        };
        match view {
            Some(view) if nullable => view.nullable().unwrap_or(view),
            Some(view) => view,
            None => actual,
        }
    }

    /// Type-parameter substitutions for `parent` as seen from `child`, given the
    /// substitutions already known for `child`.
    pub(super) fn parent_map(
        &self,
        child: usize,
        parent: usize,
        map: &std::collections::HashMap<Kind, Kind>,
    ) -> std::collections::HashMap<Kind, Kind> {
        match self.superclass_kinds[child] {
            Some(kind) if self.types.native_class(kind) == Some(parent) => {
                let applied = self.types.replace_parameters(kind, map);
                self.types.class_map(applied)
            }
            _ => std::collections::HashMap::new(),
        }
    }

    /// Whether `class` has inner classes, which reach its private members.
    pub(super) fn has_inner_classes(&self, class: &crate::syntax::Class<'_>) -> bool {
        self.class_declarations
            .iter()
            .any(|other| other.inner_of == Some(class.name.text))
    }

    /// The parameterized Java superclass at the top of the superclass chain
    /// of a receiver of Mammoth type `receiver`, with the receiver's type
    /// arguments substituted, such as `ArrayList<Text>`.
    pub(super) fn java_super_view(&self, receiver: Kind) -> Option<Kind> {
        let receiver = receiver.non_null();
        let class = self.types.native_class(receiver)?;
        let top = self.ancestors(class).last()?;
        let java = Kind::Java(self.java_supers[top]?);
        let view = self.class_view(receiver, top)?;
        Some(
            self.types
                .replace_parameters(java, &self.types.class_map(view)),
        )
    }

    /// The Java class at the top of a Mammoth class's superclass chain.
    pub(super) fn java_root(&self, class: usize) -> Option<String> {
        let top = self.ancestors(class).last()?;
        Some(
            self.java_supers[top]
                .map(|index| self.types.java_name(index))
                .unwrap_or_else(|| "java/lang/Object".into()),
        )
    }

    /// Checks that each interface inherits every parent consistently: one
    /// parameterization per interface, matching overrides, and no unresolved
    /// method name shared by unrelated parents.
    pub(super) fn check_interfaces(&self, own: &[usize]) -> Result<(), Error> {
        for &index in own {
            let declaration = self.interface_declarations[index];
            let mut seen: Vec<Kind> = Vec::new();
            for (&parent, &written) in self.interface_parents[index]
                .iter()
                .zip(&declaration.extends)
            {
                let views = self.interface_ancestors(parent);
                for &view in &views {
                    let root = self.types.interface_root(view);
                    if let Some(&other) = seen
                        .iter()
                        .find(|&&other| self.types.interface_root(other) == root)
                    {
                        if !self.types.same_kind(other, view) {
                            return Err(at(
                                written,
                                "An interface cannot inherit the same interface with different type arguments.",
                            ));
                        }
                        continue;
                    }
                    seen.push(view);
                }
            }
            for method in &declaration.methods {
                let inherited: Vec<_> = seen
                    .iter()
                    .copied()
                    .filter(|&view| {
                        let root = self.types.interface_root(view).unwrap();
                        self.interfaces[root].contains_key(method.name.text)
                    })
                    .collect();
                if !inherited.is_empty() && method.is_abstract {
                    return Err(at(
                        method.name,
                        "This method is already declared by a parent interface.",
                    ));
                }
                if inherited.is_empty() && method.is_override {
                    return Err(at(
                        method.name,
                        "This method does not override a method from a parent interface.",
                    ));
                }
                for view in inherited {
                    let root = self.types.interface_root(view).unwrap();
                    let map = self.types.class_map(view);
                    let expected =
                        self.substitute_signature(&self.interfaces[root][method.name.text], &map);
                    if !same_signature(
                        &self.types,
                        &self.interfaces[index][method.name.text],
                        &expected,
                    ) {
                        return Err(at(
                            method.name,
                            "This method must have the same parameter and result types as the interface method it overrides.",
                        ));
                    }
                    let actual = &self.interfaces[index][method.name.text];
                    let mut bound_map = map;
                    bound_map.extend(
                        expected
                            .type_parameters
                            .iter()
                            .zip(&actual.type_parameters)
                            .map(|((_, from), (_, to))| (*from, *to)),
                    );
                    for ((_, parent), (_, child)) in
                        expected.type_parameters.iter().zip(&actual.type_parameters)
                    {
                        let inherited = self.types.all_bounds(*parent);
                        let own = self.types.all_bounds(*child);
                        if inherited.len() != own.len()
                            || inherited.iter().zip(&own).any(|(&from, &to)| {
                                !self
                                    .types
                                    .same_kind(self.types.replace_parameters(from, &bound_map), to)
                            })
                        {
                            return Err(at(
                                method.name,
                                "An overriding interface method must keep its parent's type parameter bounds.",
                            ));
                        }
                    }
                }
            }
            for &view in &seen {
                let root = self.types.interface_root(view).unwrap();
                for method in &self.interface_declarations[root].methods {
                    if !self.interfaces[index].contains_key(method.name.text)
                        && self
                            .interface_method_views(seen.clone(), method.name.text)
                            .len()
                            > 1
                    {
                        return Err(at(
                            declaration.name,
                            "Two parent interfaces declare a method with this name. Declare an override with a body in the child interface.",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Whether an override differs only by returning a subtype, which a
    /// bridge method keeps callable through the parent's type.
    fn narrows_result(&self, signature: &Signature<'a>, overridden: &Signature<'a>) -> bool {
        let (Some(actual), Some(parent)) = (signature.result, overridden.result) else {
            return false;
        };
        if !actual.is_reference() || !parent.is_reference() || !self.fits(actual, parent) {
            return false;
        }
        let mut relaxed = overridden.clone();
        relaxed.result = Some(actual);
        same_signature(&self.types, signature, &relaxed)
    }

    /// Checks overrides, abstract methods, and interfaces after the project
    /// resolver has rejected inheritance cycles.
    pub(super) fn check_classes(&self, program: &Program<'a>) -> Result<(), Error> {
        for (&index, class) in self.own_classes.iter().zip(&program.classes) {
            let parent = self.superclasses[index];
            if let (Some(parent), Some((name, _))) = (parent, &class.extends)
                && self.enum_values[parent].is_some()
            {
                return Err(at(*name, "An enum cannot be extended."));
            }
            if let (Some(parent), Some((name, _))) = (parent, &class.extends)
                && self.class_declarations[parent].is_object
            {
                return Err(at(*name, "An object cannot be extended."));
            }
            if let (Some(parent), Some((name, _))) = (parent, &class.extends)
                && self.class_declarations[parent].is_sealed
                && !self.own_classes.contains(&parent)
            {
                return Err(at(
                    *name,
                    "A sealed class can only be extended in the file that declares it.",
                ));
            }
            for (&kind, name) in self.implementations[index].iter().zip(&class.implements) {
                if let Some(interface) = self.types.interface_root(kind)
                    && self.interface_declarations[interface].is_sealed
                    && !self.own_interfaces.contains(&interface)
                {
                    return Err(at(
                        *name,
                        "A sealed interface can only be implemented in the file that declares it.",
                    ));
                }
            }
            for method in &class.methods {
                let signature = &self.methods[index][method.name.text];
                self.check_generic_java_override(index, method, signature)?;
                if method.protected
                    && let Some(root) = self.java_root(index)
                    && self
                        .types
                        .java_inherited_methods(&root, method.name.text)
                        .iter()
                        .any(|inherited| {
                            inherited.access & crate::classfile::ACC_PUBLIC != 0
                                && self.java_compatible(signature, inherited)
                        })
                {
                    return Err(at(
                        method.name,
                        "A protected method cannot override a public Java method. Keep it public.",
                    ));
                }
                if method.exported {
                    let root = self
                        .java_root(index)
                        .unwrap_or_else(|| "java/lang/Object".into());
                    let parameters = signature.descriptor.split_once(')').unwrap().0;
                    for inherited in self.types.java_inherited_methods(&root, method.name.text) {
                        if inherited.descriptor.split_once(')').unwrap().0 == parameters {
                            if inherited.access & crate::classfile::ACC_FINAL != 0 {
                                return Err(at(
                                    method.name,
                                    "An exported method cannot override a final Java method.",
                                ));
                            }
                            if !self.java_compatible(signature, &inherited) {
                                return Err(at(
                                    method.name,
                                    "An exported method must match the inherited Java method's parameter and result types.",
                                ));
                            }
                        }
                    }
                    if self
                        .types
                        .java_methods(&root, method.name.text, true)
                        .iter()
                        .any(|inherited| {
                            inherited.descriptor.split_once(')').unwrap().0 == parameters
                        })
                    {
                        return Err(at(
                            method.name,
                            "An exported instance method cannot replace a static Java method.",
                        ));
                    }
                }
                let inherited =
                    parent.and_then(|parent| self.find_method(parent, method.name.text));
                if let Some((owner, overridden)) = inherited {
                    let (parent_private, _) = self.method_flags[owner][method.name.text];
                    if parent_private {
                        continue;
                    }
                    if method.private {
                        return Err(at(
                            method.name,
                            "A private method cannot override a method from the superclass.",
                        ));
                    }
                    if method.protected && !self.protected_methods[owner].contains(method.name.text)
                    {
                        return Err(at(
                            method.name,
                            "A protected method cannot override a public method. Keep it public.",
                        ));
                    }
                    if signature.type_parameters.len() == overridden.type_parameters.len()
                        && !self.same_method_bounds(
                            signature,
                            overridden,
                            &std::collections::HashMap::new(),
                            &self.ancestor_map(index, owner),
                        )
                    {
                        return Err(at(
                            method.name,
                            "An overriding class method must keep its parent's type parameter bounds.",
                        ));
                    }
                    let overridden = &self.inherited_signature(index, owner, overridden);
                    if !same_signature(&self.types, signature, overridden)
                        && !self.narrows_result(signature, overridden)
                    {
                        return Err(at(
                            method.name,
                            "This method must have the same parameter and result types as the method it overrides.",
                        ));
                    }
                } else if method.is_override
                    && !self.interface_declares(index, method.name.text)
                    && !self.java_root(index).is_some_and(|root| {
                        !self
                            .types
                            .java_inherited_methods(&root, method.name.text)
                            .is_empty()
                    })
                {
                    return Err(at(
                        method.name,
                        "This method does not override a method from a superclass or interface.",
                    ));
                }
            }
            if let Some(parent) = self.superclasses[index] {
                for method in class.methods.iter().filter(|method| method.is_override) {
                    let setter = format!("{}$set", method.name.text);
                    if self.find_method(parent, &setter).is_some()
                        && !class.methods.iter().any(|item| item.name.text == setter)
                    {
                        return Err(at(
                            method.name,
                            "A let property cannot override a var property, because callers could still assign it.",
                        ));
                    }
                }
            }
            if !class.is_abstract
                && let Some(root) = self.java_root(index)
            {
                for required in self.types.java_abstract_methods(&root) {
                    let implemented =
                        self.find_method(index, &required.name)
                            .is_some_and(|(owner, found)| {
                                !self.method_flags[owner][required.name.as_str()].1
                                    && self.java_compatible(found, &required)
                            });
                    if !implemented {
                        return Err(at(
                            class.name,
                            "This class must implement every abstract method of its Java superclass.",
                        ));
                    }
                }
            }
            if !class.is_abstract {
                for owner in self.ancestors(index) {
                    for (name, &(_, is_abstract)) in &self.method_flags[owner] {
                        if !is_abstract {
                            continue;
                        }
                        let implemented = self
                            .find_method(index, name)
                            .is_some_and(|(found, _)| !self.method_flags[found][name].1);
                        if !implemented {
                            return Err(at(
                                class.name,
                                "This class must implement every abstract method of its superclass.",
                            ));
                        }
                    }
                }
            }
            let mut listed: Vec<(Kind, Name<'a>)> = Vec::new();
            for (&kind, &name) in self.implementations[index].iter().zip(&class.implements) {
                let views = match kind {
                    Kind::Interface(_) | Kind::AppliedInterface(_) => {
                        let mut views = self.interface_ancestors(kind);
                        for parent in self.java_parents(kind) {
                            if !self.implementations[index].contains(&parent) {
                                views.push(parent);
                            }
                        }
                        views
                    }
                    _ => vec![kind],
                };
                for view in views {
                    let root = self.types.interface_root(view);
                    let inherited = root.and_then(|root| {
                        let parent = self.superclass_kinds[index]
                            .or(self.superclasses[index].map(Kind::Object))?;
                        self.interface_view(parent, root)
                    });
                    let earlier = listed
                        .iter()
                        .map(|&(other, _)| other)
                        .chain(inherited)
                        .find(|&other| root.is_some() && self.types.interface_root(other) == root);
                    match earlier {
                        Some(other) if self.types.same_kind(other, view) => {}
                        Some(_) => {
                            return Err(at(
                                name,
                                "A class cannot implement the same interface with different type arguments.",
                            ));
                        }
                        None => {}
                    }
                    if !listed.iter().any(|&(other, _)| {
                        root.is_some() && self.types.interface_root(other) == root
                    }) {
                        listed.push((view, name));
                    }
                }
            }
            for (kind, name) in listed {
                if let Kind::Java(java) = kind
                    && !self.java_classes.contains_key(&java)
                {
                    self.check_java_interface(index, java, name)?;
                    continue;
                }
                let map = self.types.class_map(kind);
                let (required_methods, required) = match kind {
                    Kind::Interface(_) | Kind::AppliedInterface(_) => {
                        let interface = self.types.interface_root(kind).unwrap();
                        (
                            &self.interface_declarations[interface].methods,
                            &self.interfaces[interface],
                        )
                    }
                    Kind::Java(java) => (
                        &self.java_classes[&java].declaration.methods,
                        &self.java_classes[&java].methods,
                    ),
                    _ => unreachable!("implements only lists interfaces"),
                };
                for method in required_methods {
                    let expected = &self.substitute_signature(&required[method.name.text], &map);
                    let Some((owner, found)) = self.find_method(index, method.name.text) else {
                        if matches!(kind, Kind::Interface(_) | Kind::AppliedInterface(_)) {
                            if self.default_method(index, method.name.text).is_some() {
                                continue;
                            }
                            let owners = self.class_interface_methods(index, method.name.text);
                            if owners.len() > 1
                                && owners.iter().any(|&owner| {
                                    self.has_default(
                                        self.types.interface_root(owner).unwrap(),
                                        method.name.text,
                                    )
                                })
                            {
                                return Err(at(
                                    name,
                                    "This class inherits two default methods with the same name. Declare the method in the class.",
                                ));
                            }
                        }
                        return Err(at(
                            name,
                            "This class is missing a method required by the interface.",
                        ));
                    };
                    if found.type_parameters.len()
                        == required[method.name.text].type_parameters.len()
                        && !self.same_method_bounds(
                            found,
                            &required[method.name.text],
                            &self.ancestor_map(index, owner),
                            &map,
                        )
                    {
                        return Err(at(
                            name,
                            "A method implementing an interface must keep its type parameter bounds.",
                        ));
                    }
                    let found = &self.inherited_signature(index, owner, found);
                    if !same_signature(&self.types, found, expected) {
                        let name = if self.own_classes.contains(&owner) {
                            self.class_declarations[owner]
                                .methods
                                .iter()
                                .find(|entry| entry.name.text == method.name.text)
                                .map_or(name, |entry| entry.name)
                        } else {
                            name
                        };
                        return Err(at(
                            name,
                            "This method's parameter and result types must match the interface.",
                        ));
                    }
                    if self.method_flags[owner][method.name.text].0 {
                        return Err(at(
                            name,
                            "A method required by an interface cannot be private.",
                        ));
                    }
                    if self.protected_methods[owner].contains(method.name.text) {
                        return Err(at(
                            name,
                            "A method required by an interface must be public, not protected.",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// A copy of a signature with type parameters replaced.
    pub(super) fn substitute_signature(
        &self,
        signature: &Signature<'a>,
        map: &std::collections::HashMap<Kind, Kind>,
    ) -> Signature<'a> {
        let mut copy = signature.clone();
        if map.is_empty() {
            return copy;
        }
        for (_, kind) in &mut copy.parameters {
            *kind = self.types.replace_parameters(*kind, map);
        }
        copy.result = copy
            .result
            .map(|kind| self.types.replace_parameters(kind, map));
        copy
    }

    /// The signature of a method declared in `owner` as seen from its subclass
    /// `class`, with the superclass type arguments substituted.
    pub(super) fn inherited_signature(
        &self,
        class: usize,
        owner: usize,
        signature: &Signature<'a>,
    ) -> Signature<'a> {
        let map = self.ancestor_map(class, owner);
        self.substitute_signature(signature, &map)
    }

    /// Substitutions for the type parameters of ancestor `owner` in terms of
    /// the type parameters of `class`.
    pub(super) fn ancestor_map(
        &self,
        class: usize,
        owner: usize,
    ) -> std::collections::HashMap<Kind, Kind> {
        let mut map = std::collections::HashMap::new();
        let mut current = class;
        while current != owner {
            let Some(parent) = self.superclasses[current] else {
                break;
            };
            map = self.parent_map(current, parent, &map);
            current = parent;
        }
        map
    }

    fn interface_declares(&self, class: usize, name: &str) -> bool {
        self.ancestors(class).any(|owner| {
            self.implementations[owner].iter().any(|kind| match kind {
                Kind::Interface(_) | Kind::AppliedInterface(_) => {
                    self.interface_method(*kind, name).is_some()
                }
                Kind::Java(index) => match self.java_classes.get(index) {
                    Some(declared) => declared.methods.contains_key(name),
                    None => self
                        .types
                        .java_methods(&self.types.java_name(*index), name, false)
                        .iter()
                        .any(|method| method.access & crate::classfile::ACC_STATIC == 0),
                },
                _ => false,
            })
        })
    }

    /// Checks that a class provides every abstract method of a Java interface
    /// found on the class path.
    fn check_java_interface(&self, class: usize, java: usize, name: Name<'a>) -> Result<(), Error> {
        let binary = self.types.java_name(java);
        for required in self.types.java_abstract_methods(&binary) {
            let Some((owner, found)) = self.find_method(class, &required.name) else {
                return Err(at(
                    name,
                    "This class is missing a method required by the Java interface.",
                ));
            };
            if owner != class || !self.java_compatible(found, &required) {
                let name = if self.own_classes.contains(&owner) {
                    self.class_declarations[owner]
                        .methods
                        .iter()
                        .find(|entry| entry.name.text == required.name)
                        .map_or(name, |entry| entry.name)
                } else {
                    name
                };
                return Err(at(
                    name,
                    "This method's parameter and result types must match the Java interface.",
                ));
            }
            let generic = required
                .signature
                .as_deref()
                .and_then(super::generics::parse_method_signature)
                .filter(|generic| generic.variables.is_empty());
            let view = self
                .java_generic_views(self.types.self_kind(class))
                .into_iter()
                .find(|&view| {
                    self.types
                        .runtime_class_name(view)
                        .is_some_and(|binary| self.types.is_java_subclass(&binary, &required.owner))
                });
            if let (Some(generic), Some(view)) = (generic, view)
                && !self.uses_java_arguments(
                    found,
                    &generic,
                    &self.types.substitution(view, &required.owner),
                )
            {
                let name = self.class_declarations[owner]
                    .methods
                    .iter()
                    .find(|entry| entry.name.text == required.name)
                    .map_or(name, |entry| entry.name);
                return Err(at(
                    name,
                    "This method must use the Java interface's type arguments.",
                ));
            }
            if self.method_flags[owner][required.name.as_str()].0 {
                return Err(at(
                    name,
                    "A method required by an interface cannot be private.",
                ));
            }
            if self.protected_methods[owner].contains(required.name.as_str()) {
                return Err(at(
                    name,
                    "A method required by an interface must be public, not protected.",
                ));
            }
        }
        Ok(())
    }

    /// Whether a Mammoth method can stand in for a Java interface method.
    /// Checks that a method overriding a method of a generic Java superclass
    /// uses the superclass's type arguments: `get(index: Int): Text` in a
    /// subclass of `ArrayList<Text>`, not `get(index: Int): Int`. The erased
    /// bridge would otherwise pass values of the wrong type to Java callers.
    fn check_generic_java_override(
        &self,
        class: usize,
        method: &crate::syntax::Function<'_>,
        signature: &Signature<'_>,
    ) -> Result<(), Error> {
        let Some(root) = self.java_root(class) else {
            return Ok(());
        };
        let Some(view) = self.java_super_view(self.types.self_kind(class)) else {
            return Ok(());
        };
        for inherited in self.types.java_inherited_methods(&root, method.name.text) {
            if !self.java_compatible(signature, &inherited) {
                continue;
            }
            if inherited.access & crate::classfile::ACC_FINAL != 0 {
                return Err(at(
                    method.name,
                    "This Java method is final, so it cannot be overridden.",
                ));
            }
            let Some(generic) = inherited
                .signature
                .as_deref()
                .and_then(super::generics::parse_method_signature)
            else {
                continue;
            };
            if !generic.variables.is_empty() {
                continue;
            }
            let map = self.types.substitution(view, &inherited.owner);
            if !self.uses_java_arguments(signature, &generic, &map) {
                return Err(at(
                    method.name,
                    "This method must use the generic Java superclass's type arguments in the method it overrides.",
                ));
            }
        }
        Ok(())
    }

    /// Whether a method's types are the Java method's types with the class's
    /// type arguments filled in. Erasure alone would accept `conv(a: Int)`
    /// for `conv(A)` in a `Conv<Text, Int>`.
    fn uses_java_arguments(
        &self,
        signature: &Signature<'_>,
        generic: &super::generics::MethodSignature,
        map: &HashMap<String, Kind>,
    ) -> bool {
        let parameters_match =
            generic
                .parameters
                .iter()
                .zip(&signature.parameters)
                .all(|(gtype, &(_, kind))| {
                    self.types.generic_kind(gtype, map).is_none_or(|expected| {
                        self.types.same_kind(expected.non_null(), kind.non_null())
                    })
                });
        let result_matches = match (
            self.types.generic_kind(&generic.result, map),
            signature.result,
        ) {
            (Some(expected), Some(actual)) if !expected.is_reference() => {
                self.types.same_kind(expected, actual)
            }
            (Some(expected), Some(actual)) => {
                self.fits(actual, expected.nullable().unwrap_or(expected))
            }
            _ => true,
        };
        parameters_match && result_matches
    }

    pub(super) fn java_compatible(
        &self,
        signature: &Signature<'_>,
        required: &super::interop::JavaMethod,
    ) -> bool {
        use super::interop::JType;
        let accepts = |kind: Kind, jtype: &JType| -> bool {
            if !kind.is_nullable_primitive() && self.types.kind_of(jtype) == Some(kind.non_null()) {
                return true;
            }
            let JType::Class(name) = jtype else {
                return false;
            };
            let actual = match kind.non_null() {
                Kind::Object(_) | Kind::Interface(_) | Kind::Applied(_) => {
                    return name == "java/lang/Object";
                }
                Kind::Int => "java/lang/Integer".to_owned(),
                Kind::Long => "java/lang/Long".to_owned(),
                Kind::Double => "java/lang/Double".to_owned(),
                Kind::Bool => "java/lang/Boolean".to_owned(),
                Kind::Char => "java/lang/Character".to_owned(),
                other => match self.types.runtime_class_name(other) {
                    Some(actual) => actual,
                    None => return false,
                },
            };
            self.types.is_java_subclass(&actual, name)
        };
        signature.parameters.len() == required.parameters.len()
            && signature
                .parameters
                .iter()
                .zip(&required.parameters)
                .all(|(&(_, kind), jtype)| accepts(kind, jtype))
            && match (signature.result, &required.result) {
                (None, JType::Void) => true,
                (Some(kind), jtype) => *jtype != JType::Void && accepts(kind, jtype),
                _ => false,
            }
    }

    /// Java interface methods (found on the class path) that `name` in `class`
    /// implements, each of which needs a bridge method.
    pub(super) fn java_bridges(&self, class: usize, name: &str) -> Vec<super::interop::JavaMethod> {
        let mut bridges: Vec<super::interop::JavaMethod> = Vec::new();
        if let Some(root) = self.java_root(class)
            && let Some(signature) = self.methods[class].get(name)
        {
            for method in self.types.java_inherited_methods(&root, name) {
                if method.access & crate::classfile::ACC_FINAL == 0
                    && self.java_compatible(signature, &method)
                {
                    bridges.push(method);
                }
            }
        }
        for kind in &self.java_interfaces(class) {
            let Kind::Java(index) = *kind else { continue };
            if self.java_classes.contains_key(&index) {
                continue;
            }
            for method in self
                .types
                .java_abstract_methods(&self.types.java_name(index))
            {
                if method.name == name
                    && !bridges
                        .iter()
                        .any(|existing| existing.descriptor == method.descriptor)
                {
                    bridges.push(method);
                }
            }
        }
        bridges
    }

    /// Classes ordered so that each superclass comes before its subclasses.
    pub(super) fn classes_by_depth(&self) -> Vec<usize> {
        let mut order = Vec::with_capacity(self.superclasses.len());
        let mut seen = vec![false; self.superclasses.len()];
        for class in 0..self.superclasses.len() {
            let mut chain = Vec::new();
            let mut current = Some(class);
            while let Some(index) = current {
                if seen[index] {
                    break;
                }
                seen[index] = true;
                chain.push(index);
                current = self.superclasses[index];
            }
            order.extend(chain.into_iter().rev());
        }
        order
    }

    /// Resolves each field's type from its annotation or its starting value.
    pub(super) fn resolve_fields(&self, index: usize) -> Result<Vec<FieldInfo<'a>>, Error> {
        let class = self.class_declarations[index];
        let mut fields: Vec<FieldInfo<'a>> = Vec::new();
        let mut names = HashSet::new();
        for field in &class.fields {
            let name = field.name;
            if name.text.len() > u16::MAX as usize || !names.insert(name.text) {
                return Err(at(name, "This field name is already in use."));
            }
            if self.methods[index].contains_key(name.text)
                && matches!(name.text, "toString" | "hashCode")
            {
                return Err(at(name, "This field name is already used by a method."));
            }
            let kind = if let Some(annotation) = field.annotation {
                resolve_type(&self.types, annotation)?
            } else {
                let value = field
                    .value
                    .as_ref()
                    .expect("fields without a type have a value");
                let scope = |variable: &str| -> Option<Kind> {
                    if let Some((_, annotation)) = class
                        .parameters
                        .iter()
                        .find(|(parameter, _)| parameter.text == variable)
                    {
                        return resolve_type(&self.types, *annotation).ok();
                    }
                    if let Some(field) = fields.iter().find(|field| field.name == variable) {
                        return Some(field.kind);
                    }
                    let parent = self.superclasses[index]?;
                    self.find_field(parent, variable)
                        .filter(|(_, field)| !field.private)
                        .map(|(owner, field)| {
                            self.types
                                .replace_parameters(field.kind, &self.ancestor_map(index, owner))
                        })
                };
                self.static_kind(value, &scope).ok_or_else(|| {
                    at(
                        name,
                        "Add a type to this field, such as `var total: Int = ...`.",
                    )
                })?
            };
            fields.push(FieldInfo {
                name: name.text,
                kind,
                mutable: field.mutable,
                private: field.private,
                protected: field.protected,
                lateinit: field.is_lateinit,
            });
        }
        Ok(fields)
    }

    /// Works out the type of a field's starting value without generating code.
    /// Returns `None` when the type depends on something this simple pass does
    /// not follow, so the program needs an explicit type.
    pub(super) fn static_kind(
        &self,
        expr: &Expr<'_>,
        scope: &dyn Fn(&str) -> Option<Kind>,
    ) -> Option<Kind> {
        Some(match expr {
            Expr::Named(_, value) => return self.static_kind(value, scope),
            Expr::Temp(kind, _) => *kind,
            Expr::StaticHelper(..)
            | Expr::Jump(_)
            | Expr::Progression(..)
            | Expr::Spread(_)
            | Expr::TryValue(..) => {
                return None;
            }
            Expr::InRange(..) => Kind::Bool,
            Expr::Int(..) => Kind::Int,
            Expr::Long(..) => Kind::Long,
            Expr::Char(..) => Kind::Char,
            Expr::Double(..) => Kind::Double,
            Expr::Bool(..) | Expr::Compare(..) | Expr::Logic(..) | Expr::Not(..) | Expr::Is(..) => {
                Kind::Bool
            }
            Expr::Cast(_, annotation, safe) => {
                let kind = resolve_type(&self.types, *annotation).ok()?;
                if *safe { kind.nullable()? } else { kind }
            }
            Expr::Text(..) => Kind::Text,
            Expr::Negate(_, inner) => self.static_kind(inner, scope)?,
            Expr::Variable(name) => scope(name.text)?,
            Expr::List(_, values) => Kind::List(ListKind::for_element(
                self.static_kind(values.first()?, scope)?,
            )?),
            Expr::Index(list, _) => match self.static_kind(list, scope)? {
                Kind::List(list) | Kind::MutableList(list) => list.element(),
                _ => return None,
            },
            Expr::Arithmetic(left, rest) => {
                let mut kind = self.static_kind(left, scope)?;
                if kind == Kind::Text {
                    return Some(Kind::Text);
                }
                for (_, right) in rest {
                    let right = self.static_kind(right, scope)?;
                    if !kind.is_numeric() || !right.is_numeric() {
                        return None;
                    }
                    if super::numeric_rank(right) > super::numeric_rank(kind) {
                        kind = right;
                    }
                }
                kind
            }
            Expr::Call(name, args) if name.text == "Any" && args.is_empty() => Kind::Any,
            Expr::Call(name, args) => {
                let (base, explicit) = super::native_generics::call_name(*name);
                if self.functions.contains_key(base.text) {
                    return self.generic_result(*name, args, scope);
                }
                if matches!(base.text, "listOf" | "mutableListOf") {
                    let element = match explicit.as_deref() {
                        Some([element]) => resolve_type(&self.types, *element).ok()?,
                        _ => self.static_kind(args.first()?, scope)?,
                    };
                    return Some(Kind::MutableList(ListKind::for_element(element)?));
                }
                if matches!(self.types.names.get(base.text), Some(Kind::Object(_))) {
                    return self.constructor_result(*name, args, scope);
                }
                if let Some(&kind) = self.types.names.get(name.text) {
                    return matches!(kind, Kind::Object(_) | Kind::Java(_)).then_some(kind);
                }
                if name.text.contains('<') {
                    return resolve_type(&self.types, *name).ok();
                }
                if let Some(kind @ Kind::Java(_)) = self.types.implicit_java_class(name.text) {
                    return Some(kind);
                }
                match name.text {
                    "mutableList" => Kind::MutableList(ListKind::for_element(
                        self.static_kind(args.first()?, scope)?,
                    )?),
                    "requireValue" => self.static_kind(args.first()?, scope)?.non_null(),
                    "input" => Kind::Text,
                    "sqrt" | "pow" | "random" => Kind::Double,
                    "randomInt" => Kind::Int,
                    "currentTimeMillis" => Kind::Long,
                    "abs" | "min" | "max" => self.static_kind(args.first()?, scope)?,
                    _ => return None,
                }
            }
            Expr::MethodCall(receiver, name, args) => match self.static_kind(receiver, scope)? {
                kind @ (Kind::Object(_)
                | Kind::Applied(_)
                | Kind::Interface(_)
                | Kind::AppliedInterface(_)) => {
                    return self.generic_member_result(kind, *name, args, scope);
                }
                Kind::Java(index) => {
                    self.java_classes
                        .get(&index)?
                        .methods
                        .get(name.text)?
                        .result?
                }
                Kind::List(_) | Kind::MutableList(_) if name.text == "size" => Kind::Int,
                kind => super::builtins::builtin_result(kind, name.text, args.len())??,
            },
            Expr::Field(receiver, name) => match self.static_kind(receiver, scope)? {
                kind @ Kind::Applied(_) => {
                    let class = self.types.native_class(kind)?;
                    let result = self.find_field(class, name.text)?.1.kind;
                    self.types
                        .replace_parameters(result, &self.types.class_map(kind))
                }
                Kind::Object(class) => self.find_field(class, name.text)?.1.kind,
                _ => return None,
            },
            Expr::Elvis(_, fallback) => self.static_kind(fallback, scope)?,
            Expr::When(_, _, branches) => {
                self.static_kind(branches.first()?.value.as_ref()?, scope)?
            }
            Expr::Null(_) | Expr::Range(..) | Expr::Lambda(_) | Expr::Safe(_) => return None,
        })
    }
}

fn same_signature(types: &Types<'_>, left: &Signature<'_>, right: &Signature<'_>) -> bool {
    if left.type_parameters.len() != right.type_parameters.len() {
        return false;
    }
    let map = right
        .type_parameters
        .iter()
        .zip(&left.type_parameters)
        .map(|((_, right), (_, left))| (*right, *left))
        .collect();
    let result = right
        .result
        .map(|kind| types.replace_parameters(kind, &map));
    types.same_result(left.result, result)
        && left.parameters.len() == right.parameters.len()
        && left
            .parameters
            .iter()
            .zip(&right.parameters)
            .all(|((_, left), (_, right))| {
                types.same_kind(*left, types.replace_parameters(*right, &map))
            })
}

impl<'a> Emitter<'a> {
    /// Finds a method on a Mammoth class or its superclasses, checking privacy.
    pub(super) fn find_class_method(
        &self,
        class: usize,
        name: Name<'_>,
    ) -> Result<&'a Signature<'a>, Error> {
        let symbols = self.symbols;
        let (owner, signature) = symbols.find_method(class, name.text).ok_or_else(|| {
            let known: Vec<&str> = symbols
                .ancestors(class)
                .chain(std::iter::once(class))
                .flat_map(|owner| symbols.methods[owner].keys().copied())
                .collect();
            at(
                name,
                super::suggest("This type has no method with that name.", name.text, &known),
            )
        })?;
        if symbols.method_flags[owner][name.text].0 && !self.can_reach_private(owner) {
            return Err(at(name, "This method is private to its class."));
        }
        self.check_protected_method(class, owner, name)?;
        Ok(signature)
    }

    /// Checks a call to a method declared in `owner` through a receiver whose
    /// static class is `class`. A protected method is visible only inside its
    /// class and subclasses, and, as the JVM requires, a subclass may call it
    /// only on receivers of that subclass (or a further subclass).
    pub(super) fn check_protected_method(
        &self,
        class: usize,
        owner: usize,
        name: Name<'_>,
    ) -> Result<(), Error> {
        let symbols = self.symbols;
        if !symbols.protected_methods[owner].contains(name.text)
            || self.current_class == Some(owner)
        {
            return Ok(());
        }
        let Some(current) = self
            .current_class
            .filter(|&current| symbols.ancestors(current).any(|ancestor| ancestor == owner))
        else {
            return Err(at(
                name,
                "This method is protected. Call it inside its class or a subclass.",
            ));
        };
        if !symbols.ancestors(class).any(|ancestor| ancestor == current) {
            return Err(at(
                name,
                "Call this protected method through this class or one of its subclasses.",
            ));
        }
        Ok(())
    }

    fn class_field(
        &self,
        class: usize,
        name: Name<'_>,
    ) -> Result<(usize, &'a FieldInfo<'a>), Error> {
        let symbols = self.symbols;
        let (owner, field) = symbols.find_field(class, name.text).ok_or_else(|| {
            let known: Vec<&str> = symbols
                .ancestors(class)
                .chain(std::iter::once(class))
                .flat_map(|owner| symbols.class_fields[owner].iter().map(|field| field.name))
                .collect();
            at(
                name,
                super::suggest("This class has no field with that name.", name.text, &known),
            )
        })?;
        if field.private && !self.can_reach_private(owner) {
            return Err(at(name, "This field is private to its class."));
        }
        if field.protected && self.current_class != Some(owner) {
            let Some(current) = self
                .current_class
                .filter(|&current| symbols.ancestors(current).any(|ancestor| ancestor == owner))
            else {
                return Err(at(
                    name,
                    "This field is protected. Access it inside its class or a subclass.",
                ));
            };
            // JVM protected access from a subclass requires a receiver whose
            // static type is that subclass (or a further subclass), not Base.
            if !symbols.ancestors(class).any(|ancestor| ancestor == current) {
                return Err(at(
                    name,
                    "Access this protected field through this class or one of its subclasses.",
                ));
            }
        }
        Ok((owner, field))
    }

    fn field_member(&mut self, kind: Kind, field: &'a FieldInfo<'a>) -> u16 {
        if let Some(&index) = self.field_members.get(&(kind, field.name)) {
            return index;
        }
        let class = self.object_class(kind);
        let descriptor = field.kind.descriptor(&self.symbols.types);
        let index = self.pool.member(9, class, field.name, &descriptor);
        self.field_members.insert((kind, field.name), index);
        index
    }

    /// `Type.class`: the `java.lang.Class` object for a type, typed
    /// `Class<Type>`, as Java APIs such as `EnumMap` and `EnumSet.allOf`
    /// expect. `Int.class` is `int.class`, as in annotation values.
    fn class_literal(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        name: Name<'a>,
        base: u16,
    ) -> Result<Kind, Error> {
        let erased_only = matches!(kind, Kind::Java(_))
            && !self.symbols.types.type_arguments(kind).is_empty()
            || matches!(kind, Kind::Applied(_) | Kind::AppliedInterface(_));
        if matches!(
            kind,
            Kind::Nullable(_) | Kind::TypeParameter(_) | Kind::Function(_)
        ) || erased_only
            || self.symbols.types.is_star(kind)
        {
            return Err(at(
                name,
                "Write .class after a plain class name, such as Text.class or ArrayList.class.",
            ));
        }
        if let Some(boxed) = Types::boxed_class(kind) {
            let class = self.pool_class(boxed);
            let reference = self.pool.member_for(9, class, "TYPE", "Ljava/lang/Class;");
            instruction(&mut method.code, 0xb2, reference); // getstatic
        } else {
            let class = self.instance_class(kind);
            super::load_constant(&mut method.code, class);
        }
        method.max_stack = method.max_stack.max(base + 1);
        Ok(Kind::Java(
            self.symbols
                .types
                .java_generic("java/lang/Class", vec![kind]),
        ))
    }

    pub(super) fn field_access(
        &mut self,
        method: &mut Method<'a>,
        receiver: &'a Expr<'a>,
        name: Name<'a>,
        base: u16,
    ) -> Result<Kind, Error> {
        if matches!(receiver, Expr::Variable(this) if this.text == "this") {
            self.check_property_ready(name)?;
        }
        if matches!(receiver, Expr::Variable(parent) if parent.text == "super") {
            let saved = method.snapshot();
            if let Ok(Some(kind)) = self.method_call(method, receiver, name, &[], base) {
                return Ok(kind);
            }
            method.restore(saved);
        }
        if name.text == "class"
            && let Expr::Variable(type_name) = receiver
            && method.binding(*type_name).is_none()
            && let Ok(kind) = resolve_type(&self.type_scope, *type_name)
        {
            return self.class_literal(method, kind, *type_name, base);
        }
        if let Some(class) = self.static_class(method, receiver)
            && let Some(kind) = self.read_static_field(method, class, name, base)?
        {
            return Ok(kind);
        }
        // `Sub.FIELD` for a static field of Mammoth class Sub's Java superclass.
        if let Some(class) = self.static_class(method, receiver)
            && let Some(kind) = self.inherited_java_static_field(
                method,
                class,
                name,
                base,
                self.current_class
                    .is_some_and(|current| self.symbols.ancestors(current).any(|a| a == class)),
            )?
        {
            return Ok(kind);
        }
        if let Some(class) = self.enum_owner(method, receiver) {
            return self.enum_value(method, class, name, base);
        }
        if let Some(class) = self.static_owner(method, receiver) {
            return self.java_static_field(method, &class, name, base);
        }
        if matches!(receiver, Expr::Variable(this) if this.text == "this") {
            self.check_initialized(name)?;
        }
        let allow_null = std::mem::take(&mut self.allow_null);
        let kind = self.expression(method, receiver, base)?;
        let result = self.field_on(method, kind, name, base, allow_null)?;
        if let Expr::Variable(owner) = receiver
            && let Some(binding) = method.binding(*owner)
            && !binding.mutable
            && let Some(narrowed) = method.field_type_fact(binding.slot, name.text)
            && let Some(class) = self.symbols.types.native_class(binding.kind.non_null())
            && self
                .symbols
                .find_field(class, name.text)
                .is_some_and(|(_, field)| !field.mutable && !field.lateinit)
        {
            self.cast_top(method, result, narrowed, base);
            return Ok(narrowed);
        }
        // A read-only field already checked against null needs no second check.
        if matches!(result, Kind::Nullable(_))
            && let Expr::Variable(owner) = receiver
            && let Some(binding) = method.binding(*owner)
            && !binding.mutable
            && method.field_is_checked(binding.slot, name.text)
            && let Some(class) = self.symbols.types.native_class(binding.kind.non_null())
            && self
                .symbols
                .find_field(class, name.text)
                .is_some_and(|(_, field)| !field.mutable && !field.lateinit)
        {
            let plain = result.non_null();
            if !plain.is_reference() {
                self.unbox_element(method, plain, base);
            }
            return Ok(plain);
        }
        Ok(result)
    }

    /// Reads field `name` of a receiver of type `kind` that is already on the stack.
    /// Reads a field, or failing that calls `name()` or `getName()`, so
    /// `list.size` and a Java bean's `person.name` work like properties.
    pub(super) fn field_on(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        name: Name<'a>,
        base: u16,
        allow_null: bool,
    ) -> Result<Kind, Error> {
        let start = method.code.len();
        match self.declared_field_on(method, kind, name, base, allow_null) {
            Err(error) if method.code.len() == start && !matches!(kind, Kind::Nullable(_)) => self
                .property_call(method, kind, name, base, allow_null)
                .unwrap_or(Err(error)),
            result => result,
        }
    }

    /// `None` when the receiver has no method that can stand for the property.
    fn property_call(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        name: Name<'a>,
        base: u16,
        allow_null: bool,
    ) -> Option<Result<Kind, Error>> {
        let mut capital = name.text.chars();
        let first = capital.next()?.to_ascii_uppercase();
        let getter: &'a str = Box::leak(format!("get{first}{}", capital.as_str()).into_boxed_str());
        for text in [name.text, getter] {
            let target = Name {
                text,
                offset: name.offset,
            };
            let attempt = method.snapshot();
            match self.call_method_on(method, kind, target, &[], base, allow_null) {
                Ok(Some(result)) => return Some(Ok(result)),
                _ => method.restore(attempt),
            }
        }
        let attempt = method.snapshot();
        match self.extension_on_stack(method, kind, name, &[], base, None) {
            Ok(Some(Some(result))) => Some(Ok(result)),
            _ => {
                method.restore(attempt);
                None
            }
        }
    }

    fn declared_field_on(
        &mut self,
        method: &mut Method<'a>,
        kind: Kind,
        name: Name<'a>,
        base: u16,
        allow_null: bool,
    ) -> Result<Kind, Error> {
        let kind = self.symbols.types.bound_view(kind);
        if matches!(kind, Kind::Java(_) | Kind::Any) {
            self.allow_null = allow_null;
            return self.java_instance_field(method, kind, name, base);
        }
        if let Some(class) = self.symbols.types.native_class(kind)
            && self.symbols.find_field(class, name.text).is_none()
            && let Some(result) =
                self.read_inherited_java_field(method, kind, class, name, base, allow_null)?
        {
            return Ok(result);
        }
        let Some(class) = self.symbols.types.native_class(kind) else {
            return Err(at(
                name,
                match kind {
                    Kind::Nullable(_) | Kind::Null => {
                        "Check for null and use requireValue before reading a field."
                    }
                    Kind::List(_) | Kind::MutableList(_) | Kind::Text => {
                        "Add parentheses to call this method, such as size()."
                    }
                    _ => "This value has no fields.",
                },
            ));
        };
        let (owner, field) = self.class_field(class, name)?;
        let reference = self.field_member(kind, field);
        instruction(&mut method.code, 0xb4, reference); // getfield
        method.max_stack = method.max_stack.max(base + field.kind.slots());
        if field.lateinit {
            self.guard_non_null(method, field.kind, base);
        }
        let result = self
            .symbols
            .types
            .replace_parameters(field.kind, &self.symbols.member_map(kind, owner));
        if result != field.kind {
            self.cast_top(method, field.kind, result, base);
        }
        Ok(result)
    }

    pub(super) fn set_field(
        &mut self,
        method: &mut Method<'a>,
        receiver: &'a Expr<'a>,
        name: Name<'a>,
        operator: Option<Arithmetic>,
        value: &'a Expr<'a>,
    ) -> Result<(), Error> {
        if let Some(class) = self.static_class(method, receiver)
            && self.symbols.find_static_field(class, name.text).is_some()
        {
            if let Some(operator) = operator {
                let (owner, field) = self.symbols.find_static_field(class, name.text).unwrap();
                if !field.mutable {
                    return Err(at(
                        name,
                        "This static field is read-only. Declare it with static var to change it.",
                    ));
                }
                let current = self.read_static_field(method, owner, name, 0)?.unwrap();
                let result =
                    self.arithmetic_tail(method, value, current, &[(operator, value)], 0)?;
                self.coerce(method, value, result, current, 0)?;
                let class_index = self.object_class(Kind::Object(owner));
                let descriptor = current.descriptor(&self.symbols.types);
                let reference = self.pool.member_for(9, class_index, name.text, &descriptor);
                instruction(&mut method.code, 0xb3, reference); // putstatic
                return Ok(());
            }
            self.write_static_field(method, class, name, value)?;
            return Ok(());
        }
        if let Some(class) = self.static_owner(method, receiver) {
            if operator.is_some() {
                return Err(at(name, "Use name = name + value to change a Java field."));
            }
            return self.write_java_field(method, None, &class, name, value);
        }
        if self.property_setter(method, receiver, name, operator, value)? {
            return Ok(());
        }
        let kind = self.expression(method, receiver, 0)?;
        if let Some(class) = self.symbols.types.runtime_class_name(kind.non_null())
            && matches!(kind, Kind::Java(_))
        {
            if operator.is_some() {
                return Err(at(name, "Use name = name + value to change a Java field."));
            }
            return self.write_java_field(method, Some(kind), &class, name, value);
        }
        let Some(class) = self.symbols.types.native_class(kind) else {
            return Err(at(
                name,
                "Only fields of Mammoth objects can be changed here.",
            ));
        };
        if self.symbols.find_field(class, name.text).is_none() {
            if operator.is_some() {
                if self.inherited_java_field_exists(class, name)? {
                    return Err(at(
                        name,
                        "Use name = name + value to change an inherited Java field.",
                    ));
                }
            } else if self.write_inherited_java_field(method, kind, class, name, value, 0)? {
                return Ok(());
            }
        }
        let (owner, field) = self.class_field(class, name)?;
        if !field.mutable {
            return Err(at(
                name,
                "This field is read-only. Declare it with var to change its value.",
            ));
        }
        let reference = self.field_member(kind, field);
        let map = self.symbols.member_map(kind, owner);
        if self.symbols.types.writes_star(field.kind, &map) {
            return Err(at(
                name,
                "This field has a star-projected type (*), so it cannot be changed through this value.",
            ));
        }
        let target = self.symbols.types.replace_parameters(field.kind, &map);
        method.stack.push(self.local_type(kind));
        let result = if let Some(operator) = operator {
            method.code.push(0x59); // dup
            instruction(&mut method.code, 0xb4, reference); // getfield
            method.max_stack = method.max_stack.max(1 + field.kind.slots());
            if target != field.kind {
                self.cast_top(method, field.kind, target, 1);
            }
            self.arithmetic_tail(method, value, target, &[(operator, value)], 1)?
        } else {
            self.expression_as(method, value, 1, Some(target))?
        };
        self.coerce(method, value, result, target, 1)?;
        if field.kind.is_reference() && !target.is_reference() {
            self.box_value(method, target);
        }
        method.stack.pop();
        instruction(&mut method.code, 0xb5, reference); // putfield
        Ok(())
    }

    fn outer_of(&self, class: usize) -> Option<usize> {
        let name = self.symbols.class_declarations[class].inner_of?;
        match self.type_scope.names.get(name) {
            Some(&Kind::Object(outer)) => Some(outer),
            // Code in another file may not have imported the outer class.
            _ => self
                .symbols
                .class_declarations
                .iter()
                .position(|class| class.name.text == name),
        }
    }

    /// Whether code here may use a private member of `owner`: the class
    /// itself and the inner classes inside it can.
    fn can_reach_private(&self, owner: usize) -> bool {
        let mut current = self.current_class;
        while let Some(class) = current {
            if class == owner {
                return true;
            }
            current = self.outer_of(class);
        }
        false
    }

    /// `this.__outer`, repeated up to the enclosing class that has member
    /// `name`, when the current class is an inner class.
    pub(super) fn outer_path(
        &self,
        method: &Method<'a>,
        name: Name<'a>,
        call: bool,
    ) -> Option<&'a Expr<'a>> {
        let this = Name {
            text: "this",
            offset: name.offset,
        };
        method.binding(this)?;
        let symbols = self.symbols;
        let mut class = self.current_class?;
        let mut path = Expr::Variable(this);
        while let Some(outer) = self.outer_of(class) {
            path = Expr::Field(
                Box::new(path),
                Name {
                    text: "__outer",
                    offset: name.offset,
                },
            );
            if symbols.find_method(outer, name.text).is_some()
                || (!call && symbols.find_field(outer, name.text).is_some())
            {
                return Some(Box::leak(Box::new(path)));
            }
            class = outer;
        }
        None
    }

    /// `this@Outer`.
    pub(super) fn labeled_this(
        &self,
        method: &Method<'a>,
        name: Name<'a>,
    ) -> Result<&'a Expr<'a>, Error> {
        let label = &name.text["this@".len()..];
        let this = Name {
            text: "this",
            offset: name.offset,
        };
        let mut path = Expr::Variable(this);
        let mut current = self
            .current_class
            .filter(|_| method.binding(this).is_some());
        while let Some(class) = current {
            if self.symbols.class_declarations[class].name.text == label {
                return Ok(Box::leak(Box::new(path)));
            }
            path = Expr::Field(
                Box::new(path),
                Name {
                    text: "__outer",
                    offset: name.offset,
                },
            );
            current = self.outer_of(class);
        }
        Err(at(name, "No enclosing class has that name."))
    }

    /// `Inner(...)` written where an enclosing object is available.
    pub(super) fn create_inner(
        &mut self,
        method: &mut Method<'a>,
        class: usize,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        let target = self.outer_of(class);
        let this = Name {
            text: "this",
            offset: name.offset,
        };
        let mut path = Expr::Variable(this);
        let mut current = self
            .current_class
            .filter(|_| method.binding(this).is_some());
        while let Some(here) = current {
            if Some(here) == target {
                let mut full = Vec::with_capacity(args.len() + 1);
                full.push(path);
                full.extend(args.iter().cloned());
                let full: &'a [Expr<'a>] = Box::leak(full.into_boxed_slice());
                self.inner_ready = true;
                return self.call_expected(method, name, full, base, expected);
            }
            path = Expr::Field(
                Box::new(path),
                Name {
                    text: "__outer",
                    offset: name.offset,
                },
            );
            current = self.outer_of(here);
        }
        Err(at(
            name,
            "An inner class needs its outer object. Write outer.Inner(...).",
        ))
    }

    /// `outer.Inner(...)`.
    pub(super) fn inner_on_receiver(
        &mut self,
        method: &mut Method<'a>,
        receiver: &'a Expr<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Option<Kind>>, Error> {
        let Some(&Kind::Object(class)) = self.type_scope.names.get(name.text) else {
            return Ok(None);
        };
        let Some(target) = self.outer_of(class) else {
            return Ok(None);
        };
        let saved = method.snapshot();
        let depth = method.stack.len() as u16;
        let kind = self.expression(method, receiver, depth);
        method.restore(saved);
        let Some(owner) = kind
            .ok()
            .and_then(|kind| self.symbols.types.native_class(kind))
        else {
            return Ok(None);
        };
        if !self
            .symbols
            .ancestors(owner)
            .any(|ancestor| ancestor == target)
            && owner != target
        {
            return Ok(None);
        }
        let mut full = Vec::with_capacity(args.len() + 1);
        full.push(receiver.clone());
        full.extend(args.iter().cloned());
        let full: &'a [Expr<'a>] = Box::leak(full.into_boxed_slice());
        self.inner_ready = true;
        self.call_expected(method, name, full, base, expected)
            .map(Some)
    }

    /// After `previous` assigns a local variable, what was known about it ends.
    pub(super) fn forget_assigned(method: &mut Method<'a>, previous: Option<&'a Statement<'a>>) {
        if let Some(Statement::Assign(name, _)) = previous
            && let Some(binding) = method.binding(*name)
        {
            method.forget_facts(binding.slot);
        }
    }

    /// Facts about a variable that a loop assigns cannot be used in the loop,
    /// because a later turn sees the assigned value.
    pub(super) fn forget_in_loop(method: &mut Method<'a>, body: &'a [Statement<'a>]) {
        let slots: Vec<u16> = method
            .variables
            .iter()
            .filter(|binding| binding.mutable && super::globals::assigns(body, binding.name))
            .map(|binding| binding.slot)
            .collect();
        for slot in slots {
            method.forget_facts(slot);
        }
    }

    /// `name = value` inside a class whose property `name` has a setter.
    pub(super) fn assign_property(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        value: &'a Expr<'a>,
    ) -> Result<bool, Error> {
        let Some(class) = self.current_class else {
            return Ok(false);
        };
        let text: &'a str = Box::leak(format!("{}$set", name.text).into_boxed_str());
        if self.symbols.find_method(class, text).is_none() {
            return Ok(false);
        }
        let this = Name {
            text: "this",
            offset: name.offset,
        };
        if method.binding(this).is_none() {
            return Ok(false);
        }
        let receiver: &'a Expr<'a> = Box::leak(Box::new(Expr::Variable(this)));
        self.property_setter(method, receiver, name, None, value)
    }

    /// `receiver.name = value` when the receiver's type has a setter for it.
    fn property_setter(
        &mut self,
        method: &mut Method<'a>,
        receiver: &'a Expr<'a>,
        name: Name<'a>,
        operator: Option<Arithmetic>,
        value: &'a Expr<'a>,
    ) -> Result<bool, Error> {
        let text: &'a str = Box::leak(format!("{}$set", name.text).into_boxed_str());
        let symbols = self.symbols;
        if !symbols
            .methods
            .iter()
            .any(|methods| methods.contains_key(text))
            && !symbols
                .interfaces
                .iter()
                .any(|methods| methods.contains_key(text))
        {
            return Ok(false);
        }
        let saved = method.snapshot();
        let kind = self.expression(method, receiver, 0);
        method.restore(saved);
        let Ok(kind) = kind else {
            return Ok(false);
        };
        let kind = kind.non_null();
        let found = if let Some(class) = symbols.types.native_class(kind) {
            // A plain field answering an interface property is written directly.
            symbols.find_method(class, text).is_some()
                && symbols.find_field(class, name.text).is_none()
        } else {
            symbols
                .types
                .interface_root(kind)
                .is_some_and(|interface| symbols.interfaces[interface].contains_key(text))
        };
        if !found {
            return Ok(false);
        }
        let setter = Name {
            text,
            offset: name.offset,
        };
        let (receiver, value): (&'a Expr<'a>, Expr<'a>) = match operator {
            None => (receiver, value.clone()),
            Some(operator) => {
                // The receiver is evaluated once, then read and written.
                let held: &'a Expr<'a> = Box::leak(Box::new(self.hold_value(method, receiver)?));
                let current = Expr::Field(Box::new(held.clone()), name);
                (
                    held,
                    Expr::Arithmetic(Box::new(current), vec![(operator, value.clone())]),
                )
            }
        };
        let args: &'a [Expr<'a>] = Box::leak(vec![value].into_boxed_slice());
        self.method_call(method, receiver, setter, args, 0)?;
        Ok(true)
    }

    /// Calls `name` on the current object when the class (or a superclass)
    /// declares a method with that name. Returns `None` otherwise.
    pub(super) fn implicit_this_call(
        &mut self,
        method: &mut Method<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Option<Kind>>, Error> {
        if let Some(result) = self.implicit_generic_method(method, name, args, base, None)? {
            return Ok(Some(result));
        }
        let this = Name {
            text: "this",
            offset: 0,
        };
        // Inside a default method, unqualified calls go to the interface.
        if let Some(interface) = self.current_interface
            && !self.symbols.functions.contains_key(name.text)
            && self
                .symbols
                .interface_method(interface, name.text)
                .is_some()
            && method.binding(this).is_some()
        {
            method.access(0x19, 0);
            let allow_null = std::mem::take(&mut self.allow_null);
            return self
                .call_method_on(method, interface, name, args, base, allow_null)
                .map(Some);
        }
        let Some(class) = self.current_class else {
            return Ok(None);
        };
        // A class calls a default method it inherits from an interface.
        if self.symbols.find_method(class, name.text).is_none()
            && self.this_ready
            && !self.symbols.functions.contains_key(name.text)
            && method.binding(this).is_some()
            && self.symbols.default_method(class, name.text).is_some()
        {
            method.access(0x19, 0);
            let receiver = self.symbols.types.self_kind(class);
            let allow_null = std::mem::take(&mut self.allow_null);
            return self
                .call_method_on(method, receiver, name, args, base, allow_null)
                .map(Some);
        }
        if self.symbols.find_method(class, name.text).is_none()
            && self.this_ready
            && !self.symbols.functions.contains_key(name.text)
            && method
                .binding(Name {
                    text: "this",
                    offset: 0,
                })
                .is_some()
            && let Some(root) = self.symbols.java_root(class)
            && !self
                .symbols
                .types
                .java_inherited_methods(&root, name.text)
                .is_empty()
        {
            method.access(0x19, 0);
            let allow_null = std::mem::take(&mut self.allow_null);
            return self
                .java_inherited_call(
                    method,
                    self.symbols.types.self_kind(class),
                    class,
                    name,
                    args,
                    base,
                    allow_null,
                    expected,
                )
                .map(Some);
        }
        if self.symbols.find_method(class, name.text).is_none()
            || !self.this_ready
            || method
                .binding(Name {
                    text: "this",
                    offset: 0,
                })
                .is_none()
        {
            return Ok(None);
        }
        let signature = self.find_class_method(class, name)?;
        let owner = self.symbols.find_method(class, name.text).unwrap().0;
        if self.symbols.generic_owner(class, owner) {
            method.access(0x19, 0);
            let receiver = self.symbols.types.self_kind(class);
            let allow_null = std::mem::take(&mut self.allow_null);
            return self
                .applied_method(method, receiver, name, args, base, allow_null)
                .map(Some);
        }
        let kind = Kind::Object(class);
        method.access(0x19, 0);
        let args =
            self.complete_args(method, name, signature, args, DefaultHost::Instance(kind))?;
        method.stack.push(self.local_type(kind));
        self.call_arguments(method, name, args, signature, base + 1)?;
        method.stack.pop();
        method.max_stack = method.max_stack.max(base + 1);
        let reference = self.member(Some(kind), name.text, &signature.descriptor);
        instruction(&mut method.code, 0xb6, reference); // invokevirtual
        Ok(Some(signature.result))
    }

    /// Invoke a default body through a directly named Mammoth interface. The
    /// method signature comes from its declaring ancestor, but invokespecial
    /// must name the direct superinterface in the current class file.
    pub(super) fn interface_super_call(
        &mut self,
        method: &mut Method<'a>,
        keyword: Name<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        let qualifier = keyword
            .text
            .split_once('<')
            .unwrap()
            .1
            .trim_end_matches('>')
            .trim();
        let target = self
            .type_scope
            .names
            .get(qualifier)
            .copied()
            .and_then(|kind| self.symbols.types.interface_root(kind))
            .ok_or_else(|| at(keyword, "Name a Mammoth interface in super<Interface>."))?;
        let direct = if let Some(class) = self.current_class.filter(|_| self.this_ready) {
            &self.symbols.implementations[class]
        } else if let Some(kind) = self.current_interface {
            &self.symbols.interface_parents[self.symbols.types.interface_root(kind).unwrap()]
        } else {
            return Err(at(
                keyword,
                "Use super<Interface> inside an instance method or an interface default method.",
            ));
        };
        let owner = direct
            .iter()
            .copied()
            .find(|&kind| self.symbols.types.interface_root(kind) == Some(target))
            .ok_or_else(|| {
                at(
                    keyword,
                    "The named interface must be a direct parent of this class or interface.",
                )
            })?;
        // The JVM also forbids naming a redundant direct interface when another
        // direct parent is more specific (including a class's superclass).
        let redundant = direct.iter().copied().any(|other| {
            self.symbols.types.interface_root(other) != Some(target)
                && self
                    .symbols
                    .interface_ancestors(other)
                    .iter()
                    .any(|&view| self.symbols.types.interface_root(view) == Some(target))
        }) || self
            .current_class
            .and_then(|class| self.symbols.superclasses[class])
            .is_some_and(|parent| {
                self.symbols
                    .interface_view(self.symbols.types.self_kind(parent), target)
                    .is_some()
            });
        if redundant {
            return Err(at(
                keyword,
                "Call the more specific direct parent instead of this redundant interface.",
            ));
        }
        let (local, _) = super::native_generics::call_name(name);
        let (declaring, signature) = self
            .symbols
            .interface_method(owner, local.text)
            .ok_or_else(|| at(name, "This interface has no method with that name."))?;
        let root = self.symbols.types.interface_root(declaring).unwrap();
        if !self.symbols.has_default(root, local.text) {
            return Err(at(
                name,
                "This interface method has no default body to call.",
            ));
        }
        method.access(0x19, 0);
        if let Some(result) = self.generic_method(
            method,
            super::generic_methods::MemberCall {
                receiver: owner,
                name,
                args,
                base,
                expected,
                special: true,
            },
        )? {
            return Ok(result);
        }
        if signature.parameters.len() != args.len() {
            return Err(at(name, "This call has the wrong number of arguments."));
        }
        let map = self.symbols.types.class_map(declaring);
        let abi = self.symbols.interface_body_signature(root, local.text);
        method.stack.push(method.locals[0]);
        self.adapted_arguments_to(method, args, base + 1, signature, &map, &abi)?;
        method.stack.pop();
        method.max_stack = method
            .max_stack
            .max(base + 1)
            .max(base + signature.result.map_or(0, Kind::slots));
        let class = self.object_class(owner);
        let reference = self
            .pool
            .member(11, class, &format!("${}", local.text), &abi.descriptor);
        instruction(&mut method.code, 0xb7, reference);
        Ok(signature.result.map(|erased| {
            let result = self.symbols.types.replace_parameters(erased, &map);
            let returned = abi.result.unwrap();
            if returned != result {
                self.cast_top(method, returned, result, base);
            }
            result
        }))
    }

    /// `super.name(args)` for a default method that the superclass inherits
    /// from an interface without overriding it. Like any super call it binds
    /// statically: `invokespecial` on the superclass, which the JVM resolves
    /// to the inherited default body, so an override in this class is not
    /// called again.
    fn inherited_default_super_call(
        &mut self,
        method: &mut Method<'a>,
        parent: usize,
        root: usize,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
    ) -> Result<Option<Kind>, Error> {
        let symbols = self.symbols;
        let class = self.current_class.unwrap();
        let parent_kind = symbols.superclass_kinds[class].unwrap_or(Kind::Object(parent));
        let (local, _) = super::native_generics::call_name(name);
        let view = symbols
            .interface_view(parent_kind, root)
            .ok_or_else(|| at(name, "The superclass has no method with that name."))?;
        let (declaring, signature) = symbols
            .interface_method(view, local.text)
            .ok_or_else(|| at(name, "The superclass has no method with that name."))?;
        if !signature.type_parameters.is_empty() {
            return Err(at(
                name,
                "A generic default method that the superclass inherits cannot be called with super yet.",
            ));
        }
        if signature.parameters.len() != args.len() {
            return Err(at(name, "This call has the wrong number of arguments."));
        }
        let map = symbols.types.class_map(declaring);
        let abi = symbols.interface_body_signature(root, local.text);
        method.access(0x19, 0);
        method.stack.push(method.locals[0]);
        self.adapted_arguments_to(method, args, base + 1, signature, &map, &abi)?;
        method.stack.pop();
        method.max_stack = method
            .max_stack
            .max(base + 1)
            .max(base + signature.result.map_or(0, Kind::slots));
        let owner = self.object_class(Kind::Object(parent));
        let reference = self
            .pool
            .member(10, owner, &format!("${}", local.text), &abi.descriptor);
        instruction(&mut method.code, 0xb7, reference); // invokespecial
        Ok(signature.result.map(|erased| {
            let result = self.symbols.types.replace_parameters(erased, &map);
            let returned = abi.result.unwrap();
            if returned != result {
                self.cast_top(method, returned, result, base);
            }
            result
        }))
    }

    /// `super.f()` where only one directly implemented interface offers `f`.
    fn implied_interface_super(&self, name: Name<'a>) -> Option<Name<'a>> {
        let symbols = self.symbols;
        let class = self.current_class.filter(|_| self.this_ready)?;
        let (local, _) = super::native_generics::call_name(name);
        let inherited = symbols.superclasses[class].is_some_and(|parent| {
            symbols.find_method(parent, local.text).is_some()
                || symbols.default_method(parent, local.text).is_some()
        });
        if inherited {
            return None;
        }
        let mut found = symbols.class_declarations[class]
            .implements
            .iter()
            .map(|written| written.text.split('<').next().unwrap_or(written.text))
            .filter(|plain| {
                self.type_scope
                    .names
                    .get(plain)
                    .and_then(|kind| symbols.types.interface_root(*kind))
                    .is_some_and(|root| symbols.has_default(root, local.text))
            });
        let plain = found.next()?;
        if found.next().is_some() {
            return None;
        }
        Some(Name {
            text: Box::leak(format!("super<{plain}>").into_boxed_str()),
            offset: name.offset,
        })
    }

    pub(super) fn super_call_expected(
        &mut self,
        method: &mut Method<'a>,
        keyword: Name<'a>,
        name: Name<'a>,
        args: &'a [Expr<'a>],
        base: u16,
        expected: Option<Kind>,
    ) -> Result<Option<Kind>, Error> {
        if let Some(qualified) = self.implied_interface_super(name) {
            return self.interface_super_call(method, qualified, name, args, base, expected);
        }
        if let Some(class) = self.current_class
            && self.this_ready
            && self.symbols.superclasses[class].is_none()
            && let Some(root) = self.symbols.java_root(class)
        {
            return self.java_super_call(method, &root, name, args, base, expected);
        }
        let parent = self
            .current_class
            .and_then(|class| self.symbols.superclasses[class])
            .filter(|_| self.this_ready)
            .ok_or_else(|| {
                at(
                    keyword,
                    "Use super only inside the methods of a class that extends another class.",
                )
            })?;
        let symbols = self.symbols;
        let (local, explicit) = super::native_generics::call_name(name);
        if symbols.find_method(parent, local.text).is_none()
            && let Some(root) = symbols.default_method(parent, local.text)
        {
            return self.inherited_default_super_call(method, parent, root, name, args, base);
        }
        let (owner, signature) = symbols
            .find_method(parent, local.text)
            .ok_or_else(|| at(name, "The superclass has no method with that name."))?;
        let (private, is_abstract) = symbols.method_flags[owner][local.text];
        if private {
            return Err(at(name, "This method is private to its class."));
        }
        if is_abstract {
            return Err(at(name, "This method is abstract in the superclass."));
        }
        if signature.type_parameters.is_empty() && explicit.is_some() {
            return Err(at(name, "This method does not take type arguments."));
        }
        method.access(0x19, 0);
        let class = self.current_class.unwrap();
        let parent_kind = self.symbols.superclass_kinds[class].unwrap_or(Kind::Object(parent));
        if !signature.type_parameters.is_empty() {
            return self
                .generic_method(
                    method,
                    super::generic_methods::MemberCall {
                        receiver: parent_kind,
                        name,
                        args,
                        base,
                        expected,
                        special: true,
                    },
                )?
                .ok_or_else(|| at(name, "This method does not give a value."));
        }
        let map = self.symbols.member_map(parent_kind, owner);
        method.stack.push(method.locals[0]);
        if signature.parameters.len() != args.len() {
            return Err(at(name, "This call has the wrong number of arguments."));
        }
        self.adapted_arguments(method, args, base + 1, signature, &map)?;
        method.stack.pop();
        method.max_stack = method
            .max_stack
            .max(base + 1)
            .max(base + signature.result.map_or(0, Kind::slots));
        let reference = self.member(Some(Kind::Object(parent)), name.text, &signature.descriptor);
        instruction(&mut method.code, 0xb7, reference); // invokespecial
        Ok(signature.result.map(|erased| {
            let result = self.symbols.types.replace_parameters(erased, &map);
            if erased != result {
                self.cast_top(method, erased, result, base);
            }
            result
        }))
    }
}
