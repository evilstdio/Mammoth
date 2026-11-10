# Mammoth

Mammoth is a beginner-friendly language for the JVM. It compiles `.mam` files directly to JVM bytecode, and it can use any Java library: the JDK, Minecraft's Fabric API, or any JAR on your `CLASSPATH`.

```text
fun greet(name: Text) {
    print("Hello, {name}!")
}

for name in ["Maya", "Sam"] {
    greet(name)
}
```

With Rust and Java 17 installed, run these commands once in the VS Code PowerShell terminal from the project folder.

```powershell
cargo build --release
$env:Path = "$(Join-Path $PWD 'target\release');$env:Path"
```

Then run any Mammoth source file with a single command.

```powershell
mammoth .\examples\greet.mam
```

This compiles and runs the program. The example prints `Hello, Maya!` and `Hello, Sam!` on separate lines. Repeat the PATH line in a new terminal.

To keep the generated `Main.class` for later, run `mammoth .\examples\greet.mam .\build`.

Functions can call other functions defined in the same file. Arguments and list items are evaluated from left to right. Loop variables are scoped to the loop body. In text, `{name}` inserts a value, and `{price * 2}` inserts the result of any expression. Write `\{name}` to print the braces and name literally. Lines beginning with `//` and trailing `//` comments are supported.

Run `cargo test --release` to compile every program under `tests/` and `examples/`, check the bytecode with the JVM's strict verifier, and compare each program's output with its expected `.out` file.

## VS Code

Install the Mammoth Language VSIX with **Extensions → Install from VSIX**. Build the compiler with `cargo build --release`, then open a saved `.mam` file and use the play button or **Mammoth: Run Mammoth File**. Java must be on `PATH`. The extension uses the workspace release compiler when present. Set `mammoth.compilerPath` when the compiler lives elsewhere.

Compiler diagnostics appear in the Problems panel. **Mammoth: Check Java Bindings** runs the optional Java declaration check. The extension source is in `editors/vscode`.

## Values and conditions

Try `examples/values.mam` using the same terminal commands with the new filename. It prints `Total: 20`.

- `let total = (2 + 3) * 4` creates a read-only binding and infers its type. Bindings inside a block are visible only within that block.
- `Int` is a signed 32-bit integer. Arithmetic supports `+`, `-`, `*`, `/`, and `%`. Overflow wraps. Division truncates toward zero, and division or remainder by zero raises an error at runtime. A whole number too large for `Int`, such as `5000000000`, is a `Long`. You can also write `5L`.
- `Long` is a 64-bit integer, `Double` is a 64-bit decimal number, and `Char` is one character written in single quotes, such as `'a'`.
- `Bool` holds `true` or `false`. An `if` condition accepts a `Bool` value or a comparison (`==`, `!=`, `<`, `<=`, `>`, `>=`). Chain conditions with `else if`. An `else` block is optional.
- `print` accepts any value, including lists and objects. `print()` prints an empty line.

```text
fun grade(score: Int): Text {
    if score >= 90 {
        return "A"
    } else if score >= 80 {
        return "B"
    } else {
        return "C"
    }
}
```

### Numbers

A decimal literal such as `2.5` or `1e3` has type `Double`. When an expression mixes `Int`, `Long`, and `Double`, Mammoth converts the narrower value automatically, so `2.5 * 4` is `10.0`. An `Int` can also be passed where a `Long` or `Double` is expected. Converting the other way is explicit: use `toInt()`, `toLong()`, or `round()`.

```text
let price = 2.5
let quantity = 4
print(price * quantity)
print(-price)
print(7.9.toInt())
```

`Double` uses the JVM’s 64-bit floating-point operations. Dividing by zero can produce infinity or NaN. Run `mammoth .\examples\double.mam` to try an average that calls Java’s square-root method.

Built-in math functions: `abs(x)`, `min(a, b)`, `max(a, b)`, `sqrt(x)`, `pow(x, y)`, `random()` (a `Double` from 0 up to 1), and `randomInt(from, until)`. Number methods include `toInt()`, `toLong()`, `toDouble()`, `toText()`, `abs()`, and on `Double` also `round()`, `floor()`, and `ceil()`. A `Char` has `isDigit()`, `isLetter()`, `isWhitespace()`, `uppercase()`, `lowercase()`, and `toInt()`.

### Text

Join text with `+`, compare it with `==` and `<`, and call methods on it.

```text
let name = "Mammoth"
print("Hello, " + name + "!")
print(name.length())
print(name.uppercase())
print(name.contains("moth"))
print(name.substring(0, 3))
print("one, two, three".split(", "))
print("{name} has {name.length()} letters")
```

`==` compares the characters, not the identity of two text values. Text methods include `length()`, `uppercase()`, `lowercase()`, `trim()`, `contains(t)`, `startsWith(t)`, `endsWith(t)`, `indexOf(t)`, `substring(start)`, `substring(start, end)`, `replace(old, new)`, `split(separator)`, `lines()`, `words()`, `repeat(n)`, `get(i)` (a `Char`), `chars()`, `isEmpty()`, `isBlank()`, `toInt()`, `toLong()`, and `toDouble()`. Every method of Java's `String` also works, such as `matches(regex)` or `codePointAt(i)`. Text can only start a `+` join: write `"Total: " + count` or `"{count} items"` rather than `count + " items"`.

### Reading input

`input()` reads one line typed by the user. `input("Prompt: ")` prints the prompt first. At the end of the input it returns empty text.

```text
let name = input("What is your name? ")
let age = input("How old are you? ").toInt()
print("Hello, {name}. Next year you will be {age + 1}.")
```

### Boolean expressions

Comparisons produce a `Bool` value, so you can save the result or pass it to a function. Use `and`, `or`, and `not` to combine conditions. Mammoth evaluates from left to right and skips the right side of `and` when the left side is false, or of `or` when the left side is true.

```text
let ready = 2 + 3 * 4 >= 14 and not false
print(ready)
```

Arithmetic binds first, followed by comparisons, `not`, `and`, and `or`. Use parentheses to group expressions. Numbers and characters support all six comparison operators. Boolean values support `==` and `!=`, and references can be compared with `null`. A check such as `value != null and value.isReady()` narrows a read-only `value` on the right side.

Run `mammoth .\examples\logic.mam` to see which calls are evaluated.

## Mutable variables and loops

Use `var` for a variable you want to update. An assignment keeps the type inferred from the initial value. `+=`, `-=`, `*=`, `/=`, and `%=` update a variable in place. A `while` loop checks its condition before each iteration and accepts the same conditions as `if`.

```text
var count = 0

while count < 3 {
    print(count)
    count += 1
}
```

A `for` loop can count through a range. `1..5` includes both ends, and `0 until 5` stops before 5. Ranges also work with characters, as in `'a'..'z'`.

```text
for i in 1..5 {
    print(i)
}
```

`break` leaves the innermost loop, and `continue` skips to its next iteration.

Run `mammoth .\examples\checksum.mam` to try a one-million-iteration example. It prints `256782`.

### Top-level variables

A variable declared at the top of the main file can be used and changed inside functions and classes.

```text
var score = 0

fun addPoints(points: Int) {
    score += points
}

addPoints(5)
print(score)
```

If the starting value's type cannot be worked out from the declaration alone, add a type, such as `var log: MutableList<Text> = mutableList()`.

### Lists

Use square brackets to create a list of values with the same type. Store it in a variable, pass it to a function, or loop over it. Indices start at zero. Use `size()` for the number of items, and `list[index] = value` to replace an item.

```text
fun sum(numbers: List<Int>): Int {
    var total = 0
    for number in numbers {
        total += number
    }
    return total
}

let numbers = [10, 20, 30]
numbers[0] = 15
print(numbers)
print(sum(numbers))
```

Fixed-size lists can hold text, numbers, characters, booleans, or objects. An annotation such as `List<Greeter>` lets a list hold different classes that implement the same interface. Run `mammoth .\examples\object-lists.mam` to try it. Use an element type such as `List<Int?>` or `List<Text?>` when list elements may be null. A typed context determines the type of `[]`, for example `let empty: List<Int> = []` or `sum([])`. Without a type, an empty list defaults to `List<Text>`. An out-of-range index raises a runtime error. Run `mammoth .\examples\lists.mam` for a complete example.

### Growable lists

Use `mutableList(...)` when you need to add or remove items after creating a list. An empty list can take its element type from a declaration, as in `let names: MutableList<Text> = mutableList()`. A `let` binding keeps the same list while its contents can change.

```text
let names = mutableList("Maya", "Sam")
names.add("Lee")
names[1] = "Jordan"
print(names)
print(names.removeAt(0))
print(names.contains("Lee"))
```

`set(index, value)` returns the previous item, and `removeAt(index)` returns the removed item. `clear()` empties the list. A mutable list is a Java `ArrayList`, so its other methods, such as `contains`, `indexOf`, and `isEmpty`, also work. A `for` loop uses the Java list iterator. Adding or removing items during iteration can raise a concurrent-modification error. Run `mammoth .\examples\mutable-lists.mam` to try it.

## Returning values

Add a result type after the parameter list when a function produces a value.

```text
fun double(number: Int): Int {
    return number * 2
}

print(double(21))
```

Every path through a function with a result type must return a value of that type. A loop may run zero times, so a return inside a loop still needs a return for the path that skips it. Calls can appear in expressions and arguments are evaluated from left to right. In a function without a result type, a bare `return` leaves the function early.

## Choosing with when

`when` picks the first branch whose condition matches. A branch can list several values, check a range with `in`, or check a type with `is`. `else` handles everything else.

```text
fun describe(number: Int): Text {
    return when number {
        0 -> "zero"
        1, 2, 3 -> "small"
        in 4..9 -> "medium"
        else -> "large"
    }
}
```

Without a subject, each condition is a `Bool`:

```text
when {
    temperature < 0 -> print("freezing")
    temperature < 25 -> print("mild")
    else -> {
        print("hot")
        print("drink water")
    }
}
```

`when` and `if` can also produce a value. Each branch ends with the value it gives, and an `else` branch is required.

```text
let biggest = if a > b { a } else { b }
let label = if score > 90 { "great" } else if score > 50 { "good" } else { "keep going" }
```

## Generic functions

A type parameter lets one function work with different types while keeping its arguments and result checked:

```text
fun identity<T>(value: T): T {
    return value
}

print(identity(42) + 1)
print(identity("Mammoth").uppercase())
```

`T` stands for the caller's type. Mammoth usually infers it from the arguments or an annotated result. You can also write it explicitly, such as `identity<Text>("Mammoth")`. The function body must work for every allowed type; it cannot return unrelated text where it promises `T`.

Type arguments are non-null. Write `T?` when a parameter or result may be missing:

```text
fun orElse<T>(value: T?, fallback: T): T {
    return value ?: fallback
}
print(orElse(null, "No name yet"))
```

Generic functions can use `MutableList<T>`, Java collections such as `ArrayList<T>`, and function types such as `(T) -> R`. Lambdas can supply an inferred result type:

```text
fun transform<T, R>(values: MutableList<T>, convert: (T) -> R): MutableList<R> {
    let result: MutableList<R> = mutableList()
    for value in values { result.add(convert(value)) }
    return result
}
print(transform(mutableList("Maya", "Sam")) { it.length() })
```

Use `export fun` to share a generic function between project modules or call it from Java. Mammoth emits JVM generic signatures so Java callers can infer the types too. Generic calls box primitive arguments and unbox primitive results; ordinary numeric functions retain their primitive representation.

Top-level generic functions, classes, interfaces, and methods are supported, along with bounds such as `T: Comparable<T>` (see [Type parameter bounds](#type-parameter-bounds)). Methods can use their class's arguments and declare additional type parameters. Fixed-size `List<T>` works too (see below). Runtime `is`, `as`, and `as?` checks accept parameterized types only when the value's static type implies the arguments.

Run `mammoth .\examples\generic-functions.mam` for a complete example.

### Generic fixed-size lists

Generic functions and classes can take a fixed-size `List<T>` and read, write, loop over, and return it. The list is never copied, so changes are visible to the caller:

```text
fun swap<T>(items: List<T>, a: Int, b: Int) {
    let saved = items[a]
    items[a] = items[b]
    items[b] = saved
}

let words = ["pear", "apple", "fig"]
swap(words, 0, 2)
print(words[0])     // fig
```

A fixed-size list is a JVM array, and lists of `Int`, `Double`, `Bool`, `Long`, or `Char` are primitive arrays such as `int[]`, while a `List<T>` is an array of objects. Two rules keep these from mixing without copying:

- A `List<T>` argument must hold objects (`Text`, classes, interfaces, or lists), never primitive values. Passing `[1, 2]` where `List<T>` is expected is rejected. Use `MutableList` for generic number collections.
- Generic code cannot create a `List<T>` with a list literal, because the element class is erased. Take the list as a parameter instead, or use `MutableList<T>`.

Lists are invariant, so a `List<Dog>` cannot be passed as a `List<Animal>`, which rules out `ArrayStoreException`. Java sees these functions as `<T> T[] swapEnds(T[])`, so javac also rejects primitive arrays.

Run `mammoth .\examples\generic-lists.mam` for a complete example.

## Functions as values

A lambda is a function written in braces. Parameters come before `->`. The last expression is the result.

```text
let double = { x: Int -> x * 2 }
print(double(21))

fun applyTwice(f: (Int) -> Int, value: Int): Int {
    return f(f(value))
}

print(applyTwice({ it + 3 }, 10))
```

A function type is written `(Int, Int) -> Int`. Use `Unit` as the result when the function gives nothing, as in `(Text) -> Unit`. When the parameter types are known from where the lambda is used, you can leave them out, and a single parameter can be called `it`. When a call's last argument is a lambda, it can follow the parentheses, or replace them entirely:

```text
let names = mutableList("Sam", "Alexandra", "Lee")
names.forEach { print("name: {it}") }
names.sort { a: Text, b: Text -> a.length() - b.length() }

let thread = Thread { print("running in a thread") }
thread.start()

fun twice(action: () -> Unit) {
    action()
    action()
}
twice { print("hello") }
```

This works for your own functions too, both as a statement and inside an expression.

A lambda can be passed to any Java method that expects an interface with one abstract method, such as `Runnable`, `Comparator`, or an event listener. A lambda can read variables from the code around it. It can also change `var` variables from the code around it.

Function types can have any number of parameters, such as `(Text, Int, Bool) -> Text`. Up to two parameters use Java's own interfaces (`Function`, `BiFunction`, and so on). Larger function types compile to small generated interfaces such as `mammoth.function.Function3`, which are written next to your program, so Java code can pass a lambda to them too. Run `mammoth .\examples\function-types.mam` for an example.

## Handling errors

`try` runs code that might fail. `catch` handles a particular kind of exception, and `finally` runs whether or not something failed. `throw` raises an exception, and `error("message")` is a short way to raise one with a message.

```text
fun parse(text: Text): Int {
    try {
        return text.toInt()
    } catch (problem: NumberFormatException) {
        print("not a number: {text}")
        return 0
    } finally {
        print("done parsing")
    }
}

fun check(age: Int) {
    if age < 0 {
        throw IllegalArgumentException("age cannot be negative")
    }
}
```

A `catch` without a type, as in `catch { ... }`, handles any `Exception` and names it `error`. The parentheses around the exception are optional. Mammoth does not require you to declare or catch Java's checked exceptions.

### Stack traces

Compiled classes record their source file and line numbers (the JVM `SourceFile` and `LineNumberTable` attributes), so an uncaught exception points at your code:

```text
Exception in thread "main" java.lang.IllegalStateException: too big: 10
	at Mammoth$0.$explode(trace.mam:5)
	at Main.lambda$0(trace.mam:22)
	at Main.run(trace.mam:16)
	at Main.main(trace.mam:20)
```

Every function, method, constructor, field initializer, `init` block, lambda, interface default, and generated forwarding method has line entries, across all files of a project. Lines are recorded per statement, so an exception in the middle of a long expression reports the line where its statement starts. Debuggers can set breakpoints by line and show parameters and local variables by their Mammoth names (the JVM `LocalVariableTable`), for example in `jdb` or IntelliJ. A variable appears from the statement after its declaration until its block ends. A `var` changed inside a lambda is stored in a one-item array, so the debugger shows it as an array.

Mixin needs these line numbers to find field initializers, so `@Unique` fields with starting values work in Minecraft mods (see `examples/fabric-demo`).

## Classes and objects

Run `mammoth .\examples\counter.mam` to try two independent counters. The first counter prints `1` and `2`. The second prints `1`.

A `class` groups fields and methods. `var` declares a field that can change and `let` declares a read-only field. Other code reads a field with `object.field` and changes a `var` field with `object.field = value`. Mark a field or method `private` to hide it from code outside the class. Within a method, refer to a field or another method of the same class by its name. Use `this` when you need the current object.

Put `let` or `var` before a constructor parameter to make it a field. A field can start with any expression, and an `init` block runs code when the object is created. Fields and `init` blocks run in the order they are written.

```text
class Account(let owner: Text, start: Double) {
    var balance = start
    private var history = mutableList("opened")

    init {
        print("Created an account for {owner}")
    }

    fun deposit(amount: Double) {
        balance += amount
        record("deposit {amount}")
    }

    private fun record(entry: Text) {
        history.add(entry)
    }

    fun toString(): Text {
        return "{owner}: {balance}"
    }
}

let account = Account("Maya", 10)
account.deposit(5)
print(account.balance)
print(account)
```

A method named `toString` that returns `Text` controls how `print` and text interpolation show the object. Use a class name as a parameter or return type to pass objects between functions and methods. If the compiler cannot work out a field's type from its starting value, it asks you to add one, as in `var next: Node? = null`.

Compile-only mode writes `Main.class` and a class file for each declared class. Keep those files together when running with Java.

### Static members

A `static` member belongs to the class itself rather than to each object. Call it on the class name:

```text
class Counter(let name: Text) {
    static var created = 0
    static let prefix = "counter-"

    init {
        created += 1
    }

    static fun make(name: Text): Counter {
        return Counter(name)
    }

    fun label(): Text { return prefix + name }
}

let counter = Counter.make("a")
print(Counter.created)       // 1
Counter.created += 5
```

- **Access.** Inside the class and its subclasses you can leave out the class name. `static let` fields are read-only. A `private static` member is visible only inside its class.
- **Starting values.** Static fields and `static init { }` blocks run in source order when the class is first used. A static field without a type takes the type of its starting value, which may be a literal, a constructor such as `ArrayList<Text>()`, a function or static method call, or an expression using earlier static fields. When the type cannot be worked out, write it.
- **No object.** A static method has no `this`, so it cannot use the object's fields or methods. Pass an object as a parameter instead.
- **Not supported yet.** Static methods cannot have type parameters, cannot be protected, abstract, or exported, and cannot share a name with an instance method. A `static init` block cannot return.
- **Java and Mixin.** Static members compile to JVM static methods and fields with their plain names, so Java calls `Counter.make("a")` directly, and annotations work on them. That includes Mixin handlers for static Minecraft methods (see `examples/fabric-demo`).

Run `mammoth .\examples\static-members.mam` for a complete example.

**Initialization order.** Field initializers and `init` blocks, both instance and static, may read only fields declared above them. `let next: Int = start + 1` before `let start: Int = 5` is rejected with "This field is read before its starting value is set", because it would read 0 (or null for objects). Reads through `this.` or the class name are checked too. Lambdas created in an initializer may read later fields, since they run later. Reads through a method called from an initializer are not checked, as in Java and Kotlin.

```text
class Table {
    static var rows = ArrayList<Text>()
    static var size = 0
    static init {
        rows.add("first")
        size = rows.size() + 10
    }
    static let doubled = size * 2
}
```

### Inheritance

A class can extend one other class with `extends`, passing arguments to its constructor. It inherits the fields and methods that are not private, and it can replace a method by declaring one with the same parameters and result. Write `override` to have the compiler check that a method really replaces one. Call the original with `super.method()`.

A `protected fun` can be called only inside its class and its subclasses. As on the JVM, a subclass can call it on `this` or on a value of the subclass's own type (or a further subclass), but not on a value typed as the base class or a sibling class. An override may keep a protected method protected or make it public, but cannot make a public method protected. A protected method cannot implement an interface method, and protected methods can be abstract, generic, and called through `super`.

```text
abstract class Shape(let name: Text) {
    protected abstract fun area(): Int
    protected fun label(): Text { return "{name}={area()}" }
    fun describe(): Text { return "shape " + label() }
}

class Square(let side: Int) extends Shape("square") {
    protected override fun area(): Int { return side * side }
    fun compare(other: Square): Text { return "{label()} vs {other.label()}" }
}
```

Protected methods compile to JVM `protected` methods, so a Java subclass in another package can override and call them, and javac rejects unrelated callers. Run `mammoth .\examples\protected-methods.mam` for a complete example.

An `abstract class` cannot be created directly. Its `abstract fun` methods have no body, and every subclass that is not abstract must provide them.

```text
abstract class Shape(let name: Text) {
    abstract fun area(): Double

    fun describe(): Text {
        return "{name} with area {area()}"
    }
}

class Rectangle(let width: Double, let height: Double) extends Shape("rectangle") {
    override fun area(): Double {
        return width * height
    }
}

class Square(side: Double) extends Rectangle(side, side) {
    override fun describe(): Text {
        return "square! " + super.describe()
    }
}

let shapes: List<Shape> = [Rectangle(2, 3), Square(2)]
for shape in shapes {
    print(shape.describe())
}
```

### Enums

An `enum` lists a fixed set of values. Use them with `==`, `when`, and loops. `name()` gives a value's name, `ordinal()` its position, `Color.values()` all values, and `Color.valueOf("Red")` looks one up by name.

```text
enum Color { Red, Green, Blue }

let favorite = Color.Green
when favorite {
    Color.Red -> print("warm")
    else -> print("not red")
}
for color in Color.values() {
    print(color)
}
```

An enum can take constructor values and have methods:

```text
enum Planet(let mass: Double, let radius: Double) {
    Mercury(3.303e23, 2.4397e6),
    Earth(5.976e24, 6.37814e6)

    fun gravity(): Double {
        return 6.673e-11 * mass / (radius * radius)
    }
}
```

Mammoth enums are Java enums, so they also work with Java APIs such as `EnumMap`.

### Data classes

A `data class` holds values. Mammoth generates `toString`, `equals`, and `hashCode` from its constructor fields, so two objects with the same values are `==`, and printing shows the values. A class that needs no body can leave off the braces.

```text
data class Point(let x: Int, let y: Int)

let a = Point(1, 2)
print(a)
print(a == Point(1, 2))
```

Any class can define its own `equals(other: Any): Bool` and `hashCode(): Int`, and `==` uses them.

### Generic classes

A class can take type parameters just like a generic function:

```text
class Box<T>(var value: T) {
    fun get(): T { return value }
    fun replace(next: T) { value = next }
}

let score = Box(42)
print(score.get() + 1)
score.replace(95)
print(score.value)

let name: Box<Text> = Box("Mammoth")
print(name.get().uppercase())
```

The constructor usually infers the type argument, or you can write `Box<Int>(42)`. `Box<Int>` accepts and returns integers; assigning text to its `value` or passing text to `replace` is a compile error. Type arguments are invariant, so a `Box<Int>` cannot be assigned to a `Box<Any>` variable.

Classes may have several type parameters, including data classes such as `data class Pair<T, R>(let first: T, let second: R)`. Nested types, lists of boxes, function-valued fields, closures, and imported generic classes also work. A field may be `T?` when its value can be missing; type arguments themselves must be non-null. A class with no constructor values from which to infer its arguments needs explicit arguments or an annotated variable type.

Mammoth emits one JVM class per declaration, with boxed values where a type parameter holds a primitive. Class, constructor, field, and method generic signatures preserve type information for Java callers of exported factories and helpers.

Type arguments are erased at runtime, so `is`, `as`, and `as?` can only check a parameterized type when the value's static type already decides the arguments:

```text
fun describe(source: Source<Int>): Text {
    if source is Box<Int> {          // Box<T> implements Source<T>, so T must be Int
        return "box {source.value}"
    }
    return "other"
}
```

The check compiles to a plain `instanceof Box`, and the value is smart-cast to `Box<Int>` inside the branch. This works through generic superclasses, interface inheritance, and type parameters (`source is Box<T>` for a `Source<T>`). A check whose arguments do not follow from the static type, such as `value is Box<Int>` for an `Any` value or `Box<Dog>` for a `Source<Animal>`, is rejected. Use `value is Box<*>` to check only the class.

Run `mammoth .\examples\generic-classes.mam` for a complete example.

### Generic methods

A method can introduce its own type parameters. In this example, `T` is the box's value type and `R` is the result of the supplied function:

```text
class Box<T>(let value: T) {
    fun map<R>(convert: (T) -> R): Box<R> {
        return Box(convert(value))
    }
}

let score = Box(21)
print(score.map { it * 2 }.value)
print(score.map { "Score: {it}" }.value)
```

Mammoth infers method arguments from values, lambdas, or an annotated result. Explicit calls such as `score.map<Text> { "Score: {it}" }` also work. A method's new type parameter must have a different name from one already in scope.

Generic methods work with implicit calls inside a class, overrides, `super`, nullable safe calls, and project imports. Interfaces may declare generic methods, for example `interface Identity { fun keep<T>(value: T): T }`. Implementations can choose another parameter name while keeping the same type relationships. Java callers receive generic method signatures too.

Run `mammoth .\examples\generic-methods.mam` for a complete example.

### Generic interfaces

An interface can take type parameters. A class chooses the arguments when it implements the interface:

```text
interface Source<T> {
    fun next(): T
}

class Counter(var count: Int) implements Source<Int> {
    fun next(): Int {
        count += 1
        return count
    }
}

class Repeat<T>(let value: T) implements Source<T> {
    fun next(): T { return value }
}

let numbers: Source<Int> = Counter(0)
print(numbers.next() + 1)
```

Calls through `Source<Int>` accept and return `Int`, and functions such as `fun take<T>(source: Source<T>)` infer `T` from any class that implements the interface. Interfaces can have several type parameters, nullable member types such as `T?`, and generic methods of their own. They can be imported between project modules.

Interface type arguments are invariant unless the parameter is marked `out` or `in` (see below): `Counter` is a `Source<Int>` but not a `Source<Any>`. A class lists an interface at most once, so it cannot implement both `Source<Int>` and `Source<Text>`. When an implementation uses a primitive such as `Int` where the erased JVM interface expects an object, Mammoth generates a bridge method that boxes the value, so Java code can implement and call Mammoth generic interfaces as well.

Run `mammoth .\examples\generic-interfaces.mam` for a complete example.

### Variance: `out` and `in`

By default, `Source<Dog>` and `Source<Animal>` are unrelated types. Mark an interface type parameter `out` when values of that type only come out of the interface, or `in` when they only go in:

```text
interface Source<out T> {
    fun next(): T
}

interface Sink<in T> {
    fun put(value: T)
}

let animals: Source<Animal> = Kennel()     // Kennel implements Source<Dog>
let dogs: Sink<Dog> = Shelter()            // Shelter implements Sink<Animal>
```

These rules apply:

- `out T` may appear in method results but not in parameter types. `in T` may appear in parameter types but not in results.
- Positions flip through function types, so `fun each(action: (T) -> Unit)` is allowed for `out T`. They also follow other interfaces' variance, so `fun putAll(values: Source<T>)` is allowed for `in T`.
- A marked parameter cannot appear inside an invariant type, such as `MutableList<T>`, a generic class, or a Java generic type.
- Only interface type parameters can be marked. Classes and functions stay invariant.
- Type arguments are compared after boxing, so `Source<Int>` fits `Source<Any>` but never `Source<Long>`.

Variance is checked entirely by the compiler. The JVM erases type arguments, so calls through a `Source<Animal>` view use the same method as before, and Java sees an ordinary generic interface.

Run `mammoth .\examples\variance.mam` for a complete example.

### Star projections: `Box<*>`

Write `*` as a type argument when any argument will do:

```text
fun describe(box: Box<*>): Text {
    let inside: Any = box.value
    return "holds {inside}"
}

let sources = MutableList<Source<*>>()
sources.add(Box(1))
sources.add(Box("two"))

fun kind(value: Any): Text {
    if value is Box<*> {
        return "a box with {value.value}"
    }
    return "something else"
}
```

These rules apply:

- Every `Box<X>` fits `Box<*>`, for classes and interfaces alike.
- Through a `Box<*>`, a member whose type is the starred parameter can be read, as an unknown type that fits `Any`, or the parameter's bound when the bound doesn't mention other parameters. It cannot be written: methods taking a `T` cannot be called, and `var` fields of type `T` cannot be assigned. This keeps a `Box<Int>` from receiving text.
- `value is Box<*>`, `as Box<*>`, and `as? Box<*>` work from any value, because they only check the class. Inside the branch, the value is smart-cast to `Box<*>`.
- Java sees `Box<?>`.

`*` is allowed in arguments of generic Mammoth classes and interfaces. It is not allowed in `List` or `MutableList`, or as a type on its own.

Run `mammoth .\examples\star-projections.mam` for a complete example.

### Generic superclasses

A class can extend a generic class by giving its type arguments:

```text
open class Box<T>(var value: T) {
    fun get(): T { return value }
}

class Score(points: Int) extends Box<Int>(points) {
    fun add(extra: Int) { value += extra }
}

let score = Score(40)
score.add(2)
print(score.get())
```

Inherited fields and methods use the substituted types, so `score.get()` is an `Int` and `value += extra` is checked as integer arithmetic. Arguments flow through several levels, as in `class Labeled<T> extends Box<T>` and `class Name extends Labeled<Text>`. Overrides must match the substituted signature, `super` calls work, and a `Score` can be used wherever a `Box<Int>` is expected. Private members stay private, and inheritance cycles are rejected.

Run `mammoth .\examples\generic-inheritance.mam` for a complete example.

### Type parameter bounds

A bound limits a type parameter to subtypes of one class or interface. Write it after a colon:

```text
fun largest<T: Comparable<T>>(first: T, second: T): T {
    if first.compareTo(second) >= 0 {
        return first
    }
    return second
}

class Pen<A: Animal>(let resident: A) {
    fun call(): Text { return resident.name }
}

print(largest(3, 8))
print(largest("pear", "apple"))
```

These rules apply:

- A bound is a single class or interface type: a Mammoth class or interface, a Java class or interface, or a parameterized type such as `Comparable<T>` or `Source<Int>`. A bound may mention the parameter itself. Primitive types, function types, lists, and nullable types cannot be bounds.
- Inside the generic code, a value of type `T` can call the bound's methods, read its fields, and be passed wherever the bound is expected. Without a bound, `T` only has the methods every object has.
- Every type argument must satisfy the bound, whether it is written, as in `Pen<Dog>`, or inferred from a call. The compiler reports `This type argument does not satisfy the bound of its type parameter.` at the type or call.
- A primitive argument satisfies a Java bound that its boxed class implements, so `Int` satisfies `Comparable<Int>`.
- As in Java, a bounded type parameter erases to its bound in the JVM class file, and its generic signature records the bound for Java callers.

A parameter can have several bounds joined with `&`, as in Java:

```text
fun announce<T: Named & Scored>(item: T): Text {
    return "{item.name()} scored {item.score()}"
}

fun describe<A: Animal & Named>(animal: A): Text {
    return "{animal.name()} has {animal.legs} legs"
}
```

- Only the first bound can be a class. Later bounds must be interfaces, and each bound is listed once.
- A type argument must satisfy every bound. Inside the generic code, `T` offers the methods of all its bounds and fits wherever any one of them is expected.
- The parameter erases to its first bound. Calling a method of a later bound inserts a cast to that interface, which always succeeds because the argument was checked. The JVM signature lists every bound, so javac also rejects Java arguments that miss one.

`where` clauses are not supported.

Run `mammoth .\examples\bounds.mam` for a complete example.

## Interfaces

An `interface` declares the methods a class must provide. A class lists its interfaces after `implements`. Its methods must match the parameter types and return types in the interface. Parameter names may differ.

```text
interface Greeter {
    fun greet(name: Text): Text
}

class Friendly implements Greeter {
    fun greet(name: Text): Text {
        return "Hi, {name}!"
    }
}

let greeter: Greeter = Friendly()
print(greeter.greet("Maya"))
```

Use an interface type in a function signature or in a `let` or `var` binding to work with any class that implements it. Run `mammoth .\examples\greeters.mam` to see the same function greet someone in two different ways.

### Default methods

An interface method can have a body. Classes that implement the interface inherit it, and can still declare their own version:

```text
interface Named {
    fun name(): Text
    fun greet(): Text {
        return "hello, " + name()
    }
}

class Person(let first: Text) implements Named {
    fun name(): Text { return first }
}

print(Person("Maya").greet())     // hello, Maya
```

Inside a default method, `this` is the interface value, and other interface methods can be called with or without `this.`. A class must still provide every method without a body. A class that inherits two default methods with the same name from unrelated interfaces must declare the method itself. Default methods compile to JVM default methods, so Java classes that implement the interface inherit them too.

Run `mammoth .\examples\default-methods.mam` for a complete example.

### Calling parent defaults

Use `super<Interface>.method()` to call a particular parent's default body. This works inside a class method or a child interface's default method:

```text
interface Friendly {
    fun greet(): Text { return "Hello" }
}

interface Enthusiastic extends Friendly {
    override fun greet(): Text {
        return super<Friendly>.greet() + "!"
    }
}

class Host implements Enthusiastic {
    override fun greet(): Text {
        return super<Enthusiastic>.greet() + " Welcome to Mammoth."
    }
}

print(Host().greet()) // Hello! Welcome to Mammoth.
```

The named Mammoth interface must be a direct parent listed after `implements` or `extends`. It can inherit the default from an ancestor. Use its name alone, such as `super<Source>`, even when the parent is `Source<Int>`. Type arguments come from the parent declaration. An abstract method has no body to call. If another direct parent is more specific than the named interface, call that more specific parent instead.

A child interface can provide a body for an inherited method, including an abstract one, with the same parameter types, result type, and method type-parameter bounds. `override` makes this intent explicit and checks that a parent method exists. The most specific interface override wins regardless of parent order. Unrelated parents with the same method name need an explicit implementation in the class or a child interface; that implementation can combine both parent defaults.

Generic parent defaults and specialized primitive overrides work through parent types and with Java callers. Java can inherit or override these defaults too. Ordinary primitive methods retain their existing representation; specialized overrides of generic interface methods use boxed JVM entries and forwarding bridges for compatible dispatch.

Run `mammoth .\examples\parent-defaults.mam` to combine two parent defaults.

### Interface inheritance

An interface can extend one or more other interfaces, generic or not:

```text
interface Source<T> {
    fun next(): T
}

interface Named {
    fun name(): Text
}

interface Stream<T> extends Source<T>, Named {
    fun peek(): T
}

interface Numbers extends Source<Int> {
    fun total(): Int
}
```

A class that implements `Stream<Int>` must provide `next`, `name`, and `peek`, with `T` replaced by `Int`. A `Stream<Int>` value can call all three methods and can be used wherever a `Source<Int>` or a `Named` is expected, including as an argument to `fun drain<T>(source: Source<T>)`, which infers `T = Int`.

These rules apply:

- An interface can extend Mammoth interfaces and Java interfaces, each listed once. Cycles such as `A extends B` and `B extends A` are rejected.
- A child interface can override an inherited method with a matching signature and a body. Two unrelated parents declaring the same method name require a matching override with a body in the child. A bodyless redeclaration is currently rejected. The same parent reached by two paths is fine.
- An interface, or a class, cannot inherit the same interface with different type arguments, such as `Source<Int>` through one parent and `Source<Text>` through another.

In the class file, the parents are listed as JVM superinterfaces with generic signatures, so Java code can implement a Mammoth `Stream<Integer>` and pass it wherever a `Source<Integer>` is expected.

Run `mammoth .\examples\interface-inheritance.mam` for a complete example.

A Mammoth interface can also extend a Java interface:

```text
import java.lang.Runnable

interface Task extends Runnable {
    fun label(): Text
}

class Printer(let message: Text) implements Task {
    fun run() { print("running " + message) }
    fun label(): Text { return message }
}

let task: Task = Printer("report")
task.run()
let thread = Thread(task)    // a Task is a Runnable
```

A class that implements `Task` must also implement the Java interface's abstract methods, and it gets the usual bridge methods for generic Java parents such as `Comparable<T>`. A `Task` value can call `run()` and can be passed wherever a `Runnable` is expected, both in Mammoth and in Java.

Run `mammoth .\examples\java-parent-interfaces.mam` for a complete example.

## Writing less code

### Default and named arguments

```text
fun greet(name: Text, punctuation: Text = "!", times: Int = 1): Text { ... }
greet("a")
greet("b", times = 3)
greet(times = 2, name = "c")
```

A default can read earlier parameters and is evaluated each time it is needed. Functions, methods, static methods, constructors, interfaces and imported functions can have defaults. An `override` uses the defaults of the method it overrides. Arguments run in the order you write them. A constructor default cannot use the class's type parameters yet.

### Single-expression functions

`fun twice(x: Int): Int = x * 2`. The result type is required.

### Sealed types

A `sealed class` or `sealed interface` can only be extended in its own file, so `when` knows every case:

```text
sealed class Shape
class Circle(let r: Int) extends Shape
object Point extends Shape

fun area(shape: Shape): Int = when shape {
    is Circle -> shape.r * 3
    Point -> 0
}
```

A `when` over a sealed type must cover every case or have an `else`, as a statement too. A nullable subject also needs a `null` branch.

### Extension functions

`fun Text.shout(): Text = this.toUpperCase() + "!"` adds a call `"hi".shout()`. Inside, `this` is the receiver and its members can be used directly, except from a lambda's implicit receiver use. A real method of the same name always wins. Receivers can be generic, as in `fun MutableList<T>.second<T>(): T`. Extensions are declared at the top level and imported by name.

### Constructors, properties and object expressions

A class can have more constructors. Each runs another constructor of the class first, and a class with only `constructor(...)` declarations has no other way to be created. These are real JVM constructors, so Java can call them. A `class K private constructor(...)` can only be created from inside `K`, usually by a companion object.

```
class Point(let x: Int, let y: Int) {
    constructor(both: Int) : this(both, both)
}
```

A `var` or `let` can have accessors. `field` is the stored value. `private set` lets everyone read it but only the class change it. A property marked `open`, `override` or `abstract` is reached through methods, so subclasses can replace it, and an interface can declare `var` properties. A `let` cannot replace a `var`, and reading a property before its starting value is rejected.

```
class Account {
    var balance = 0
        set(value) { field = if value < 0 { 0 } else { value } }
    var uses = 0
        private set
    let empty: Bool get() = balance == 0
}
```

`object implements Runnable { ... }`, `object extends Task() { ... }` and `object { ... }` make a one-off object. Its methods can use the local variables around it. Limits: a method needs written parameter and result types, its methods cannot have type parameters, a field's starting value and the arguments to the superclass cannot use those local variables, and `super` and inherited Java members need `this.` where available.

Classes can mark type parameters `out` or `in`, with the same rules as interfaces. A `var` field counts as both directions. `ArrayList<*>` and `MutableList<*>` accept any element type and can only be read, as `Any`.

`List<Int>(n)` makes a list of zeros or nulls and `List<Text>(n) { ... }` or `MutableList<Int>(n) { ... }` fill one with a block. Items that cannot be null need the block, and generic code cannot create a `List<T>`.

`1..3` used as a value is a `MutableList<Int>` holding those numbers. Loops and `in` over a written range do not build a list. `infix fun` names can be written between their operands, `fun f(): Nothing` never returns and can be used wherever a value is expected, `return@forEach` leaves a lambda, and `{ (a, b) -> ... }` unpacks its parameter. `Regex`, `compareBy`, `sortedWith`, `shuffled`, `buildList`, `readLine`, `distinctBy`, `filterKeys`, `mapKeys`, `union`, `intersect`, `subtract`, `coerceIn`, `trimMargin` and `sumOf` for `Long` and `Double` are in the library. `mapValues` and `mapKeys` receive the value or key rather than an entry.

### More class and flow features

An `inner class` holds a reference to the object that made it. Its code can use the outer object's members by name, including private ones, and `this@Outer` names the outer object. Create one inside the outer class with `Inner()`, or from outside with `outer.Inner()`. An inner class of a generic class shares its type parameters and cannot add its own yet. An inner class cannot have default constructor values, more constructors or static members yet.

`implements I by value` forwards every abstract method of `I` to `value`. The interface must be declared in the same file and have no type parameters. Methods the class writes itself win.

An enum value can have its own methods: `Red { override fun label(): Text = "r" }`. Every value must override an `abstract` method, and each override writes the same parameter names as the enum's method.

Null checks now reach local `var`s and read-only fields. After `if a != null`, `a` can be used without another check until it is assigned. A variable that a lambda can change never narrows, and a loop that assigns a variable drops what was known about it. `k.a` narrows when `k` is a `let` and `a` is a plain read-only field. Properties with accessors never narrow, because reading them runs code.

`let Text.first: Char get() = this[0]` declares an extension property. `when (let v = next()) { ... }` names the subject for its branches. `synchronized(lock) { ... }` locks an object. `Any()` makes a plain object. `k?.a = 1` assigns only when `k` is not null. `in values` works in `when` for anything with `contains`. `super.f()` reaches an interface's default method when one implemented interface has it. A statement can start with a value, such as `"x".let { print(it) }`.

The library has lazy sequences: `asSequence()`, `generateSequence`, and on a sequence `map`, `filter`, `take`, `drop`, `takeWhile` and `toList`. Every other operation on an `Iterable` also works on a sequence, by consuming it. Library operations work on lists of nullable items, so `list.map { it ?: 0 }` works when `list` is a `MutableList<Int?>`. Generic functions you write still reject nullable items as type arguments.

Java types can be written in full, as in `java.util.Iterator<Int>`, and `catch (e: java.io.IOException)`. Classes can be used in full too, as in `java.util.Objects.hash(1)`. `Runnable { ... }` makes a Java functional interface from a lambda. `IntArray(3)` and `Array<Text>(2) { "" }` are other spellings of `List<Int>(3)` and `List<Text>(2) { "" }`.

`is` narrows local `var`s and read-only fields by the same rules as null checks. An object with an `iterator()` method, whose result has `hasNext()` and `next()`, can be used in a `for` loop. `listOf`, `setOf` and `mapOf` work in generic code. A local function can call itself even when it uses the variables around it. Inside a class, `apply { ... }` and other extensions work on the current object without naming it, and a `by lazy` block can use constructor parameters. A list can be destructured, as in `val (a, b) = text.split(",")`. `a === b` compares object identity. A value whose class declares `invoke` can be called. `++count` and `--count` are statements. `require` and `check` take a message or a block that makes one. `value class` is a data class here, without the unboxed representation.

### Objects

`object Counter { var total = 0 }` declares a single instance, used by name: `Counter.total`. It can implement interfaces and extend classes.

### Data class helpers

A data class has `copy`, with every property optional: `person.copy(age = 31)`. Destructure with `let (name, age) = person`, and in loops with `for (name, age) in people`. Use `_` to skip a position. `for (key, value) in map` visits a map's entries.

### Collection operations

Lists and Java collections have `map`, `mapIndexed`, `flatMap`, `filter`, `filterNot`, `any`, `all`, `none`, `countOf`, `find`, `first`, `firstOrNull`, `last`, `lastOrNull`, `fold`, `reduce`, `sum`, `sumOf`, `average`, `minOrNull`, `maxOrNull`, `minByOrNull`, `maxByOrNull`, `sorted`, `sortedDescending`, `sortedBy`, `sortedByDescending`, `reversed`, `distinct`, `take`, `drop`, `takeWhile`, `dropWhile`, `joinToString`, `groupBy`, `associateBy`, `toSet`, `toMutableList`, `withIndex`, `zip`, `partition` and `isNotEmpty`. Maps have `getOrPut`, `mapValues` and `filterValues`. Text has `take`, `drop`, `reversed`, `padStart`, `padEnd`, `isNotEmpty`, `isNotBlank` and `trimIndent`. Each returns a new list. A class's own method of the same name wins. `Pair(a, b)` holds two values unless your program declares its own `Pair`.

These are written in Mammoth in `src/prelude.mam`, and only the ones a program uses are compiled into it.

### Operators, `in`, ranges and raw text

- `a + b`, `-`, `*`, `/` and `%` call `plus`, `minus`, `times`, `div` and `rem` on objects. `a < b` uses `compareTo`, and `a[i]` and `a[i] = v` call `get` and `set`, including on Java lists.
- `x in values`, `x not in values` and `x in 1..5` work in any condition.
- `for i in 10 downTo 0 step 2`, `0..10 step 5` and `repeat(3) { ... }`. A step must be positive.
- `"""raw text"""` spans lines and has no escapes or `{}` values. `trimIndent()` removes the common indentation.
- `value ?: return 0`, `value ?: throw Oops()`, `value ?: continue` and `value ?: break` leave early. `throw` and `return` also work as a `when` branch.

### Overloading

Functions and methods can share a name when their parameter types differ. The call picks the closest match: an exact type first, then widening such as `Int` to `Long`, then a parent type. A lambda is matched by its parameter count. A call that fits two versions equally is an error. On the JVM each version has a hidden name, such as `show$ov$Int`, so Java callers and Java interfaces cannot use these names directly.

### Nested types

`class`, `data class`, `enum`, `object`, `sealed class` and `interface` can be declared inside a class and named `Outer.Inner`, or just `Inner`. A nested type has no access to an instance of the outer class, and each nested name must be unique within its file.

### Building collections

`listOf(1, 2)` and `mutableListOf("a")` make a `MutableList`, `setOf(1, 2)` a set, and `mapOf("a" to 1)` a map. `a to b` inside call arguments makes a `Pair`.

### Variable arguments, references and aliases

- `fun total(vararg numbers: Int): Int` takes any number of values, as a list named `numbers`. `total(*existing)` passes a list.
- `::double` is a function value for a function in the same file.
- `typealias Names = MutableList<Text>` names a type.
- An `override` can return a subtype of the parent method's result.

### Properties and scope functions

`list.size`, `text.length` and a Java bean's `person.name` read like fields: Mammoth calls `size()` or `getName()` when there is no such field. An interface can declare `let name: Text`. A class answers it with a field of that name. `value.let { ... }`, `also`, `takeIf` and `takeUnless` work on any value, including with `?.`. `list.indices` and `list.lastIndex` give positions.

### Small syntax

`value!!` is `requireValue(value)`. `x !is Text`, `x !in list`, `i++` and `i--` work as in Kotlin. `shl`, `shr`, `ushr`, `&`, `|` and `^` work on `Int` and `Long`. Numbers can be written `0xFF`, `0b101` and `1_000_000`. `do { } while cond` runs its body first. `outer@ for ...` with `break@outer` or `continue@outer` leaves named loops. `/* */` comments nest. `val` means `let`. `if (ready) start()` and `if (a) 1 else 2` work without braces.

`let x = try { parse() } catch (e: Exception) { 0 }` uses `try` as a value, on its own line.

### Functions inside functions

A `fun` can be declared inside another. If it uses nothing from around it, it can call itself. If it uses outer variables it cannot, yet, and it cannot have default arguments.

### Class members

`companion object { ... }` holds static members. `lateinit var name: Text` starts unset and throws if read before it is set. `let total: Int by lazy { compute() }` computes on first read and is not synchronized. `let area: Int get() = w * h` is computed on every read.

### Receivers in lambdas

`x.apply { ... }`, `x.run { ... }`, `with(x) { ... }` and `buildString { ... }` make the receiver's members available by name inside the block.

## Using Java

Mammoth reads the JDK and the JARs on your `CLASSPATH` directly, so you can use Java classes without writing any declarations. Classes in `java.lang`, such as `Math`, `System`, `StringBuilder`, and `Thread`, are always available. Import other classes by their full name, or import a whole package with `*`.

```text
import java.util.Random
import java.time.*

print(Math.max(3, 7))
print(Integer.MAX_VALUE)
System.out.println("Printed by Java")

let builder = StringBuilder()
builder.append("Mam").append("moth")
print(builder.toString())

let random = Random(42)
print(random.nextInt(100))
print(String.format("%s has %d points", "Maya", 12))
print(LocalDate.of(2024, 2, 29).plusDays(1))
```

- Call a constructor by the class name, such as `Random(42)`, and call static methods and fields on the class, such as `Math.max(1, 2)` or `Math.PI`.
- Mammoth chooses between overloaded Java methods the way Java does. It prefers exact types, then widening such as `Int` to `long`, then subclasses, then boxing. Java methods that take a variable number of arguments, such as `String.format`, also work.
- `Int`, `Long`, `Double`, `Bool`, `Char`, and `Text` map to Java `int`, `long`, `double`, `boolean`, `char`, and `String`. `Any` is Java's `Object`. A Java `float`, `byte`, or `short` result becomes a `Double` or `Int`.
- Java can return null where Mammoth expects a value. By default, a null result raises an error at the call. When a Java method can return null, store the result in a nullable type or compare it with `null`:

```text
let home: Text? = System.getenv("MAMMOTH_HOME")
if System.getProperty("debug") == null {
    print("debug is off")
}
```

- In static calls such as `Int.MAX_VALUE` or `Long.parseLong(text)`, a Mammoth type name refers to its Java wrapper class.

To call a method from a JAR, include the JAR in `CLASSPATH` before running Mammoth. The one-command runner adds its generated classes to that classpath.

### Java collections and generics

Give a Java class type arguments in angle brackets, as in `ArrayList<Text>` or `HashMap<Text, Int>`. Mammoth then knows the types of the values that go in and come out, so `names.get(0)` is `Text` and a lambda's `it` has the right type. Index a Java map with `map[key]`, and loop over any Java collection with `for`.

```text
import java.util.ArrayList
import java.util.HashMap
import java.util.TreeMap

let names = ArrayList<Text>()
names.add("Sam")
names.add("Alexandra")
print(names.get(0).uppercase())
names.forEach { print("hi {it.lowercase()}") }

let ages: HashMap<Text, Int> = HashMap()
ages["Maya"] = 31
ages["Maya"] += 1
print(ages["Maya"])
print(ages.getOrDefault("Nobody", 0))

let counts = TreeMap<Text, Int>()
for word in "the cat and the hat".words() {
    counts[word] = counts.getOrDefault(word, 0) + 1
}
for entry in counts.entrySet() {
    print("{entry.getKey()}: {entry.getValue()}")
}
```

Reading a missing key with `map[key]` raises an error unless you use a nullable context, as in `let found: Int? = map[key]`, or provide a fallback with `map[key] ?: 0`. This works with numeric and reference values. Without type arguments, a Java collection holds `Any` values, which you can convert with `as`.

Values passed in are checked against the type arguments too. Java erases them, so the check happens in Mammoth: `names.add(5)` on an `ArrayList<Text>`, `scores.put("ann", "three")` on a `HashMap<Text, Int>`, or a `Cat` added to an `ArrayList<Dog>` is rejected with "This argument does not match the Java type's type argument". Anything that really fits is accepted: subclasses, nullable values (Java collections can hold null), and type parameters inside generic Mammoth code. Numbers are not widened inside collections, so `ArrayList<Long>().add(5)` is rejected; write `5L`. Lambdas, and parameters that are already concrete Java classes, use the usual overload checks.

### Implementing Java interfaces

A Mammoth class can implement any Java interface. Pass an object of the class to Java, and Java calls the Mammoth methods.

```text
import java.util.ArrayList
import java.util.Collections
import java.util.Comparator

class Worker(let label: Text) implements Runnable {
    fun run() {
        print("running {label}")
    }
}

let thread = Thread(Worker("background"))
thread.start()
thread.join()

class ByLength implements Comparator {
    fun compare(left: Text, right: Text): Int {
        return left.length() - right.length()
    }
}

let words = ArrayList()
words.add("banana")
words.add("fig")
Collections.sort(words, ByLength())
print(words)
```

Generic Java interfaces such as `Comparator` and `Comparable` pass their values as Java objects. Your method can declare the type it expects, as `compare(left: Text, right: Text)` does, and Mammoth converts the values when Java calls it.

### Extending Java classes

A Mammoth class can extend a Java class, which is how you write Bukkit plugins, Swing windows, custom exceptions, and many framework classes. Pass the Java constructor's arguments after the class name. Inside the class, call inherited methods by name, including protected ones. A method with the same name and compatible types replaces the Java method, and `super.method()` calls the original.

```text
import javax.swing.JFrame
import javax.swing.JButton
import java.awt.FlowLayout

class CounterWindow extends JFrame("Mammoth counter") {
    var clicks = 0

    init {
        setLayout(FlowLayout())
        let button = JButton("Click me")
        button.addActionListener { event ->
            clicks += 1
            button.setText("Clicked {clicks} times")
        }
        add(button)
        setSize(320, 120)
        setDefaultCloseOperation(JFrame.EXIT_ON_CLOSE)
    }
}

CounterWindow().setVisible(true)
```

Run `mammoth .\examples\window.mam` to open the window. Custom exceptions work the same way:

```text
class InsufficientFunds(needed: Double) extends RuntimeException("Need {needed} more") {}

try {
    throw InsufficientFunds(25)
} catch (error: InsufficientFunds) {
    print(error.getMessage())
}
```

Inside the class, fields that the Java superclass declares as public or protected can be read and assigned by name or through `this` and other receivers of the class's own type. For example, `count` in a subclass of `ByteArrayOutputStream`. Final Java fields are read-only, and private or package-private Java fields are not visible. Other code cannot use a protected Java field, and an explicit receiver must have the subclass type, as the JVM requires. Compound assignment such as `count += 1` works for a bare name; with an explicit receiver, write `other.count = other.count + 1`. The Java superclass can be generic. Give its type arguments after `extends`:

```text
import java.util.ArrayList

class Names extends ArrayList<Text>() {
    override fun add(value: Text): Bool {
        return super.add(value.toUpperCase())
    }
    fun first(): Text { return get(0) }        // get returns Text
    fun trim(count: Int) { removeRange(0, count) }   // a protected Java method
    fun changes(): Int { return modCount }     // a protected Java field
}

class Bag<T> extends ArrayList<T>() {
    fun firstOr(fallback: T): T { return if isEmpty() { fallback } else { get(0) } }
}
```

Inherited methods, `super` calls, and generic protected fields (such as `protected T value`) use the type arguments, including a Mammoth class's own type parameters. An override must use them too: in a subclass of `ArrayList<Text>`, `override fun get(index: Int): Int` is rejected. Overrides get erased bridge methods, so Java code calling `add(Object)` through a `Collection<String>` reaches the Mammoth method. The class file records the parameterized superclass, so Java sees `class Names extends ArrayList<String>`. Arguments to inherited methods are checked against the type arguments, as for any Java generic (see Java collections and generics). Run `mammoth .\examples\generic-java-superclass.mam` for an example.

A class that extends an abstract Java class must provide its abstract methods. Run `mammoth .\examples\bank.mam` for a longer example with inheritance and exceptions, and `mammoth .\examples\guess.mam` for a guessing game that reads the keyboard.

### Exhaustive `when` over enums

A `when` used as a value normally needs an `else` branch. It does not when its subject is an enum, Mammoth or Java, and its branches name every constant as `Type.CONSTANT`:

```text
return when (light) {
    Light.RED -> "stop"
    Light.AMBER -> "wait"
    Light.GREEN -> "go"
}
```

A nullable subject still needs `else`, since null matches no constant. If a Java enum later gains a constant, the unmatched value throws `IncompatibleClassChangeError`, as javac's enum switches do.

### Assigning Java fields

Public fields of Java objects and Java classes can be assigned: `point.x = 10`, `constraints.weightx = 0.5`, `Config.total = 7`. The value is checked against the field's type, including type arguments (a `List<String>` field takes a `JavaList<Text>`). Final fields are rejected. Write compound updates out in full: `point.x = point.x + 1`.

### Class objects

`Type.class` gives the `java.lang.Class` object for a type, typed `Class<Type>`, for Java APIs that take one: `EnumMap<DayOfWeek, Int>(DayOfWeek.class)`, `EnumSet.allOf(DayOfWeek.class)`, `Text.class.getName()`. `Int.class` is Java's `int.class`. Type parameters and parameterized types have no class object.

### Java interface default methods on Mammoth classes

A Mammoth class that implements a Java interface can call that interface's default methods, with its type arguments substituted: `Increment().andThen(Tens())` for a `Function<Int, Int>`, or `ByLength().reversed()` for a `Comparator<Text>`.

### Calling a function a call returns

When a call or list index gives a function value, call it directly: `adder(2)(5)`, `multiply(3)(4)`, `handlers[0](3)`, `steps.get(0)(21)`. The `(` must follow the `)` or `]` immediately. A parenthesized expression on the next line is not a call.

### Nullable function values

Put a function type in parentheses to make it nullable: `var handler: ((Int) -> Int)? = null`. Check it for null (or use `requireValue`) before calling it. `(Int) -> Text?` is still a function whose result may be null. This is useful for a function looked up in a map: `let found: ((Int) -> Int)? = actions.get("inc")`.

`requireValue` also accepts a number that a null check has already narrowed, such as after `if value == null { throw ... }`.

### Java number boxes

Java's `Integer`, `Long`, `Double`, `Boolean` and `Character` are Mammoth's `Int`, `Long`, `Double`, `Bool` and `Char`:

- A Java method or field declared as `Integer` gives an `Int`. It is an `Int?` where the destination accepts null; elsewhere a null fails at the call.
- Type arguments match too: a Java `List<Integer>` is a `JavaList<Int>`, and a `Map<String, Integer>` is a `JavaMap<Text, Int>`.
- Methods of the box can be called on a number: `5.compareTo(3)`, `seven.hashCode()`, `2.5.isNaN()`.

`Short`, `Byte` and `Float` values stay Java objects, since Mammoth has no such types.

### Nested Java classes and float parameters

Import a nested Java class with dots, as in Java source:

```text
import net.minecraft.block.AbstractBlock.Settings as BlockSettings
import java.util.AbstractMap.SimpleEntry
```

Mammoth has no `Float` type. A `Double` value can be passed to a Java `float` parameter, and it is rounded to float. When a method also has a `double` overload, that one is chosen. This covers Minecraft settings such as `strength(1.5)` and sound volumes.

### Explicit Java bindings

You can still declare a Java member yourself, which gives it a Mammoth name and fixes its exact signature. This is useful for obfuscated or remapped names, such as Minecraft's intermediary names.

```text
java fun maximum(left: Int, right: Int): Int = "java.lang.Math.max"
print(maximum(12, 20))
```

`Int`, `Bool`, `Text`, and `Double` map to Java `int`, `boolean`, `String`, and `double`. Omit the result type for a Java `void` method. A Java method that returns `null` for a `Text` result raises a runtime error. Mammoth checks arguments at compile time. Java resolves the declared method when the call runs, so the class, method name, and Java types must match.

Run `mammoth .\examples\java.mam` for JDK examples.

### Checking Java declarations

Run `mammoth --check-java file.mam` to check Mammoth source and verify its Java bindings against the JDK and `CLASSPATH`. It reports missing classes and mismatched public constructors, methods, and fields at their declaration lines. The checker uses Java 17 or later and inspects declarations without initializing their classes.

The regular `mammoth file.mam` command uses JVM linkage when a declared member is accessed. Use the explicit check when adding or changing Java dependencies.

### Java objects

Declare the constructor and instance methods your program uses. Create the object with the Mammoth name, then call its methods with a dot.

```text
java class TextBuilder = "java.lang.StringBuilder" {
    new(start: Text)
    fun append(text: Text): TextBuilder
    fun toString(): Text
}

let message = TextBuilder("Hello")
print(message.append(", ").append("Mammoth!").toString())
```

Run `mammoth .\examples\java-objects.mam` to try it. A Java class binding can declare one public constructor and one signature per public instance method name. Omit the constructor if you only receive objects from Java. Use declared Java class names in Mammoth signatures and in Java static bindings. Java resolves the constructor and methods at runtime. Use a nullable result type if the Java method can return null.

## Nullable values

Add `?` to a type when a value can be missing, such as `Text?`, `Int?`, `Long?`, `Double?`, `Bool?`, or `Char?`. Use `== null` or `!= null` in an `if` or `while` condition. A null check makes a read-only binding or parameter usable as a non-null value on the checked path. This also works after an early return handles the null case. For mutable bindings and fields, copy the value to a `let` binding before checking it, or use `requireValue` to get the value and raise an error if it is null.

```text
java fun property(name: Text): Text? = "java.lang.System.getProperty"

let version = property("java.version")
if version != null {
    print(version)
}
```

Run `mammoth .\examples\nullable.mam` to see both a missing and a present value. Use an explicit type when initializing a variable with `null`, such as `var label: Text? = null`. Java bindings can also use nullable reference types for parameters. Printing a nullable value that is null prints `null`.

`?:` gives a fallback for a missing value, and `?.` calls a method or reads a field only when the value is present. Otherwise the result is null.

```text
let nickname: Text? = null
print(nickname ?: "no nickname")
print(nickname?.uppercase())
print(nickname?.length() ?: 0)
let home = System.getenv("HOME") ?: "unknown"
```

A numeric safe call can be stored directly, such as `let length: Int? = nickname?.length()`. Nullable primitive values use Java wrapper objects; ordinary `Int`, `Long`, `Double`, `Bool`, and `Char` values keep their unboxed representation. Lists can hold nullable elements, such as `List<Int?>`.

Use `toIntOrNull()`, `toLongOrNull()`, or `toDoubleOrNull()` to parse text without throwing an error for invalid input or integer overflow:

```text
let age = "24".toIntOrNull()
if age != null {
    print(age + 1)
}
print("unknown".toIntOrNull() ?: 0)
```

Integer parsers use decimal notation. The floating-point parser follows Java's `Double.valueOf`, including `NaN` and `Infinity`. Run `mammoth .\examples\nullable-numbers.mam` for a complete example.

### Checking types

`is` checks whether a value has a type, and inside the checked branch the value can be used as that type. `as` converts a value to a more specific type and raises an error if it has another type. `as?` gives null instead of raising an error.

```text
fun describe(value: Any): Text {
    if value is Text {
        return "text of length {value.length()}"
    }
    return when value {
        is Int -> "number {value + 1}"
        else -> "something else"
    }
}

let count = map.get("apples") as Int
```

`is` and `as` are most useful with values typed `Any`, such as items from Java collections. Smart casts apply to read-only variables and parameters.

### Java callbacks

A Mammoth class can implement an interface declared with `java interface`. Pass an object of that class to a Java method that expects the interface, and Java can call the Mammoth implementation.

```text
java interface IntListener = "java.util.function.IntConsumer" {
    fun accept(value: Int)
}

class Report implements IntListener {
    fun accept(value: Int) {
        print("Received {value}.")
    }
}
```

Run `mammoth .\examples\java-callback.mam` to see Java’s `OptionalInt.ifPresent` call the Mammoth method. When implementing a Java interface, declare every abstract method your class must provide. Java resolves the actual interface at runtime.

## Exporting to Java

Give a source file a qualified JVM class name with `module`. Use `export fun` for functions that Java or a JVM framework should call.

```text
module demo.mammoth.Logic

export fun double(value: Int): Int {
    return value * 2
}
```

Java can call `demo.mammoth.Logic.double(21)`. The ordinary `mammoth file.mam` command still runs top-level code. To package a JAR, use `mammoth --jar file.mam app.jar`. This uses the JDK's `jar` tool and includes the generated classes. Keep external dependencies on the host classpath.

### Exporting instance methods

Use `export fun` inside a class to expose a public JVM method under its source name, without the internal `$` prefix. This lets Java code and reflection call a method even when it is not implementing a Java callback interface:

```text
class Greeter(let name: Text) {
    export fun greet(): Text { return "Hello, {name}!" }
    export fun identity<T>(value: T): T { return value }
}
```

Java code receiving a `Greeter` from an exported factory can call `greeter.greet()` or `greeter.identity(42)`. Generic class and method signatures are preserved. Reflection can look up `greet` by name. Exports are instance methods, and must be public with a body. Non-null reference arguments are checked at the Java entry point; nullable arguments accept null. Inherited exported methods still dispatch to Mammoth overrides. Exports that conflict with final or static Java methods are rejected.

This exposes method names; generated class names retain the existing naming scheme. It does not add annotations or Mixin support. Java callback implementations remain automatically exposed without requiring `export`.

Run `mammoth .\examples\export-methods.mam` for an example.

### Java annotations

Use Java declaration annotations before classes, interfaces, functions, methods, and fields. Import the annotation type, or use its fully qualified name. Arguments are named:

```text
@Deprecated(since = "0.3", forRemoval = false)
class OldGreeter {
    @Deprecated(since = "0.3")
    export fun greet(): Text { return "Hello from Mammoth" }
}
```

Mammoth reads the Java annotation declaration and checks allowed targets, required elements, duplicate arguments, and element types. Java defaults apply to omitted optional elements. `RUNTIME` annotations are available through reflection; `CLASS` annotations are emitted as runtime-invisible attributes; `SOURCE` annotations are checked and omitted from annotation attributes. Missing retention defaults to `CLASS`.

Values can be literal text, booleans, numbers, characters, enum constants such as `RetentionPolicy.RUNTIME`, class literals such as `String.class` or `Int.class`, nested annotations, and arrays such as `[String.class, Int.class]`. Import class/enum names used in values. Use `L` for long literals and decimal literals for float/double elements. Byte and short values are range-checked. Executable expressions and string interpolation are not supported in annotation values.

Nested annotation values use the same named-argument syntax, including arrays of annotations. With the annotation types imported from a Java library, Mammoth can express:

```text
@Inject(method = "tick", at = @At(value = "HEAD"))
```

Or several values:

```text
@Inject(method = ["tick", "draw"], at = [@At(value = "HEAD"), @At(value = "TAIL", ordinal = 2)])
```

The nested annotation's type must match the Java element declaration. Required elements, defaults, duplicate names, and constant types are checked at every level. A nested value's own `@Target` and retention do not restrict it as though it were a declaration; the enclosing declaration annotation controls whether it is retained. Nesting is bounded to protect the compiler from malformed inputs.

Java's single-element array shorthand is supported: an array element can be supplied as one scalar or one nested annotation. For example, `@SuppressWarnings(value = "unchecked")` is equivalent to `@SuppressWarnings(value = ["unchecked"])`. Empty arrays use `[]`. Nested annotations are metadata constants and cannot be assigned to variables or executed as ordinary expressions.

On exported instance methods and Java callbacks, annotations are attached to the Java-facing entry point. An ordinary Mammoth method keeps its internal JVM name. Interface annotations apply to the declared/default body; generated generic forwarding bridges do not duplicate them. This release does not support annotation declarations, repeatable annotations, constructor/parameter/type-use annotations, or annotation processors. It does not implement Java compiler-specific analyses such as checking every meaning of `@Override` or `@SafeVarargs`.

Run `mammoth .\examples\annotations.mam` for an example. These are prerequisites for Minecraft integration. Specific Mixin methods and Shadow/Unique/Final field shapes have now been verified in the real development client. Static/external fields, broader initializer shapes, and production-profile testing remain open; this does not establish complete Mixin parity.

### Annotated and protected fields

Annotations on fields preserve their JVM name, visibility, mutability, and generic signature. An annotation before a constructor `let`/`var` property also targets its field, not the constructor parameter.

```text
class Counter(@Deprecated(since = "0.4") protected var count: Int) {
    @Deprecated(since = "0.4")
    private let label = "counter"
}
class StepCounter extends Counter(0) {
    fun step(): Int { count += 1 return count }
}
```

Protected fields are accessible in their declaring class and subclasses. Other classes and top-level code cannot access them, even in the same file. Within a subclass, an explicit receiver must have that subclass type or a further subclass type; a receiver typed only as the base class is rejected. This preserves JVM access rules across packages. Read-only `let` fields remain final. Protected methods and protected fields declared by Java superclasses are supported too, as described under Inheritance and Extending Java classes.

Field annotations use the same target, element, and retention checks as other declarations. A METHOD-only annotation cannot be placed on a field. Run `mammoth .\examples\field-annotations.mam` for an example.

The Fabric demo now exercises real `@Shadow`, `@Unique`, and `@Final` fields. Its final nullable Shadow reference has no initializer, so it does not assign a placeholder into Minecraft's existing reference. Unique fields with nondefault starting values, such as `= 40` and a computed Text value, are also verified in the real game. They depend on Mammoth's line-number tables. Those specific field shapes are verified; arbitrary Mixin constructor behavior is not established.

### Minecraft Fabric example

The starter in `examples/fabric` exports an initialization method and supplies the matching Fabric metadata. Build it from the project folder.

```powershell
mammoth --jar .\examples\fabric\mod.mam .\mammoth-demo.jar --resources .\examples\fabric\resources
```

The `--resources` directory is copied into the JAR. The entry point has been exercised with Fabric Loader 0.16.14's language adapter in a JVM test harness. The example logs an initialization message when the adapter calls it.

### Minecraft 1.20.1 keybind example

`examples/fabric-client` registers a G keybind and a client tick callback. Each press toggles a Boolean and logs the new state. Build it from the project folder.

```powershell
mammoth --jar .\examples\fabric-client\mod.mam .\mammoth-controls.jar --resources .\examples\fabric-client\resources
```

The example targets Minecraft Java Edition 1.20.1 with Fabric Loader 0.16.14 or later and Fabric API 0.92.2 for 1.20.1. Put the mod and matching Fabric API JAR in the Fabric profile’s `mods` folder. The key appears as **Toggle Mammoth demo** under **Mammoth** in the controls menu. Press G and look for the toggle messages in the game log.

The JAR was verified with the official Fabric Loader and API, using simulated Minecraft classes and key presses in a JVM harness. Launching Minecraft and checking the controls menu remain a separate test.

The example uses `java let` to read the static Fabric event field. `fun wasPressed(): Bool = "method_1436"` gives a readable Mammoth name to the runtime Java method. A binding to `java.lang.Object` accepts reference values, including Mammoth objects, for Java APIs whose generic parameter is erased to `Object`.

### Complete Minecraft 1.21.1 development example

`examples/fabric-demo` contains a mod whose item registration, keybind, Fabric event handler, and Mixin are written in Mammoth. It has a checksum-pinned Gradle wrapper, Loom remapping, Java 21 setup helper, production-JAR checks, and an automated real-client smoke test.

From that folder on Windows:

```powershell
.\dev.ps1 build
.\dev.ps1 runClient
.\dev.ps1 smokeTest
```

The smoke test has been run successfully in the actual Minecraft development client with strict bytecode verification. It checks registration of `mammoth_demo:tusk`, the injected tick handler, the real Fabric callback, and one simulated G-key press, then closes after resources load. The production JAR's remapped targets and metadata are checked separately. A normal launcher/profile run, physical keyboard input, and item appearance in a world still need manual testing. See `examples/fabric-demo/README.md` and its `validation` folder for exact versions, commands, results, and limitations.

## Multi-file projects

Put top-level code in `main.mam`. Put named supporting modules in `src`, including nested folders. Mark a function `export fun` and use `import` to call it from another file. Classes and interfaces are available for import without an `export` keyword. You can give an import a local name with `as`.

```text
import demo.mammoth.Statistics.average
print(average([1.5, 2.5, 5.0]))
```

Run the example or package it with these commands.

```powershell
mammoth --project .\examples\project
mammoth --project .\examples\project .\project-demo.jar
java -jar .\project-demo.jar
```

A project JAR also includes files under `resources` if that directory exists. Exported functions can accept and return Mammoth classes, interfaces, Java types, lists, nullable values, and function types. Each file keeps its own imports and local names; an exported signature uses the types from its declaring module.

For example, `src/model.mam` can declare:

```text
module school.Model
class Student(let name: Text, let points: Int)
```

Then `main.mam` can use the class:

```text
import school.Model.Student
let maya = Student("Maya", 95)
print(maya.points)
```

Imported classes support constructors, methods, fields, inheritance, interface implementation, enums, and data classes. Field privacy and read-only fields are checked across files. Use an alias when two modules declare the same name, such as `import other.Model.Student as OtherStudent`. Project imports name individual declarations; wildcard imports currently apply to Java packages.

Run `mammoth --project .\examples\project-classes` for a three-file example with students, a shared interface, and an average function. The same project can be packaged with `mammoth --project .\examples\project-classes .\school.jar`.

## Performance measurements

The checksum benchmark compares Mammoth, Java, and Kotlin with the same integer workload on Java 17. See [the benchmark](benchmarks/checksum/README.md) for measurements and instructions to rerun it.

See [PLAN.md](PLAN.md) for the language and performance milestones.

### Java generic assignment safety and current limits

Java generic destinations are invariant: an ArrayList<Text> cannot be assigned to ArrayList<Int> or Collection<Int>. Compatible ancestor views such as ArrayList<Text> to Collection<Text> are allowed, including inherited Java types on Mammoth subclasses. Nested arguments must also match. An existing raw Java value cannot be assigned to a parameterized destination. A fresh constructor can use a same-class declared destination: `let ages: HashMap<Text, Int> = HashMap()`.

This does not establish full Java generic safety. Original Java method parameter signatures are now checked, including upper/lower wildcard directions, nested invariance and spread generic varargs. Inferred method arguments are also checked against their declared bounds, with ancestor substitutions and method-variable shadowing respected. Full wildcard capture/read typing, constraint-based inference, Java class-application bounds, generic-aware overload selection, casts and remaining boxed primitive paths still require auditing. Raw Java APIs retain erased operations. See the production-readiness queue in PLAN.md.

Generic method argument examples: `ArrayList<Text>.addAll(ArrayList<Int>)` is rejected; copying a collection of a subtype into a collection of its parent is allowed by `? extends T`. Lower-bound Java parameters accept compatible parent-element destinations. These checks also apply to generic Java constructors.

Java generic method inference collects constraints across arguments rather than choosing the first argument type. Mixed subtypes can infer a common parent (for example Dog and Cat to Animal), while List<Dog> remains an exact constraint and cannot accept Cat. Candidate selection is deterministic across argument order. This finite-candidate solver does not yet provide full intersection, bound-propagating, contextual-result or lambda inference.

Declared Java method bounds now feed dependent and nested constraints back into inference. Independent and self-recursive bounds filter candidate choices; final argument/bound checks still run. Propagation stops after a bounded number of rounds and rejects nonconverging cases. Full intersection, contextual-result, lambda and recursive-bound constraint inference remain incomplete.

Declared expected types can now guide generic Java static/direct-instance results: `let names: JavaList<Text> = Collections.emptyList()` (using `import java.util.List as JavaList`) and `let names: Collection<Text> = Collections.emptyList()` infer Text. Function returns and parameters can supply the same context. Argument invariance and declared bounds still apply. Context through inherited/safe calls, nested target-dependent factories, and lambda constraints remains incomplete.

Nested generic factories can receive fixed-parameter context when one Java signature matches the call arity. For example, a typed `Collections.singletonList(Collections.emptyList())` can infer both collection levels. Supported direct-SAM Java lambda return bodies also contribute method constraints, such as Optional.map inferring Int from a Text length. Multiple lambda results can infer a common parent. Lambda input inference, inherited SAM signatures, speculative overload inference and target-dependent varargs remain incomplete.

Explicit Java lambda parameter annotations now constrain method inference, and inherited functional-interface signatures retain their substituted arguments. Typed lambdas cannot narrow the input promised by their target (including Any); compatible broader parameters are accepted. Known Any type arguments are preserved. Deliberately raw generic Java interfaces still permit unchecked parameter annotations. Implicit lambda inputs without enough context, overload reinference and full wildcard capture remain incomplete.

Source-written Java generic class/interface applications now enforce argument counts and declared bounds. Invalid declarations such as EnumSet<Text> are rejected even when no object is constructed. Nested types and Java superclasses receive the same checks; bounded Mammoth type parameters can satisfy Java bounds. Raw Java types remain available; Java wildcard source syntax and full wildcard capture remain unsupported.

Java overloads with incompatible generic constraints, bounds or explicit lambda input annotations are filtered before conversion ranking when several erased-compatible candidates exist. This allows a valid overload to be selected instead of failing on an incompatible earlier candidate. Equal-ranked specificity, ambiguity diagnostics and lambda-body-based candidate retry remain incomplete. Compatible primitive results in concrete Java boxed/Number SAM return signatures are boxed automatically.

When applicable Java overloads tie under Mammoth conversion ranking, parameter specificity selects a unique narrower candidate. For example List is more specific than Collection, String than Object for null, and String[] than Object[]. Equally good unrelated/crossed candidates now produce an ambiguity error instead of using declaration order; explicit argument types can disambiguate them. Among equally ranked candidates, specificity also compares type arguments: `f(List<String>)` versus `f(ArrayList<Integer>)` with `null` is ambiguous, as in javac, while `f(ArrayList<String>)` still beats `f(List<String>)`. Parameters using the method's own type variables keep the erased comparison. Candidates are compared in javac's phases: subtyping and widening, then boxing, then variable arity. Within a phase the most specific candidate wins, and a call with no single most specific candidate is an error, as in javac. A Mammoth `Double` reaches a Java `float` parameter only when no `double` overload applies.

For overloaded Java calls with literal lambdas, compatible candidates now undergo contextual lambda-body checking in isolated bytecode state. A lambda returning an Int can distinguish an int-result SAM from a Text-result SAM; operations on an implicit parameter can also exclude incompatible input targets. Runtime captures execute only in the selected call. Multiple compatible unrelated targets remain ambiguous. Candidate-specific nested factory inference and full Java phased/generic overload semantics remain incomplete.

Overload candidates now give nested generic calls their own parameter context. `nest(Collections.singletonList(Collections.emptyList()))` selects `nest(List<List<String>>)` over `nest(String)`, and `String.join(",", Collections.emptyList())` compiles. Nested calls, list literals and conditionals are probed again with each applicable candidate's parameter types; only refinements with the same erasure are kept, so conversion ranking and specificity are unchanged. Once a varargs call is known to spread, its elements receive the component type and are checked against it. Re-probing is skipped for parameters without type arguments and stops after four nested overload levels.

Declared result types also reach generic Java methods called through a Mammoth subclass, as an unqualified call inside the subclass, through `this.` or `super.`, and through `?.`. For example, `fun names(): JavaList<Text> { return super.contextualEmpty() }` infers `Text`. A Java `super.` call's result is checked for null like any other Java result, unless the destination accepts null.

Static fields of a Java superclass and its interfaces can be read the same way: `MAX_PRIORITY` inside a `Thread` subclass, `FACING` inside a Minecraft block, protected ones included, or `Worker.MIN_PRIORITY` through the subclass name. Generic static fields keep their type arguments. Static methods of a Java superclass can be called by their bare name inside a Mammoth subclass, as in Java, including protected ones: `createCuboidShape(0.0, 0.0, 0.0, 16.0, 8.0, 16.0)` in a Minecraft block. They can also be called through a subclass name, such as `SubHolder.listIdentity(...)`. Interface static methods are not inherited. A top-level Mammoth function with the same name takes precedence over an inherited static method.

Lambdas can be passed where a Java generic parameter is erased to `Object`, and they take the parameter's type argument: `ArrayList<(Int) -> Int>().add { value -> value + 1 }` or `ArrayList<Runnable>().add { ... }`. Real functional-interface overloads are still preferred.

#### Runtime checks where Java can break a promise

- Reading a non-null reference from a list, such as `items[i]` or a `for` loop, checks for null, since a Java array can contain null. Use `let value: Text? = items[i]` to allow it. A list passed through a generic function is not checked element by element.
- `value as Text` throws if `value` is null. Use `as Text?` to keep null.
- A top-level statement cannot reach a variable declared later through the functions, constructors and methods it calls. Methods are matched by name, and lambdas are not followed.
- Mammoth has no `Byte` or `Short` type, so an `Int` can be passed to Java `byte` and `short` parameters after any `int` overload. The value is truncated as a Java cast would.
- A Java `byte[]` is a list whose items read and write as `Int`: bytes are signed, and writing a larger number truncates it. It is a separate type from `List<Int>`.
- Call a generic Java method with explicit type arguments like `Collections.emptyList<Text>()`. Arguments must fit them.

#### Casts to parameterized Java types

The JVM cannot check type arguments, so `value as ArrayList<Text>`, `as?` and `is` with a parameterized Java type are accepted only when the value's type already determines the arguments:

- An upcast or same type: `ArrayList<Text>` as `Collection<Text>`.
- A downcast whose arguments follow from the value: `Collection<Text>` as `ArrayList<Text>`, `JavaMap<Text, Int>` as? `HashMap<Text, Int>`.

Casting an `Any` to `ArrayList<Text>`, or a `Collection<Text>` to `ArrayList<Int>`, is rejected with "Java type arguments are erased at runtime". Before, such casts compiled, and `is JavaList<Text>` answered true for a list of numbers. Check the raw type, such as `value as ArrayList`, when the elements really are unknown. Raw Java types stay unchecked, as before.

#### Java wildcard values

A Java method result or field whose type has wildcards, such as `List<? extends Number>`, gets a hidden *capture* type for each wildcard, as Java does:

- Reading works through the bound: `numbers.get(0).intValue()`, `for n in numbers { ... }`.
- Writing is rejected: `numbers.add(2.5)` reports "This Java value has a wildcard type ... so nothing can be added to it". The same value can be written back into itself (`numbers.add(numbers.get(0))`).
- A wildcard value is not a `JavaList<Number>`, and two wildcard values from different calls cannot be mixed, through Java or Mammoth generic methods.
- A `? super Text` value, such as `TreeMap.comparator()`, accepts `Text` values and reads as `Any`.
- To keep or change the values, copy them: `ArrayList<Number>(numbers)`.

Captures belong to the source position of the call or field read. As a result, a `var` holding one cannot be reassigned from another call, and a lambda without a declared result type cannot return one. Both are conservative rejections; Java would accept them with a declared wildcard type, which Mammoth cannot write yet. Generic Java fields read from Java objects and static fields now keep their type arguments; before, they were raw.

## mammoth.requests

```text
import mammoth.requests
let response = requests.post(url, json = {"content": "Hello from MAMMOTH!"})
print(response.status)
response.raiseForStatus()
```

Works in single-file runs, project builds and packaged application JARs with Java 17+. The compiler embeds the library source and links it only when imported, so there is nothing to install. It is one bundled library, not a package manager.

### Making requests

`requests.get`, `post`, `put`, `patch`, `delete`, `head` and `options` take the URL and then any of these by name. `requests.request(method, url, ...)` sends any other method.

| Option | Meaning |
| --- | --- |
| `timeout` | Seconds, a positive `Int`. Default 30 |
| `params` | Map of query values added to the URL's query, before any fragment. Values are text, numbers, booleans, or a list that repeats the key |
| `headers` | Map of header names and values. A value here replaces a default such as `Content-Type` |
| `json` | A JSON body, see below |
| `text` | A UTF-8 text body, `text/plain` by default |
| `bytes` | A `ByteArray` body, `application/octet-stream` by default |
| `form` | A map sent as `application/x-www-form-urlencoded` |

```text
requests.get(url, params = {"q": "two words", "page": "2"}, headers = {"Accept": "application/json"})
requests.put(url, text = "plain body")
requests.post(url, form = {"name": "Maya"})
```

Give at most one of `json`, `text`, `bytes` and `form`. A second one throws `InvalidRequestError` before anything is sent. So do a URL that is not valid, a scheme other than http or https, credentials written in the URL, and a header with a line break. `Connection`, `Content-Length`, `Expect`, `Host` and `Upgrade` belong to the HTTP client and cannot be set. `post`, `put` and `patch` take `json` as their second positional argument, and the others take `timeout`.

### JSON

`json =` accepts text, booleans, finite numbers, maps with text keys, lists, arrays and `Json` values, nested to 512 levels. Anything else throws `JsonError` before a request is sent, including NaN, infinity, a map key that is not text, and a value that contains itself. Because `json = null` means "no JSON body", send a JSON null with `json = Json.NULL`.

Mammoth does not mix value types in a dictionary, and this library does not change that. Wrap each value with `Json.of` when the types differ, or build a `MutableMap<Text, Any>`:

```text
import mammoth.Json
requests.post(url, json = {"name": Json.of("Maya"), "age": Json.of(31), "nick": Json.NULL})
```

`response.json()` and `Json.parse(text)` give a `Json` value and follow RFC 8259 strictly. Malformed text throws `JsonError`, whose `position` is the character offset. Messages never quote the document.

| Read | Result |
| --- | --- |
| `v["key"]`, `v[0]` | A child. Throws `JsonError` if it is missing or `v` is the wrong kind |
| `v.find("key")` | A child or `null` |
| `v.has("key")`, `v.size()`, `v.isNull()`, `v.kind()` | Inspection |
| `v.asText()`, `v.asBool()`, `v.asInt()`, `v.asLong()`, `v.asDouble()` | The value. Throws `JsonError` on the wrong kind, or a number that does not fit exactly |
| `v.asList()`, `v.asMap()` | A copy as `MutableList<Json>` or a key-ordered `LinkedHashMap<Text, Json>` |
| `v.toString()`, `Json.stringify(value)` | Compact JSON text |

Numbers keep the text they were written with, so a 64-bit id does not lose precision. A repeated key keeps its last value. Import `mammoth.JsonText`, `JsonNumber`, `JsonBool`, `JsonNull`, `JsonArray` and `JsonObject` to match on the kind in a `when`.

### Current limits

Response headers are Java `HttpHeaders`. The request timeout is a positive `Int` in seconds and connection timeout is 10 seconds. Redirects are not followed and requests are not retried.

Run `node tests/requests.cjs` for the local-only integration tests (requires Node).
