"""Fifth round: operators, collection corners, class and generic corners."""
import os, subprocess, shutil, sys
HERE = __import__('tempfile').mkdtemp().replace(chr(92), '/')
COMPILER = os.environ.get('MAMMOTH', 'target/release/mammoth.exe')
cases = {
'op_inc': 'class C(let n: Int) { fun inc(): C = C(n + 1) }\nvar c = C(1)\nc++\nprint(c.n)\n',
'op_dec': 'class C(let n: Int) { fun dec(): C = C(n - 1) }\nvar c = C(1)\nc--\nprint(c.n)\n',
'op_not': 'class C(let b: Bool) { fun not(): C = C(not b) }\nprint((not C(true)).b)\n',
'op_rem': 'class C(let n: Int) { fun rem(o: C): C = C(n % o.n) }\nprint((C(7) % C(4)).n)\n',
'op_div': 'class C(let n: Int) { fun div(o: Int): C = C(n / o) }\nprint((C(8) / 2).n)\n',
'op_plus_assign': 'class C { var n = 0\n fun plusAssign(k: Int) { n += k } }\nlet c = C()\nc += 3\nprint(c.n)\n',
'op_minus_assign_list': 'let l = mutableListOf(1, 2, 3)\nl -= 2\nprint(l)\n',
'op_plus_assign_list': 'let l = mutableListOf(1)\nl += 2\nl += listOf(3, 4)\nprint(l)\n',
'op_set_two': 'class G { var v = 0\n fun set(a: Int, b: Int, x: Int) { v = a + b + x } }\nlet g = G()\ng[1, 2] = 3\nprint(g.v)\n',
'op_compare': 'class V(let n: Int) implements Comparable<V> { override fun compareTo(o: V): Int = n - o.n }\nprint(V(1) < V(2))\nprint(V(3) >= V(3))\n',
'op_unary_plus': 'class V(let n: Int) { fun unaryPlus(): V = V(n + 100) }\nprint((+V(1)).n)\n',
'to_typed_array': 'let l = mutableListOf(1, 2)\nlet a: List<Int> = l.toTypedArray()\nprint(a.size)\n',
'array_of': 'let a = arrayOf(1, 2, 3)\nprint(a.size)\nprint(a[0])\n',
'int_array_of': 'let a = intArrayOf(1, 2, 3)\nprint(a.sum())\n',
'array_fill': 'let a = List<Int>(3) { it * 2 }\nprint(a)\n',
'mutable_list_fill': 'let a = MutableList<Int>(3) { it * 2 }\nprint(a)\n',
'list_2d': 'let g = MutableList<MutableList<Int>>(2) { mutableListOf(0, 0) }\ng[1][0] = 5\nprint(g)\n',
'map_get_or_put': 'let m = mutableMapOf<Text, MutableList<Int>>()\nm.getOrPut("a") { mutableListOf() }.add(1)\nprint(m)\n',
'map_entries_loop': 'let m = mapOf("a" to 1, "b" to 2)\nfor (k, v) in m { print(k + v.toString()) }\n',
'map_entries_map': 'let m = mapOf("a" to 1)\nprint(m.map { (k, v) -> k + v.toString() })\n',
'map_filter_keys': 'let m = mapOf("a" to 1, "b" to 2)\nprint(m.filter { it.value > 1 })\nprint(m.filterKeys { it == "a" })\nprint(m.mapValues { it.value * 2 })\n',
'map_to_list_sorted': 'let m = mapOf("b" to 1, "a" to 2)\nprint(m.toList().sortedBy { it.first })\nprint(m.keys.sorted())\n',
'group_by_count': 'let w = listOf("a", "bb", "cc")\nprint(w.groupBy { it.length }.mapValues { it.value.size })\nprint(w.groupingBy { it.length }.eachCount())\n',
'associate': 'let w = listOf("a", "bb")\nprint(w.associateWith { it.length })\nprint(w.associateBy { it.length })\nprint(w.associate { it to it.length })\n',
'fold_reduce': 'let l = listOf(1, 2, 3)\nprint(l.fold(0) { a, b -> a + b })\nprint(l.reduce { a, b -> a * b })\nprint(l.runningFold(0) { a, b -> a + b })\n',
'windowed_chunked': 'let l = listOf(1, 2, 3, 4)\nprint(l.chunked(2))\nprint(l.windowed(2))\nprint(l.zipWithNext())\n',
'flat_map': 'print(listOf(1, 2).flatMap { listOf(it, it) })\nprint(listOf(listOf(1), listOf(2)).flatten())\n',
'take_drop_while': 'let l = listOf(1, 2, 3, 1)\nprint(l.takeWhile { it < 3 })\nprint(l.dropWhile { it < 3 })\nprint(l.takeLast(2))\n',
'distinct_by': 'print(listOf("a", "b", "aa").distinctBy { it.length })\nprint(listOf(1, 1, 2).distinct())\n',
'sorted_with': 'data class P(let a: Int, let b: Text)\nlet l = listOf(P(1, "b"), P(1, "a"), P(0, "z"))\nprint(l.sortedWith(compareBy({ it.a }, { it.b })))\nprint(l.sortedByDescending { it.b })\n',
'min_max': 'let l = listOf(3, 1, 2)\nprint(l.max())\nprint(l.minOrNull())\nprint(l.maxOf { it * 2 })\nprint(l.sumOf { it * 2 })\nprint(l.average())\n',
'any_all_none': 'let l = listOf(1, 2)\nprint(l.any { it > 1 })\nprint(l.all { it > 1 })\nprint(l.none { it > 5 })\nprint(l.count { it > 1 })\n',
'first_last_or_null': 'let l = listOf(1, 2)\nprint(l.firstOrNull { it > 5 })\nprint(l.last { it < 5 })\nprint(l.getOrNull(9))\nprint(l.getOrElse(9) { -1 })\nprint(l.indexOfFirst { it == 2 })\n',
'with_index': 'for (i, v) in listOf("a", "b").withIndex() { print(i.toString() + v) }\nlistOf("a").forEachIndexed { i, v -> print(i.toString() + v) }\nprint(listOf("a").mapIndexed { i, v -> v + i.toString() })\n',
'set_ops': 'let a = setOf(1, 2)\nlet b = setOf(2, 3)\nprint(a union b)\nprint(a intersect b)\nprint(a subtract b)\nprint(a + 5)\nprint(2 in a)\n',
'list_plus_minus': 'let l = listOf(1, 2)\nprint(l + 3)\nprint(l + listOf(4))\nprint(l - 1)\n',
'string_builder': 'let s = buildString {\n append("a")\n append(1)\n appendLine("x") }\nprint(s.trim())\n',
'build_list': 'let l = buildList<Int> {\n add(1)\n addAll(listOf(2, 3)) }\nprint(l)\n',
'text_ops': 'let s = "Hello, World"\nprint(s.substringBefore(","))\nprint(s.substringAfter(", "))\nprint(s.lowercase())\nprint(s.reversed())\nprint(s.padStart(14, \'*\'))\nprint(s.lines().size)\nprint(s.indexOf("W"))\nprint(s.split(", ").size)\nprint(s.toList().size)\nprint(s.count { it == \'l\' })\n',
'text_multiline': 'let s = """\n    a\n    b\n""".trimIndent()\nprint(s)\n',
'text_format': 'print("%d-%s".format(1, "a"))\nprint(String.format("%.2f", 1.5))\n',
'text_tonum_null': 'print("x".toIntOrNull())\nprint("1.5".toDoubleOrNull())\nprint("12".toLong() + 1L)\n',
'char_ops': "let c = 'a'\nprint(c.isLetter())\nprint(c.isDigit())\nprint(c.code)\nprint((c + 1))\nprint('7'.digitToInt())\nprint(97.toChar())\n",
'when_ranges': 'let n = 5\nlet s = when (n) {\n in 1..3 -> "low"\n in 4..6 -> "mid"\n else -> "high" }\nprint(s)\n',
'when_multi': 'let n = 5\nlet s = when (n) {\n 1, 2 -> "a"\n 5, 6 -> "b"\n else -> "c" }\nprint(s)\n',
'when_is_generic': 'fun f(x: Any): Text = when (x) {\n is Int -> "int"\n is Text -> "text"\n is MutableList<*> -> "list"\n else -> "other" }\nprint(f(1))\nprint(f(mutableListOf(1)))\n',
'if_else_chain_expr': 'let n = 2\nlet s = if n == 1 { "a" } else if n == 2 { "b" } else { "c" }\nprint(s)\n',
'elvis_return': 'fun f(s: Text?): Int {\n let t = s ?: return -1\n return t.length }\nprint(f(null))\nprint(f("ab"))\n',
'elvis_throw': 'fun f(s: Text?): Int = (s ?: throw IllegalStateException("x")).length\nprint(f("ab"))\n',
'safe_chain': 'class A(let b: B?)\nclass B(let n: Int)\nlet a: A? = A(B(1))\nprint(a?.b?.n)\nprint(a?.b?.n ?: 0)\nlet z: A? = null\nprint(z?.b?.n)\n',
'safe_let': 'let s: Text? = "a"\ns?.let { print(it.length) }\nlet n = s?.let { it.length } ?: 0\nprint(n)\n',
'smart_cast_and': 'fun f(x: Any?): Int = if x is Text and x.length > 1 { x.length } else { 0 }\nprint(f("abc"))\n',
'smart_cast_early': 'fun f(x: Text?): Int {\n if x == null { return 0 }\n return x.length }\nprint(f("abc"))\n',
'generic_bound': 'fun <T extends Comparable<T>> big(a: T, b: T): T = if a > b { a } else { b }\nprint(big(1, 2))\nprint(big("a", "b"))\n',
'generic_class_bound': 'class Box<T extends Number>(let v: T) { fun d(): Double = v.toDouble() }\nprint(Box(1).d())\n',
'generic_fn_lambda': 'fun <T, R> conv(x: T, f: (T) -> R): R = f(x)\nprint(conv(1) { it.toString() + "!" })\n',
'generic_ext': 'fun <T> T.twice(): MutableList<T> = mutableListOf(this, this)\nprint(1.twice())\n',
'generic_where_iface': 'interface Shape { fun area(): Double }\nclass Sq(let s: Double) implements Shape { override fun area(): Double = s * s }\nfun <T extends Shape> total(xs: MutableList<T>): Double = xs.sumOf { it.area() }\nprint(total(mutableListOf(Sq(2.0))))\n',
'fun_type_alias': 'typealias Op = (Int, Int) -> Int\nlet add: Op = { a, b -> a + b }\nprint(add(1, 2))\n',
'fun_ref': 'fun dbl(x: Int): Int = x * 2\nprint(listOf(1, 2).map(::dbl))\nprint(listOf("a").map(Text::length))\n',
'fun_returning_fun': 'fun adder(n: Int): (Int) -> Int = { it + n }\nprint(adder(1)(2))\n',
'fun_nullable_fn': 'let f: ((Int) -> Int)? = null\nprint(f?.invoke(1))\n',
'fun_vararg_spread': 'fun s(vararg xs: Int): Int = xs.sum()\nlet a = listOf(1, 2)\nprint(s(*a))\nprint(s(1, 2, 3))\n',
'fun_tailrec': 'tailrec fun f(n: Int, acc: Int): Int = if n == 0 { acc } else { f(n - 1, acc + n) }\nprint(f(100000, 0))\n',
'fun_single_expr_unit': 'fun hi() = print("hi")\nhi()\n',
'fun_infer_return': 'fun two() = 2\nprint(two() + 1)\n',
'class_init_block': 'class C(n: Int) {\n let d: Int\n init { d = n * 2 }\n}\nprint(C(2).d)\n',
'class_companion_const': 'class C { companion object { const val MAX = 5\n fun make(): C = C() } }\nprint(C.MAX)\nprint(C.make() is C)\n',
'class_nested_enum': 'class C { enum E { A, B } }\nprint(C.E.A)\n',
'class_abstract': 'abstract class S { abstract fun area(): Int\n fun twice(): Int = area() * 2 }\nclass Q extends S { override fun area(): Int = 3 }\nprint(Q().twice())\n',
'class_iface_default_prop': 'interface N { val name: Text\n fun hi(): Text = "hi " + name }\nclass P(override val name: Text) implements N\nprint(P("a").hi())\n',
'class_equals_override': 'class P(let n: Int) {\n override fun equals(other: Any?): Bool = other is P and other.n == n\n override fun hashCode(): Int = n }\nprint(P(1) == P(1))\nprint(mutableSetOf(P(1), P(1)).size)\n',
'class_to_string_interp': 'class P(let n: Int) { override fun toString(): Text = "P{n}" }\nprint("{P(1)}")\n',
'class_comparable_sort': 'data class V(let n: Int) implements Comparable<V> { override fun compareTo(o: V): Int = n - o.n }\nprint(listOf(V(2), V(1)).sorted())\nprint(listOf(V(2), V(1)).max())\n',
'class_iterator': 'class R(let n: Int) implements Iterable<Int> {\n override fun iterator(): Iterator<Int> = (0 until n).toList().iterator() }\nfor x in R(2) { print(x) }\nprint(R(3).toList())\n',
'enum_when': 'enum E { A, B }\nfun f(e: E): Int = when (e) {\n E.A -> 1\n E.B -> 2 }\nprint(f(E.B))\n',
'enum_iface': 'interface L { fun l(): Text }\nenum E implements L { A, B;\n override fun l(): Text = name.lowercase() }\nprint(E.A.l())\n',
'enum_map': 'enum E { A, B }\nlet m = mutableMapOf<E, Int>()\nm[E.A] = 1\nprint(m[E.A])\nprint(E.entries.size)\n',
'object_singleton': 'object Reg { let items = mutableListOf<Text>()\n fun add(s: Text) { items.add(s) } }\nReg.add("a")\nprint(Reg.items)\n',
'sealed_iface': 'sealed interface R\ndata class Ok(let v: Int) implements R\nobject Err implements R\nfun f(r: R): Int = when (r) {\n is Ok -> r.v\n Err -> -1 }\nprint(f(Ok(1)))\nprint(f(Err))\n',
'try_finally': 'fun f(): Int {\n try { return 1 } finally { print("f") } }\nprint(f())\n',
'try_multi_catch': 'try { "x".toInt() } catch (e: NumberFormatException) { print("nf") } catch (e: Exception) { print("e") }\n',
'custom_exception': 'class MyErr(message: Text) extends RuntimeException(message)\ntry { throw MyErr("boom") } catch (e: MyErr) { print(e.message) }\n',
'require_check': 'fun f(n: Int): Int {\n require(n > 0) { "neg" }\n check(n < 9)\n return n }\nprint(f(1))\ntry { f(-1) } catch (e: IllegalArgumentException) { print(e.message) }\n',
'use_resource': 'import java.io.StringReader\nimport java.io.BufferedReader\nlet t = BufferedReader(StringReader("a\\nb")).use { it.readLine() }\nprint(t)\n',
'run_catching': 'let r = runCatching { "x".toInt() }\nprint(r.isFailure)\nprint(r.getOrNull())\nprint(runCatching { 1 }.getOrDefault(0))\n',
'pair_triple': 'let p = 1 to "a"\nlet (a, b) = p\nprint(a.toString() + b)\nlet t = Triple(1, 2, 3)\nprint(t.third)\nprint(Pair(1, 2).first)\n',
'while_do': 'var i = 0\ndo { i += 1 } while i < 3\nprint(i)\n',
'for_indices': 'let l = listOf("a", "b")\nfor i in l.indices { print(i) }\nfor i in l.lastIndex downTo 0 { print(l[i]) }\n',
'bit_ops': 'print(5 and 3)\nprint(5 shl 1)\nprint(5 xor 1)\nprint(5 shr 1)\nprint(5.inv())\n',
'long_math': 'let a = 1L shl 40\nprint(a)\nprint(Long.MAX_VALUE)\nprint(Int.MAX_VALUE + 1)\nprint(7 / 2)\nprint(7 % 3)\nprint(-7 / 2)\nprint(7.0 / 2)\n',
'lazy_seq': 'print(generateSequence(1) { it * 2 }.take(5).toList())\nprint(sequence { yield(1)\n yield(2) }.toList())\n',
'time_random': 'import java.util.Random\nlet r = Random(1L)\nprint(r.nextInt(10) >= 0)\nprint(System.currentTimeMillis() > 0L)\n',
'string_switch_char': 'for c in "ab" { when (c) {\n \'a\' -> print("A")\n else -> print("?") } }\n',
'nested_lambda_capture': 'var total = 0\nlistOf(1, 2).forEach { a -> listOf(10, 20).forEach { b -> total += a * b } }\nprint(total)\n',
'local_class': 'fun f(): Int {\n class L(let n: Int)\n return L(3).n }\nprint(f())\n',
'anonymous_fun': 'let f = fun(x: Int): Int { return x + 1 }\nprint(f(1))\n',
'destructure_lambda_map': 'let m = mapOf(1 to "a")\nm.forEach { (k, v) -> print(k.toString() + v) }\nm.forEach { k, v -> print(k.toString() + v) }\n',
'also_apply_chain': 'let l = mutableListOf<Int>().apply { add(1) }.also { print(it.size) }\nprint(l.takeIf { it.isNotEmpty() }?.first())\n',
}
os.makedirs(HERE + '/ko5', exist_ok=True)
only = sys.argv[1:]
ok, rej, bad = 0, [], []
for name, src in cases.items():
    if only and name not in only:
        continue
    path = HERE + '/ko5/' + name + '.mam'
    open(path, 'w', encoding='utf-8').write(src)
    out = HERE + '/ko5/o'
    shutil.rmtree(out, ignore_errors=True)
    p = subprocess.run([COMPILER, path, out], capture_output=True, timeout=60, stdin=subprocess.DEVNULL)
    if p.returncode != 0:
        lines = p.stderr.decode('utf-8', 'replace').strip().splitlines()
        msg = lines[0].split(': ', 1)[-1] if lines else f'exit {p.returncode}'
        rej.append(f'{name}: {msg[:110]}')
        continue
    r = subprocess.run(['java', '-Xverify:all', '-cp', out, 'Main'], capture_output=True, timeout=30, stdin=subprocess.DEVNULL)
    if r.returncode != 0:
        bad.append(f'{name}: ' + r.stderr.decode('utf-8', 'replace').strip().splitlines()[0][:150])
    else:
        ok += 1
        if only or os.environ.get('SHOW'):
            print(name, '=>', r.stdout.decode('utf-8', 'replace').strip().replace('\r', '').replace('\n', ' | ')[:160])
print('ok', ok, 'rejected', len(rej), 'runtime', len(bad))
print('--- rejected'); print('\n'.join(rej))
print('--- runtime'); print('\n'.join(bad))
