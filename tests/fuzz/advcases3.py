"""Third battery: Java interop soundness. Java helpers are in lib; each case either must
be rejected (R_) or must run without a type failure (A_)."""
import os, subprocess, shutil, sys, tempfile
HERE = tempfile.mkdtemp().replace('\\', '/')
COMPILER = os.environ.get('MAMMOTH', 'target/release/mammoth.exe')
JAVA = r'''
package j;
import java.util.*;
import java.util.function.*;
public class Lib {
    public static List raw() { List l = new ArrayList(); l.add(1); return l; }
    public static void fillRaw(List l) { l.add(5); }
    public static List<String> strings() { return new ArrayList<>(List.of("a")); }
    public static List<? extends Number> numbers() { return new ArrayList<>(List.of(1, 2.5)); }
    public static void sink(List<? super Integer> l) { l.add(1); }
    public static <T> T first(List<T> l) { return l.get(0); }
    public static <T extends Comparable<T>> T max(T a, T b) { return a.compareTo(b) > 0 ? a : b; }
    public static <T> void swap(T[] a) { T t = a[0]; a[0] = a[1]; a[1] = t; }
    public static String maybe(boolean b) { return b ? "x" : null; }
    public static int[] ints() { return new int[]{1, 2}; }
    public static String[][] grid() { return new String[][]{{"a"}, {null}}; }
    public static Object obj() { return "s"; }
    public static Integer boxedNull() { return null; }
    public static int unbox(Integer i) { return i; }
    public static String text(CharSequence c) { return c.toString(); }
    public static long wide(long x, int y) { return x + y; }
    public static double fl(float f) { return f; }
    public static short sh(short s) { return s; }
    public static byte by(byte b) { return b; }
    public static char ch(char c) { return c; }
    public static Map<String, List<Integer>> nested() { return new HashMap<>(Map.of("a", List.of(1))); }
    public static Function<String, Integer> fn() { return String::length; }
    public static void take(Function<? super String, ? extends Number> f) { System.out.println(f.apply("ab")); }
    public static <R> R apply(Supplier<? extends R> s) { return s.get(); }
    public static int over(int a) { return 1; }
    public static int over(Integer a) { return 2; }
    public static int over(Object a) { return 3; }
    public static int over(long a) { return 4; }
    public static int over(int... a) { return 5; }
    public static int varr(String a, Object... b) { return b.length; }
    public static class Base<T> { public T v; public Base(T v) { this.v = v; } public T get() { return v; } public void set(T x) { v = x; } }
    public static class StrBase extends Base<String> { public StrBase() { super("s"); } }
    public static abstract class Tmpl<T> { public abstract T make(); public T twice() { make(); return make(); } }
    public interface Conv<A, B> { B conv(A a); default B again(A a) { return conv(a); } }
    public enum Color { RED, GREEN; public Color next() { return values()[(ordinal() + 1) % 2]; } }
    public static final int CONST = 7;
    public static final String NAME = "n";
    public int inst = 3;
    public static class Thrower { public void go() throws Exception { throw new Exception("checked"); } }
}
'''
H = 'import j.Lib\nimport j.Lib.Base\nimport j.Lib.StrBase\nimport j.Lib.Tmpl\nimport j.Lib.Conv\nimport j.Lib.Color\nimport j.Lib.Thrower\nimport java.util.ArrayList\nimport java.util.List as JList\nimport java.util.function.Function\n'
cases = {
'R_raw_to_typed': 'let l: JList<Text> = Lib.raw()\nprint(l.get(0).length())\n',
'R_wild_add': 'Lib.numbers().add(1)\n',
'R_wild_read_int': 'let x: Int = Lib.numbers().get(0)\n',
'R_sink_text': 'Lib.sink(ArrayList<Text>())\n',
'R_first_wrong': 'let x: Int = Lib.first(Lib.strings())\n',
'R_max_mixed': 'print(Lib.max(1, "a"))\n',
'R_max_noncomparable': 'class Q\nprint(Lib.max(Q(), Q()))\n',
'R_swap_int_array': 'Lib.swap([1, 2])\n',
'R_unbox_text': 'print(Lib.unbox("a"))\n',
'R_text_int': 'print(Lib.text(1))\n',
'R_wide_double': 'print(Lib.wide(1.5, 1))\n',
'R_short_big_var': 'let a = 5\nprint(Lib.sh(a))\n',
'R_nested_wrong': 'let l: JList<Text> = Lib.nested().get("a")\n',
'R_fn_wrong': 'let f: Function<Text, Text> = Lib.fn()\n',
'R_take_wrong': 'Lib.take({ x: Int -> x })\n',
'R_base_set': 'let b = Base<Text>("a")\nb.set(1)\n',
'R_strbase_set': 'StrBase().set(1)\n',
'R_strbase_field': 'StrBase().v = 1\n',
'R_strbase_as_int': 'let b: Base<Int> = StrBase()\n',
'R_tmpl_wrong_ret': 'class T1 extends Tmpl<Text>() { override fun make(): Int { return 1 } }\nprint(1)\n',
'R_conv_wrong': 'class C1 implements Conv<Text, Int> { fun conv(a: Int): Int { return a } }\nprint(1)\n',
'R_final_static_write': 'Lib.CONST = 8\n',
'R_color_wrong_when': 'let c = Color.RED\nlet x = when (c) {\n Color.RED -> 1 }\n',
'R_nullable_string_call': 'let x: Text? = Lib.maybe(false)\nprint(x.length())\n',
'R_jlist_int_text': 'let l = ArrayList<Int>()\nl.add("a")\n',
'R_arraylist_ctor_wrong': 'let l: ArrayList<Text> = ArrayList<Int>()\n',
'R_jlist_contravariant': 'open class A\nclass B extends A\nlet l: ArrayList<A> = ArrayList<B>()\n',
'R_lambda_ret_wrong_java': 'let f: Function<Text, Int> = { x -> x }\n',
'R_lambda_arity_java': 'let f: Function<Text, Int> = { x, y -> 1 }\n',
'R_lambda_void_for_value': 'let f: Function<Text, Int> = { x -> print(x) }\n',
'R_implements_generic_wrong': 'class C2 implements Comparable<Text> { fun compareTo(o: Int): Int { return 0 } }\nprint(1)\n',
'A_raw_int_loop': 'let l = Lib.raw()\nprint(l.size())\nprint(l.get(0))\n',
'A_null_flow_checked': 'let x: Text? = Lib.maybe(false)\nprint(x == null)\nprint(x ?: "d")\n',
'A_null_flow_inferred': 'let x = Lib.maybe(true)\nprint(x?.length())\n',
'A_null_flow_nonnull_decl': 'try {\n let x: Text = Lib.maybe(false)\n print("bad")\n print(x)\n} catch (e: NullPointerException) { print("npe") }\n',
'A_null_flow_arg': 'fun f(a: Text) { print("in f") }\ntry {\n f(Lib.maybe(false))\n} catch (e: NullPointerException) { print("npe") }\n',
'A_null_flow_return': 'fun f(): Text { return Lib.maybe(false) }\ntry {\n print(f())\n} catch (e: NullPointerException) { print("npe") }\n',
'A_null_flow_generic_box': 'class P<T>(let v: T)\ntry {\n let p = P<Text>(Lib.maybe(false))\n print(p.v)\n} catch (e: NullPointerException) { print("npe") }\n',
'A_null_flow_list_add': 'let l = MutableList<Text>()\ntry {\n l.add(Lib.maybe(false))\n print(l.size())\n} catch (e: NullPointerException) { print("npe") }\n',
'A_null_flow_field': 'class K { var a: Text = "x" }\nlet k = K()\ntry {\n k.a = Lib.maybe(false)\n print("bad")\n} catch (e: NullPointerException) { print("npe") }\n',
'A_null_flow_template': 'print("{Lib.maybe(false)}")\n',
'A_null_flow_concat': 'print("a" + Lib.maybe(false))\n',
'A_null_flow_compare': 'print(Lib.maybe(false) == "x")\n',
'A_box_null_decl': 'try {\n let x: Int = Lib.boxedNull()\n print(x)\n} catch (e: NullPointerException) { print("npe") }\n',
'A_box_null_nullable': 'let x: Int? = Lib.boxedNull()\nprint(x == null)\n',
'A_grid_null': 'let g = Lib.grid()\nprint(g[0][0])\ntry {\n let s: Text = g[1][0]\n print(s)\n} catch (e: NullPointerException) { print("npe") }\n',
'A_obj_cast': 'print(Lib.obj() as Text)\nprint(Lib.obj() is Text)\n',
'A_int_array_ops': 'let a = Lib.ints()\na[0] = 5\nprint(a[0] + a.size())\n',
'A_prim_widen': 'print(Lib.wide(1, 1))\nprint(Lib.fl(1.5))\nprint(Lib.sh(1))\nprint(Lib.by(1))\nprint(Lib.ch(\'a\'))\n',
'A_overload_int': 'print(Lib.over(1))\nprint(Lib.over(1L))\nprint(Lib.over("a"))\nlet x: Int? = 1\nprint(Lib.over(x))\nprint(Lib.over())\nprint(Lib.over(1, 2))\n',
'A_varargs': 'print(Lib.varr("a"))\nprint(Lib.varr("a", 1))\nprint(Lib.varr("a", 1, "b"))\nprint(Lib.varr("a", null))\n',
'A_generic_nested': 'print(Lib.nested().get("a").get(0) + 1)\n',
'A_fn_apply': 'print(Lib.fn().apply("ab") + 1)\n',
'A_take_lambda': 'Lib.take({ x: Text -> x.length() })\nLib.take { x -> x.length() + 1 }\n',
'A_apply_infer': 'print(Lib.apply { "s" }.length())\n',
'A_base_generic': 'let b = Base<Text>("a")\nb.set("c")\nprint(b.get().length())\nprint(b.v.length())\nb.v = "d"\nprint(b.v)\n',
'A_strbase': 'let s = StrBase()\ns.set("z")\nprint(s.get().length())\nprint(s.v.length())\nlet b: Base<Text> = s\nprint(b.get())\n',
'A_tmpl_impl': 'class T1 extends Tmpl<Text>() { var n = 0\n override fun make(): Text { n += 1\n return "m{n}" } }\nprint(T1().twice())\nlet t: Tmpl<Text> = T1()\nprint(t.make().length())\n',
'A_conv_impl': 'class C1 implements Conv<Text, Int> { fun conv(a: Text): Int { return a.length() } }\nprint(C1().again("ab"))\nlet c: Conv<Text, Int> = C1()\nprint(c.conv("a") + c.again("b"))\n',
'A_conv_lambda': 'let c: Conv<Text, Int> = { a -> a.length() }\nprint(c.again("abc"))\n',
'A_enum': 'let c = Color.RED\nprint(c.next())\nprint(c.next().next() == Color.RED)\nprint(Color.values().size())\nprint(when (c) {\n Color.RED -> "r"\n Color.GREEN -> "g" })\nprint(c.compareTo(Color.GREEN))\n',
'A_consts': 'print(Lib.CONST + 1)\nprint(Lib.NAME.length())\n',
'A_instance_field': 'let l = Lib()\nl.inst = 4\nprint(l.inst + 1)\n',
'A_checked_exc': 'try { Thrower().go() } catch (e: Exception) { print(e.getMessage()) }\n',
'A_comparable_impl_generic': 'class V(let n: Int) implements Comparable<V> { fun compareTo(o: V): Int { return n - o.n } }\nprint(Lib.max(V(1), V(2)).n)\n',
'A_max_text': 'print(Lib.max("a", "b").length())\nprint(Lib.max(1, 2) + 1)\n',
'A_object_methods_on_mammoth': 'class K\nprint(K().equals(K()))\nprint(K().hashCode() != 0 or true)\nprint(K().toString().length() > 0)\n',
'A_object_methods_on_prim': 'print(1.toString())\nprint(1.equals(1))\nprint(1.hashCode())\nprint((1.5).isNaN())\n',
'A_generic_class_java_list': 'class Store<T> { let items = ArrayList<T>()\n fun add(x: T) { items.add(x) }\n fun first(): T { return items.get(0) } }\nlet s = Store<Int>()\ns.add(4)\nprint(s.first() + 1)\nlet t = Store<Text>()\nt.add("a")\nprint(t.first().length())\n',
'A_java_list_of_mammoth_generic': 'class P<T>(let v: T)\nlet l = ArrayList<P<Int>>()\nl.add(P(1))\nprint(l.get(0).v + 1)\nfor p in l { print(p.v + 2) }\n',
'A_java_map_nullable_get': 'import java.util.HashMap\nlet m = HashMap<Text, Int>()\nlet v: Int? = m.get("a")\nprint(v == null)\nlet w: Int? = m["a"]\nprint(w == null)\n',
'A_map_missing_nonnull': 'import java.util.HashMap\nlet m = HashMap<Text, Int>()\ntry {\n let v: Int = m.get("a")\n print(v)\n} catch (e: NullPointerException) { print("npe") }\n',
'A_map_long_key_widen': 'import java.util.HashMap\nlet m = HashMap<Long, Int>()\nm[1] = 2\nprint(m[1L] + 1)\nprint(m[1] + 1)\n',
'A_map_any_key': 'import java.util.HashMap\nlet m = HashMap<Any, Int>()\nm[1] = 2\nm["a"] = 3\nprint(m["a"] + m[1])\n',
'A_map_null_key': 'import java.util.HashMap\nlet m = HashMap<Text, Int>()\nm[null] = 2\nprint(m[null])\n',
'A_mlist_null_elem': 'let l = MutableList<Text?>()\nl.add(null)\nprint(l[0] == null)\n',
'A_list_null_elem': 'let x: Text? = null\nlet l: List<Text?> = [x, "a"]\nprint(l[0])\nprint(l[1]?.length())\nfor s in l { print(s == null) }\n',
}
L = HERE + '/adv3'
os.makedirs(L + '/j', exist_ok=True)
open(L + '/j/Lib.java', 'w', encoding='utf-8').write(JAVA)
subprocess.run(['javac', '-nowarn', '-Xlint:none', '-d', L + '/cp', L + '/j/Lib.java'], check=True, capture_output=True)
env = dict(os.environ, CLASSPATH=L + '/cp')
for name, src in cases.items():
    path = L + '/' + name + '.mam'
    lines = src.split('\n')
    extra = [l for l in lines if l.startswith('import ')]
    body = '\n'.join(l for l in lines if not l.startswith('import '))
    open(path, 'w', encoding='utf-8').write(H + '\n'.join(extra) + ('\n' if extra else '') + body)
    out = L + '/o'
    shutil.rmtree(out, ignore_errors=True)
    p = subprocess.run([COMPILER, path, out], capture_output=True, timeout=60, env=env)
    if p.returncode != 0:
        lines = p.stderr.decode('utf-8', 'replace').strip().splitlines()
        msg = lines[0].split(': ', 1)[-1] if lines else f'exit {p.returncode}'
        flag = '' if name.startswith('R_') else '   <<<<'
        print(f'REJ  {name}: {msg[:100]}{flag}  [{lines[1].strip()[:60] if len(lines)>1 and flag else ""}]')
    else:
        r = subprocess.run(['java', '-Xverify:all', '-cp', out + ';' + L + '/cp', 'Main'], capture_output=True, timeout=30)
        text = (r.stdout + r.stderr).decode('utf-8', 'replace').strip().replace('\r', '').split('\n')
        flag = '   <<<<' if name.startswith('R_') or r.returncode != 0 else ''
        print(f'OK   {name} => {" | ".join(text[-7:])[:200]}{flag}')
