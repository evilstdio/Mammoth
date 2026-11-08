"""Third round: generics, interfaces, operators and larger interactions. Prints each program's output."""
import os, subprocess, shutil, sys
HERE = __import__('tempfile').mkdtemp().replace(chr(92), '/')
COMPILER = os.environ.get('MAMMOTH', 'target/release/mammoth.exe')
cases = {
'ext_property': 'let Text.firstChar: Char get() = this[0]\nprint("ab".firstChar)\n',
'ext_property_generic': 'let MutableList<T>.second<T>: T get() = this[1]\nprint(listOf(1, 2).second)\n',
'custom_iterator': 'class R(let n: Int) implements Iterable<Int> {\n override fun iterator(): java.util.Iterator<Int> = mutableListOf(n).iterator()\n}\nfor x in R(2) { print(x) }\n',
'iterator_operator': 'class C { fun iterator(): java.util.Iterator<Int> = mutableListOf(1).iterator() }\nfor x in C() { print(x) }\n',
'data_sorted_comparable': 'data class V(let n: Int) implements Comparable<V> { override fun compareTo(o: V): Int = n - o.n }\nprint(listOf(V(2), V(1)).sorted())\nprint(V(1) < V(2))\nprint(listOf(V(2), V(1)).maxOrNull())\n',
'data_in_set_map': 'data class V(let n: Int)\nprint(setOf(V(1), V(1)).size)\nlet m = mutableMapOf(V(1) to "a")\nprint(m[V(1)])\n',
'generic_where': 'fun f<T>(x: T): Text where T: Comparable<T> { return "ok" }\nprint(f(1))\n',
'generic_bound_class': 'class S<T: Comparable<T>>(let a: T, let b: T) { fun max(): T = if a > b { a } else { b } }\nprint(S(1, 2).max())\nprint(S("a", "b").max())\n',
'generic_two_bounds': 'fun f<T: Comparable<T>>(l: MutableList<T>): T { var b = l[0]\n for x in l { if x > b { b = x } }\n return b }\nprint(f(listOf(1, 3, 2)))\n',
'generic_lambda_infer': 'fun <A, B> pipe(a: A, f: (A) -> B): B = f(a)\nprint(pipe(1) { it + 1 })\n',
'generic_lambda_infer2': 'fun pipe<A, B>(a: A, f: (A) -> B): B = f(a)\nprint(pipe(1) { it.toString() + "x" })\n',
'generic_nested_call': 'fun id<T>(x: T): T = x\nprint(id(id(1)) + 1)\nprint(id(listOf(1)).size)\n',
'generic_class_method_generic': 'class B<T>(let v: T) { fun map<R>(f: (T) -> R): B<R> = B<R>(f(v)) }\nprint(B(1).map { it + 1 }.map { it.toString() }.v.length)\n',
'iface_default_super': 'interface I { fun f(): Int = 1 }\nclass A implements I { override fun f(): Int = super.f() + 1 }\nprint(A().f())\n',
'iface_two_defaults': 'interface I { fun f(): Int = 1 }\ninterface J { fun f(): Int = 2 }\nclass A implements I, J { override fun f(): Int = 3 }\nprint(A().f())\n',
'abstract_template': 'abstract class T { abstract fun step(): Int\n fun run(): Int = step() * 2 }\nclass A extends T { override fun step(): Int = 4 }\nprint(A().run())\n',
'abstract_ctor': 'abstract class T(let n: Int) { abstract fun f(): Int }\nclass A extends T(3) { override fun f(): Int = n + 1 }\nprint(A().f())\n',
'init_order': 'open class P { init { print("p") } }\nclass C extends P { let x = 1\n init { print(x) } }\nC()\n',
'polymorphic_list': 'abstract class S { abstract fun a(): Double }\nclass C(let r: Double) extends S { override fun a(): Double = 3.0 * r }\nclass Q(let w: Double) extends S { override fun a(): Double = w * w }\nlet l: MutableList<S> = mutableListOf(C(1.0), Q(2.0))\nprint(l.sumOf { it.a() })\n',
'companion_const': 'class K { companion object { let MAX = 3\n fun twice(): Int = MAX * 2 } }\nprint(K.MAX + K.twice())\n',
'object_state': 'object C { var n = 0\n fun inc(): Int { n += 1\n return n } }\nC.inc()\nprint(C.inc())\n',
'enum_iter': 'enum E { A, B }\nfor e in E.values() { print(e) }\nprint(E.values().map { it.name })\n',
'enum_compare': 'enum E { A, B }\nprint(E.A == E.A)\nprint(E.A != E.B)\nprint(E.A.compareTo(E.B))\n',
'enum_when_expr_no_else': 'enum E { A, B }\nfun f(e: E): Int = when (e) {\n E.A -> 1\n E.B -> 2\n}\nprint(f(E.B))\n',
'enum_abstract_method': 'enum E { A { override fun f(): Int = 1 }, B { override fun f(): Int = 2 };\n abstract fun f(): Int }\nprint(E.B.f())\n',
'use_closeable': 'class R implements AutoCloseable { override fun close() { print("closed") } }\nR().use { print("in") }\n',
'try_with_return': 'fun f(): Int { try { return 1 } finally { print("f") } }\nprint(f())\n',
'exception_hierarchy': 'open class A(m: Text) extends Exception(m)\nclass B(m: Text) extends A(m)\ntry { throw B("x") } catch (e: A) { print(e.message) }\n',
'exception_cause': 'try { throw RuntimeException("a", IllegalStateException("b")) } catch (e: RuntimeException) { print(e.cause?.message) }\n',
'nested_try': 'try { try { throw RuntimeException("a") } finally { print("in") } } catch (e: Exception) { print("out") }\n',
'stack_trace_line': 'fun f() { throw RuntimeException("x") }\ntry { f() } catch (e: RuntimeException) { print(e.getStackTrace()[0].getLineNumber()) }\n',
'closure_counter': 'fun c(): () -> Int { var n = 0\n return { n += 1\n n } }\nlet k = c()\nk()\nprint(k())\n',
'lambda_in_list': 'let fs: MutableList<(Int) -> Int> = mutableListOf({ it + 1 }, { it * 2 })\nprint(fs.map { it(3) })\n',
'lambda_nullable_call': 'let f: ((Int) -> Int)? = { it + 1 }\nprint(f?.invoke(1))\nif f != null { print(f(2)) }\n',
'lambda_invoke': 'let f = { x: Int -> x + 1 }\nprint(f.invoke(1))\n',
'fun_type_param_ret': 'fun twice(f: (Int) -> Int): (Int) -> Int = { f(f(it)) }\nprint(twice { it + 1 }(1))\n',
'recursion_deep': 'fun s(n: Int): Long = if n == 0 { 0L } else { n + s(n - 1) }\nprint(s(1000))\n',
'tailrec': 'tailrec fun s(n: Int, a: Int): Int = if n == 0 { a } else { s(n - 1, a + n) }\nprint(s(10, 0))\n',
'long_overflow': 'print(Long.MAX_VALUE + 1)\nprint(Int.MAX_VALUE + 1)\n',
'int_long_mix': 'let a: Long = 1\nlet b = a + 1\nprint(b * 2)\nprint(3 + b)\n',
'double_format': 'print(1.0)\nprint(100.5)\nprint(1.0 / 3)\n',
'char_digit': 'print(\'7\' - \'0\')\nprint(\'a\'.code + 1)\n',
'string_template_expr': 'let a = listOf(1, 2)\nprint("n={a.size} first={a[0]} s={"x".length}")\n',
'string_escape': 'print("a\\tb\\n\\"q\\"\\\\")\n',
'string_unicode': 'print("é✓".length)\nprint("é✓")\n',
'string_compare_ops': 'print("a".compareTo("b"))\nprint("a" == "a")\nprint("abc".equals("ABC", true))\n',
'string_index_for': 'for (i, c) in "ab".withIndex() { print(i)\n print(c) }\n',
'string_split_map': 'print("1,2".split(",").map { it.toInt() }.sum())\n',
'string_builder_ops': 'let b = StringBuilder("a")\nb.insert(0, "x")\nb.reverse()\nprint(b)\n',
'if_else_if_chain': 'fun f(a: Int): Text { if a > 2 { return "b" } else if a > 1 { return "m" } else { return "s" } }\nprint(f(2))\n',
'nested_when': 'print(when (1) {\n 1 -> when (2) {\n  2 -> "x"\n  else -> "y"\n }\n else -> "z"\n})\n',
'when_range_list': 'print(when (3) {\n in listOf(1, 3) -> "l"\n else -> "n"\n})\n',
'list_mutate': 'let l = mutableListOf(1)\nl.add(0, 2)\nl.remove(1)\nl.addAll(listOf(5))\nl[0] = 9\nprint(l)\nprint(l.contains(5))\nl.removeAt(0)\nl.clear()\nprint(l.isEmpty())\n',
'list_sublist': 'print(listOf(1, 2, 3).subList(1, 2))\nprint(listOf(1, 2, 3).slice(0..1))\n',
'list_equals': 'print(listOf(1) == listOf(1))\nprint([1] == [1])\n',
'array_ops': 'let a = [3, 1]\nprint(a.size)\nprint(a.sorted())\nprint(a.contains(1))\nprint(a.toList())\nprint(a.map { it * 2 })\nprint(a.sum())\n',
'map_mutate': 'let m = mutableMapOf<Text, Int>()\nm["a"] = 1\nm.put("b", 2)\nm.remove("a")\nprint(m.containsKey("b"))\nprint(m.size)\nfor (k, v) in m { print(k) }\nm["b"] = m["b"]!! + 1\nprint(m)\n',
'map_getorput': 'let m = mutableMapOf<Text, MutableList<Int>>()\nm.getOrPut("a") { mutableListOf() }.add(1)\nprint(m)\n',
'map_null_get': 'let m = mapOf("a" to 1)\nlet v: Int? = m["z"]\nprint(v ?: 0)\n',
'map_entries_sorted': 'print(mapOf("b" to 1, "a" to 2).entries.sortedBy { it.key }.map { it.value })\n',
'map_tolist': 'print(mapOf("a" to 1).toList())\n',
'pair_ops': 'let p = 1 to "a"\nprint(p.first)\nprint(p.second)\nlet (a, b) = p\nprint(a + 1)\nprint(p)\n',
'function_returns_pair': 'fun f(): Pair<Int, Text> = Pair(1, "a")\nlet (x, y) = f()\nprint(y)\n',
'triple': 'let t = Triple(1, "a", 2.0)\nprint(t.third)\n',
'nullable_generics': 'fun f<T>(x: T?): T? = x\nprint(f<Int>(null))\nprint(f(1))\n',
'nullable_list': 'let l: MutableList<Int?> = mutableListOf(1, null)\nprint(l.filterNotNull())\nprint(l.mapNotNull { it })\n',
'elvis_chain': 'let a: Int? = null\nlet b: Int? = null\nprint(a ?: b ?: 3)\n',
'safe_call_chain_method': 'let s: Text? = "ab"\nprint(s?.uppercase()?.length)\n',
'safe_call_assign': 'class K { var a = 1 }\nlet k: K? = K()\nk?.a = 2\nprint(k?.a)\n',
'not_null_assert_chain': 'class K(let n: K?)\nprint(K(K(null)).n!!.n == null)\n',
'smart_cast_field_let': 'class K(let a: Text?)\nlet k = K("x")\nif k.a != null { print(k.a.length) }\n',
'smart_cast_local_var': 'var a: Text? = "x"\nif a != null { print(a.length) }\n',
'smart_cast_early_return': 'fun f(a: Text?): Int { if a == null { return 0 }\n return a.length }\nprint(f("a"))\n',
'smart_cast_and': 'fun f(a: Text?): Bool = a != null and a.length > 0\nprint(f("a"))\n',
'smart_cast_or': 'fun f(a: Text?): Bool = a == null or a.length > 0\nprint(f("a"))\n',
'smart_cast_while': 'var a: Text? = "x"\nwhile a != null { print(a.length)\n a = null }\n',
'smart_cast_is_field': 'class K(let a: Any)\nlet k = K("x")\nif k.a is Text { print(k.a.length) }\n',
'smart_cast_when': 'fun f(a: Any): Int = when {\n a is Text -> a.length\n a is Int -> a + 1\n else -> 0\n}\nprint(f(1))\n',
'java_stream': 'import java.util.stream.Collectors\nprint(mutableListOf(1, 2).stream().map({ x: Int -> x * 2 }).collect(Collectors.toList()))\n',
'java_optional': 'import java.util.Optional\nprint(Optional.of(1).map({ x: Int -> x + 1 }).get())\n',
'java_files': 'import java.nio.file.Files\nimport java.nio.file.Path\nlet p = Files.createTempFile("m", ".txt")\nFiles.writeString(p, "hi")\nprint(Files.readString(p))\nFiles.delete(p)\n',
'java_time': 'import java.time.LocalDate\nprint(LocalDate.of(2020, 1, 31).plusMonths(1))\n',
'java_bigdecimal': 'import java.math.BigDecimal\nprint(BigDecimal("1.10") + BigDecimal("2.20"))\n',
'java_bigint': 'import java.math.BigInteger\nprint(BigInteger.TWO.pow(100))\n',
'java_interface_lambda': 'import java.util.function.BiFunction\nlet f: BiFunction<Int, Int, Int> = { a, b -> a + b }\nprint(f.apply(1, 2))\n',
'java_thread_lambda': 'let t = Thread { print("t") }\nt.start()\nt.join()\n',
'java_static_field': 'print(Integer.MAX_VALUE)\nprint(Math.PI > 3.0)\n',
'java_string_methods': 'print("a b".split(" ").size)\nprint("A".toLowerCase())\nprint(String.valueOf(1))\nprint(String.join("-", listOf("a", "b")))\n',
'java_hashmap_iter': 'import java.util.HashMap\nlet m = HashMap<Text, Int>()\nm["a"] = 1\nfor e in m.entrySet() { print(e.getKey() + e.getValue().toString()) }\n',
'java_arrays': 'import java.util.Arrays\nlet a = [3, 1]\nArrays.sort(a)\nprint(Arrays.toString(a))\nprint(Arrays.asList(1, 2))\n',
'java_random': 'import java.util.Random\nprint(Random(1L).nextInt(10) >= 0)\n',
'java_scanner': 'import java.util.Scanner\nlet s = Scanner("1 2")\nprint(s.nextInt() + s.nextInt())\n',
'java_exception_message': 'try { "x".toInt() } catch (e: NumberFormatException) { print(e.message) }\n',
'java_checked': 'import java.io.FileReader\ntry { FileReader("/nope") } catch (e: java.io.IOException) { print("io") }\n',
}
os.makedirs(HERE + '/ko3', exist_ok=True)
only = sys.argv[1:]
ok, rej, bad = 0, [], []
for name, src in cases.items():
    if only and name not in only:
        continue
    path = HERE + '/ko3/' + name + '.mam'
    open(path, 'w', encoding='utf-8').write(src)
    out = HERE + '/ko3/o'
    shutil.rmtree(out, ignore_errors=True)
    p = subprocess.run([COMPILER, path, out], capture_output=True, timeout=60, stdin=subprocess.DEVNULL)
    if p.returncode != 0:
        lines = p.stderr.decode('utf-8', 'replace').strip().splitlines()
        msg = lines[0].split(': ', 1)[-1] if lines else f'exit {p.returncode}'
        rej.append(f'{name}: {msg[:90]}')
        continue
    r = subprocess.run(['java', '-Xverify:all', '-cp', out, 'Main'], capture_output=True, timeout=30, stdin=subprocess.DEVNULL)
    if r.returncode != 0:
        bad.append(f'{name}: ' + r.stderr.decode('utf-8', 'replace').strip().splitlines()[0][:150])
    else:
        ok += 1
        if only or os.environ.get('SHOW'):
            print(name, '=>', r.stdout.decode('utf-8', 'replace').strip().replace('\r', '').replace('\n', ' | ')[:130])
print('ok', ok, 'rejected', len(rej), 'runtime', len(bad))
print('--- rejected'); print('\n'.join(rej))
print('--- runtime'); print('\n'.join(bad))
