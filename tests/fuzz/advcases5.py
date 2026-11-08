"""Adversarial checks for constructors, property accessors, and the sized list and library additions."""
import os, subprocess, shutil, sys
HERE = __import__('tempfile').mkdtemp().replace(chr(92), '/')
COMPILER = os.environ.get('MAMMOTH', 'target/release/mammoth.exe')
FLIP = 'class K {\n var n = 0\n var a: Text?\n  get() { n += 1\n   return if n == 1 { "x" } else { null } }\n  set(v) { }\n}\n'
cases = {
# Smart casts must not trust anything that can run code.
'R_smartcast_getter': FLIP + 'let k = K()\nif k.a != null { print(k.a.length) }\n',
'R_smartcast_getter_this': 'class K {\n var n = 0\n let a: Text? get() = null\n fun f(): Int { if a != null { return a.length }\n return 0 }\n}\nprint(K().f())\n',
'R_smartcast_open_prop': 'open class B { open let a: Text? = "x" }\nfun f(b: B): Int { if b.a != null { return b.a.length }\n return 0 }\nprint(f(B()))\n',
'R_smartcast_interface_prop': 'interface I { let a: Text? }\nfun f(i: I): Int { if i.a != null { return i.a.length }\n return 0 }\n',
'R_elvis_ok_but_bang_needed': FLIP + 'let k = K()\nlet s: Text = k.a\n',
'A_elvis_getter': FLIP + 'let k = K()\nprint(k.a ?: "none")\nprint(k.a ?: "none")\n',
'A_safe_call_getter': FLIP + 'let k = K()\nprint(k.a?.length)\nprint(k.a?.length)\n',
# Variance and star through accessors.
'R_out_var_property': 'interface I<out T> { var x: T }\nprint(1)\n',
'A_out_let_property': 'interface I<out T> { let x: T }\nclass C(let x: Text) implements I<Text>\nlet i: I<Any> = C("a")\nprint(i.x)\n',
'R_star_write_setter': 'class B<T>(var v: T) {\n var w: T = v\n  set(n) { field = n }\n}\nlet b: B<*> = B<Text>("a")\nb.w = "x"\n',
'R_generic_setter_type': 'class B<T>(var v: T) {\n var w: T = v\n  set(n) { field = n }\n}\nlet b = B<Text>("a")\nb.w = 1\n',
'A_generic_setter_int': 'class B<T>(var v: T) {\n var w: T = v\n  set(n) { field = n }\n}\nlet b = B<Int>(1)\nb.w = 5\nb.w += 2\nprint(b.w + 1)\n',
'A_generic_getter_cast': 'class B<T>(let v: T) {\n let w: T get() = v\n}\nprint(B("abc").w.length)\nprint(B(2).w + 1)\n',
'R_setter_nullable_into_nonnull': 'class K { var a: Text = "x"\n set(v) { field = v } }\nlet n: Text? = null\nK().a = n\n',
# Order of initialization.
'R_global_getter_order': 'class K { let a: Text get() = t }\nprint(K().a.length)\nlet t: Text = "x"\n',
'R_global_setter_order': 'class K { var a: Int\n get() = t.length\n set(v) { print(t.length) } }\nK().a = 1\nlet t: Text = "x"\n',
'R_global_secondary_order': 'class K(let v: Int) { constructor(s: Text) : this(t.length) }\nprint(K("x").v)\nlet t: Text = "x"\n',
'R_global_secondary_body_order': 'class K(let v: Int) { var w = 0\n constructor(s: Text) : this(1) { w = t.length } }\nprint(K("x").w)\nlet t: Text = "x"\n',
'R_this_in_delegate_lambda': 'class A(let f: () -> Int) { let x = 1\n constructor(n: Int) : this({ x }) }\nprint(A(1).f())\n',
'R_this_in_delegate': 'class A(let f: Any) { constructor(n: Int) : this(this) }\n',
'R_method_in_delegate': 'class A(let n: Int) { fun g(): Int = 1\n constructor(s: Text) : this(g()) }\n',
'R_field_read_in_initializer_via_this': 'class A { let y: Text = this.x\n open let x: Text = "a" }\n',
'A_secondary_then_field_order': 'class A(let n: Int) { var log = "a"\n init { log = log + "b" }\n constructor(s: Text) : this(1) { log = log + "c" }\n}\nprint(A("x").log)\n',
# Constructors: generics, Java, inheritance.
'A_generic_secondary_infer': 'class A<T>(let a: T) { var n = 0\n constructor(a: T, n: Int) : this(a) { this.n = n } }\nprint(A("x", 2).a.length)\nprint(A(1, 2).a + 1)\nlet e: A<Text> = A("y", 1)\nprint(e.n)\n',
'R_generic_secondary_expected': 'class A<T>(let a: T) { constructor(a: T, n: Int) : this(a) }\nlet e: A<Text> = A(1, 2)\n',
'A_super_picks_secondary': 'open class P(let a: Int) { constructor(s: Text) : this(s.length) }\nclass C extends P("abc")\nclass D extends P(7)\nprint(C().a + D().a)\n',
'R_super_no_match': 'open class P(let a: Int) { constructor(s: Text) : this(s.length) }\nclass C extends P(1.5)\n',
'R_abstract_secondary': 'abstract class P(let a: Int) { constructor(s: Text) : this(1) }\nP("x")\n',
'A_java_sees_ctor': 'import java.lang.reflect.Modifier\nclass K(let a: Int) { constructor(s: Text) : this(1) }\nprint(K(1).getClass().getConstructors().size)\n',
'A_hidden_primary_private': 'class K { constructor(a: Int) { } }\nprint(K(1).getClass().getConstructors().size)\n',
'A_lambda_in_secondary_body': 'class K(let a: Int) { var f: () -> Int = { 0 }\n constructor(s: Text) : this(1) { f = { a + s.length } } }\nprint(K("ab").f())\n',
'A_named_args_secondary': 'class K(let a: Int) { constructor(x: Text, y: Int) : this(y) }\nprint(K(y = 2, x = "s").a)\n',
'A_default_primary_with_secondary': 'class K(let a: Int, let b: Int = 5) { constructor(s: Text) : this(1) }\nprint(K(1).b)\nprint(K("s").b)\nprint(K(1, 2).b)\n',
'R_vararg_secondary': 'class K(let a: Int) { constructor(vararg s: Text) : this(1) }\n',
'R_default_secondary': 'class K(let a: Int) { constructor(s: Text = "x") : this(1) }\n',
'R_nullable_overload_null': 'class K(let a: Text) { constructor(b: Int) : this("x") }\nK(null)\n',
# Override corner cases.
'R_override_var_with_let': 'open class B { open var a: Int = 1 }\nclass C extends B { override let a: Int = 2 }\nlet b: B = C()\nb.a = 3\nprint(b.a)\n',
'A_override_let_with_var': 'open class B { open let a: Int = 1 }\nclass C extends B { override var a: Int = 2 }\nlet c = C()\nc.a = 3\nlet b: B = c\nprint(b.a)\n',
'R_private_override': 'open class B { open let a: Int = 1 }\nclass C extends B { private override let a: Int = 2 }\n',
'R_abstract_private': 'abstract class B { private abstract let a: Int }\n',
'R_static_abstract': 'abstract class B { static abstract let a: Int = 1 }\n',
'R_data_var_override_missing': 'interface I { var x: Int }\ndata class D(let x: Int) implements I\n',
'A_super_property': 'open class B { open let a: Int = 1 }\nclass C extends B { override let a: Int = 2\n fun f(): Int = super.a + a }\nprint(C().f())\n',
'A_interface_prop_lambda': 'interface I { var n: Int }\nclass C(var n: Int) implements I\nlet i: I = C(1)\nlet f = { i.n = i.n + 1 }\nf()\nprint(i.n)\n',
'A_setter_uses_other': 'class K { var a = 0\n var b = 0\n  set(v) { field = v\n a = v * 2 } }\nlet k = K()\nk.b = 2\nprint(k.a)\n',
'A_field_local_shadow': 'class K { var a = 1\n  set(v) { let field2 = v\n field = field2 + 1 } }\nlet k = K()\nk.a = 2\nprint(k.a)\n',
'A_field_not_keyword': 'fun f(field: Int): Int = field + 1\nprint(f(1))\nclass K(let field: Int)\nprint(K(1).field)\nvar field = 2\nfield = 3\nprint(field)\n',
'A_get_set_as_names': 'let get = 1\nlet set = 2\nprint(get + set)\nclass K { fun get(): Int = 1\n fun set(v: Int): Int = v }\nprint(K().get() + K().set(2))\n',
'A_prop_then_get_method': 'class K { var a = 1\n fun get(): Int = a\n fun set(): Int = 2 }\nprint(K().get())\n',
'A_private_then_set_method': 'class K { var a = 1\n private fun set2(): Int = 2\n fun g(): Int = set2() }\nprint(K().g())\n',
'A_private_member_after_field': 'class K { var a = 1\n private let set = 2\n fun g(): Int = set }\nprint(K().g())\n',
# Sized lists and library.
'R_list_negative_type': 'let a = List<Text>(2) { 1 }\n',
'A_list_negative_runtime': 'try { print(List<Int>(0 - 1).size) } catch (e: NegativeArraySizeException) { print("neg") }\n',
'A_list_of_lists': 'let a = List<List<Int>>(2) { i -> List<Int>(i + 1) { it } }\nprint(a[1][1])\n',
'A_list_objects': 'data class P(let x: Int)\nlet a = List<P>(2) { P(it) }\nprint(a[1])\n',
'A_list_nullable_int': 'let a = List<Int?>(2)\nprint(a[0] == null)\na[1] = 3\nprint(a[1])\n',
'A_list_capture_var': 'var c = 0\nlet a = List<Int>(3) { c += 1\n c * 2 }\nprint(a[2] + c)\n',
'R_sort_with_wrong_cmp': 'let c: java.util.Comparator<Text>? = null\nprint(listOf(1).sortedWith(compareBy { it.length }))\n',
'R_compare_by_uncomparable': 'class V\nprint(listOf(V()).sortedWith(compareBy { it }))\n',
'R_map_plus_wrong': 'print(mapOf("a" to 1) + ("b" to "c"))\n',
'A_sum_of_nested_lambda_vars': 'var t = 0.0\nprint(listOf(1, 2).sumOf { x -> t += 1.0\n x * 1.5 })\nprint(t)\n',
'A_user_sum_of': 'class B { fun sumOf(f: (Int) -> Text): Text = f(1) }\nprint(B().sumOf { "x" })\n',
'A_user_list_generic_class': 'class List2<T>(let v: T)\nprint(List2<Int>(1).v)\n',
}
os.makedirs(HERE + '/adv5', exist_ok=True)
only = sys.argv[1:]
for name, src in cases.items():
    if only and name not in only:
        continue
    path = HERE + '/adv5/' + name + '.mam'
    open(path, 'w', encoding='utf-8').write(src)
    out = HERE + '/adv5/o'
    shutil.rmtree(out, ignore_errors=True)
    p = subprocess.run([COMPILER, path, out], capture_output=True, timeout=60, stdin=subprocess.DEVNULL)
    if p.returncode != 0:
        lines = p.stderr.decode('utf-8', 'replace').strip().splitlines()
        msg = lines[0].split(': ', 1)[-1] if lines else f'exit {p.returncode}'
        flag = '' if name.startswith('R_') else '   <<<<'
        print(f'REJ  {name}: {msg[:95]}{flag}')
    else:
        r = subprocess.run(['java', '-Xverify:all', '-cp', out, 'Main'], capture_output=True, timeout=30, stdin=subprocess.DEVNULL)
        text = (r.stdout + r.stderr).decode('utf-8', 'replace').strip().replace('\r', '').split('\n')
        flag = '   <<<<' if name.startswith('R_') or r.returncode != 0 else ''
        print(f'OK   {name} => {" | ".join(text[-6:])[:170]}{flag}')
