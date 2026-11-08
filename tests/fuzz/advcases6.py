"""Adversarial checks for object expressions, infix calls, Nothing, private constructors and range values."""
import os, subprocess, shutil, sys
HERE = __import__('tempfile').mkdtemp().replace(chr(92), '/')
COMPILER = os.environ.get('MAMMOTH', 'target/release/mammoth.exe')
I = 'interface I { fun f(): Int }\n'
cases = {
'R_obj_missing_method': I + 'let i: I = object implements I { }\n',
'R_obj_wrong_result': I + 'let i: I = object implements I { override fun f(): Text = "x" }\n',
'R_obj_wrong_assign': I + 'let t: Text = object implements I { override fun f(): Int = 1 }\n',
'R_obj_extra_member_via_iface': I + 'let i: I = object implements I { override fun f(): Int = 1\n fun g(): Int = 2 }\nprint(i.g())\n',
'R_obj_body_type_error': I + 'let n = "x"\nlet i: I = object implements I { override fun f(): Int = n }\n',
'R_obj_final_java': 'let s = object extends String() { }\n',
'R_obj_local_declared_after': I + 'let i: I = object implements I { override fun f(): Int = later }\nlet later = 1\nprint(i.f())\n',
'R_obj_return_wrong': I + 'let i: I = object implements I { override fun f(): Int { return "x" } }\n',
'R_obj_no_return': I + 'let i: I = object implements I { override fun f(): Int { } }\n',
'R_obj_write_let': I + 'let n = 1\nlet i: I = object implements I { override fun f(): Int { n = 2\n return n } }\n',
'A_obj_shadow_param': 'interface J { fun g(x: Int): Int }\nlet j: J = object implements J { let x = 10\n override fun g(x: Int): Int = x + this.x }\nprint(j.g(1))\n',
'A_obj_shadow_local': I + 'let i: I = object implements I { let x = 10\n override fun f(): Int { let x = 3\n return x } }\nprint(i.f())\n',
'A_obj_nested': I + 'let i: I = object implements I { let a = 1\n override fun f(): Int { let j: I = object implements I { override fun f(): Int = a + 1 }\n return j.f() + a } }\nprint(i.f())\n',
'A_obj_in_lambda': I + 'let mk = { n: Int -> object implements I { override fun f(): Int = n * 2 } }\nprint(mk(3).f())\n',
'A_obj_in_field': I + 'class K(let n: Int) { let i: I = object implements I { override fun f(): Int = 7 } }\nprint(K(1).i.f())\n',
'A_obj_mutates_outer': I + 'var c = 0\nlet i: I = object implements I { override fun f(): Int { c += 1\n return c } }\ni.f()\nprint(i.f() + c)\n',
'A_obj_field_property': I + 'let i: I = object implements I { var n = 1\n  set(v) { field = v * 2 }\n override fun f(): Int { n = 5\n return n } }\nprint(i.f())\n',
'A_obj_overload_methods': 'interface J { fun g(x: Int): Int\n fun g(x: Text): Int }\nlet j: J = object implements J { override fun g(x: Int): Int = x\n override fun g(x: Text): Int = x.length }\nprint(j.g(2) + j.g("abc"))\n',
'A_obj_java_generic_param': 'import java.util.function.Function\nfun app(f: Function<Text, Int>): Int = f.apply("ab")\nprint(app(object implements Function<Text, Int> { override fun apply(t: Text): Int = t.length }))\n',
'A_obj_super_method': 'open class A { open fun g(): Int = 1 }\nlet a: A = object extends A() { override fun g(): Int = 5 }\nprint(a.g())\n',
'A_obj_abstract_java': 'import java.util.TimerTask\nlet t = object extends TimerTask() { override fun run() { print("x") } }\nt.run()\n',
'A_obj_tostring': 'let o = object { override fun toString(): Text = "custom" }\nprint(o)\n',
'A_obj_two_on_one_line': I + 'let a: I = object implements I { override fun f(): Int = 1 }; let b: I = object implements I { override fun f(): Int = 2 }\nprint(a.f() + b.f())\n',
'R_infix_wrong_type': 'infix fun Int.addTo(o: Int): Int = this + o\nprint(1 addTo "x")\n',
'R_infix_null_receiver': 'infix fun Int.addTo(o: Int): Int = this + o\nlet a: Int? = null\nprint(a addTo 1)\n',
'A_infix_precedence': 'infix fun Int.addTo(o: Int): Int = this + o\nprint(1 + 2 addTo 3 * 2)\nprint(1 addTo 2 == 3)\n',
'A_infix_newline_is_new_statement': 'infix fun Int.addTo(o: Int): Int = this + o\nlet addTo2 = 1\nlet x = 1\nprint(x)\n',
'A_infix_name_as_variable': 'let union = 1\nlet subtract = union + 1\nprint(subtract)\n',
'A_infix_name_method_call': 'infix fun Int.addTo(o: Int): Int = this + o\nprint(1.addTo(2))\n',
'R_nothing_as_int_assign': 'fun f(): Nothing = throw RuntimeException("x")\nlet x: Text = f().length\n',
'R_nothing_falls_off': 'fun f(): Nothing { print(1) }\n',
'A_nothing_in_if_value': 'fun f(): Nothing = throw RuntimeException("x")\nfun g(a: Int): Int = if a > 0 { a } else { f() }\nprint(g(1))\n',
'A_nothing_unreachable_ok': 'fun f(): Nothing = throw RuntimeException("x")\nfun g(): Int { f() }\ntry { g() } catch (e: RuntimeException) { print(e.message) }\n',
'A_nothing_when_branch': 'fun f(): Nothing = throw RuntimeException("x")\nfun g(a: Int): Text = when (a) {\n 1 -> "one"\n else -> f()\n}\nprint(g(1))\n',
'R_private_ctor_subclass': 'open class K private constructor(let a: Int)\nclass L extends K(1)\n',
'R_private_ctor_outside_secondary': 'class K private constructor(let a: Int) { constructor(s: Text) : this(1) }\nprint(K(1))\n',
'A_private_ctor_inside': 'class K private constructor(let a: Int) { fun copy2(): K = K(a + 1) }\n',
'R_range_text': 'let r = "a".."b"\n',
'R_range_elem_type': 'let r = 1..3\nlet t: Text = r[0]\n',
'R_range_long': 'let r = 1L..3L\n',
'A_range_negative': 'print((3..1).size)\n',
'A_range_function': 'fun f(): Int = 3\nlet r = 1..f() + 1\nprint(r)\n',
'A_range_in_when': 'print(when (2) {\n in 1..3 -> "y"\n else -> "n"\n})\n',
'A_range_header_paren': 'if (1..3).size == 3 { print("y") }\n',
'A_range_arg': 'fun f(l: MutableList<Int>): Int = l.size\nprint(f(1..3))\n',
'R_return_label_outside_lambda': 'fun f(): Int { return@nope 1 }\n',
'A_return_label_unit_lambda': 'var n = 0\nlistOf(1, 2).forEach { if it == 1 { return@forEach }\n n += it }\nprint(n)\n',
'R_return_label_mismatch': 'print(listOf(1).map { if it == 1 { return@map "a" }\n it })\n',
'A_destructure_shadow': 'print(listOf(Pair(1, 2)).map { (a, _) -> a })\n',
'R_destructure_lambda_type': 'print(listOf(1).map { (a, b) -> a })\n',
'R_compare_regex_type': 'print(Regex("a").matches(1))\n',
'A_regex_ops': 'let r = Regex("[0-9]+")\nprint(r.findAll("a1b22"))\nprint(r.replace("a1", "x"))\nprint(r.find("zz"))\nprint("a,b".toRegex().matches("a,b"))\n',
'A_user_regex_wins': 'class Regex(let a: Int)\nprint(Regex(1).a)\n',
'A_generic_ref_member': 'data class P(let n: Int)\nprint(listOf(P(1)).map(P::n))\n',
'R_member_ref_type': 'let f: (Text) -> Text = Text::length\n',
}
os.makedirs(HERE + '/adv6', exist_ok=True)
only = sys.argv[1:]
for name, src in cases.items():
    if only and name not in only:
        continue
    path = HERE + '/adv6/' + name + '.mam'
    open(path, 'w', encoding='utf-8').write(src)
    out = HERE + '/adv6/o'
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
